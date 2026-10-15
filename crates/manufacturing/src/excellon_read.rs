//! Excellon drill **reader** (metric or inch, decimal coordinates), independent of the writer.
//! Coordinates without a decimal point need a format declaration this reader does not implement:
//! they are an error rather than a guess.

use crate::gerber_read::P;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub tool: u32,
    pub at: P,
    pub slot_to: Option<P>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrillFile {
    pub function: Option<String>,
    /// `(tool number, diameter nm)`.
    pub tools: Vec<(u32, i64)>,
    pub hits: Vec<Hit>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Excellon line {line}: {msg}")]
pub struct DrillError {
    pub line: usize,
    pub msg: String,
}

impl DrillFile {
    pub fn diameter(&self, tool: u32) -> Option<i64> {
        self.tools.iter().find(|t| t.0 == tool).map(|t| t.1)
    }

    pub fn bounds(&self) -> Option<(P, P)> {
        let pts: Vec<P> = self
            .hits
            .iter()
            .flat_map(|h| std::iter::once(h.at).chain(h.slot_to))
            .collect();
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        }))
    }
}

fn num(s: &str, mm: bool) -> Option<i64> {
    if !s.contains('.') {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    let nm = (v * if mm { 1e6 } else { 25.4e6 }).round();
    // Beyond ±1000 m is not a circuit board: refused (the caller reports "bad coordinates").
    (nm.abs() <= 1e12).then_some(nm as i64)
}

/// Splits `X1.5Y-2.0` style text into `(letter, text)`.
fn fields(s: &str) -> Vec<(char, String)> {
    let mut out: Vec<(char, String)> = Vec::new();
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            out.push((c, String::new()));
        } else if let Some(l) = out.last_mut() {
            l.1.push(c);
        }
    }
    out
}

pub fn parse(text: &str) -> Result<DrillFile, DrillError> {
    let mut f = DrillFile::default();
    let mut mm = true;
    let mut header = true;
    let mut tool: Option<u32> = None;
    let mut ended = false;
    for (i, raw) in text.lines().enumerate() {
        let ln = i + 1;
        let err = |m: &str| DrillError {
            line: ln,
            msg: m.into(),
        };
        let l = raw.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(c) = l.strip_prefix(';') {
            if let Some(r) = c.trim().strip_prefix("#@! TF.FileFunction,") {
                f.function = Some(r.to_string());
            }
            continue;
        }
        match l {
            "M48" => continue,
            "%" | "M95" => {
                header = false;
                continue;
            }
            "M30" | "M00" => {
                ended = true;
                break;
            }
            "G90" | "G05" | "FMAT,2" | "VER,1" => continue,
            "METRIC" | "METRIC,TZ" | "METRIC,LZ" => {
                mm = true;
                continue;
            }
            "INCH" | "INCH,TZ" | "INCH,LZ" => {
                mm = false;
                continue;
            }
            _ => {}
        }
        if let Some(r) = l.strip_prefix('T') {
            let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
            let n: u32 = digits.parse().map_err(|_| err("bad tool number"))?;
            let rest = &r[digits.len()..];
            if let Some(d) = rest.strip_prefix('C') {
                let d = num(d, mm)
                    .filter(|d| *d > 0)
                    .ok_or_else(|| err("bad tool diameter"))?;
                f.tools.push((n, d));
            } else if rest.is_empty() {
                if f.diameter(n).is_none() {
                    return Err(err(&format!("tool T{n} is not defined")));
                }
                tool = Some(n);
                header = false;
            } else {
                return Err(err("unsupported tool command"));
            }
            continue;
        }
        if header {
            continue;
        }
        if l.starts_with('X') || l.starts_with('Y') {
            let t = tool.ok_or_else(|| err("hole before a tool selection"))?;
            let (a, b) = match l.split_once("G85") {
                Some((a, b)) => (a, Some(b)),
                None => (l, None),
            };
            let pt = |s: &str| -> Result<P, DrillError> {
                let (mut x, mut y) = (None, None);
                for (c, v) in fields(s) {
                    match c {
                        'X' => x = num(&v, mm),
                        'Y' => y = num(&v, mm),
                        _ => return Err(err("unexpected field")),
                    }
                }
                match (x, y) {
                    (Some(x), Some(y)) => Ok([x, y]),
                    _ => Err(err("coordinates need X, Y and a decimal point")),
                }
            };
            let at = pt(a)?;
            let slot_to = b.map(pt).transpose()?;
            f.hits.push(Hit {
                tool: t,
                at,
                slot_to,
            });
            continue;
        }
        if l.starts_with('G') || l.starts_with('M') {
            f.warnings.push(format!(
                "command '{l}' is not supported (routed slots are not drawn)"
            ));
            continue;
        }
        return Err(err("unrecognised line"));
    }
    if !ended {
        f.warnings.push("the file has no M30 end command".into());
    }
    Ok(f)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_tools_hits_and_slots() {
        let t = "M48\n; #@! TF.FileFunction,Plated,1,2,PTH\nMETRIC\nT1C0.300000\nT2C0.800000\n%\nG90\nG05\nT1\nX3.000000Y2.000000\nT2\nX0.000000Y0.000000G85X2.000000Y0.000000\nX-1.500001Y0.000007\nM30\n";
        let d = parse(t).unwrap();
        assert_eq!(d.function.as_deref(), Some("Plated,1,2,PTH"));
        assert_eq!(d.tools, vec![(1, 300_000), (2, 800_000)]);
        assert_eq!(d.hits.len(), 3);
        assert_eq!(d.hits[1].slot_to, Some([2_000_000, 0]));
        assert_eq!(d.hits[2].at, [-1_500_001, 7]);
        assert!(d.warnings.is_empty());
        assert_eq!(d.bounds().unwrap().0, [-1_500_001, 0]);
    }

    #[test]
    fn errors() {
        assert!(parse("M48\nMETRIC\nT1C0.3\n%\nT1\nX1Y2\nM30\n").is_err());
        assert!(parse("M48\nMETRIC\n%\nX1.0Y2.0\nM30\n").is_err());
        assert!(parse("M48\nMETRIC\n%\nT5\nM30\n").is_err());
        assert!(
            parse("M48\nMETRIC\nT1C0.3\n%\nT1\nX1.0Y2.0\n")
                .unwrap()
                .warnings
                .len()
                == 1
        );
    }
}
