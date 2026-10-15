//! Footprint suggestions for a placed part (stage 53), the way KiCad's footprint chooser filters
//! them: by the symbol's **footprint filters** (glob patterns such as `Chip_*`, `SOIC-8*`) and by
//! **pins vs pads** (a footprint fits only if it has a pad for every pin of the symbol).
//!
//! Pure functions over plain data, so the desktop app, scripts and agents get the same answer.

use buturecad_pcb::Footprint;
use buturecad_schematic::Symbol;
use std::collections::BTreeSet;

/// Why a footprint is suggested, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// Matches one of the symbol's filters and has a pad for every pin.
    Filter,
    /// Its pad numbers are exactly the symbol's pin numbers.
    ExactPads,
}

impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::Filter => "matches the symbol's footprint filter",
            Reason::ExactPads => "pads match the symbol's pins",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// `Library:Name` as stored in the footprint.
    pub id: String,
    pub reason: Reason,
    pub pad_count: usize,
}

/// `*` (any run) and `?` (one character) wildcards, case-insensitive.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn pad_numbers(f: &Footprint) -> BTreeSet<&str> {
    f.pads
        .iter()
        .map(|p| p.number.as_str())
        .filter(|n| !n.is_empty())
        .collect()
}

/// Footprints that fit `symbol`, best first: filter matches, then exact pad matches (fewest pads,
/// then name). At most `limit`. A footprint missing a pad for any pin never appears.
pub fn suggest(symbol: &Symbol, footprints: &[Footprint], limit: usize) -> Vec<Suggestion> {
    let pins: BTreeSet<&str> = symbol.pins.iter().map(|p| p.number.as_str()).collect();
    if pins.is_empty() {
        return Vec::new();
    }
    let filters = &symbol.meta.footprint_filters;
    let mut out: Vec<Suggestion> = footprints
        .iter()
        .filter_map(|f| {
            let pads = pad_numbers(f);
            if !pins.is_subset(&pads) {
                return None;
            }
            let short = f.name.rsplit(':').next().unwrap_or(&f.name);
            let by_filter = filters.iter().any(|g| glob(g, short) || glob(g, &f.name));
            let reason = if by_filter {
                Reason::Filter
            } else if pads == pins {
                Reason::ExactPads
            } else {
                return None;
            };
            Some(Suggestion {
                id: f.name.clone(),
                reason,
                pad_count: f.pads.len(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.reason
            .cmp(&b.reason)
            .then(a.pad_count.cmp(&b.pad_count))
            .then(a.id.cmp(&b.id))
    });
    out.truncate(limit);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::standard;

    fn sym(name: &str) -> Symbol {
        standard::symbols()
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
    }

    fn ids(s: &[Suggestion]) -> Vec<&str> {
        s.iter().map(|x| x.id.as_str()).collect()
    }

    #[test]
    fn globs() {
        assert!(glob("Chip_*", "Chip_0603") && glob("chip_*", "CHIP_0603"));
        assert!(glob("SOIC-8*", "SOIC-8_3.9mm_P1.27mm") && !glob("SOIC-8*", "SOIC-14_3.9mm"));
        assert!(glob("R_0?03", "R_0603") && !glob("R_0?03", "R_06003"));
        assert!(glob("*", "x") && glob("a*b*c", "aXbYc") && !glob("a*b*c", "aXbY"));
    }

    #[test]
    fn a_resistor_is_offered_chip_footprints_first() {
        let fps = standard::footprints();
        let mut res = buturecad_schematic::builtin::symbols()
            .into_iter()
            .find(|s| s.name == "Buture:R")
            .unwrap();
        res.meta.footprint_filters = vec!["Chip_*".into()];
        let s = suggest(&res, &fps, 100);
        let first: Vec<&str> = ids(&s).into_iter().take(5).collect();
        assert!(first.iter().all(|i| i.contains("Chip_")), "{first:?}");
        assert!(s.iter().any(|x| x.id.ends_with("Chip_0603")));
        // Other 2-pad footprints still appear, after the filter matches.
        let pos_chip = s.iter().rposition(|x| x.reason == Reason::Filter).unwrap();
        let pos_other = s.iter().position(|x| x.reason == Reason::ExactPads);
        assert!(pos_other.is_none_or(|p| p > pos_chip));
    }

    #[test]
    fn an_ic_gets_only_packages_that_have_all_its_pins() {
        let fps = standard::footprints();
        let s = suggest(&sym("Std:OpAmp_Dual_8"), &fps, 100);
        assert!(!s.is_empty());
        for x in &s {
            let f = fps.iter().find(|f| f.name == x.id).unwrap();
            for pin in &sym("Std:OpAmp_Dual_8").pins {
                assert!(
                    f.pads.iter().any(|p| p.number == pin.number),
                    "{} lacks pad {}",
                    x.id,
                    pin.number
                );
            }
        }
        let names = ids(&s);
        assert!(
            names.iter().any(|n| n.contains("SOIC-8"))
                && names.iter().any(|n| n.contains("TSSOP-8"))
                && names.iter().any(|n| n.contains("MSOP-8"))
        );
        assert!(
            !names
                .iter()
                .any(|n| n.contains("SOT-23") || n.contains("Chip_")),
            "{names:?}"
        );
    }

    #[test]
    fn a_connector_gets_its_header_and_nothing_with_too_few_pads() {
        let fps = standard::footprints();
        let s = suggest(&sym("Std:Conn_01x06"), &fps, 20);
        assert_eq!(
            s.first().map(|x| x.id.as_str()),
            Some("Std:PinHeader_1x06_P2.54mm"),
            "{s:?}"
        );
        assert!(s.iter().all(|x| x.pad_count >= 6));
    }

    #[test]
    fn symbols_without_pins_get_nothing_and_limit_is_respected() {
        let mut empty = sym("Std:Conn_01x02");
        empty.pins.clear();
        assert!(suggest(&empty, &standard::footprints(), 10).is_empty());
        assert!(suggest(&sym("Std:Conn_01x02"), &standard::footprints(), 3).len() <= 3);
    }
}
