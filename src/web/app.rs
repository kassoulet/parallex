//! Web frontend state: the same shape as the gpui and TUI frontends — one
//! shared byte anchor, per-column bytes-per-row, drag state — with browser
//! event handlers on top.

use std::ops::Range;

use super::layout::{self, Divider};
use crate::core::color::{Colormap, Rgb, human_size};
use crate::core::geom::{self, Nav};

/// Entropy window for the web build. The desktop frontends make it a
/// preference; the web keeps one fixed value until preferences (localStorage)
/// exist.
pub(crate) const ENTROPY_WINDOW: usize = 256;

/// Pixels per byte in the zoom column, matching the desktop default and range.
pub(crate) const PIXEL_ZOOM_DEFAULT: f32 = 8.0;
pub(crate) const PIXEL_ZOOM_MIN: f32 = 1.0;
pub(crate) const PIXEL_ZOOM_MAX: f32 = 24.0;
pub(crate) const ZOOM_STEP: f32 = 1.25;

/// Address-gutter padding inside the hex column, matching `gui::paint`.
pub(crate) const ADDR_X: f32 = 8.0;

/// One drag gesture. A press can start at most one; it ends on window-wide
/// mouse-up, wherever the pointer went meanwhile.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Drag {
    /// Moving a column divider; `start_w` is the width when grabbed and
    /// `grab_dx` the offset of the press inside the strip.
    Divider(layout::Divider, f32, f32),
    /// Scrubbing the hex column's scrollbar.
    Scrollbar,
    /// Panning the shared anchor from the zoom column.
    ZoomPan { last_y: f32 },
    /// Dragging a selection through the hex column.
    HexSelect { anchor: usize },
    /// Pressing the overview navigates to the pressed region.
    Overview,
}

/// Cycle to the next colormap in display order (header clicks).
fn next_colormap(c: Colormap) -> Colormap {
    let all = Colormap::ALL;
    let i = all.iter().position(|&x| x == c).unwrap_or(0);
    all[(i + 1) % all.len()]
}

/// All frontend state, owned by a thread-local (wasm has one thread).
pub(crate) struct App {
    pub data: Vec<u8>,
    pub file_name: String,
    pub entropies: Vec<f32>,

    pub colormaps: [Colormap; 3], // overview, zoom, hex
    pub pixel_zoom: f32,

    /// The shared byte anchor: the byte at each panel's vertical centre, as
    /// in the other two frontends.
    pub scroll_offset: usize,
    pub hex_bpr: usize,
    pub zoom_bpr: usize,

    pub overview_width: f32,
    pub zoom_width: f32,

    /// Canvas size in CSS pixels and the device pixel ratio.
    pub css_w: f32,
    pub css_h: f32,
    pub dpr: f32,
    pub char_w: f32,

    pub hovered: Option<usize>,
    pub selected: Option<usize>,
    pub selection: Option<Range<usize>>,
    pub drag: Option<Drag>,

    /// Cached overview thumbnail and the key it was built for.
    pub overview: Option<(Vec<u8>, usize, usize)>,
    pub overview_key: Option<(usize, usize, Colormap)>,
    /// Cached zoom texture and the key it was built for.
    pub zoom: Option<(Vec<u8>, usize, usize)>,
    pub zoom_key: Option<(usize, usize, usize, Colormap)>,
}

impl App {
    pub(crate) fn new() -> Self {
        Self {
            data: Vec::new(),
            file_name: String::new(),
            entropies: Vec::new(),
            colormaps: [Colormap::Entropy, Colormap::Value, Colormap::Class],
            pixel_zoom: PIXEL_ZOOM_DEFAULT,
            scroll_offset: 0,
            hex_bpr: 16,
            zoom_bpr: 32,
            overview_width: 200.0,
            zoom_width: 320.0,
            css_w: 0.0,
            css_h: 0.0,
            dpr: 1.0,
            char_w: 8.0,
            hovered: None,
            selected: None,
            selection: None,
            drag: None,
            overview: None,
            overview_key: None,
            zoom: None,
            zoom_key: None,
        }
    }

    pub(crate) fn layout(&self) -> layout::Layout {
        layout::Layout::new(self.css_w, self.overview_width, self.zoom_width)
    }

    pub(crate) fn byte_source(&self, colormap: Colormap) -> geom::ByteSource<'_> {
        geom::ByteSource {
            data: &self.data,
            entropies: &self.entropies,
            entropy_window: ENTROPY_WINDOW,
            colormap,
        }
    }

    /// Height of the column bodies: below the headers, above the status bar.
    pub(crate) fn body_h(&self) -> f32 {
        (self.css_h - layout::HEADER_H - layout::STATUS_H).max(1.0)
    }

    /// The row-aligned start of the hex column's first visible row.
    pub(crate) fn first_row_start(&self) -> usize {
        geom::first_row_centred(
            self.scroll_offset,
            self.hex_bpr,
            geom::visible_rows(self.body_h(), layout::BLOCK_H),
        )
    }

    pub(crate) fn clamp_anchor(&mut self) {
        let last = geom::max_anchor(self.data.len(), self.hex_bpr.max(8));
        self.scroll_offset = self.scroll_offset.min(last);
    }

    /// Replace the file. Entropy runs synchronously: wasm has no rayon, and a
    /// sub-second pass on load is acceptable for a browser app (the desktop
    /// frontends stream it in the background instead).
    pub(crate) fn set_data(&mut self, name: String, data: Vec<u8>) {
        self.entropies = crate::core::entropy::block_entropies(&data, ENTROPY_WINDOW);
        self.data = data;
        self.file_name = name;
        self.scroll_offset = 0;
        self.selected = None;
        self.selection = None;
        self.hovered = None;
        // Force thumbnail rebuilds.
        self.overview_key = None;
        self.zoom_key = None;
        self.recompute_bprs();
    }

    pub(crate) fn set_viewport(&mut self, css_w: f32, css_h: f32, dpr: f32) {
        self.css_w = css_w;
        self.css_h = css_h;
        self.dpr = dpr;
        self.recompute_bprs();
    }

    pub(crate) fn recompute_bprs(&mut self) {
        let l = self.layout();
        self.hex_bpr = geom::hex_bytes_per_row(
            (l.hex_w - layout::SCROLLBAR_W - 4.0).max(8.0),
            self.char_w,
            ADDR_X,
        );
        self.zoom_bpr = layout::zoom_bpr(l.zoom_w, self.pixel_zoom);
        self.clamp_anchor();
    }

    /// Scroll so `off` is on a visible row.
    fn reveal(&mut self, off: usize) {
        let first = self.first_row_start();
        let rows = geom::visible_rows(self.body_h(), layout::BLOCK_H);
        let end = first + rows * self.hex_bpr;
        if off < first || off >= end {
            self.scroll_offset = off;
            self.clamp_anchor();
        }
    }

    pub(crate) fn zoom_block(&self) -> f32 {
        layout::zoom_block(self.layout().zoom_w, self.zoom_bpr)
    }

    // ----- input ---------------------------------------------------------

    pub(crate) fn scroll_anchor_by(&mut self, delta: isize) {
        self.scroll_offset = if delta >= 0 {
            self.scroll_offset.saturating_add(delta as usize)
        } else {
            self.scroll_offset.saturating_sub(delta.unsigned_abs())
        };
        self.clamp_anchor();
    }

    pub(crate) fn on_mouse_down(&mut self, x: f32, y: f32) {
        let l = self.layout();
        if y < layout::HEADER_H {
            // A header click cycles that column's colormap.
            let panel = if x < l.overview_w + layout::DIVIDER_W {
                0
            } else if x < l.hex_x {
                1
            } else {
                2
            };
            self.colormaps[panel] = next_colormap(self.colormaps[panel]);
            self.overview_key = None;
            self.zoom_key = None;
            return;
        }
        if y >= self.css_h - layout::STATUS_H {
            return;
        }
        let Some(kind) = l.divider_at(x) else {
            if x >= l.scrollbar_x() && x < l.hex_x + l.hex_w {
                self.drag = Some(Drag::Scrollbar);
                self.scrollbar_drag(x, y);
            } else if x >= l.hex_x {
                if let Some(off) = self.hex_offset_at(x, y) {
                    self.drag = Some(Drag::HexSelect { anchor: off });
                    self.selected = Some(off);
                    self.selection = Some(off..off + 1);
                }
            } else if x >= l.zoom_x {
                self.drag = Some(Drag::ZoomPan { last_y: y });
            } else if !self.data.is_empty() {
                // Overview: jump the anchor to the pressed fraction of the file.
                let t = ((y - layout::HEADER_H) / self.body_h()).clamp(0.0, 1.0);
                self.scroll_offset = (t * self.data.len() as f32) as usize;
                self.clamp_anchor();
                self.drag = Some(Drag::Overview);
            }
            return;
        };
        let start_w = match kind {
            Divider::OverviewZoom => self.overview_width,
            Divider::ZoomHex => self.zoom_width,
        };
        self.drag = Some(Drag::Divider(kind, start_w, x));
    }

    pub(crate) fn on_mouse_move(&mut self, px: f32, py: f32) {
        self.hovered = self.hex_offset_at(px, py);
        match self.drag {
            Some(Drag::Divider(kind, start_w, grab_x)) => {
                let w = layout::Layout::dragged_width(kind, start_w, px, grab_x);
                match kind {
                    Divider::OverviewZoom => self.overview_width = w,
                    Divider::ZoomHex => self.zoom_width = w,
                }
                self.recompute_bprs();
            }
            Some(Drag::Scrollbar) => self.scrollbar_drag(px, py),
            Some(Drag::ZoomPan { last_y }) => {
                let block = self.zoom_block().max(0.001);
                let delta = ((py - last_y) / block) as isize * self.zoom_bpr as isize;
                if delta != 0 {
                    if let Some(Drag::ZoomPan { last_y }) = &mut self.drag {
                        *last_y = py;
                    }
                    self.scroll_anchor_by(delta);
                }
            }
            Some(Drag::HexSelect { anchor }) => {
                if let Some(off) = self.hex_offset_at(px, py) {
                    let (lo, hi) = (anchor.min(off), anchor.max(off) + 1);
                    self.selection = Some(lo..hi.min(self.data.len()));
                    self.selected = Some(off);
                }
            }
            Some(Drag::Overview) => {
                let t = ((py - layout::HEADER_H) / self.body_h()).clamp(0.0, 1.0);
                self.scroll_offset = (t * self.data.len() as f32) as usize;
                self.clamp_anchor();
            }
            None => {}
        }
    }

    pub(crate) fn end_drag(&mut self) {
        self.drag = None;
    }

    /// Move the anchor to the position `y` selects on the hex scrollbar.
    fn scrollbar_drag(&mut self, x: f32, y: f32) {
        let l = self.layout();
        if x < l.scrollbar_x() {
            return;
        }
        let track_y = (y - layout::HEADER_H).max(0.0);
        let visible = (self.hex_bpr * geom::visible_rows(self.body_h(), layout::BLOCK_H))
            .min(self.data.len());
        let last = geom::max_anchor(self.data.len(), self.hex_bpr.max(8));
        self.scroll_offset =
            geom::scrollbar_anchor_at(track_y, self.body_h(), last, visible, self.data.len());
        self.clamp_anchor();
    }

    /// Pane-local `(x, y)` inside the hex column to a file offset.
    fn hex_offset_at(&self, x: f32, y: f32) -> Option<usize> {
        let l = self.layout();
        if x < l.hex_x || x >= l.scrollbar_x() || y < layout::HEADER_H {
            return None;
        }
        let geo = geom::RowGeo::new(ADDR_X, self.char_w, self.hex_bpr);
        geom::hex_offset_at(
            x - l.hex_x,
            y - layout::HEADER_H,
            &geo,
            layout::BLOCK_H,
            self.first_row_start(),
            self.data.len(),
        )
    }

    pub(crate) fn navigate(&mut self, nav: Nav) {
        if self.data.is_empty() {
            return;
        }
        let page_bytes = self.hex_bpr * geom::visible_rows(self.body_h(), layout::BLOCK_H);
        let cur = self.selected.unwrap_or(self.scroll_offset);
        let next = geom::nav_next(nav, cur, self.hex_bpr, page_bytes, self.data.len());
        self.selected = Some(next);
        self.selection = Some(next..next + 1);
        self.reveal(next);
    }

    pub(crate) fn zoom_in(&mut self) {
        self.set_pixel_zoom(self.pixel_zoom * ZOOM_STEP);
    }

    pub(crate) fn zoom_out(&mut self) {
        self.set_pixel_zoom(self.pixel_zoom / ZOOM_STEP);
    }

    fn set_pixel_zoom(&mut self, zoom: f32) {
        let next = zoom.clamp(PIXEL_ZOOM_MIN, PIXEL_ZOOM_MAX);
        if next != self.pixel_zoom {
            self.pixel_zoom = next;
            self.recompute_bprs();
            self.zoom_key = None;
        }
    }

    // ----- render support --------------------------------------------------

    /// The color of the byte at `off` under a column's colormap.
    pub(crate) fn color_at(&self, panel: usize, off: usize) -> Option<Rgb> {
        let src = self.byte_source(self.colormaps[panel]);
        src.color_at(off)
    }

    /// Status-bar summary, rendered by `render`.
    pub(crate) fn status_text(&self) -> String {
        let rows = geom::visible_rows(self.body_h(), layout::BLOCK_H);
        let first = self.first_row_start();
        let end = (first + rows * self.hex_bpr).min(self.data.len());
        let pct = if self.data.is_empty() {
            0
        } else {
            self.scroll_offset * 100 / self.data.len()
        };
        format!(
            "{} · {} · rows {}–{} / {} · {}%",
            if self.file_name.is_empty() {
                "no file"
            } else {
                &self.file_name
            },
            human_size(self.data.len()),
            first,
            end,
            self.data.len(),
            pct
        )
    }
}
