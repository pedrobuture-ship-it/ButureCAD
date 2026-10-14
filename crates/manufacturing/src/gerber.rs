//! Gerber RS-274X writer (format 4.6, millimetres, so every coordinate is exactly the integer
//! nanometre value). Features are written in order; apertures are numbered by first use.

use crate::model::{Aperture, Feature, Layer, Pt};
use crate::{fmt_mm, FabError};
use std::fmt::Write;

const LIMIT: i64 = 9_999_999_999; // 4 integer digits of mm

fn xy(p: Pt) -> Result<String, FabError> {
    if p.x.abs() > LIMIT || p.y.abs() > LIMIT {
        return Err(FabError::Invalid(
            "a coordinate exceeds the Gerber 4.6 range (±9999 mm)".into(),
        ));
    }
    Ok(format!("X{}Y{}", p.x, p.y))
}

fn ap_def(n: u32, a: Aperture) -> String {
    match a {
        Aperture::Circle { d } => format!("%ADD{n}C,{}*%", fmt_mm(d)),
        Aperture::Rect { w, h } => format!("%ADD{n}R,{}X{}*%", fmt_mm(w), fmt_mm(h)),
        Aperture::Obround { w, h } => format!("%ADD{n}O,{}X{}*%", fmt_mm(w), fmt_mm(h)),
    }
}

fn contour(out: &mut String, pts: &[Pt]) -> Result<(), FabError> {
    if pts.len() < 3 {
        return Err(FabError::Invalid(
            "a region has fewer than 3 vertices".into(),
        ));
    }
    let _ = writeln!(out, "G36*\n{}D02*", xy(pts[0])?);
    for p in &pts[1..] {
        let _ = writeln!(out, "{}D01*", xy(*p)?);
    }
    let _ = writeln!(out, "{}D01*\nG37*", xy(pts[0])?);
    Ok(())
}

pub fn write(layer: &Layer) -> Result<String, FabError> {
    // Pass 1: aperture table in order of first use.
    let mut aps: Vec<Aperture> = Vec::new();
    let id_of = |a: Aperture, aps: &mut Vec<Aperture>| -> u32 {
        let i = aps.iter().position(|x| *x == a).unwrap_or_else(|| {
            aps.push(a);
            aps.len() - 1
        });
        10 + i as u32
    };
    let mut ids = Vec::with_capacity(layer.features.len());
    for f in &layer.features {
        ids.push(match f {
            Feature::Flash { ap, .. } => {
                let ok = match ap {
                    Aperture::Circle { d } => *d > 0,
                    Aperture::Rect { w, h } | Aperture::Obround { w, h } => *w > 0 && *h > 0,
                };
                if !ok {
                    return Err(FabError::Invalid("a flash has a non-positive size".into()));
                }
                id_of(*ap, &mut aps)
            }
            Feature::Draw { width, .. } => {
                if *width <= 0 {
                    return Err(FabError::Invalid("a draw has a non-positive width".into()));
                }
                id_of(Aperture::Circle { d: *width }, &mut aps)
            }
            Feature::Region { .. } => 0,
        });
    }
    let mut out = String::new();
    let _ = writeln!(out, "G04 ButureCAD Gerber, deterministic output*");
    let _ = writeln!(out, "%TF.GenerationSoftware,ButureCAD,ButureCAD,0.1*%");
    let _ = writeln!(out, "%TF.FileFunction,{}*%", layer.function);
    let _ = writeln!(out, "%TF.FilePolarity,Positive*%");
    let _ = writeln!(out, "%FSLAX46Y46*%\n%MOMM*%");
    for (i, a) in aps.iter().enumerate() {
        let _ = writeln!(out, "{}", ap_def(10 + i as u32, *a));
    }
    let _ = writeln!(out, "%LPD*%\nG01*");
    let mut current = 0u32;
    for (f, id) in layer.features.iter().zip(ids) {
        match f {
            Feature::Flash { at, .. } => {
                if id != current {
                    let _ = writeln!(out, "D{id}*");
                    current = id;
                }
                let _ = writeln!(out, "{}D03*", xy(*at)?);
            }
            Feature::Draw { a, b, .. } => {
                if id != current {
                    let _ = writeln!(out, "D{id}*");
                    current = id;
                }
                let _ = writeln!(out, "{}D02*\n{}D01*", xy(*a)?, xy(*b)?);
            }
            Feature::Region { outer, holes } => {
                contour(&mut out, outer)?;
                if !holes.is_empty() {
                    let _ = writeln!(out, "%LPC*%");
                    for h in holes {
                        contour(&mut out, h)?;
                    }
                    let _ = writeln!(out, "%LPD*%");
                }
            }
        }
    }
    let _ = writeln!(out, "M02*");
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Pt {
        Pt { x, y }
    }

    fn layer(features: Vec<Feature>) -> Layer {
        Layer {
            file: "a.gtl".into(),
            function: "Copper,L1,Top".into(),
            features,
        }
    }

    #[test]
    fn coordinates_are_exact_nanometres_and_apertures_are_shared() {
        let g = write(&layer(vec![
            Feature::Flash {
                at: p(1_234_567, -2_000_001),
                ap: Aperture::Circle { d: 800_000 },
            },
            Feature::Draw {
                a: p(0, 0),
                b: p(10_000_000, 5),
                width: 250_000,
            },
            Feature::Flash {
                at: p(5, 6),
                ap: Aperture::Circle { d: 800_000 },
            },
            Feature::Flash {
                at: p(7, 8),
                ap: Aperture::Rect {
                    w: 1_600_000,
                    h: 900_000,
                },
            },
        ]))
        .unwrap();
        assert!(g.contains("%FSLAX46Y46*%") && g.contains("%MOMM*%"));
        assert!(g.contains("%ADD10C,0.800000*%"));
        assert!(g.contains("%ADD11C,0.250000*%"));
        assert!(g.contains("%ADD12R,1.600000X0.900000*%"));
        assert_eq!(g.matches("%ADD").count(), 3);
        assert!(g.contains("X1234567Y-2000001D03*"));
        assert!(g.contains("X0Y0D02*\nX10000000Y5D01*"));
        assert!(g.ends_with("M02*\n"));
        assert_eq!(
            g,
            write(&layer(vec![
                Feature::Flash {
                    at: p(1_234_567, -2_000_001),
                    ap: Aperture::Circle { d: 800_000 }
                },
                Feature::Draw {
                    a: p(0, 0),
                    b: p(10_000_000, 5),
                    width: 250_000
                },
                Feature::Flash {
                    at: p(5, 6),
                    ap: Aperture::Circle { d: 800_000 }
                },
                Feature::Flash {
                    at: p(7, 8),
                    ap: Aperture::Rect {
                        w: 1_600_000,
                        h: 900_000
                    }
                },
            ]))
            .unwrap()
        );
    }

    #[test]
    fn regions_close_and_holes_use_clear_polarity() {
        let sq = |a: i64, b: i64| vec![p(a, a), p(b, a), p(b, b), p(a, b)];
        let g = write(&layer(vec![Feature::Region {
            outer: sq(0, 10_000_000),
            holes: vec![sq(2_000_000, 4_000_000)],
        }]))
        .unwrap();
        assert_eq!(g.matches("G36*").count(), 2);
        assert_eq!(g.matches("G37*").count(), 2);
        let lpc = g.find("%LPC*%").unwrap();
        let lpd = g.rfind("%LPD*%").unwrap();
        assert!(lpc < lpd);
        // Each contour ends on its first point.
        assert!(g.contains(
            "X0Y0D02*\nX10000000Y0D01*\nX10000000Y10000000D01*\nX0Y10000000D01*\nX0Y0D01*\nG37*"
        ));
    }

    #[test]
    fn invalid_features_are_errors() {
        assert!(write(&layer(vec![Feature::Region {
            outer: vec![p(0, 0), p(1, 1)],
            holes: vec![]
        }]))
        .is_err());
        assert!(write(&layer(vec![Feature::Draw {
            a: p(0, 0),
            b: p(1, 1),
            width: 0
        }]))
        .is_err());
        assert!(write(&layer(vec![Feature::Flash {
            at: p(i64::MAX / 2, 0),
            ap: Aperture::Circle { d: 1 }
        }]))
        .is_err());
    }
}
