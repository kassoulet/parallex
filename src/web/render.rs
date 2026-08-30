//! Canvas painting for the web frontend: blit the `core` thumbnail buffers
//! for the overview and zoom columns, draw the hex column's visible rows as
//! text over colored cells, and chrome (headers, scrollbar, status bar).
//!
//! Mirrors `gui::paint` in meaning; the mechanics are the 2D canvas API. All geometry
//! comes from `core::geom`, all colors from `core::color`.
use wasm_bindgen::Clamped;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use super::app::{ADDR_X, App};
use super::layout::{self, DIVIDER_W, HEADER_H, Layout, SCROLLBAR_W, STATUS_H};
use crate::core::color::{Rgb, fg_for_bg, printable};
use crate::core::geom::{self, RowGeo};

pub(crate) const HEX_FONT: &str =
    "13px ui-monospace, 'JetBrains Mono', 'Fira Mono', Menlo, Consolas, monospace";
const UI_FONT: &str = "12px system-ui, sans-serif";
const BOLD_FONT: &str = "bold 12px system-ui, sans-serif";

// Same theme as the desktop frontends.
const BG: &str = "#16161e";
const PANEL_BG: &str = "#0c0d14";
const DIVIDER_BG: &str = "#1a1b26";
const TEXT: &str = "#c0caf5";
const MUTED: &str = "#565f89";
const ACCENT: &str = "#9d7cd8";
const SEL_OVERLAY: &str = "rgba(255,255,255,0.24)";

pub(crate) fn ctx_of(canvas: &HtmlCanvasElement) -> CanvasRenderingContext2d {
    canvas
        .get_context("2d")
        .expect("2d context")
        .expect("2d context present")
        .dyn_into()
        .expect("2d context type")
}

fn rgb_str(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn set_fill(ctx: &CanvasRenderingContext2d, s: &str) {
    ctx.set_fill_style_str(s);
}

fn fill_rect(ctx: &CanvasRenderingContext2d, x: f32, y: f32, w: f32, h: f32, color: &str) {
    set_fill(ctx, color);
    ctx.fill_rect(f64::from(x), f64::from(y), f64::from(w), f64::from(h));
}

fn fill_text(ctx: &CanvasRenderingContext2d, s: &str, x: f32, y: f32, color: &str) {
    set_fill(ctx, color);
    let _ = ctx.fill_text(s, f64::from(x), f64::from(y));
}

/// Draw one frame over the whole canvas.
pub(crate) fn draw(app: &mut App, canvas: &HtmlCanvasElement) {
    let ctx = ctx_of(canvas);
    let dpr = f64::from(app.dpr);
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    let l = app.layout();
    fill_rect(&ctx, 0.0, 0.0, app.css_w, app.css_h, BG);

    draw_overview(app, &ctx, &l);
    draw_zoom(app, &ctx, &l);
    draw_hex(app, &ctx, &l);
    draw_dividers(app, &ctx, &l);
    draw_headers(app, &ctx, &l);
    draw_status(app, &ctx);
}

// ----- thumbnails -----------------------------------------------------------

/// Blit a cached RGBA buffer into the panel at `(x, y, w, h)`: upload it to an
/// offscreen canvas as `ImageData`, then `draw_image` scales it into place
/// (device pixels handled by the transform; smoothing off, like gpui).
#[allow(clippy::too_many_arguments)] // one blit, one rect: the args are the rect
fn blit_rgba(
    ctx: &CanvasRenderingContext2d,
    scratch: &HtmlCanvasElement,
    pixels: &[u8],
    iw: usize,
    ih: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    scratch.set_width(iw as u32);
    scratch.set_height(ih as u32);
    let sctx = ctx_of(scratch);
    if let Ok(img) =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels), iw as u32, ih as u32)
    {
        let _ = sctx.put_image_data(&img, 0.0, 0.0);
        let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
            scratch,
            f64::from(x),
            f64::from(y),
            f64::from(w),
            f64::from(h),
        );
    }
}

fn scratch_canvas(document: &web_sys::Document, id: &str) -> HtmlCanvasElement {
    document
        .get_element_by_id(id)
        .unwrap_or_else(|| {
            let c = document.create_element("canvas").expect("create canvas");
            c.set_id(id);
            let _ = document.body().expect("body").append_child(&c);
            c
        })
        .dyn_into()
        .expect("canvas element")
}

fn draw_overview(app: &mut App, ctx: &CanvasRenderingContext2d, l: &Layout) {
    fill_rect(
        ctx,
        l.overview_x,
        HEADER_H,
        l.overview_w,
        app.body_h(),
        PANEL_BG,
    );
    if app.data.is_empty() {
        return;
    }
    let w = (l.overview_w as usize).clamp(16, 512);
    let h = (app.body_h() as usize).clamp(16, 2048);
    let key = (w, h, app.colormaps[0]);
    if app.overview_key != Some(key) {
        let src = app.byte_source(app.colormaps[0]);
        let pixels = crate::core::thumb::build_overview_rgba(&src, w, h);
        app.overview = Some((pixels, w, h));
        app.overview_key = Some(key);
    }
    if let Some((pixels, iw, ih)) = &app.overview {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let scratch = scratch_canvas(&document, "parallex-scratch-overview");
        blit_rgba(
            ctx,
            &scratch,
            pixels,
            *iw,
            *ih,
            l.overview_x,
            HEADER_H,
            l.overview_w,
            app.body_h(),
        );
    }
    // Viewport marker: where the zoom column is looking, like the gpui build.
    let rows = geom::visible_rows(app.body_h(), layout::BLOCK_H);
    let first = app.first_row_start();
    let len = app.data.len();
    let y0 = HEADER_H + first as f32 / len as f32 * app.body_h();
    let y1 = HEADER_H + ((first + rows * app.hex_bpr).min(len)) as f32 / len as f32 * app.body_h();
    fill_rect(
        ctx,
        l.overview_x,
        y0,
        l.overview_w,
        (y1 - y0).max(2.0),
        "rgba(255,255,255,0.12)",
    );
}

fn draw_zoom(app: &mut App, ctx: &CanvasRenderingContext2d, l: &Layout) {
    fill_rect(ctx, l.zoom_x, HEADER_H, l.zoom_w, app.body_h(), PANEL_BG);
    if app.data.is_empty() || app.zoom_bpr == 0 {
        return;
    }
    let rows = geom::visible_rows(app.body_h(), app.zoom_block());
    let block = app.zoom_block();
    let first = geom::first_row_centred(app.scroll_offset, app.zoom_bpr, rows);
    let iw = ((app.zoom_bpr as f32 * block).ceil() as usize).max(1);
    let ih = ((rows as f32 * block).ceil() as usize).max(1);
    let key = (app.zoom_bpr, first, iw * ih, app.colormaps[1]);
    if app.zoom_key != Some(key) {
        let src = app.byte_source(app.colormaps[1]);
        let (pixels, iw, ih) =
            crate::core::thumb::build_zoom_rgba(&src, app.zoom_bpr, first, rows, block);
        app.zoom = Some((pixels, iw, ih));
        app.zoom_key = Some(key);
    }
    if let Some((pixels, iw, ih)) = &app.zoom {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let scratch = scratch_canvas(&document, "parallex-scratch-zoom");
        blit_rgba(
            ctx,
            &scratch,
            pixels,
            *iw,
            *ih,
            l.zoom_x + 1.0,
            HEADER_H,
            *iw as f32,
            *ih as f32,
        );
    }
}

// ----- hex column -----------------------------------------------------------

/// One visible hex row at a time over colored cells; long because the row
/// loop is the whole column.
#[allow(clippy::too_many_lines)]
fn draw_hex(app: &mut App, ctx: &CanvasRenderingContext2d, l: &Layout) {
    fill_rect(ctx, l.hex_x, HEADER_H, l.hex_w, app.body_h(), PANEL_BG);
    let len = app.data.len();
    if len == 0 || app.hex_bpr == 0 {
        return;
    }
    let row_h = layout::BLOCK_H;
    let rows = geom::visible_rows(app.body_h(), row_h);
    let first = app.first_row_start();
    let bpr = app.hex_bpr;
    let geo = RowGeo::new(ADDR_X, app.char_w, bpr);

    ctx.set_font(HEX_FONT);
    ctx.set_text_baseline("middle");

    let mut text = String::with_capacity(128);
    let mut hex_offsets = Vec::with_capacity(bpr);
    let mut ascii_offsets = Vec::with_capacity(bpr);
    let mut colors: Vec<Option<Rgb>> = Vec::with_capacity(bpr);
    let src = app.byte_source(app.colormaps[2]);
    let selection = app.selection.clone();

    for r in 0..rows {
        let row_start = first + r * bpr;
        if row_start >= len {
            break;
        }
        let y0 = HEADER_H + r as f32 * row_h;
        let n = (len - row_start).min(bpr);
        geom::build_row_text_into(
            app.data.as_slice(),
            row_start,
            n,
            bpr,
            &mut text,
            &mut hex_offsets,
            &mut ascii_offsets,
        );
        colors.clear();
        colors.extend((0..n).map(|i| src.color_at(row_start + i)));

        // Address.
        fill_text(
            ctx,
            &format!("{row_start:08X}"),
            l.hex_x + geo.hex_start - 10.0 * app.char_w,
            y0 + row_h * 0.5,
            MUTED,
        );

        // Byte cells: hex digits over a colored cell, ASCII glyphs over a
        // contiguous band — same meaning as `gui::paint`. Indexed in parallel
        // arrays like `paint_hex` does, hence the indexed loop.
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            let off = row_start + i;
            let selected = selection.as_ref().is_some_and(|s| s.contains(&off));
            let bg = colors[i];
            if let Some(c) = bg {
                fill_rect(
                    ctx,
                    l.hex_x + geo.cell_x(i),
                    y0,
                    geo.hex_fill_w(),
                    layout::ROW_H,
                    &rgb_str(c),
                );
                fill_text(
                    ctx,
                    &format!("{:02X}", app.data[off]),
                    l.hex_x + geo.cell_x(i),
                    y0 + row_h * 0.5,
                    &rgb_str(fg_for_bg(c)),
                );
                fill_rect(
                    ctx,
                    l.hex_x + geo.ascii_x(i),
                    y0,
                    app.char_w,
                    layout::ROW_H,
                    &rgb_str(c),
                );
                fill_text(
                    ctx,
                    &printable(app.data[off]).to_string(),
                    l.hex_x + geo.ascii_x(i),
                    y0 + row_h * 0.5,
                    &rgb_str(fg_for_bg(c)),
                );
            }
            if selected {
                fill_rect(
                    ctx,
                    l.hex_x + geo.cell_x(i),
                    y0,
                    geo.cell_w,
                    layout::ROW_H,
                    SEL_OVERLAY,
                );
                fill_rect(
                    ctx,
                    l.hex_x + geo.ascii_x(i),
                    y0,
                    app.char_w,
                    layout::ROW_H,
                    SEL_OVERLAY,
                );
            }
            if app.hovered == Some(off) && !selected {
                fill_rect(
                    ctx,
                    l.hex_x + geo.cell_x(i),
                    y0,
                    geo.cell_w,
                    layout::ROW_H,
                    "rgba(255,255,255,0.12)",
                );
            }
        }
    }

    draw_scrollbar(app, ctx, l);
}

fn draw_scrollbar(app: &App, ctx: &CanvasRenderingContext2d, l: &Layout) {
    let track_h = app.body_h();
    let visible =
        (app.hex_bpr * geom::visible_rows(track_h, layout::BLOCK_H)).min(app.data.len().max(1));
    let last = geom::max_anchor(app.data.len(), app.hex_bpr.max(8));
    let (top, height) =
        geom::scrollbar_thumb(track_h, app.scroll_offset, last, visible, app.data.len());
    fill_rect(
        ctx,
        l.scrollbar_x(),
        HEADER_H,
        SCROLLBAR_W,
        track_h,
        "#1a1b26",
    );
    fill_rect(
        ctx,
        l.scrollbar_x() + 1.0,
        HEADER_H + top,
        SCROLLBAR_W - 2.0,
        height,
        "#3b4261",
    );
}

// ----- chrome ---------------------------------------------------------------

fn draw_dividers(app: &App, ctx: &CanvasRenderingContext2d, l: &Layout) {
    for kind in [layout::Divider::OverviewZoom, layout::Divider::ZoomHex] {
        fill_rect(
            ctx,
            l.divider_pos(kind),
            HEADER_H,
            DIVIDER_W,
            app.body_h(),
            DIVIDER_BG,
        );
    }
}

fn draw_headers(app: &App, ctx: &CanvasRenderingContext2d, l: &Layout) {
    ctx.set_font(BOLD_FONT);
    ctx.set_text_baseline("middle");
    let titles = [("Overview", 0), ("Zoom", 1), ("Hex", 2)];
    let xs = [l.overview_x, l.zoom_x, l.hex_x];
    let ws = [l.overview_w + DIVIDER_W, l.zoom_w + DIVIDER_W, l.hex_w];
    for &(title, panel) in &titles {
        fill_rect(ctx, xs[panel], 0.0, ws[panel], HEADER_H, BG);
        fill_text(ctx, title, xs[panel] + 8.0, HEADER_H * 0.5, ACCENT);
        ctx.set_font(UI_FONT);
        let label = format!("Color: {}", app.colormaps[panel].label());
        fill_text(ctx, &label, xs[panel] + 90.0, HEADER_H * 0.5, MUTED);
        ctx.set_font(BOLD_FONT);
    }
}

fn draw_status(app: &App, ctx: &CanvasRenderingContext2d) {
    fill_rect(ctx, 0.0, app.css_h - STATUS_H, app.css_w, STATUS_H, BG);
    ctx.set_font(UI_FONT);
    ctx.set_text_baseline("middle");
    let y = app.css_h - STATUS_H * 0.5;
    fill_text(ctx, &app.status_text(), 8.0, y, MUTED);
    if let Some(off) = app.hovered {
        let b = app.data.get(off).copied();
        let text = match b {
            Some(b) => format!("0x{off:08X} · 0x{b:02X}"),
            None => format!("0x{off:08X}"),
        };
        ctx.set_text_align("right");
        fill_text(ctx, &text, app.css_w - 8.0, y, MUTED);
        ctx.set_text_align("left");
    }
}
