//! Adaptive grid and grid snapping.

use crate::view::View;
use buturecad_geometry::{Coord, Point};

/// Minimum on-screen distance between drawn grid lines.
pub const MIN_GRID_PX: f64 = 8.0;
/// Every `MAJOR_EVERY`-th drawn line is emphasised.
pub const MAJOR_EVERY: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridStyle {
    Lines,
    Dots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSettings {
    /// User grid pitch in nm (also the snap pitch).
    pub spacing: Coord,
    pub visible: bool,
    pub style: GridStyle,
}

impl GridSettings {
    pub fn new(spacing: Coord) -> Self {
        Self {
            spacing: spacing.max(1),
            visible: true,
            style: GridStyle::Lines,
        }
    }
}

/// Drawn grid pitch: the user grid multiplied by 2, 5, 10, 20, 50 … until lines are at least
/// [`MIN_GRID_PX`] apart. The result is always a multiple of the user grid, so drawn lines
/// coincide with snap points.
pub fn adaptive_spacing(base: Coord, scale: f64) -> Coord {
    let base = base.max(1);
    let mut step = 0;
    loop {
        let mult = one_two_five(step);
        if (base.saturating_mul(mult)) as f64 * scale >= MIN_GRID_PX || mult >= (1 << 40) {
            return base.saturating_mul(mult);
        }
        step += 1;
    }
}

/// The n-th element of 1, 2, 5, 10, 20, 50, 100, …
fn one_two_five(n: usize) -> i64 {
    let decade = 10i64.saturating_pow((n / 3) as u32);
    decade.saturating_mul([1, 2, 5][n % 3])
}

/// Nearest grid node (ties round up, i.e. towards +∞).
pub fn snap_to_grid(p: Point, spacing: Coord) -> Point {
    let s = spacing.max(1);
    let snap = |v: Coord| {
        let r = v.rem_euclid(s);
        let base = v - r;
        if r * 2 >= s {
            base + s
        } else {
            base
        }
    };
    Point::new(snap(p.x), snap(p.y))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridLineKind {
    Minor,
    Major,
}

/// A grid line in screen space (pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    pub vertical: bool,
    /// x (vertical lines) or y (horizontal lines) in pixels.
    pub position: f64,
    pub kind: GridLineKind,
}

/// Grid lines visible in `view`. Bounded by `width/8 + height/8` entries thanks to
/// [`adaptive_spacing`].
pub fn grid_lines(view: &View, settings: &GridSettings) -> Vec<GridLine> {
    if !settings.visible {
        return Vec::new();
    }
    let step = adaptive_spacing(settings.spacing, view.scale);
    let vis = view.visible_world();
    let mut out = Vec::new();
    let first = |min: Coord| min.div_euclid(step) * step;
    let kind = |index: i64| {
        if index.rem_euclid(MAJOR_EVERY) == 0 {
            GridLineKind::Major
        } else {
            GridLineKind::Minor
        }
    };
    let mut x = first(vis.min.x);
    while x <= vis.max.x {
        let sx = view.world_to_screen(Point::new(x, 0)).0;
        out.push(GridLine {
            vertical: true,
            position: sx,
            kind: kind(x / step),
        });
        x += step;
    }
    let mut y = first(vis.min.y);
    while y <= vis.max.y {
        let sy = view.world_to_screen(Point::new(0, y)).1;
        out.push(GridLine {
            vertical: false,
            position: sy,
            kind: kind(y / step),
        });
        y += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_is_1_2_5() {
        let v: Vec<i64> = (0..7).map(one_two_five).collect();
        assert_eq!(v, vec![1, 2, 5, 10, 20, 50, 100]);
    }

    #[test]
    fn spacing_adapts_with_zoom() {
        let base = 1_270_000; // 50 mil
        assert_eq!(adaptive_spacing(base, 1.0e-5), base); // 12.7 px: fine as is
        let coarse = adaptive_spacing(base, 1.0e-6); // would be 1.27 px
        assert!(coarse as f64 * 1.0e-6 >= MIN_GRID_PX);
        assert_eq!(
            coarse % base,
            0,
            "drawn grid stays aligned with the snap grid"
        );
        // Not needlessly coarse: the previous step would have been too small.
        assert!(coarse == base * 10 || coarse == base * 20 || coarse == base * 5);
    }

    #[test]
    fn snapping_rounds_to_nearest_node() {
        assert_eq!(snap_to_grid(Point::new(14, 26), 10), Point::new(10, 30));
        assert_eq!(snap_to_grid(Point::new(15, -15), 10), Point::new(20, -10));
        assert_eq!(snap_to_grid(Point::new(-14, -26), 10), Point::new(-10, -30));
        assert_eq!(snap_to_grid(Point::new(0, 5), 10), Point::new(0, 10));
    }

    #[test]
    fn grid_line_count_is_bounded() {
        let mut v = View::new(1200.0, 800.0);
        for scale in [1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 0.05] {
            v.scale = scale;
            let lines = grid_lines(&v, &GridSettings::new(1_270_000));
            assert!(
                lines.len() <= (1200 / 8 + 800 / 8) + 8,
                "{} lines at scale {scale}",
                lines.len()
            );
        }
    }

    #[test]
    fn hidden_grid_has_no_lines() {
        let mut g = GridSettings::new(1000);
        g.visible = false;
        assert!(grid_lines(&View::new(100.0, 100.0), &g).is_empty());
    }
}
