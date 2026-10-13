//! Geometry of interactive routing: how a pointer position becomes track vertices.
//!
//! Pure, integer-exact functions; the editor layers snapping, nets and the Command Engine on
//! top. Obstacle awareness (walk-around, push & shove) arrives in roadmap 21–22: nothing here
//! avoids other copper.

use buturecad_geometry::{Coord, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngleMode {
    /// Horizontal/vertical legs joined by a 45° leg.
    #[default]
    Deg45,
    /// Horizontal and vertical legs only.
    Deg90,
    /// A single straight segment at any angle.
    Free,
}

impl AngleMode {
    pub fn id(self) -> &'static str {
        match self {
            AngleMode::Deg45 => "45",
            AngleMode::Deg90 => "90",
            AngleMode::Free => "free",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "45" => AngleMode::Deg45,
            "90" => AngleMode::Deg90,
            "free" => AngleMode::Free,
            _ => return None,
        })
    }
}

/// Vertices of the path from `a` to `b` (including both ends, no duplicates, no collinear
/// interior points). `flip` picks the other of the two possible elbows (what Space does).
pub fn path(a: Point, b: Point, mode: AngleMode, flip: bool) -> Vec<Point> {
    if a == b {
        return vec![a];
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let mid = match mode {
        AngleMode::Free => None,
        AngleMode::Deg90 => {
            if dx == 0 || dy == 0 {
                None
            } else if flip {
                Some(Point::new(a.x, b.y))
            } else {
                Some(Point::new(b.x, a.y))
            }
        }
        AngleMode::Deg45 => {
            let (ax, ay) = (dx.abs(), dy.abs());
            if dx == 0 || dy == 0 || ax == ay {
                None
            } else {
                let (sx, sy) = (dx.signum(), dy.signum());
                let diag = ax.min(ay);
                // `flip == false`: the straight leg runs first along the longer axis, then the
                // 45° leg; `flip == true`: the 45° leg first.
                let straight_first = !flip;
                if ax > ay {
                    let straight = ax - ay;
                    if straight_first {
                        Some(Point::new(a.x + sx * straight, a.y))
                    } else {
                        Some(Point::new(a.x + sx * diag, a.y + sy * diag))
                    }
                } else {
                    let straight = ay - ax;
                    if straight_first {
                        Some(Point::new(a.x, a.y + sy * straight))
                    } else {
                        Some(Point::new(a.x + sx * diag, a.y + sy * diag))
                    }
                }
            }
        }
    };
    match mid {
        Some(m) => vec![a, m, b],
        None => vec![a, b],
    }
}

/// Removes zero-length steps and merges consecutive collinear segments.
pub fn simplify(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() == Some(p) {
            continue;
        }
        while out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let cross = i128::from(b.x - a.x) * i128::from(p.y - b.y)
                - i128::from(b.y - a.y) * i128::from(p.x - b.x);
            let dot = i128::from(b.x - a.x) * i128::from(p.x - b.x)
                + i128::from(b.y - a.y) * i128::from(p.y - b.y);
            if cross == 0 && dot > 0 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(*p);
    }
    out
}

/// Total length of a polyline in nm.
pub fn length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Default track width (0.25 mm) and clearance (0.2 mm) until net classes exist (roadmap 20).
pub const DEFAULT_WIDTH: Coord = 250_000;
pub const DEFAULT_CLEARANCE: Coord = 200_000;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn ninety_degree_paths_are_two_orthogonal_legs() {
        assert_eq!(
            path(p(0, 0), p(10, 4), AngleMode::Deg90, false),
            [p(0, 0), p(10, 0), p(10, 4)]
        );
        assert_eq!(
            path(p(0, 0), p(10, 4), AngleMode::Deg90, true),
            [p(0, 0), p(0, 4), p(10, 4)]
        );
        assert_eq!(
            path(p(0, 0), p(10, 0), AngleMode::Deg90, false),
            [p(0, 0), p(10, 0)]
        );
    }

    #[test]
    fn forty_five_paths_use_exactly_one_diagonal_leg() {
        for (a, b) in [
            (p(0, 0), p(10, 4)),
            (p(0, 0), p(4, 10)),
            (p(5, 5), p(-9, 2)),
            (p(0, 0), p(-3, -12)),
        ] {
            for flip in [false, true] {
                let v = path(a, b, AngleMode::Deg45, flip);
                assert_eq!((v[0], *v.last().unwrap()), (a, b));
                assert_eq!(v.len(), 3);
                let mut diagonals = 0;
                let mut orth = 0;
                for w in v.windows(2) {
                    let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
                    if dx == dy {
                        diagonals += 1;
                    } else if dx == 0 || dy == 0 {
                        orth += 1;
                    }
                }
                assert_eq!((diagonals, orth), (1, 1), "{a:?}→{b:?} flip={flip}: {v:?}");
            }
        }
        // Pure diagonals and axis-aligned moves are single segments.
        assert_eq!(
            path(p(0, 0), p(7, 7), AngleMode::Deg45, false),
            [p(0, 0), p(7, 7)]
        );
        assert_eq!(
            path(p(0, 0), p(0, -7), AngleMode::Deg45, true),
            [p(0, 0), p(0, -7)]
        );
        // The two elbows differ and have equal length.
        let (a, b) = (
            path(p(0, 0), p(10, 4), AngleMode::Deg45, false),
            path(p(0, 0), p(10, 4), AngleMode::Deg45, true),
        );
        assert_ne!(a, b);
        assert_eq!(a[1], p(6, 0));
        assert_eq!(b[1], p(4, 4));
    }

    #[test]
    fn free_angle_is_one_segment_and_degenerate_input_is_safe() {
        assert_eq!(
            path(p(0, 0), p(7, 3), AngleMode::Free, false),
            [p(0, 0), p(7, 3)]
        );
        assert_eq!(path(p(3, 3), p(3, 3), AngleMode::Deg45, false), [p(3, 3)]);
    }

    #[test]
    fn simplify_drops_repeats_and_collinear_points() {
        assert_eq!(
            simplify(&[p(0, 0), p(0, 0), p(5, 0), p(10, 0), p(10, 5)]),
            [p(0, 0), p(10, 0), p(10, 5)]
        );
        // A reversal is not collinear-forward: both ends are kept.
        assert_eq!(simplify(&[p(0, 0), p(10, 0), p(5, 0)]).len(), 3);
        assert_eq!(length(&[p(0, 0), p(3, 4)]), 5.0);
        assert_eq!(AngleMode::parse("45"), Some(AngleMode::Deg45));
        assert_eq!(AngleMode::parse("7"), None);
    }
}
