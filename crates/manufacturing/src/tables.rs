//! BOM, pick and place and the assembly information text.

use crate::model::{FabInput, Mount, Part};
use crate::{fmt_mm, Report};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write;

fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// `R2` < `R10` < `U1`: letters first, then the number.
fn natural(a: &str, b: &str) -> Ordering {
    let split = |s: &str| {
        let i = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
        (
            s[..i].to_string(),
            s[i..].parse::<u64>().unwrap_or(0),
            s.to_string(),
        )
    };
    split(a).cmp(&split(b))
}

/// BOM columns, in order.
pub const BOM_HEADER: &str = "Reference,Quantity,Value,Footprint,Manufacturer,MPN,Supplier,SPN";

type BomKey = (String, String, String, String, String, String);

/// One row per distinct (value, footprint, manufacturer, MPN, supplier, SPN); references sorted
/// naturally. Parts that differ only in purchasing data are separate lines.
pub fn bom_csv(parts: &[Part]) -> String {
    let mut groups: BTreeMap<BomKey, Vec<&str>> = BTreeMap::new();
    for p in parts.iter().filter(|p| p.in_bom) {
        groups
            .entry((
                p.value.clone(),
                p.footprint.clone(),
                p.fields.manufacturer.clone(),
                p.fields.mpn.clone(),
                p.fields.supplier.clone(),
                p.fields.supplier_pn.clone(),
            ))
            .or_default()
            .push(&p.reference);
    }
    let mut rows: Vec<(Vec<&str>, &BomKey)> = groups
        .iter()
        .map(|(k, refs)| {
            let mut r = refs.clone();
            r.sort_by(|a, b| natural(a, b));
            (r, k)
        })
        .collect();
    rows.sort_by(|a, b| natural(a.0[0], b.0[0]));
    let mut o = format!("{BOM_HEADER}\n");
    for (refs, (value, footprint, mfr, mpn, sup, spn)) in rows {
        let _ = writeln!(
            o,
            "{},{},{},{},{},{},{},{}",
            csv(&refs.join(" ")),
            refs.len(),
            csv(value),
            csv(footprint),
            csv(mfr),
            csv(mpn),
            csv(sup),
            csv(spn)
        );
    }
    o
}

/// Does `name` match `pattern`, where `*` stands for any run of characters (case-insensitive)?
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (
        pattern.to_lowercase().chars().collect(),
        name.to_lowercase().chars().collect(),
    );
    // Iterative wildcard matching with backtracking to the last `*`.
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The pick-and-place rotation: the placement angle plus the first matching offset, modulo 360.
pub fn placed_rotation(part: &Part, offsets: &[(String, i32)]) -> u16 {
    let off = offsets
        .iter()
        .find(|(pat, _)| glob_match(pat, &part.footprint))
        .map_or(0, |(_, d)| *d);
    (i32::from(part.rotation_deg) + off).rem_euclid(360) as u16
}

/// Parses a rotation-offset file: one `footprint_pattern,degrees` per line; `#` starts a comment.
pub fn parse_rotation_offsets(text: &str) -> Result<Vec<(String, i32)>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (pat, deg) = line
            .rsplit_once(',')
            .ok_or_else(|| format!("line {}: expected `footprint_pattern,degrees`", i + 1))?;
        let deg: i32 = deg.trim().parse().map_err(|_| {
            format!(
                "line {}: “{}” is not a whole number of degrees",
                i + 1,
                deg.trim()
            )
        })?;
        let pat = pat.trim().trim_matches('"');
        if pat.is_empty() {
            return Err(format!("line {}: the footprint pattern is empty", i + 1));
        }
        out.push((pat.to_owned(), deg));
    }
    Ok(out)
}

/// Surface-mount parts (and unspecified ones, which the preflight flags). Positions are the
/// placement origin in the design coordinate system (the same origin as the Gerber files).
/// `offsets` are the per-footprint rotation corrections an assembly house expects.
pub fn pick_and_place_csv(parts: &[Part], offsets: &[(String, i32)]) -> String {
    let mut ps: Vec<&Part> = parts
        .iter()
        .filter(|p| p.in_position_file && p.mount != Mount::Through)
        .collect();
    ps.sort_by(|a, b| natural(&a.reference, &b.reference));
    let mut o = String::from("Ref,Val,Package,PosX_mm,PosY_mm,Rot_deg,Side\n");
    for p in ps {
        let _ = writeln!(
            o,
            "{},{},{},{},{},{},{}",
            csv(&p.reference),
            csv(&p.value),
            csv(&p.footprint),
            fmt_mm(p.position.x),
            fmt_mm(p.position.y),
            placed_rotation(p, offsets),
            if p.bottom { "bottom" } else { "top" }
        );
    }
    o
}

/// Human-readable assembly information.
pub fn assembly_info(input: &FabInput, files: &[String], report: &Report) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "ASSEMBLY INFORMATION — {}", input.name);
    let _ = writeln!(o, "Generated by ButureCAD (deterministic, no timestamp)\n");
    if let Some((lo, hi)) = input.board_bounds() {
        let _ = writeln!(
            o,
            "Board size: {} x {} mm (outline bounding box)",
            fmt_mm(hi.x - lo.x),
            fmt_mm(hi.y - lo.y)
        );
    }
    let _ = writeln!(o, "Copper layers: {}", input.copper_layers);
    let _ = writeln!(o, "Board thickness: {} mm", fmt_mm(input.thickness_nm));
    let top = input.parts.iter().filter(|p| !p.bottom).count();
    let bottom = input.parts.len() - top;
    let _ = writeln!(
        o,
        "Components: {} ({top} top, {bottom} bottom); BOM lines: {}",
        input.parts.len(),
        bom_csv(&input.parts).lines().count().saturating_sub(1)
    );
    let _ = writeln!(
        o,
        "Mounting: {} SMD, {} through-hole, {} unspecified",
        input.parts.iter().filter(|p| p.mount == Mount::Smd).count(),
        input
            .parts
            .iter()
            .filter(|p| p.mount == Mount::Through)
            .count(),
        input
            .parts
            .iter()
            .filter(|p| p.mount == Mount::Unspecified)
            .count(),
    );
    let _ = writeln!(
        o,
        "Drilled holes: {} plated, {} non-plated",
        input.holes.iter().filter(|h| h.plated).count(),
        input.holes.iter().filter(|h| !h.plated).count()
    );
    let _ = writeln!(
        o,
        "\nCoordinates: millimetres, design origin, +Y up — the same origin in Gerber, drill and\n\
         pick-and-place. Rotation is the counter-clockwise placement angle in board coordinates;\n\
         bottom-side parts are mirrored. {}",
        if input.rotation_offsets.is_empty() {
            "No per-footprint rotation offsets are applied (some assembly houses expect them:\n\
             pass a rotation-offset file when exporting)."
                .to_owned()
        } else {
            format!(
                "Per-footprint rotation offsets were applied to the pick-and-place file ({} rule(s)).",
                input.rotation_offsets.len()
            )
        }
    );
    let _ = writeln!(o, "\nFiles:");
    for f in files {
        let _ = writeln!(o, "  {f}");
    }
    let _ = writeln!(
        o,
        "\nPreflight: {} error(s), {} warning(s), {} note(s). See PREFLIGHT.txt.",
        report.count(crate::Severity::Error),
        report.count(crate::Severity::Warning),
        report.count(crate::Severity::Info)
    );
    o
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::Pt;

    fn part(r: &str, v: &str, f: &str, mount: Mount, bottom: bool) -> Part {
        Part {
            reference: r.into(),
            value: v.into(),
            footprint: f.into(),
            position: Pt {
                x: 1_500_000,
                y: -2_000_000,
            },
            rotation_deg: 90,
            bottom,
            mount,
            in_bom: true,
            in_position_file: true,
            fields: Default::default(),
        }
    }

    #[test]
    fn bom_groups_and_sorts_naturally_and_escapes() {
        let ps = vec![
            part("R10", "10k", "R_0603", Mount::Smd, false),
            part("R2", "10k", "R_0603", Mount::Smd, false),
            part("C1", "100n, X7R", "C_0603", Mount::Smd, false),
            part("U1", "NE555", "SOIC-8", Mount::Smd, true),
        ];
        let b = bom_csv(&ps);
        assert_eq!(
            b,
            "Reference,Quantity,Value,Footprint,Manufacturer,MPN,Supplier,SPN\nC1,1,\"100n, X7R\",C_0603,,,,\nR2 R10,2,10k,R_0603,,,,\nU1,1,NE555,SOIC-8,,,,\n"
        );
        let mut ex = ps.clone();
        ex[0].in_bom = false;
        assert!(bom_csv(&ex).contains("R2,1,10k"));
    }

    #[test]
    fn pick_and_place_skips_through_hole_and_excluded_parts() {
        let mut ps = vec![
            part("U1", "x", "p", Mount::Smd, true),
            part("J1", "x", "hdr", Mount::Through, false),
            part("R1", "x", "r", Mount::Unspecified, false),
        ];
        ps.push({
            let mut p = part("TP1", "x", "tp", Mount::Smd, false);
            p.in_position_file = false;
            p
        });
        let c = pick_and_place_csv(&ps, &[]);
        assert_eq!(
            c,
            "Ref,Val,Package,PosX_mm,PosY_mm,Rot_deg,Side\nR1,x,r,1.500000,-2.000000,90,top\nU1,x,p,1.500000,-2.000000,90,bottom\n"
        );
    }
}
