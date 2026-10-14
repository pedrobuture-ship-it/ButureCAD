//! The simulator's input: a flat netlist, independent of the schematic and of any backend.

use crate::SimError;
use std::f64::consts::PI;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    R,
    C,
    L,
    V,
    I,
    D,
    Q,
    M,
    X,
}

impl Kind {
    pub fn from_prefix(c: char) -> Option<Self> {
        Some(match c.to_ascii_uppercase() {
            'R' => Kind::R,
            'C' => Kind::C,
            'L' => Kind::L,
            'V' => Kind::V,
            'I' => Kind::I,
            'D' => Kind::D,
            'Q' => Kind::Q,
            'M' => Kind::M,
            'X' => Kind::X,
            _ => return None,
        })
    }

    pub fn letter(self) -> char {
        match self {
            Kind::R => 'R',
            Kind::C => 'C',
            Kind::L => 'L',
            Kind::V => 'V',
            Kind::I => 'I',
            Kind::D => 'D',
            Kind::Q => 'Q',
            Kind::M => 'M',
            Kind::X => 'X',
        }
    }

    pub fn nodes_needed(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Kind::R | Kind::C | Kind::L | Kind::V | Kind::I | Kind::D => 2..=2,
            Kind::Q => 3..=4,
            Kind::M => 4..=4,
            Kind::X => 1..=64,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Wave {
    Sin {
        offset: f64,
        amp: f64,
        freq: f64,
        delay: f64,
        damping: f64,
        phase_deg: f64,
    },
    Pulse {
        v1: f64,
        v2: f64,
        delay: f64,
        rise: f64,
        fall: f64,
        width: f64,
        period: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceSpec {
    pub dc: f64,
    pub ac_mag: f64,
    pub ac_phase_deg: f64,
    pub wave: Option<Wave>,
}

impl SourceSpec {
    /// Value at time `t` for a transient analysis.
    pub fn at(&self, t: f64) -> f64 {
        match &self.wave {
            None => self.dc,
            Some(Wave::Sin {
                offset,
                amp,
                freq,
                delay,
                damping,
                phase_deg,
            }) => {
                if t < *delay {
                    offset + amp * (phase_deg.to_radians()).sin()
                } else {
                    let tt = t - delay;
                    offset
                        + amp
                            * (-tt * damping).exp()
                            * (2.0 * PI * freq * tt + phase_deg.to_radians()).sin()
                }
            }
            Some(Wave::Pulse {
                v1,
                v2,
                delay,
                rise,
                fall,
                width,
                period,
            }) => {
                if t < *delay {
                    return *v1;
                }
                let mut tt = t - delay;
                if *period > 0.0 {
                    tt %= period;
                }
                if tt < *rise {
                    if *rise <= 0.0 {
                        *v2
                    } else {
                        v1 + (v2 - v1) * tt / rise
                    }
                } else if tt < rise + width {
                    *v2
                } else if tt < rise + width + fall {
                    if *fall <= 0.0 {
                        *v1
                    } else {
                        v2 + (v1 - v2) * (tt - rise - width) / fall
                    }
                } else {
                    *v1
                }
            }
        }
    }

    /// Parses `5`, `5V`, `DC 5`, `AC 1`, `DC 0 AC 1 90`, `SIN(0 1 1k)`, `PULSE(0 5 0 1n 1n 5m 10m)`.
    pub fn parse(text: &str) -> Result<Self, SimError> {
        let bad = |m: String| SimError::Circuit(m);
        let up = text.trim().to_string();
        let mut spec = SourceSpec::default();
        // Split off a function-style waveform (`SIN(...)`, `PULSE(...)`).
        let upper = up.to_ascii_uppercase();
        let wave_at = ["SIN", "PULSE"]
            .iter()
            .filter_map(|k| {
                upper
                    .find(k)
                    .filter(|i| upper[i + k.len()..].trim_start().starts_with('('))
            })
            .min();
        let (head, wave) = match wave_at {
            Some(i) => (up[..i].to_string(), Some(up[i..].to_string())),
            None => (up.clone(), None),
        };
        let toks: Vec<&str> = head.split_whitespace().collect();
        let mut i = 0;
        let mut first = true;
        while i < toks.len() {
            match toks[i].to_ascii_uppercase().as_str() {
                "DC" => {
                    spec.dc = parse_value(
                        toks.get(i + 1)
                            .ok_or_else(|| bad("DC needs a value".into()))?,
                    )?;
                    i += 2;
                }
                "AC" => {
                    spec.ac_mag = parse_value(
                        toks.get(i + 1)
                            .ok_or_else(|| bad("AC needs a magnitude".into()))?,
                    )?;
                    i += 2;
                    if let Some(p) = toks.get(i).and_then(|t| parse_value(t).ok()) {
                        spec.ac_phase_deg = p;
                        i += 1;
                    }
                }
                _ if first => {
                    spec.dc = parse_value(toks[i])?;
                    i += 1;
                }
                other => return Err(bad(format!("unexpected “{other}” in a source value"))),
            }
            first = false;
        }
        if let Some(w) = wave {
            let open = w
                .find('(')
                .ok_or_else(|| bad("waveform needs (...)".into()))?;
            let close = w
                .rfind(')')
                .ok_or_else(|| bad("waveform needs a closing )".into()))?;
            let args: Vec<f64> = w[open + 1..close]
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(parse_value)
                .collect::<Result<_, _>>()?;
            let a = |k: usize, d: f64| args.get(k).copied().unwrap_or(d);
            spec.wave = Some(if w[..open].trim().eq_ignore_ascii_case("SIN") {
                if args.len() < 3 {
                    return Err(bad(
                        "SIN needs at least offset, amplitude and frequency".into()
                    ));
                }
                Wave::Sin {
                    offset: a(0, 0.0),
                    amp: a(1, 0.0),
                    freq: a(2, 0.0),
                    delay: a(3, 0.0),
                    damping: a(4, 0.0),
                    phase_deg: a(5, 0.0),
                }
            } else {
                if args.len() < 7 {
                    return Err(bad("PULSE needs v1 v2 delay rise fall width period".into()));
                }
                Wave::Pulse {
                    v1: a(0, 0.0),
                    v2: a(1, 0.0),
                    delay: a(2, 0.0),
                    rise: a(3, 0.0),
                    fall: a(4, 0.0),
                    width: a(5, 0.0),
                    period: a(6, 0.0),
                }
            });
            if let Some(Wave::Sin { offset, .. }) = &spec.wave {
                spec.dc = *offset;
            } else if let Some(Wave::Pulse { v1, .. }) = &spec.wave {
                spec.dc = *v1;
            }
        }
        Ok(spec)
    }
}

/// SPICE number with engineering suffix: `4.7k`, `100n`, `1meg`, `10u`, `2.2µ`. Trailing unit
/// letters (`F H V A Ω ohm s Hz`) are ignored. As in SPICE, `m`/`M` is **milli**; mega is `meg`.
/// A lone `F` is the unit farad, not femto.
pub fn parse_value(s: &str) -> Result<f64, SimError> {
    let t = s.trim();
    let split = t
        .char_indices()
        .find(|(i, c)| {
            !(c.is_ascii_digit()
                || *c == '.'
                || *c == '+'
                || *c == '-'
                || ((*c == 'e' || *c == 'E')
                    && t[i + 1..]
                        .starts_with(|n: char| n.is_ascii_digit() || n == '-' || n == '+')))
        })
        .map_or(t.len(), |(i, _)| i);
    let num: f64 = t[..split]
        .parse()
        .map_err(|_| SimError::Circuit(format!("“{s}” is not a number")))?;
    let rest = t[split..].trim();
    let low = rest.to_lowercase();
    let mult = if low.starts_with("meg") {
        1e6
    } else if low.starts_with("mil") {
        25.4e-6
    } else {
        match low.chars().next() {
            Some('t') => 1e12,
            Some('g') => 1e9,
            Some('k') => 1e3,
            Some('m') => 1e-3,
            Some('u') | Some('µ') | Some('μ') => 1e-6,
            Some('n') => 1e-9,
            Some('p') => 1e-12,
            // `f` is femto only when more than the unit letter follows is unlikely; a lone F is farad.
            Some('f') if low.len() > 1 && !low.starts_with("fa") => 1e-15,
            _ => 1.0,
        }
    };
    let v = num * mult;
    if !v.is_finite() {
        return Err(SimError::Circuit(format!("“{s}” is not a finite number")));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Source(SourceSpec),
    /// Model name for D/Q/M, subcircuit name for X.
    Model(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// Full SPICE name, starting with the kind letter (`R1`, `Vin`).
    pub name: String,
    pub kind: Kind,
    /// Node names; `0` is ground.
    pub nodes: Vec<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Circuit {
    pub title: String,
    pub elements: Vec<Element>,
    /// Raw `.model` / `.subckt` / `.include` text supplied by the user's SPICE associations.
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Analysis {
    Op,
    DcSweep {
        source: String,
        start: f64,
        stop: f64,
        step: f64,
    },
    /// Decade sweep with `points_per_decade` points.
    Ac {
        points_per_decade: u32,
        start_hz: f64,
        stop_hz: f64,
    },
    Tran {
        step: f64,
        stop: f64,
    },
}

impl Analysis {
    pub fn name(&self) -> &'static str {
        match self {
            Analysis::Op => "Operating point",
            Analysis::DcSweep { .. } => "DC sweep",
            Analysis::Ac { .. } => "AC",
            Analysis::Tran { .. } => "Transient",
        }
    }

    pub fn validate(&self) -> Result<(), SimError> {
        let bad = |m: &str| Err(SimError::Circuit(m.to_owned()));
        match self {
            Analysis::Op => {}
            Analysis::DcSweep {
                source,
                start,
                stop,
                step,
            } => {
                if source.is_empty() {
                    return bad("choose the source to sweep");
                }
                if *step == 0.0
                    || (stop - start) * step < 0.0
                    || !(start.is_finite() && stop.is_finite() && step.is_finite())
                {
                    return bad("the sweep step must be non-zero and point from start to stop");
                }
                if ((stop - start) / step).abs() > 100_000.0 {
                    return bad("the sweep has more than 100000 points");
                }
            }
            Analysis::Ac {
                points_per_decade,
                start_hz,
                stop_hz,
            } => {
                if *points_per_decade == 0
                    || *points_per_decade > 1000
                    || *start_hz <= 0.0
                    || stop_hz <= start_hz
                {
                    return bad("AC needs 1–1000 points per decade and 0 < start < stop");
                }
            }
            Analysis::Tran { step, stop } => {
                if *step <= 0.0 || *stop <= 0.0 || !(step.is_finite() && stop.is_finite()) {
                    return bad("transient needs a positive step and stop time");
                }
                if stop / step > 200_000.0 {
                    return bad("the transient has more than 200000 steps; increase the step");
                }
            }
        }
        Ok(())
    }
}

impl Circuit {
    pub fn validate(&self) -> Result<(), SimError> {
        let bad = |m: String| Err(SimError::Circuit(m));
        if self.elements.is_empty() {
            return bad("the circuit has no simulatable elements".into());
        }
        let mut names = std::collections::BTreeSet::new();
        for e in &self.elements {
            if !names.insert(e.name.to_ascii_uppercase()) {
                return bad(format!("two elements are named {}", e.name));
            }
            if !e.name.starts_with(e.kind.letter()) {
                return bad(format!(
                    "{} does not start with {}",
                    e.name,
                    e.kind.letter()
                ));
            }
            if e.name.chars().any(|c| c.is_whitespace() || c.is_control())
                || e.nodes
                    .iter()
                    .any(|n| n.is_empty() || n.chars().any(|c| c.is_whitespace() || c.is_control()))
            {
                return bad(format!("{} has an invalid name or node", e.name));
            }
            if !e.kind.nodes_needed().contains(&e.nodes.len()) {
                return bad(format!(
                    "{} has {} connected node(s)",
                    e.name,
                    e.nodes.len()
                ));
            }
        }
        if !self
            .elements
            .iter()
            .any(|e| e.nodes.iter().any(|n| n == "0"))
        {
            return bad("no node is ground (name a net GND or 0)".into());
        }
        Ok(())
    }

    /// The circuit as SPICE text, for ngspice and for the user to read.
    pub fn to_spice(&self, analysis: &Analysis) -> String {
        let mut o = String::new();
        let _ = writeln!(o, "* {}", self.title.replace('\n', " "));
        for e in &self.elements {
            let _ = write!(o, "{} {}", e.name, e.nodes.join(" "));
            match &e.value {
                Value::Number(v) => {
                    let _ = write!(o, " {v:e}");
                }
                Value::Model(m) => {
                    let _ = write!(o, " {m}");
                }
                Value::Source(s) => {
                    let _ = write!(o, " DC {:e}", s.dc);
                    if s.ac_mag != 0.0 {
                        let _ = write!(o, " AC {:e} {:e}", s.ac_mag, s.ac_phase_deg);
                    }
                    match &s.wave {
                        Some(Wave::Sin {
                            offset,
                            amp,
                            freq,
                            delay,
                            damping,
                            phase_deg,
                        }) => {
                            let _ = write!(o, " SIN({offset:e} {amp:e} {freq:e} {delay:e} {damping:e} {phase_deg:e})");
                        }
                        Some(Wave::Pulse {
                            v1,
                            v2,
                            delay,
                            rise,
                            fall,
                            width,
                            period,
                        }) => {
                            let _ = write!(o, " PULSE({v1:e} {v2:e} {delay:e} {rise:e} {fall:e} {width:e} {period:e})");
                        }
                        None => {}
                    }
                }
            }
            o.push('\n');
        }
        for m in &self.models {
            let _ = writeln!(o, "{}", m.trim_end());
        }
        let _ = match analysis {
            Analysis::Op => writeln!(o, ".op"),
            Analysis::DcSweep {
                source,
                start,
                stop,
                step,
            } => writeln!(o, ".dc {source} {start:e} {stop:e} {step:e}"),
            Analysis::Ac {
                points_per_decade,
                start_hz,
                stop_hz,
            } => writeln!(o, ".ac dec {points_per_decade} {start_hz:e} {stop_hz:e}"),
            Analysis::Tran { step, stop } => writeln!(o, ".tran {step:e} {stop:e}"),
        };
        o.push_str(".end\n");
        o
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn spice_numbers() {
        for (s, v) in [
            ("10k", 1e4),
            ("4.7kΩ", 4700.0),
            ("100nF", 1e-7),
            ("1meg", 1e6),
            ("1MEG", 1e6),
            ("2.2u", 2.2e-6),
            ("2.2µF", 2.2e-6),
            ("5V", 5.0),
            ("1F", 1.0),
            ("3mA", 3e-3),
            ("1e3", 1e3),
            ("-2.5", -2.5),
            ("10pF", 1e-11),
            ("1mil", 25.4e-6),
        ] {
            let got = parse_value(s).unwrap();
            assert!((got - v).abs() <= v.abs() * 1e-12, "{s}: {got} != {v}");
        }
        assert!(parse_value("abc").is_err());
        assert!(parse_value("").is_err());
    }

    #[test]
    fn source_specs() {
        assert_eq!(SourceSpec::parse("5V").unwrap().dc, 5.0);
        let s = SourceSpec::parse("DC 1.5 AC 2 90").unwrap();
        assert_eq!((s.dc, s.ac_mag, s.ac_phase_deg), (1.5, 2.0, 90.0));
        let s = SourceSpec::parse("SIN(0 1 1k)").unwrap();
        assert!(matches!(s.wave, Some(Wave::Sin { freq, .. }) if freq == 1000.0));
        assert!((s.at(0.00025) - 1.0).abs() < 1e-9);
        let p = SourceSpec::parse("PULSE(0 5 1m 1u 1u 2m 10m)").unwrap();
        assert_eq!((p.at(0.0), p.at(0.002), p.at(0.005)), (0.0, 5.0, 0.0));
        assert!((p.at(0.0010005) - 2.5).abs() < 1e-6, "mid-rise");
        assert_eq!(p.at(0.0120), 5.0, "second period");
        let a = SourceSpec::parse("AC 1").unwrap();
        assert_eq!((a.dc, a.ac_mag), (0.0, 1.0));
        for bad in ["SIN(0 1)", "PULSE(1 2 3)", "DC", "5V banana", "SIN(0 1 x)"] {
            assert!(SourceSpec::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn validation_and_spice_text() {
        let c = Circuit {
            title: "rc".into(),
            elements: vec![
                Element {
                    name: "V1".into(),
                    kind: Kind::V,
                    nodes: vec!["in".into(), "0".into()],
                    value: Value::Source(SourceSpec {
                        dc: 0.0,
                        ac_mag: 1.0,
                        ..Default::default()
                    }),
                },
                Element {
                    name: "R1".into(),
                    kind: Kind::R,
                    nodes: vec!["in".into(), "out".into()],
                    value: Value::Number(1000.0),
                },
                Element {
                    name: "D1".into(),
                    kind: Kind::D,
                    nodes: vec!["out".into(), "0".into()],
                    value: Value::Model("D1N4148".into()),
                },
            ],
            models: vec![".model D1N4148 D(IS=1e-14)".into()],
        };
        c.validate().unwrap();
        let t = c.to_spice(&Analysis::Ac {
            points_per_decade: 10,
            start_hz: 1.0,
            stop_hz: 1e6,
        });
        assert!(
            t.contains("V1 in 0 DC 0e0 AC 1e0 0e0\n")
                && t.contains("R1 in out 1e3\n")
                && t.contains("D1 out 0 D1N4148\n")
        );
        assert!(
            t.contains(".model D1N4148")
                && t.contains(".ac dec 10 1e0 1e6\n")
                && t.ends_with(".end\n")
        );
        let mut bad = c.clone();
        bad.elements[1].name = "V1".into();
        assert!(bad.validate().is_err());
        let mut bad = c.clone();
        for e in &mut bad.elements {
            e.nodes.retain(|n| n != "0");
        }
        assert!(bad.validate().is_err(), "no ground");
        assert!(Analysis::Tran {
            step: 1.0,
            stop: 1e9
        }
        .validate()
        .is_err());
        assert!(Analysis::DcSweep {
            source: "V1".into(),
            start: 0.0,
            stop: 5.0,
            step: -1.0
        }
        .validate()
        .is_err());
    }
}
