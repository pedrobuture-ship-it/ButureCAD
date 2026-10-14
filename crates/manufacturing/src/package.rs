//! Assembles every production file, the preflight report and `production.zip`.

use crate::model::{FabInput, Feature};
use crate::preflight::{self, Report};
use crate::{excellon, gerber, tables, zip, FabError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionFile {
    pub name: String,
    pub bytes: Vec<u8>,
    /// What is inside, for the Manufacturing Preview.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    pub name: String,
    pub bytes: usize,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub files: Vec<ProductionFile>,
    pub report: Report,
}

impl Generated {
    pub fn preview(&self) -> Vec<FilePreview> {
        self.files
            .iter()
            .map(|f| FilePreview {
                name: f.name.clone(),
                bytes: f.bytes.len(),
                summary: f.summary.clone(),
            })
            .collect()
    }

    /// `production.zip`. Refused while the preflight has errors.
    pub fn zip(&self) -> Result<Vec<u8>, FabError> {
        let n = self.report.count(preflight::Severity::Error);
        if n > 0 {
            return Err(FabError::Blocked(n));
        }
        let files: Vec<(String, Vec<u8>)> = self
            .files
            .iter()
            .map(|f| (f.name.clone(), f.bytes.clone()))
            .collect();
        zip::write(&files)
    }
}

fn counts(features: &[Feature]) -> String {
    let (mut fl, mut dr, mut rg) = (0, 0, 0);
    for f in features {
        match f {
            Feature::Flash { .. } => fl += 1,
            Feature::Draw { .. } => dr += 1,
            Feature::Region { .. } => rg += 1,
        }
    }
    format!("{fl} flashes, {dr} draws, {rg} regions")
}

/// Generates all outputs (also when the preflight has errors, so the preview can show them; the
/// zip is what is blocked).
pub fn generate(input: &FabInput) -> Result<Generated, FabError> {
    let report = preflight::run(input);
    let mut files = Vec::new();
    for l in &input.layers {
        files.push(ProductionFile {
            name: l.file.clone(),
            bytes: gerber::write(l)?.into_bytes(),
            summary: format!("Gerber {} — {}", l.function, counts(&l.features)),
        });
    }
    for plated in [true, false] {
        if let Some(d) = excellon::write(&input.holes, plated, input.copper_layers)? {
            let n = input.holes.iter().filter(|h| h.plated == plated).count();
            files.push(ProductionFile {
                name: format!("{}-{}.drl", input.name, if plated { "PTH" } else { "NPTH" }),
                bytes: d.into_bytes(),
                summary: format!(
                    "Excellon {} — {n} hole(s)",
                    if plated { "plated" } else { "non-plated" }
                ),
            });
        }
    }
    let bom = tables::bom_csv(&input.parts);
    files.push(ProductionFile {
        name: format!("{}-bom.csv", input.name),
        summary: format!("BOM — {} line(s)", bom.lines().count().saturating_sub(1)),
        bytes: bom.into_bytes(),
    });
    let pos = tables::pick_and_place_csv(&input.parts);
    files.push(ProductionFile {
        name: format!("{}-pos.csv", input.name),
        summary: format!(
            "Pick and place — {} part(s)",
            pos.lines().count().saturating_sub(1)
        ),
        bytes: pos.into_bytes(),
    });
    let mut names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    names.push("assembly-info.txt".into());
    names.push("PREFLIGHT.txt".into());
    files.push(ProductionFile {
        name: "assembly-info.txt".into(),
        bytes: tables::assembly_info(input, &names, &report).into_bytes(),
        summary: "Assembly information".into(),
    });
    files.push(ProductionFile {
        name: "PREFLIGHT.txt".into(),
        bytes: report.text().into_bytes(),
        summary: format!(
            "Preflight — {} error(s), {} warning(s)",
            report.count(preflight::Severity::Error),
            report.count(preflight::Severity::Warning)
        ),
    });
    Ok(Generated { files, report })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;
    use crate::model::*;

    fn p(x: i64, y: i64) -> Pt {
        Pt { x, y }
    }

    pub(crate) fn board() -> FabInput {
        let outline = vec![
            p(0, 0),
            p(20_000_000, 0),
            p(20_000_000, 10_000_000),
            p(0, 10_000_000),
        ];
        let edges: Vec<Feature> = (0..4)
            .map(|i| Feature::Draw {
                a: outline[i],
                b: outline[(i + 1) % 4],
                width: 100_000,
            })
            .collect();
        FabInput {
            name: "demo".into(),
            outline: vec![outline],
            copper_layers: 2,
            thickness_nm: 1_600_000,
            layers: vec![
                Layer {
                    file: "demo-F_Cu.gtl".into(),
                    function: "Copper,L1,Top".into(),
                    features: vec![Feature::Flash {
                        at: p(5_000_000, 5_000_000),
                        ap: Aperture::Circle { d: 1_600_000 },
                    }],
                },
                Layer {
                    file: "demo-B_Cu.gbl".into(),
                    function: "Copper,L2,Bot".into(),
                    features: vec![Feature::Draw {
                        a: p(1, 1),
                        b: p(5_000_000, 1),
                        width: 250_000,
                    }],
                },
                Layer {
                    file: "demo-Edge_Cuts.gko".into(),
                    function: "Profile,NP".into(),
                    features: edges,
                },
            ],
            holes: vec![Hole {
                at: p(5_000_000, 5_000_000),
                diameter: 800_000,
                slot_to: None,
                plated: true,
            }],
            parts: vec![Part {
                reference: "R1".into(),
                value: "10k".into(),
                footprint: "R_0603".into(),
                position: p(5_000_000, 5_000_000),
                rotation_deg: 0,
                bottom: false,
                mount: Mount::Smd,
                in_bom: true,
                in_position_file: true,
            }],
            drc: DrcSummary::default(),
            capabilities: Capabilities::default(),
            min_track_width: Some(250_000),
            extra_issues: Vec::new(),
        }
    }

    #[test]
    fn a_clean_board_produces_a_valid_zip_with_every_file() {
        let g = generate(&board()).unwrap();
        assert!(!g.report.has_errors(), "{}", g.report.text());
        let z = g.zip().unwrap();
        let files = zip::read_stored(&z).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        for want in [
            "demo-F_Cu.gtl",
            "demo-B_Cu.gbl",
            "demo-Edge_Cuts.gko",
            "demo-PTH.drl",
            "demo-bom.csv",
            "demo-pos.csv",
            "assembly-info.txt",
            "PREFLIGHT.txt",
        ] {
            assert!(names.contains(&want), "missing {want}");
        }
        assert!(!names.contains(&"demo-NPTH.drl"));
        assert_eq!(z, generate(&board()).unwrap().zip().unwrap());
    }

    #[test]
    fn errors_block_the_zip_but_not_the_preview() {
        let mut b = board();
        b.drc.errors = 3;
        let g = generate(&b).unwrap();
        assert!(matches!(g.zip(), Err(FabError::Blocked(1))));
        assert!(!g.preview().is_empty());
        let mut b = board();
        b.outline.clear();
        assert!(generate(&b)
            .unwrap()
            .report
            .issues
            .iter()
            .any(|i| i.code == "outline"));
    }

    #[test]
    fn preflight_flags_geometry_and_reference_problems() {
        let mut b = board();
        b.holes.push(Hole {
            at: p(99_000_000, 1),
            diameter: 100_000,
            slot_to: None,
            plated: true,
        });
        b.parts.push(b.parts[0].clone());
        b.min_track_width = Some(50_000);
        b.parts[0].value.clear();
        let r = generate(&b).unwrap().report;
        let codes: Vec<&str> = r.issues.iter().map(|i| i.code).collect();
        for c in [
            "drill_outside",
            "small_drill",
            "thin_track",
            "reference",
            "no_value",
            "not_a_signoff",
        ] {
            assert!(codes.contains(&c), "missing {c}: {codes:?}");
        }
        assert!(r.has_errors());
        // Errors are listed first.
        assert_eq!(r.issues[0].severity, preflight::Severity::Error);
    }
}
