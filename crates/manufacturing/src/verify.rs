//! Verification of generated files against the manufacturing input: the Gerber/Excellon files
//! are **parsed back** by the independent readers and compared with what the board says they
//! must contain. A mismatch means the export does not correspond to the project.

use crate::excellon_read;
use crate::gerber_read::{self, Prim};
use crate::model::{FabInput, Feature, Pt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub ok: bool,
    pub text: String,
}

fn ck(ok: bool, text: String) -> Check {
    Check { ok, text }
}

fn sorted(mut v: Vec<[i64; 2]>) -> Vec<[i64; 2]> {
    v.sort_unstable();
    v
}

fn p(q: Pt) -> [i64; 2] {
    [q.x, q.y]
}

/// Parses every Gerber/Excellon in `files` and checks it against `input`.
pub fn check(input: &FabInput, files: &[(String, Vec<u8>)]) -> Vec<Check> {
    let get = |n: &str| {
        files
            .iter()
            .find(|(f, _)| f == n)
            .and_then(|(_, b)| std::str::from_utf8(b).ok())
    };
    let mut out = Vec::new();
    for l in &input.layers {
        let Some(text) = get(&l.file) else {
            out.push(ck(
                false,
                format!("{}: file is missing from the export", l.file),
            ));
            continue;
        };
        let img = match gerber_read::parse(text) {
            Ok(i) => i,
            Err(e) => {
                out.push(ck(false, format!("{}: does not parse — {e}", l.file)));
                continue;
            }
        };
        let (mut ef, mut es, mut er) = (Vec::new(), 0usize, 0usize);
        for f in &l.features {
            match f {
                Feature::Flash { at, .. } => ef.push(p(*at)),
                Feature::Draw { .. } => es += 1,
                Feature::Region { holes, .. } => er += 1 + holes.len(),
            }
        }
        let gf: Vec<[i64; 2]> = img
            .prims
            .iter()
            .filter_map(|q| match q {
                Prim::Flash { at, .. } => Some(*at),
                _ => None,
            })
            .collect();
        let (_, gs, gr) = img.count();
        let same_function = img.function.as_deref() == Some(l.function.as_str());
        let ok = sorted(ef.clone()) == sorted(gf.clone()) && es == gs && er == gr && same_function;
        out.push(ck(
            ok,
            format!(
                "{}: {} flash(es) at the same positions, {es} stroke(s), {er} contour(s) — file has {}/{gs}/{gr}{}",
                l.file,
                ef.len(),
                gf.len(),
                if same_function { "" } else { " (FileFunction differs)" }
            ),
        ));
        if !img.warnings.is_empty() {
            out.push(ck(
                false,
                format!("{}: reader warnings: {}", l.file, img.warnings.join("; ")),
            ));
        }
        if l.function.starts_with("Profile") {
            if let (Some((lo, hi)), Some((blo, bhi))) = (img.bounds(), input.board_bounds()) {
                let w = l
                    .features
                    .iter()
                    .find_map(|f| match f {
                        Feature::Draw { width, .. } => Some(*width / 2),
                        _ => None,
                    })
                    .unwrap_or(0);
                let want = ([blo.x - w, blo.y - w], [bhi.x + w, bhi.y + w]);
                out.push(ck(
                    (lo, hi) == want,
                    format!("{}: profile extent matches the board outline", l.file),
                ));
            }
        }
    }
    for plated in [true, false] {
        let name = format!("{}-{}.drl", input.name, if plated { "PTH" } else { "NPTH" });
        let want: Vec<&crate::Hole> = input.holes.iter().filter(|h| h.plated == plated).collect();
        match get(&name) {
            None => out.push(ck(
                want.is_empty(),
                format!(
                    "{name}: {}",
                    if want.is_empty() {
                        "none needed"
                    } else {
                        "missing"
                    }
                ),
            )),
            Some(t) => match excellon_read::parse(t) {
                Err(e) => out.push(ck(false, format!("{name}: does not parse — {e}"))),
                Ok(d) => {
                    let mut a: Vec<([i64; 2], i64, Option<[i64; 2]>)> = want
                        .iter()
                        .map(|h| (p(h.at), h.diameter, h.slot_to.map(p)))
                        .collect();
                    let mut b: Vec<([i64; 2], i64, Option<[i64; 2]>)> = d
                        .hits
                        .iter()
                        .map(|h| (h.at, d.diameter(h.tool).unwrap_or(-1), h.slot_to))
                        .collect();
                    a.sort_unstable();
                    b.sort_unstable();
                    out.push(ck(
                        a == b,
                        format!("{name}: {} hole(s) with the same positions and diameters as the board — file has {}", a.len(), b.len()),
                    ));
                }
            },
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::package::{generate, tests::board};

    fn files() -> Vec<(String, Vec<u8>)> {
        generate(&board())
            .unwrap()
            .files
            .into_iter()
            .map(|f| (f.name, f.bytes))
            .collect()
    }

    #[test]
    fn generated_files_match_the_input() {
        let c = check(&board(), &files());
        assert!(c.iter().all(|x| x.ok), "{c:#?}");
        assert!(c.len() >= 5);
    }

    #[test]
    fn tampering_is_detected() {
        let mut f = files();
        for (n, b) in &mut f {
            if n.ends_with(".gtl") {
                *b = String::from_utf8(b.clone())
                    .unwrap()
                    .replace("X5000000Y5000000", "X5000001Y5000000")
                    .into_bytes();
            }
            if n.ends_with("PTH.drl") {
                *b = String::from_utf8(b.clone())
                    .unwrap()
                    .replace("0.800000", "0.900000")
                    .into_bytes();
            }
        }
        let c = check(&board(), &f);
        assert_eq!(c.iter().filter(|x| !x.ok).count(), 2, "{c:#?}");
        f.retain(|(n, _)| !n.ends_with(".gko"));
        assert!(check(&board(), &f)
            .iter()
            .any(|x| !x.ok && x.text.contains("missing")));
    }
}
