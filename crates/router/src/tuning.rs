//! Length tuning: replace a straight track by a meander that adds a requested length.
//!
//! Two styles, both one-sided from the original line:
//!
//! * **Serpentine** — rectangular wave: each period goes out by `h`, across `pitch`, back by `h`
//!   and across `pitch`; it adds exactly `2h` per period;
//! * **Accordion** — 45° trapezoids: rise over `h`, flat `pitch`, fall over `h`, flat `pitch`; it
//!   adds `2h(√2 − 1)` per period and has no 90° corners.
//!
//! Parallel runs are `pitch` apart centre to centre, and `pitch` is never allowed below
//! `width + max(clearance, width)`, so the meander does not couple with itself more than the
//! rules allow. The amplitude `h` never exceeds the requested maximum; when the segment is too
//! short to hold the whole addition the plan is **partial** and says by how much.

use buturecad_geometry::{Coord, Point, Segment};
use std::f64::consts::SQRT_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuneStyle {
    Serpentine,
    Accordion,
}

impl TuneStyle {
    pub fn id(self) -> &'static str {
        match self {
            TuneStyle::Serpentine => "serpentine",
            TuneStyle::Accordion => "accordion",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "serpentine" => Some(TuneStyle::Serpentine),
            "accordion" => Some(TuneStyle::Accordion),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TuneParams {
    pub style: TuneStyle,
    /// Largest height of a bump.
    pub amplitude: Coord,
    /// Requested centre-to-centre distance of parallel runs (raised to the minimum if smaller).
    pub pitch: Coord,
    pub width: Coord,
    pub clearance: Coord,
}

impl TuneParams {
    /// The pitch actually used.
    pub fn effective_pitch(&self) -> Coord {
        self.pitch.max(self.width + self.clearance.max(self.width))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TunePlan {
    /// Replacement for the whole segment: first point is its start, last its end.
    pub path: Vec<Point>,
    /// Length added by the meander (nm).
    pub added: f64,
    pub wanted: f64,
    pub periods: usize,
}

impl TunePlan {
    /// The addition fell short of the request (segment too short or amplitude too small).
    pub fn is_partial(&self) -> bool {
        self.wanted - self.added > 1000.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuneError {
    /// Nothing to add (already long enough).
    NothingToAdd,
    /// The request is smaller than the smallest bump that can be drawn.
    TooSmall,
    /// The segment cannot hold a single period.
    SegmentTooShort,
}

impl std::fmt::Display for TuneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuneError::NothingToAdd => write!(f, "the net is already long enough"),
            TuneError::TooSmall => {
                write!(f, "the missing length is smaller than the smallest bump")
            }
            TuneError::SegmentTooShort => write!(f, "this segment is too short for a meander"),
        }
    }
}

impl std::error::Error for TuneError {}

fn period_extra(style: TuneStyle, h: f64) -> f64 {
    match style {
        TuneStyle::Serpentine => 2.0 * h,
        TuneStyle::Accordion => 2.0 * h * (SQRT_2 - 1.0),
    }
}

fn period_len(style: TuneStyle, h: f64, p: f64) -> f64 {
    match style {
        TuneStyle::Serpentine => 2.0 * p,
        TuneStyle::Accordion => 2.0 * h + 2.0 * p,
    }
}

/// Plans a meander on `seg` adding `wanted` nm, centred on the point of the segment nearest to
/// `at`, bulging to the left of a→b when `left` and to the right otherwise.
pub fn plan_meander(
    seg: Segment,
    at: Point,
    left: bool,
    wanted: f64,
    p: &TuneParams,
) -> Result<TunePlan, TuneError> {
    if wanted <= 0.0 {
        return Err(TuneError::NothingToAdd);
    }
    let len = seg.length();
    let pitch = p.effective_pitch() as f64;
    let amp = p.amplitude as f64;
    let margin = pitch;
    let room = len - 2.0 * margin;
    // A bump is at least one pitch high, so the runs of a bump keep the pitch from each other.
    let min_h = pitch;
    if room < period_len(p.style, min_h, pitch) {
        return Err(TuneError::SegmentTooShort);
    }
    // Number of periods and height.
    let mut k = (wanted / period_extra(p.style, amp)).ceil().max(1.0);
    let mut h = wanted / (k * period_extra(p.style, 1.0));
    if h < min_h && k > 1.0 {
        k = (wanted / period_extra(p.style, min_h)).floor().max(1.0);
        h = wanted / (k * period_extra(p.style, 1.0));
    }
    if h < min_h {
        return Err(TuneError::TooSmall);
    }
    // Fit into the room; otherwise as many full-amplitude periods as fit.
    let fits = |k: f64, h: f64| k * period_len(p.style, h, pitch) <= room;
    if !fits(k, h) {
        let per = period_len(p.style, amp, pitch);
        k = (room / per).floor();
        if k < 1.0 {
            return Err(TuneError::SegmentTooShort);
        }
        h = amp;
    }
    let periods = k as usize;
    let consumed = k * period_len(p.style, h, pitch);
    // Centre on the anchor, keep inside the margins.
    let (dx, dy) = ((seg.b.x - seg.a.x) as f64, (seg.b.y - seg.a.y) as f64);
    let (ux, uy) = (dx / len, dy / len);
    let t = ((at.x - seg.a.x) as f64 * ux + (at.y - seg.a.y) as f64 * uy).clamp(0.0, len);
    let s0 = (t - consumed / 2.0).clamp(margin, len - margin - consumed);
    let side = if left { 1.0 } else { -1.0 };
    let (nx, ny) = (-uy * side, ux * side);
    let world = |s: f64, y: f64| {
        Point::new(
            (seg.a.x as f64 + ux * s + nx * y).round() as i64,
            (seg.a.y as f64 + uy * s + ny * y).round() as i64,
        )
    };
    let mut path = vec![seg.a];
    let mut x = s0;
    for _ in 0..periods {
        match p.style {
            TuneStyle::Serpentine => {
                path.extend([
                    world(x, 0.0),
                    world(x, h),
                    world(x + pitch, h),
                    world(x + pitch, 0.0),
                ]);
                x += 2.0 * pitch;
            }
            TuneStyle::Accordion => {
                path.extend([
                    world(x, 0.0),
                    world(x + h, h),
                    world(x + h + pitch, h),
                    world(x + 2.0 * h + pitch, 0.0),
                ]);
                x += 2.0 * h + 2.0 * pitch;
            }
        }
    }
    path.push(seg.b);
    path.dedup();
    let total: f64 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    Ok(TunePlan {
        path,
        added: total - len,
        wanted,
        periods,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const MM: i64 = 1_000_000;

    fn params(style: TuneStyle) -> TuneParams {
        TuneParams {
            style,
            amplitude: 3 * MM,
            pitch: 600_000,
            width: 200_000,
            clearance: 200_000,
        }
    }

    fn seg(len_mm: i64) -> Segment {
        Segment::new(Point::new(0, 0), Point::new(len_mm * MM, 0))
    }

    #[test]
    fn the_added_length_matches_the_request_for_both_styles() {
        for style in [TuneStyle::Serpentine, TuneStyle::Accordion] {
            for wanted in [2.0, 7.5, 20.0] {
                let plan = plan_meander(
                    seg(100),
                    Point::new(50 * MM, 0),
                    true,
                    wanted * 1e6,
                    &params(style),
                )
                .unwrap();
                assert!(!plan.is_partial(), "{style:?} {wanted}: {plan:?}");
                assert!(
                    (plan.added - wanted * 1e6).abs() < 50.0,
                    "{style:?} {wanted}: added {}",
                    plan.added
                );
                assert_eq!(plan.path.first(), Some(&Point::new(0, 0)));
                assert_eq!(plan.path.last(), Some(&Point::new(100 * MM, 0)));
            }
        }
    }

    #[test]
    fn bumps_never_exceed_the_amplitude_and_stay_on_the_chosen_side() {
        for (left, sign) in [(true, 1), (false, -1)] {
            let plan = plan_meander(
                seg(80),
                Point::new(40 * MM, 0),
                left,
                30.0e6,
                &params(TuneStyle::Serpentine),
            )
            .unwrap();
            let max_y = plan.path.iter().map(|p| p.y * sign).max().unwrap();
            let min_y = plan.path.iter().map(|p| p.y * sign).min().unwrap();
            assert!(max_y <= 3 * MM + 2 && min_y >= 0, "{min_y}..{max_y}");
        }
    }

    #[test]
    fn parallel_runs_keep_the_minimum_pitch() {
        let plan = plan_meander(
            seg(60),
            Point::new(30 * MM, 0),
            true,
            12.0e6,
            &params(TuneStyle::Serpentine),
        )
        .unwrap();
        let segs: Vec<Segment> = plan
            .path
            .windows(2)
            .map(|w| Segment::new(w[0], w[1]))
            .collect();
        for (i, a) in segs.iter().enumerate() {
            for b in segs.iter().skip(i + 2) {
                let d = a.distance_to_segment(b) - 200_000.0;
                assert!(d >= 200_000.0 - 2.0, "legs only {d} nm apart");
            }
        }
    }

    #[test]
    fn a_short_segment_gives_a_partial_plan_or_an_error() {
        let plan = plan_meander(
            seg(8),
            Point::new(4 * MM, 0),
            true,
            40.0e6,
            &params(TuneStyle::Serpentine),
        )
        .unwrap();
        assert!(plan.is_partial() && plan.added > 0.0);
        assert_eq!(
            plan_meander(
                seg(1),
                Point::new(MM / 2, 0),
                true,
                1.0e6,
                &params(TuneStyle::Serpentine)
            ),
            Err(TuneError::SegmentTooShort)
        );
        assert_eq!(
            plan_meander(
                seg(60),
                Point::new(0, 0),
                true,
                -1.0,
                &params(TuneStyle::Accordion)
            ),
            Err(TuneError::NothingToAdd)
        );
        assert_eq!(
            plan_meander(
                seg(60),
                Point::new(0, 0),
                true,
                100_000.0,
                &params(TuneStyle::Serpentine)
            ),
            Err(TuneError::TooSmall)
        );
    }

    #[test]
    fn the_meander_follows_a_diagonal_segment() {
        let s = Segment::new(Point::new(0, 0), Point::new(40 * MM, 40 * MM));
        let plan = plan_meander(
            s,
            Point::new(20 * MM, 20 * MM),
            false,
            10.0e6,
            &params(TuneStyle::Accordion),
        )
        .unwrap();
        assert!((plan.added - 10.0e6).abs() < 100.0, "{}", plan.added);
    }
}
