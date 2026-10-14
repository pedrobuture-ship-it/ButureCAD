//! Gerber (RS-274X) **reader**, written independently of [`crate::gerber`] so that it can check
//! what the writer produced. Coordinates become integer nanometres.
//!
//! Supported: `FS` (absolute, leading zeros omitted), `MO`, `AD` with C/R/O/P templates, `G01`
//! `G02` `G03`, `G74`/`G75`, `G36`/`G37` regions, `LPD`/`LPC`, `D01`-`D03`, X2 `TF.FileFunction`.
//! Not supported (reported in [`Image::warnings`], never silently ignored): aperture macros
//! (`AM`), step-and-repeat, mirroring/rotation/scaling, aperture holes, trailing-zero omission and
//! incremental coordinates (these are errors).

use std::f64::consts::PI;

/// A point in nm.
pub type P = [i64; 2];

#[derive(Debug, Clone, PartialEq)]
pub enum ApShape {
    Circle {
        d: i64,
    },
    Rect {
        w: i64,
        h: i64,
    },
    Obround {
        w: i64,
        h: i64,
    },
    Polygon {
        d: i64,
        n: u32,
        rot_deg: f64,
    },
    /// A macro or something else this reader cannot draw.
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApInfo {
    pub code: u32,
    pub shape: ApShape,
    /// Flashes + strokes that use it.
    pub uses: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Flash {
        ap: ApShape,
        at: P,
        dark: bool,
    },
    Stroke {
        a: P,
        b: P,
        ap: ApShape,
        dark: bool,
    },
    /// Each contour is filled on its own.
    Region {
        contours: Vec<Vec<P>>,
        dark: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Image {
    /// `TF.FileFunction` value, e.g. `Copper,L1,Top`.
    pub function: Option<String>,
    pub prims: Vec<Prim>,
    pub apertures: Vec<ApInfo>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Gerber line {line}: {msg}")]
pub struct GerberError {
    pub line: usize,
    pub msg: String,
}

impl Image {
    pub fn count(&self) -> (usize, usize, usize) {
        let (mut f, mut s, mut r) = (0, 0, 0);
        for p in &self.prims {
            match p {
                Prim::Flash { .. } => f += 1,
                Prim::Stroke { .. } => s += 1,
                Prim::Region { contours, .. } => r += contours.len(),
            }
        }
        (f, s, r)
    }

    /// Bounding box `(min, max)` including aperture extents.
    pub fn bounds(&self) -> Option<(P, P)> {
        let mut b: Option<(P, P)> = None;
        let mut add = |x: i64, y: i64| {
            b = Some(match b {
                None => ([x, y], [x, y]),
                Some((lo, hi)) => ([lo[0].min(x), lo[1].min(y)], [hi[0].max(x), hi[1].max(y)]),
            });
        };
        for p in &self.prims {
            match p {
                Prim::Flash { ap, at, .. } => {
                    let (hw, hh) = half_extent(ap);
                    add(at[0] - hw, at[1] - hh);
                    add(at[0] + hw, at[1] + hh);
                }
                Prim::Stroke { a, b, ap, .. } => {
                    let (hw, hh) = half_extent(ap);
                    for q in [a, b] {
                        add(q[0] - hw, q[1] - hh);
                        add(q[0] + hw, q[1] + hh);
                    }
                }
                Prim::Region { contours, .. } => {
                    for q in contours.iter().flatten() {
                        add(q[0], q[1]);
                    }
                }
            }
        }
        b
    }
}

pub fn half_extent(ap: &ApShape) -> (i64, i64) {
    match ap {
        ApShape::Circle { d } | ApShape::Polygon { d, .. } => (d / 2, d / 2),
        ApShape::Rect { w, h } | ApShape::Obround { w, h } => (w / 2, h / 2),
        ApShape::Unsupported(_) => (0, 0),
    }
}

struct Fmt {
    dec: u32,
    mm: bool,
}

impl Fmt {
    /// A coordinate digit string (already an integer in units of 10^-dec) → nm.
    fn nm(&self, v: i64) -> i64 {
        let num = i128::from(v) * if self.mm { 1_000_000 } else { 25_400_000 };
        let den = 10i128.pow(self.dec);
        let q = (num.abs() + den / 2) / den;
        (if num < 0 { -q } else { q }) as i64
    }
}

/// mm/inch decimal number text → nm.
fn dim(s: &str, mm: bool) -> Option<i64> {
    let v: f64 = s.trim().parse().ok()?;
    Some((v * if mm { 1e6 } else { 25.4e6 }).round() as i64)
}

fn parse_ap(def: &str, mm: bool) -> ApShape {
    let (name, rest) = def.split_once(',').unwrap_or((def, ""));
    let nums: Vec<&str> = rest.split('X').collect();
    let d = |i: usize| nums.get(i).and_then(|s| dim(s, mm));
    let ok = match name {
        "C" => d(0).map(|d| ApShape::Circle { d }),
        "R" => d(0).zip(d(1)).map(|(w, h)| ApShape::Rect { w, h }),
        "O" => d(0).zip(d(1)).map(|(w, h)| ApShape::Obround { w, h }),
        "P" => d(0).map(|d| ApShape::Polygon {
            d,
            n: nums.get(1).and_then(|s| s.trim().parse().ok()).unwrap_or(3),
            rot_deg: nums
                .get(2)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0.0),
        }),
        _ => None,
    };
    ok.unwrap_or_else(|| ApShape::Unsupported(name.to_string()))
}

/// Splits `X123Y-4I5J6D01` into letter/number pairs.
fn words(block: &str) -> Vec<(char, i64)> {
    let mut out = Vec::new();
    let b: Vec<char> = block.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        if !c.is_ascii_alphabetic() {
            continue;
        }
        let st = i;
        if i < b.len() && (b[i] == '-' || b[i] == '+') {
            i += 1;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let t: String = b[st..i].iter().collect();
        if let Ok(v) = t.parse::<i64>() {
            out.push((c, v));
        }
    }
    out
}

fn arc_points(start: P, end: P, center: [f64; 2], cw: bool) -> Vec<P> {
    let r = ((start[0] as f64 - center[0]).hypot(start[1] as f64 - center[1])).max(1.0);
    let a0 = (start[1] as f64 - center[1]).atan2(start[0] as f64 - center[0]);
    let a1 = (end[1] as f64 - center[1]).atan2(end[0] as f64 - center[0]);
    let mut sweep = a1 - a0;
    if cw {
        while sweep >= 0.0 {
            sweep -= 2.0 * PI;
        }
    } else {
        while sweep <= 0.0 {
            sweep += 2.0 * PI;
        }
    }
    // Chord error below ~1 µm or at most 1.5 degrees per piece.
    let step = (2.0 * (1.0 - 1000.0 / r).clamp(-1.0, 1.0).acos()).clamp(0.02, 0.0262);
    let n = (sweep.abs() / step).ceil().max(2.0) as usize;
    let mut pts: Vec<P> = (1..n)
        .map(|k| {
            let a = a0 + sweep * k as f64 / n as f64;
            [
                (center[0] + r * a.cos()).round() as i64,
                (center[1] + r * a.sin()).round() as i64,
            ]
        })
        .collect();
    pts.push(end);
    pts
}

pub fn parse(text: &str) -> Result<Image, GerberError> {
    let mut img = Image::default();
    let mut fmt: Option<Fmt> = None;
    let mut mm_unit = true;
    let mut aps: Vec<(u32, ApShape, usize)> = Vec::new();
    let mut cur_ap: Option<usize> = None;
    let mut dark = true;
    let (mut interp, mut multi_quadrant) = (1u8, true);
    let mut in_region = false;
    let mut contour: Vec<P> = Vec::new();
    let mut contours: Vec<Vec<P>> = Vec::new();
    let mut pos: P = [0, 0];
    let mut last_d = 2u8;
    let mut ended = false;
    let mut warned: Vec<String> = Vec::new();
    let warn = |w: &mut Vec<String>, m: String| {
        if !w.contains(&m) {
            w.push(m);
        }
    };

    // Split into blocks: `%...%` extended groups and `...*` normal blocks, remembering lines.
    let mut blocks: Vec<(usize, bool, String)> = Vec::new(); // (line, extended, text)
    let (mut line, mut ext, mut cur) = (1usize, false, String::new());
    let mut cur_line = 1usize;
    for c in text.chars() {
        match c {
            '\n' => {
                line += 1;
            }
            '\r' => {}
            '%' => {
                if ext {
                    if !cur.trim().is_empty() {
                        return Err(GerberError {
                            line,
                            msg: "missing '*' before '%'".into(),
                        });
                    }
                    ext = false;
                } else {
                    ext = true;
                }
                cur.clear();
                cur_line = line;
            }
            '*' => {
                if cur.is_empty() {
                    cur_line = line;
                }
                blocks.push((cur_line, ext, std::mem::take(&mut cur)));
            }
            _ => {
                if cur.is_empty() {
                    cur_line = line;
                }
                cur.push(c);
            }
        }
    }
    if ext || !cur.trim().is_empty() {
        return Err(GerberError {
            line,
            msg: "unterminated block".into(),
        });
    }

    let mut skip_macro = false;
    for (ln, extended, b) in blocks {
        let b = b.trim();
        let err = |m: &str| GerberError {
            line: ln,
            msg: m.into(),
        };
        if b.is_empty() {
            continue;
        }
        if extended {
            if skip_macro {
                // Macro body blocks continue until the group ends; they start with a primitive
                // code, never with a two-letter command, so treat all of them as macro body.
                if b.len() >= 2
                    && b.as_bytes()[0].is_ascii_uppercase()
                    && b.as_bytes()[1].is_ascii_uppercase()
                {
                    skip_macro = false;
                } else {
                    continue;
                }
            }
            if let Some(r) = b.strip_prefix("FS") {
                // FSLAX46Y46
                let r = r.as_bytes();
                if r.len() < 8 || r[0] != b'L' || r[1] != b'A' {
                    return Err(err(
                        "only FSLA (leading zeros omitted, absolute) is supported",
                    ));
                }
                if !r[4].is_ascii_digit() || !r[3].is_ascii_digit() || r[4] - b'0' > 8 {
                    return Err(err("bad coordinate format"));
                }
                let dec = u32::from(r[4] - b'0');
                fmt = Some(Fmt { dec, mm: mm_unit });
            } else if let Some(r) = b.strip_prefix("MO") {
                mm_unit = match r {
                    "MM" => true,
                    "IN" => false,
                    _ => return Err(err("bad MO")),
                };
                if let Some(f) = fmt.as_mut() {
                    f.mm = mm_unit;
                }
            } else if let Some(r) = b.strip_prefix("ADD") {
                let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
                let code: u32 = digits.parse().map_err(|_| err("bad aperture code"))?;
                if code < 10 {
                    return Err(err("aperture codes start at D10"));
                }
                let shape = parse_ap(&r[digits.len()..], mm_unit);
                if let ApShape::Unsupported(n) = &shape {
                    warn(
                        &mut warned,
                        format!(
                            "aperture D{code} uses unsupported template '{n}' and is not drawn"
                        ),
                    );
                }
                if r.contains("X")
                    && matches!(shape, ApShape::Circle { .. })
                    && r.matches('X').count() >= 1
                {
                    warn(&mut warned, "aperture holes are ignored".into());
                }
                aps.push((code, shape, 0));
            } else if b.starts_with("AM") {
                warn(&mut warned, "aperture macros (AM) are not supported".into());
                skip_macro = true;
            } else if let Some(r) = b.strip_prefix("LP") {
                dark = match r {
                    "D" => true,
                    "C" => false,
                    _ => return Err(err("bad LP")),
                };
            } else if let Some(r) = b.strip_prefix("TF.FileFunction,") {
                img.function = Some(r.to_string());
            } else if b.starts_with("SR") && b.len() > 2 {
                warn(&mut warned, "step-and-repeat (SR) is not supported".into());
            } else if b.starts_with("LM") || b.starts_with("LR") || b.starts_with("LS") {
                warn(
                    &mut warned,
                    "aperture mirroring/rotation/scaling is not supported".into(),
                );
            }
            continue;
        }
        if b.starts_with("G04") {
            continue;
        }
        let ws = words(b);
        let mut explicit_d: Option<u8> = None;
        let (mut nx, mut ny, mut ci, mut cj) = (None, None, 0i64, 0i64);
        let mut g_codes = Vec::new();
        let mut d_select: Option<u32> = None;
        for (c, v) in ws {
            match c {
                'G' => g_codes.push(v),
                'X' => nx = Some(v),
                'Y' => ny = Some(v),
                'I' => ci = v,
                'J' => cj = v,
                'D' => {
                    if v <= 3 {
                        explicit_d = Some(v as u8);
                    } else {
                        d_select = Some(v as u32);
                    }
                }
                'M' if v == 2 => ended = true,
                _ => {}
            }
        }
        for g in g_codes {
            match g {
                1 => interp = 1,
                2 => interp = 2,
                3 => interp = 3,
                36 => {
                    in_region = true;
                    contour.clear();
                    contours.clear();
                }
                37 => {
                    if !contour.is_empty() {
                        contours.push(std::mem::take(&mut contour));
                    }
                    if in_region && !contours.is_empty() {
                        img.prims.push(Prim::Region {
                            contours: std::mem::take(&mut contours),
                            dark,
                        });
                    }
                    in_region = false;
                }
                74 => {
                    multi_quadrant = false;
                    warn(
                        &mut warned,
                        "G74 single-quadrant arcs: centre sign is inferred".into(),
                    );
                }
                75 => multi_quadrant = true,
                70 => mm_unit = false,
                71 => mm_unit = true,
                90 => {}
                91 => return Err(err("incremental coordinates (G91) are not supported")),
                _ => {}
            }
        }
        if let Some(code) = d_select {
            cur_ap = aps.iter().position(|a| a.0 == code);
            if cur_ap.is_none() {
                return Err(err(&format!("aperture D{code} is not defined")));
            }
            if nx.is_none() && ny.is_none() {
                continue;
            }
        }
        if nx.is_none() && ny.is_none() && explicit_d.is_none() {
            continue;
        }
        let f = fmt
            .as_ref()
            .ok_or_else(|| err("coordinates before the FS command"))?;
        let target: P = [
            nx.map_or(pos[0], |v| f.nm(v)),
            ny.map_or(pos[1], |v| f.nm(v)),
        ];
        let d = explicit_d.unwrap_or(last_d);
        last_d = d;
        match d {
            2 => {
                if in_region {
                    if !contour.is_empty() {
                        contours.push(std::mem::take(&mut contour));
                    }
                    contour.push(target);
                }
                pos = target;
            }
            1 => {
                let path: Vec<P> = if interp == 1 {
                    vec![target]
                } else {
                    let (ioff, joff) = (f.nm(ci), f.nm(cj));
                    let cw = interp == 2;
                    let c = if multi_quadrant {
                        [(pos[0] + ioff) as f64, (pos[1] + joff) as f64]
                    } else {
                        // Single quadrant: unsigned offsets; pick the sign pair that fits best.
                        let mut best = ([0.0; 2], f64::MAX);
                        for (sx, sy) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
                            let c = [
                                (pos[0] + sx * ioff.abs()) as f64,
                                (pos[1] + sy * joff.abs()) as f64,
                            ];
                            let e = ((pos[0] as f64 - c[0]).hypot(pos[1] as f64 - c[1])
                                - (target[0] as f64 - c[0]).hypot(target[1] as f64 - c[1]))
                            .abs();
                            if e < best.1 {
                                best = (c, e);
                            }
                        }
                        best.0
                    };
                    arc_points(pos, target, c, cw)
                };
                if in_region {
                    if contour.is_empty() {
                        contour.push(pos);
                    }
                    contour.extend(path);
                } else {
                    let i = cur_ap.ok_or_else(|| err("draw without a selected aperture"))?;
                    aps[i].2 += 1;
                    if matches!(aps[i].1, ApShape::Polygon { .. } | ApShape::Obround { .. }) {
                        warn(
                            &mut warned,
                            "strokes with polygon/obround apertures are approximated by their hull"
                                .into(),
                        );
                    }
                    let mut from = pos;
                    for q in path {
                        img.prims.push(Prim::Stroke {
                            a: from,
                            b: q,
                            ap: aps[i].1.clone(),
                            dark,
                        });
                        from = q;
                    }
                }
                pos = target;
            }
            3 => {
                let i = cur_ap.ok_or_else(|| err("flash without a selected aperture"))?;
                aps[i].2 += 1;
                img.prims.push(Prim::Flash {
                    ap: aps[i].1.clone(),
                    at: target,
                    dark,
                });
                pos = target;
            }
            _ => return Err(err("bad D code")),
        }
        if ended {
            break;
        }
    }
    if in_region {
        return Err(GerberError {
            line: 0,
            msg: "region (G36) was never closed with G37".into(),
        });
    }
    if !ended {
        warn(
            &mut warned,
            "the file has no M02 end-of-file command".into(),
        );
    }
    img.apertures = aps
        .into_iter()
        .map(|(code, shape, uses)| ApInfo { code, shape, uses })
        .collect();
    img.warnings = warned;
    Ok(img)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    const G: &str = "%TF.FileFunction,Copper,L1,Top*%\n%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.800000*%\n%ADD11R,1.600000X0.900000*%\n%LPD*%\nG01*\nD10*\nX1234567Y-2000001D03*\nX0Y0D02*\nX10000000Y5D01*\nD11*\nX5Y6D03*\nG36*\nX0Y0D02*\nX1000000Y0D01*\nX1000000Y1000000D01*\nX0Y0D01*\nG37*\nM02*\n";

    #[test]
    fn reads_flashes_strokes_regions_and_apertures() {
        let i = parse(G).unwrap();
        assert_eq!(i.function.as_deref(), Some("Copper,L1,Top"));
        assert_eq!(i.count(), (2, 1, 1));
        assert_eq!(
            i.prims[0],
            Prim::Flash {
                ap: ApShape::Circle { d: 800_000 },
                at: [1_234_567, -2_000_001],
                dark: true
            }
        );
        assert_eq!(i.apertures.len(), 2);
        assert_eq!((i.apertures[0].uses, i.apertures[1].uses), (2, 1));
        assert!(i.warnings.is_empty());
        let (lo, hi) = i.bounds().unwrap();
        assert_eq!(lo[0], -799_995);
        assert_eq!(lo[1], -2_400_001);
        assert!(hi[0] >= 10_000_000);
    }

    #[test]
    fn arcs_polarity_and_inches() {
        let g = "%FSLAX24Y24*%\n%MOIN*%\n%ADD10C,0.0100*%\n%LPC*%\nD10*\nG75*\nG03*\nX10000Y0D02*\nX0Y10000I-10000J0D01*\nM02*\n";
        let i = parse(g).unwrap();
        // A quarter circle of radius 1 inch, counter-clockwise, ends on the exact end point.
        let strokes: Vec<_> = i.prims.iter().collect();
        assert!(strokes.len() > 10);
        let Prim::Stroke { a, dark, .. } = strokes[0] else {
            panic!()
        };
        assert_eq!(*a, [25_400_000, 0]);
        assert!(!dark);
        let Prim::Stroke { b, .. } = strokes[strokes.len() - 1] else {
            panic!()
        };
        assert_eq!(*b, [0, 25_400_000]);
        // Midpoint of the arc is on the circle.
        let Prim::Stroke { b: mid, .. } = strokes[strokes.len() / 2] else {
            panic!()
        };
        let r = ((mid[0] as f64).hypot(mid[1] as f64) - 25_400_000.0).abs();
        assert!(r < 2_000.0, "{r}");
    }

    #[test]
    fn errors_and_warnings() {
        assert!(parse("X1Y1D03*\nM02*").is_err());
        assert!(parse("%FSLAX46Y46*%\n%MOMM*%\nD99*\n").is_err());
        assert!(parse("%FSLAX46Y46*%\n%MOMM*%\nG36*\nM02*").is_err());
        assert!(parse("%FSTAX46Y46*%").is_err());
        let i =
            parse("%FSLAX46Y46*%\n%MOMM*%\n%AMX*\n1,1,0.5,0,0*%\n%ADD10X,1*%\nD10*\nX0Y0D03*\n")
                .unwrap();
        assert!(i.warnings.iter().any(|w| w.contains("macros")));
        assert!(i.warnings.iter().any(|w| w.contains("M02")));
        assert!(matches!(
            i.prims[0],
            Prim::Flash {
                ap: ApShape::Unsupported(_),
                ..
            }
        ));
    }
}
