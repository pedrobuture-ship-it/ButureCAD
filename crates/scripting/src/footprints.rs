//! Footprint names usable in scripts, resolved to generated definitions.
//!
//! * chip sizes: `0201 0402 0603 0805 1206` (also `Chip_0402`, `R_0402`, `C_0603`, …);
//! * `soic8`, `SOIC-14`, `dip8`, `header4`, `header2x5` (rows × pins per row);
//! * any generator with parameters: `soic(pins=14, wide=true)`, `qfn(pins_per_side=8, body=5)`,
//!   `bga(rows=6, columns=6)` (lengths in mm, see `buturecad_pcb::generators`).
//!
//! Generated land patterns are nominal starting points: verify them against the datasheet.

use buturecad_pcb::generators::{self, CHIP_CODES};
use buturecad_pcb::Footprint;

pub fn resolve_footprint(name: &str) -> Option<Footprint> {
    let n = name.trim();
    let mut fp = generated(n)?;
    // The project refers to the footprint by the name the script used.
    fp.name = n.to_owned();
    Some(fp)
}

fn generated(n: &str) -> Option<Footprint> {
    let low = n.to_ascii_lowercase();
    // Chip: the trailing size code, with an optional prefix such as `R_` or `Chip_`.
    let tail = low.rsplit('_').next().unwrap_or(&low);
    if CHIP_CODES.contains(&tail)
        && low[..low.len() - tail.len()]
            .trim_end_matches('_')
            .chars()
            .all(|c| c.is_ascii_alphabetic())
    {
        return generators::chip(tail).ok();
    }
    // `family(key=value, …)`
    if let Some((fam, rest)) = low.split_once('(') {
        let body = rest.strip_suffix(')')?;
        let mut params = Vec::new();
        for kv in body.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=')?;
            params.push((k.trim().to_owned(), v.trim().to_owned()));
        }
        return generators::generate(fam.trim(), &params).ok();
    }
    // `soic8`, `soic-14`, `dip8`, `header4`, `header2x5`
    let split = low.find(|c: char| c.is_ascii_digit())?;
    let fam = low[..split].trim_end_matches(['-', '_']);
    let num = &low[split..];
    let kv = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    match fam {
        "soic" | "dip" if num.chars().all(|c| c.is_ascii_digit()) => {
            generators::generate(fam, &kv(&[("pins", num)])).ok()
        }
        "header" => match num.split_once('x') {
            Some((rows, pins)) => {
                generators::generate("header", &kv(&[("rows", rows), ("pins", pins)])).ok()
            }
            None if num.chars().all(|c| c.is_ascii_digit()) => {
                generators::generate("header", &kv(&[("pins", num)])).ok()
            }
            None => None,
        },
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_to_the_expected_packages() {
        for (name, pads) in [
            ("0402", 2),
            ("R_0603", 2),
            ("Chip_1206", 2),
            ("soic8", 8),
            ("SOIC-14", 14),
            ("dip8", 8),
            ("header4", 4),
            ("header2x5", 10),
            ("soic(pins=16)", 16),
            ("qfn(pins_per_side=4)", 16),
            ("bga(rows=3, columns=3)", 9),
        ] {
            let f = resolve_footprint(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!((f.name.as_str(), f.pads.len()), (name, pads), "{name}");
        }
        for bad in [
            "",
            "0999",
            "soic",
            "soic(pins=7)",
            "soic(nope=1)",
            "mystery5",
            "header2x",
            "R_foo_0402x",
        ] {
            assert!(resolve_footprint(bad).is_none(), "{bad}");
        }
    }
}
