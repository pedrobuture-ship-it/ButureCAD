//! Excellon drill files (metric, decimal coordinates). One tool per diameter; slots use `G85`.

use crate::model::Hole;
use crate::{fmt_mm, FabError};
use std::collections::BTreeSet;
use std::fmt::Write;

/// Writes the plated or non-plated holes. Returns `None` when there are none.
pub fn write(holes: &[Hole], plated: bool, copper_layers: u8) -> Result<Option<String>, FabError> {
    let mut hs: Vec<&Hole> = holes.iter().filter(|h| h.plated == plated).collect();
    if hs.is_empty() {
        return Ok(None);
    }
    if hs.iter().any(|h| h.diameter <= 0) {
        return Err(FabError::Invalid(
            "a hole has a non-positive diameter".into(),
        ));
    }
    hs.sort_by_key(|h| (h.diameter, h.slot_to.is_some(), h.at.y, h.at.x));
    let diams: Vec<i64> = hs
        .iter()
        .map(|h| h.diameter)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut o = String::new();
    let func = if plated { "Plated" } else { "NonPlated" };
    let kind = if plated { "PTH" } else { "NPTH" };
    let _ = writeln!(o, "M48\n; ButureCAD Excellon, deterministic output");
    let _ = writeln!(o, "; #@! TF.GenerationSoftware,ButureCAD,ButureCAD,0.1");
    let _ = writeln!(o, "; #@! TF.FileFunction,{func},1,{copper_layers},{kind}");
    let _ = writeln!(o, "FMAT,2\nMETRIC");
    for (i, d) in diams.iter().enumerate() {
        let _ = writeln!(o, "T{}C{}", i + 1, fmt_mm(*d));
    }
    let _ = writeln!(o, "%\nG90\nG05");
    for (i, d) in diams.iter().enumerate() {
        let _ = writeln!(o, "T{}", i + 1);
        for h in hs.iter().filter(|h| h.diameter == *d) {
            let a = format!("X{}Y{}", fmt_mm(h.at.x), fmt_mm(h.at.y));
            match h.slot_to {
                Some(b) => {
                    let _ = writeln!(o, "{a}G85X{}Y{}", fmt_mm(b.x), fmt_mm(b.y));
                }
                None => {
                    let _ = writeln!(o, "{a}");
                }
            }
        }
    }
    let _ = writeln!(o, "M30");
    Ok(Some(o))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::Pt;

    fn hole(x: i64, y: i64, d: i64, plated: bool, slot: Option<Pt>) -> Hole {
        Hole {
            at: Pt { x, y },
            diameter: d,
            slot_to: slot,
            plated,
        }
    }

    #[test]
    fn tools_per_diameter_split_by_plating_and_slots() {
        let hs = [
            hole(1_000_000, 2_000_000, 800_000, true, None),
            hole(3_000_000, 2_000_000, 300_000, true, None),
            hole(5_000_000, 5_000_000, 3_200_000, false, None),
            hole(0, 0, 800_000, true, Some(Pt { x: 2_000_000, y: 0 })),
        ];
        let p = write(&hs, true, 2).unwrap().unwrap();
        assert!(p.contains("T1C0.300000\nT2C0.800000\n"));
        assert!(p.contains("; #@! TF.FileFunction,Plated,1,2,PTH"));
        assert!(p.contains("X1.000000Y2.000000\n"));
        assert!(p.contains("X0.000000Y0.000000G85X2.000000Y0.000000\n"));
        assert!(!p.contains("3.200000"));
        let n = write(&hs, false, 2).unwrap().unwrap();
        assert!(n.contains("T1C3.200000") && n.contains("NPTH") && n.ends_with("M30\n"));
        assert!(write(&hs[..2], false, 2).unwrap().is_none());
        assert!(write(&[hole(0, 0, 0, true, None)], true, 2).is_err());
    }

    #[test]
    fn negative_coordinates_are_exact() {
        let p = write(&[hole(-1_500_001, 7, 600_000, true, None)], true, 2)
            .unwrap()
            .unwrap();
        assert!(p.contains("X-1.500001Y0.000007"));
    }
}
