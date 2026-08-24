//! Pure layout maths for the web frontend: where each column sits inside the
//! canvas and which gesture a pointer position belongs to.
//!
//! Everything here is a plain function of its arguments so it is unit-testable
//! on the host, exactly like `core::geom` — the DOM code in `super` only wires
//! state into it.

use crate::core::geom;

/// Width of the draggable divider strips between columns, in CSS pixels.
pub(crate) const DIVIDER_W: f32 = 6.0;

/// Clamp range for the overview column's width.
pub(crate) const OVERVIEW_W_MIN: f32 = 120.0;
pub(crate) const OVERVIEW_W_MAX: f32 = 800.0;

/// Clamp range for the zoom column's width.
pub(crate) const ZOOM_W_MIN: f32 = 160.0;
pub(crate) const ZOOM_W_MAX: f32 = 1600.0;

/// Height of the per-column header and the bottom status bar.
pub(crate) const HEADER_H: f32 = 28.0;
pub(crate) const STATUS_H: f32 = 24.0;

/// Which draggable divider a pointer grabbed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Divider {
    /// Between the overview and zoom columns; resizes the overview.
    OverviewZoom,
    /// Between the zoom and hex columns; resizes the zoom.
    ZoomHex,
}

/// Column x-ranges inside the canvas, derived from the two adjustable widths.
/// The hex column takes the remainder.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    pub overview_x: f32,
    pub overview_w: f32,
    pub zoom_x: f32,
    pub zoom_w: f32,
    pub hex_x: f32,
    pub hex_w: f32,
}

impl Layout {
    /// Lay out three columns of a canvas `canvas_w` CSS pixels wide. The
    /// dividers are fixed-width strips between the columns; the hex column
    /// absorbs the rest (floored at 200 px, so extreme widths squeeze the
    /// other two instead of collapsing the hex view).
    pub fn new(canvas_w: f32, overview_w: f32, zoom_w: f32) -> Self {
        let overview_w = overview_w.clamp(OVERVIEW_W_MIN, OVERVIEW_W_MAX);
        let zoom_w = zoom_w.clamp(ZOOM_W_MIN, ZOOM_W_MAX);
        let overview_x = 0.0;
        let zoom_x = overview_w + DIVIDER_W;
        let hex_x = zoom_x + zoom_w + DIVIDER_W;
        let hex_w = (canvas_w - hex_x).max(200.0);
        Self {
            overview_x,
            overview_w,
            zoom_x,
            zoom_w,
            hex_x,
            hex_w,
        }
    }

    pub fn divider_pos(&self, kind: Divider) -> f32 {
        match kind {
            Divider::OverviewZoom => self.overview_w,
            Divider::ZoomHex => self.zoom_x + self.zoom_w,
        }
    }

    /// The divider under canvas x, if any (a `DIVIDER_W` strip around it).
    pub fn divider_at(&self, x: f32) -> Option<Divider> {
        let kinds = [Divider::OverviewZoom, Divider::ZoomHex];
        kinds.into_iter().find(|&k| {
            let d = self.divider_pos(k);
            x >= d && x < d + DIVIDER_W
        })
    }

    /// The width the column left of `kind` should take when the pointer that
    /// started on the divider is now at canvas x.
    pub fn dragged_width(kind: Divider, start_w: f32, x: f32, grab_dx: f32) -> f32 {
        let (min, max) = match kind {
            Divider::OverviewZoom => (OVERVIEW_W_MIN, OVERVIEW_W_MAX),
            Divider::ZoomHex => (ZOOM_W_MIN, ZOOM_W_MAX),
        };
        (start_w + x - grab_dx).round().clamp(min, max)
    }

    /// The hex column's scrollbar strip: the same 10 px the gpui frontend
    /// uses, at the right edge of the canvas. The web build has no window-edge
    /// resize handles, so unlike gpui the strip can sit flush.
    pub fn scrollbar_x(&self) -> f32 {
        self.hex_x + self.hex_w - SCROLLBAR_W
    }
}

/// Width of the hex column's scrollbar strip, matching `gui::paint`.
pub(crate) const SCROLLBAR_W: f32 = 10.0;

/// Hex row metrics, matching `gui::paint`: 18 px row, 3 px gap, 21 px pitch.
pub(crate) const ROW_H: f32 = 18.0;
pub(crate) const ROW_GAP: f32 = 3.0;
pub(crate) const BLOCK_H: f32 = ROW_H + ROW_GAP;

/// Bytes per row the zoom column shows for its width and pixel zoom.
pub(crate) fn zoom_bpr(zoom_w: f32, pixel_zoom: f32) -> usize {
    geom::zoom_bytes_per_row((zoom_w - 2.0).max(1.0), pixel_zoom)
}

/// The zoom column's block size after redistribution.
pub(crate) fn zoom_block(zoom_w: f32, bpr: usize) -> f32 {
    geom::zoom_block_w((zoom_w - 2.0).max(1.0), bpr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_gives_hex_the_remainder() {
        let l = Layout::new(1200.0, 200.0, 320.0);
        assert_eq!(l.overview_x, 0.0);
        assert_eq!(l.overview_w, 200.0);
        assert_eq!(l.zoom_x, 206.0);
        assert_eq!(l.zoom_w, 320.0);
        assert_eq!(l.hex_x, 532.0);
        assert_eq!(l.hex_w, 1200.0 - 532.0);
    }

    #[test]
    fn layout_clamps_extreme_widths() {
        let l = Layout::new(1200.0, 10_000.0, 1.0);
        assert_eq!(l.overview_w, OVERVIEW_W_MAX);
        assert_eq!(l.zoom_w, ZOOM_W_MIN);
    }

    #[test]
    fn hex_column_never_collapses() {
        // Two maxed columns on a tiny canvas: the hex column floors at 200 px
        // and overflows the canvas (the browser scrolls) rather than showing
        // nothing.
        let l = Layout::new(300.0, OVERVIEW_W_MAX, ZOOM_W_MAX);
        assert_eq!(l.hex_w, 200.0);
        assert!(l.hex_x + l.hex_w > 300.0);
    }

    #[test]
    fn dividers_hit_test_and_drag() {
        let l = Layout::new(1200.0, 200.0, 320.0);
        assert_eq!(l.divider_at(203.0), Some(Divider::OverviewZoom));
        assert_eq!(l.divider_at(528.0), Some(Divider::ZoomHex));
        assert_eq!(l.divider_at(400.0), None);
        // Dragging the first divider to x=400 grew the overview by the same
        // delta, relative to where the grab started.
        let w = Layout::dragged_width(Divider::OverviewZoom, 200.0, 400.0, 203.0);
        assert_eq!(w, 397.0);
    }

    #[test]
    fn zoom_geometry_leaves_a_pixel_of_padding() {
        assert_eq!(zoom_bpr(320.0, 8.0), 39); // 318 / 8
        let bpr = zoom_bpr(320.0, 8.0);
        assert!((zoom_block(320.0, bpr) - 318.0 / 39.0).abs() < 1e-4);
    }
}
