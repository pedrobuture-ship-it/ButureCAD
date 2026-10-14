//! Manufacturing preflight: what must be true before production files are trusted. It reports;
//! it never changes the board.

use crate::model::{FabInput, Feature, Mount, Pt};
use std::collections::BTreeSet;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Blocks `production.zip`.
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

impl Issue {
    pub fn new(severity: Severity, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn count(&self, s: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity == s).count()
    }

    pub fn has_errors(&self) -> bool {
        self.count(Severity::Error) > 0
    }

    pub fn text(&self) -> String {
        let mut o = String::from("MANUFACTURING PREFLIGHT\n\n");
        for i in &self.issues {
            let tag = match i.severity {
                Severity::Error => "ERROR  ",
                Severity::Warning => "WARNING",
                Severity::Info => "NOTE   ",
            };
            let _ = writeln!(o, "{tag} [{}] {}", i.code, i.message);
        }
        let _ = writeln!(
            o,
            "\n{} error(s), {} warning(s), {} note(s).",
            self.count(Severity::Error),
            self.count(Severity::Warning),
            self.count(Severity::Info)
        );
        o
    }
}

/// Even-odd point in polygon, exact in integers (boundary counts as inside).
pub fn inside(poly: &[Pt], p: Pt) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let cross = i128::from(b.x - a.x) * i128::from(p.y - a.y)
            - i128::from(b.y - a.y) * i128::from(p.x - a.x);
        if cross == 0
            && p.x >= a.x.min(b.x)
            && p.x <= a.x.max(b.x)
            && p.y >= a.y.min(b.y)
            && p.y <= a.y.max(b.y)
        {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) {
            // x of the edge at p.y compared with p.x, without division.
            let lhs = i128::from(p.x - a.x) * i128::from(b.y - a.y);
            let rhs = i128::from(b.x - a.x) * i128::from(p.y - a.y);
            if (b.y > a.y && lhs < rhs) || (b.y < a.y && lhs > rhs) {
                c = !c;
            }
        }
    }
    c
}

pub fn run(input: &FabInput) -> Report {
    use Severity::{Error, Info, Warning};
    let mut r = Report::default();
    let mut add = |s, c, m: String| r.issues.push(Issue::new(s, c, m));

    // Board outline.
    match input.outline.first() {
        None => add(
            Error,
            "outline",
            "The board has no closed outline (Edge.Cuts).".into(),
        ),
        Some(o) if o.len() < 3 => add(
            Error,
            "outline",
            "The board outline has fewer than 3 vertices.".into(),
        ),
        Some(_) => {}
    }
    // DRC run right before the export.
    if input.drc.errors > 0 {
        add(
            Error,
            "drc",
            format!(
                "DRC reports {} error(s); fix them before manufacturing.",
                input.drc.errors
            ),
        );
    }
    if input.drc.warnings > 0 {
        add(
            Warning,
            "drc",
            format!("DRC reports {} warning(s).", input.drc.warnings),
        );
    }
    // Layers.
    let mut names = BTreeSet::new();
    for l in &input.layers {
        if !names.insert(l.file.as_str()) {
            add(
                Error,
                "layers",
                format!("Two layers use the file name '{}'.", l.file),
            );
        }
        if l.function.starts_with("Copper") && l.features.is_empty() {
            add(
                Warning,
                "empty_copper",
                format!("Copper layer '{}' has no copper.", l.file),
            );
        }
    }
    let copper_any = input
        .layers
        .iter()
        .any(|l| l.function.starts_with("Copper") && !l.features.is_empty());
    if !copper_any {
        add(
            Error,
            "no_copper",
            "No copper features were found on any layer.".into(),
        );
    }
    if !input.layers.iter().any(|l| {
        l.function.starts_with("Profile")
            && l.features.iter().any(|f| matches!(f, Feature::Draw { .. }))
    }) {
        add(
            Error,
            "no_profile",
            "The board profile layer is empty.".into(),
        );
    }
    // Holes.
    let outer = input.outline.first().filter(|o| o.len() >= 3);
    let mut outside = 0;
    let mut small = 0;
    for h in &input.holes {
        if h.diameter <= 0 {
            add(Error, "drill", "A hole has a non-positive diameter.".into());
        } else if h.diameter < input.capabilities.min_drill {
            small += 1;
        }
        if let Some(o) = outer {
            if !inside(o, h.at) || h.slot_to.is_some_and(|b| !inside(o, b)) {
                outside += 1;
            }
        }
    }
    if outside > 0 {
        add(
            Error,
            "drill_outside",
            format!("{outside} hole(s) lie outside the board outline."),
        );
    }
    if small > 0 {
        add(
            Warning,
            "small_drill",
            format!(
                "{small} hole(s) are smaller than {} mm (generic limit; check your fabricator).",
                crate::fmt_mm(input.capabilities.min_drill)
            ),
        );
    }
    if let Some(w) = input.min_track_width {
        if w < input.capabilities.min_track_width {
            add(
                Warning,
                "thin_track",
                format!(
                    "The thinnest track is {} mm, below the generic {} mm limit.",
                    crate::fmt_mm(w),
                    crate::fmt_mm(input.capabilities.min_track_width)
                ),
            );
        }
    }
    // Components.
    if input.parts.is_empty() {
        add(
            Warning,
            "no_parts",
            "No components: the BOM and pick-and-place files are empty.".into(),
        );
    }
    let mut refs = BTreeSet::new();
    for p in &input.parts {
        if p.reference.trim().is_empty() {
            add(
                Error,
                "reference",
                format!("A '{}' footprint has no reference.", p.footprint),
            );
        } else if !refs.insert(p.reference.as_str()) {
            add(
                Error,
                "reference",
                format!("Duplicate reference '{}'.", p.reference),
            );
        }
    }
    let no_value: Vec<&str> = input
        .parts
        .iter()
        .filter(|p| p.in_bom && p.value.trim().is_empty())
        .map(|p| p.reference.as_str())
        .collect();
    if !no_value.is_empty() {
        add(
            Warning,
            "no_value",
            format!(
                "{} component(s) have no value in the BOM: {}.",
                no_value.len(),
                no_value.join(", ")
            ),
        );
    }
    let unspec = input
        .parts
        .iter()
        .filter(|p| p.mount == Mount::Unspecified && p.in_position_file)
        .count();
    if unspec > 0 {
        add(
            Warning,
            "mount_unspecified",
            format!("{unspec} footprint(s) have no mount type; they are included in pick-and-place as if SMD."),
        );
    }
    r.issues.extend(input.extra_issues.iter().cloned());
    r.issues.push(Issue::new(
        Info,
        "not_a_signoff",
        "A successful export is not fabrication sign-off: compare with your fabricator's capabilities and review the Gerbers.",
    ));
    r.issues.sort_by_key(|i| i.severity);
    r
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Pt {
        Pt { x, y }
    }

    #[test]
    fn point_in_polygon_is_exact() {
        let sq = [p(0, 0), p(10, 0), p(10, 10), p(0, 10)];
        assert!(inside(&sq, p(5, 5)) && inside(&sq, p(0, 5)) && inside(&sq, p(10, 10)));
        assert!(!inside(&sq, p(11, 5)) && !inside(&sq, p(-1, -1)) && !inside(&sq, p(5, 11)));
        let l = [p(0, 0), p(10, 0), p(10, 5), p(5, 5), p(5, 10), p(0, 10)];
        assert!(inside(&l, p(2, 8)) && !inside(&l, p(8, 8)));
    }
}
