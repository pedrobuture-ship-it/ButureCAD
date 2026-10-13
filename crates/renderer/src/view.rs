//! Camera: world (nm, +Y up) ↔ screen (pixels, +Y down).

use buturecad_geometry::{BoundingBox, Coord, Point};

/// Pixels per nanometre limits (1 px = 1 µm … 1 px = 10 mm per 100 px; i.e. ~1e-7 to 0.05).
pub const MIN_SCALE: f64 = 1.0e-7;
pub const MAX_SCALE: f64 = 0.05;

/// The viewport transform. `center` is the world point shown at the middle of the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub center: (f64, f64),
    /// Pixels per nanometre.
    pub scale: f64,
    pub width: f64,
    pub height: f64,
}

impl View {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            center: (0.0, 0.0),
            scale: 1.0e-5,
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    pub fn world_to_screen(&self, p: Point) -> (f64, f64) {
        (
            (p.x as f64 - self.center.0) * self.scale + self.width / 2.0,
            self.height / 2.0 - (p.y as f64 - self.center.1) * self.scale,
        )
    }

    /// Screen position → world position as floating point (no rounding).
    pub fn screen_to_world_f(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.width / 2.0) / self.scale + self.center.0,
            (self.height / 2.0 - y) / self.scale + self.center.1,
        )
    }

    /// Screen position → world point, rounded to the nearest nm and clamped to the valid range.
    pub fn screen_to_world(&self, x: f64, y: f64) -> Point {
        let (wx, wy) = self.screen_to_world_f(x, y);
        let max = buturecad_geometry::MAX_COORD as f64;
        Point::new(
            wx.round().clamp(-max, max) as Coord,
            wy.round().clamp(-max, max) as Coord,
        )
    }

    /// Moves the view so the content follows a drag of `dx, dy` pixels.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64) {
        self.center.0 -= dx / self.scale;
        self.center.1 += dy / self.scale;
    }

    /// Multiplies the zoom by `factor`, keeping the world point under screen `(x, y)` fixed.
    pub fn zoom_at(&mut self, x: f64, y: f64, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.screen_to_world_f(x, y);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.screen_to_world_f(x, y);
        self.center.0 += before.0 - after.0;
        self.center.1 += before.1 - after.1;
    }

    /// Fits `bbox` in the viewport with a pixel margin.
    pub fn fit(&mut self, bbox: &BoundingBox, margin_px: f64) {
        let (w, h) = (bbox.width().max(1) as f64, bbox.height().max(1) as f64);
        let sx = (self.width - 2.0 * margin_px).max(1.0) / w;
        let sy = (self.height - 2.0 * margin_px).max(1.0) / h;
        self.scale = sx.min(sy).clamp(MIN_SCALE, MAX_SCALE);
        let c = bbox.center();
        self.center = (c.x as f64, c.y as f64);
    }

    /// World rectangle currently visible.
    pub fn visible_world(&self) -> BoundingBox {
        BoundingBox::new(
            self.screen_to_world(0.0, self.height),
            self.screen_to_world(self.width, 0.0),
        )
    }

    /// Length in nm of `px` pixels at the current zoom.
    pub fn px_to_nm(&self, px: f64) -> f64 {
        px / self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_orientation() {
        let v = View {
            center: (1000.0, 2000.0),
            scale: 0.01,
            width: 800.0,
            height: 600.0,
        };
        assert_eq!(v.world_to_screen(Point::new(1000, 2000)), (400.0, 300.0));
        // +Y is up on screen.
        assert!(v.world_to_screen(Point::new(1000, 3000)).1 < 300.0);
        let p = Point::new(123_456, -98_765);
        let (sx, sy) = v.world_to_screen(p);
        assert_eq!(v.screen_to_world(sx, sy), p);
    }

    #[test]
    fn zoom_keeps_cursor_point_fixed() {
        let mut v = View::new(800.0, 600.0);
        let before = v.screen_to_world_f(100.0, 50.0);
        v.zoom_at(100.0, 50.0, 3.0);
        let after = v.screen_to_world_f(100.0, 50.0);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
        assert!((v.scale - 3.0e-5).abs() < 1e-12);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = View::new(100.0, 100.0);
        v.zoom_at(0.0, 0.0, 1.0e12);
        assert_eq!(v.scale, MAX_SCALE);
        v.zoom_at(0.0, 0.0, 1.0e-30);
        assert_eq!(v.scale, MIN_SCALE);
        v.zoom_at(0.0, 0.0, f64::NAN);
        assert_eq!(v.scale, MIN_SCALE);
    }

    #[test]
    fn pan_moves_content_with_the_pointer() {
        let mut v = View::new(800.0, 600.0);
        let p = Point::new(5_000, 5_000);
        let before = v.world_to_screen(p);
        v.pan_pixels(30.0, -20.0);
        let after = v.world_to_screen(p);
        assert!(
            (after.0 - before.0 - 30.0).abs() < 1e-9 && (after.1 - before.1 + 20.0).abs() < 1e-9
        );
    }

    #[test]
    fn fit_contains_bbox() {
        let mut v = View::new(800.0, 600.0);
        let bb = BoundingBox::new(Point::new(0, 0), Point::new(100_000_000, 50_000_000));
        v.fit(&bb, 20.0);
        let vis = v.visible_world();
        assert!(vis.contains_box(&bb));
    }
}
