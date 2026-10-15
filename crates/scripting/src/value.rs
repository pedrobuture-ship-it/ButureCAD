//! Values and the unit system.
//!
//! Every number is a [`Qty`]: a magnitude in the base unit of its dimension (nm, V, A, ohm, Hz)
//! and a dimension vector. Adding or comparing different dimensions is an error; multiplying and
//! dividing combine dimensions. Lengths convert to integer nanometres at the boundary and
//! anything that is not a whole number of nanometres is rejected instead of being rounded.

use std::fmt;

/// Exponents of (length, voltage, current, time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dim(pub [i8; 4]);

impl Dim {
    pub const NONE: Dim = Dim([0, 0, 0, 0]);
    pub const LENGTH: Dim = Dim([1, 0, 0, 0]);
    pub const VOLT: Dim = Dim([0, 1, 0, 0]);
    pub const AMP: Dim = Dim([0, 0, 1, 0]);
    pub const OHM: Dim = Dim([0, 1, -1, 0]);
    pub const HZ: Dim = Dim([0, 0, 0, -1]);

    pub fn times(self, o: Dim) -> Dim {
        let mut r = [0i8; 4];
        for (i, v) in r.iter_mut().enumerate() {
            *v = self.0[i].saturating_add(o.0[i]);
        }
        Dim(r)
    }

    pub fn inv(self) -> Dim {
        Dim(self.0.map(|e| -e))
    }

    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named = [
            (Dim::NONE, "a plain number"),
            (Dim::LENGTH, "length"),
            (Dim::VOLT, "voltage"),
            (Dim::AMP, "current"),
            (Dim::OHM, "resistance"),
            (Dim::HZ, "frequency"),
        ];
        if let Some((_, n)) = named.iter().find(|(d, _)| d == self) {
            return write!(f, "{n}");
        }
        let names = ["length", "voltage", "current", "time"];
        let parts: Vec<String> = self
            .0
            .iter()
            .enumerate()
            .filter(|(_, e)| **e != 0)
            .map(|(i, e)| {
                if *e == 1 {
                    names[i].to_string()
                } else {
                    format!("{}^{e}", names[i])
                }
            })
            .collect();
        write!(f, "{}", parts.join("·"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Qty {
    pub v: f64,
    pub dim: Dim,
}

impl Qty {
    pub fn plain(v: f64) -> Self {
        Self { v, dim: Dim::NONE }
    }
}

/// `(factor to the base unit, dimension)`.
pub fn unit(name: &str) -> Option<(f64, Dim)> {
    Some(match name {
        "nm" => (1.0, Dim::LENGTH),
        "um" | "µm" => (1e3, Dim::LENGTH),
        "mm" => (1e6, Dim::LENGTH),
        "mil" => (25_400.0, Dim::LENGTH),
        "inch" => (25_400_000.0, Dim::LENGTH),
        "V" => (1.0, Dim::VOLT),
        "mV" => (1e-3, Dim::VOLT),
        "A" => (1.0, Dim::AMP),
        "mA" => (1e-3, Dim::AMP),
        "ohm" | "Ohm" | "Ω" => (1.0, Dim::OHM),
        "kOhm" | "kohm" | "kΩ" => (1e3, Dim::OHM),
        "MOhm" | "Mohm" | "MΩ" => (1e6, Dim::OHM),
        "Hz" => (1.0, Dim::HZ),
        "kHz" => (1e3, Dim::HZ),
        "MHz" => (1e6, Dim::HZ),
        "GHz" => (1e9, Dim::HZ),
        _ => return None,
    })
}

pub const UNIT_LIST: &str = "nm um mm mil inch V mV A mA ohm kOhm MOhm Hz kHz MHz GHz";

/// A length in whole nanometres, or why it is not one.
pub fn to_nm(q: Qty) -> Result<i64, String> {
    if q.dim != Dim::LENGTH {
        return Err(format!("expected a length, found {}", q.dim));
    }
    let r = q.v.round();
    if (q.v - r).abs() > 1e-3 {
        return Err(format!(
            "{} nm is not a whole number of nanometres (the core stores integer nm)",
            q.v
        ));
    }
    if r.abs() > 1_073_741_824.0 {
        return Err("length is out of range (the core's coordinates stop at ±1073 mm)".into());
    }
    Ok(r as i64)
}

fn trim(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_owned()
    }
}

/// Text for a component value: SPICE-friendly engineering notation.
pub fn format_qty(q: Qty) -> String {
    let eng = |v: f64, unit: &str| -> String {
        let a = v.abs();
        let (scale, p) = if a == 0.0 {
            (1.0, "")
        } else if a >= 1e9 {
            (1e9, "G")
        } else if a >= 1e6 {
            (1e6, "M")
        } else if a >= 1e3 {
            (1e3, "k")
        } else if a >= 1.0 {
            (1.0, "")
        } else if a >= 1e-3 {
            (1e-3, "m")
        } else {
            (1e-6, "u")
        };
        format!("{}{p}{unit}", trim(v / scale))
    };
    match q.dim {
        d if d == Dim::NONE => trim(q.v),
        d if d == Dim::OHM => eng(q.v, ""),
        d if d == Dim::VOLT => eng(q.v, "V"),
        d if d == Dim::AMP => eng(q.v, "A"),
        d if d == Dim::HZ => eng(q.v, "Hz"),
        d if d == Dim::LENGTH => format!("{}mm", trim(q.v / 1e6)),
        d => format!("{} ({d})", trim(q.v)),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Q(Qty),
    Str(String),
    Bool(bool),
    Tuple(Vec<Val>),
    List(Vec<Val>),
}

impl Val {
    pub fn type_name(&self) -> String {
        match self {
            Val::Q(q) => q.dim.to_string(),
            Val::Str(_) => "a string".into(),
            Val::Bool(_) => "a boolean".into(),
            Val::Tuple(_) => "a point".into(),
            Val::List(_) => "a list".into(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn units_and_formatting() {
        assert_eq!(unit("mil"), Some((25_400.0, Dim::LENGTH)));
        assert_eq!(unit("kOhm").unwrap().1, Dim::OHM);
        assert!(unit("furlong").is_none());
        assert_eq!(Dim::LENGTH.times(Dim::LENGTH).to_string(), "length^2");
        assert_eq!(Dim::VOLT.times(Dim::AMP.inv()), Dim::OHM);
        assert_eq!(
            format_qty(Qty {
                v: 4700.0,
                dim: Dim::OHM
            }),
            "4.7k"
        );
        assert_eq!(
            format_qty(Qty {
                v: 0.0033,
                dim: Dim::VOLT
            }),
            "3.3mV"
        );
        assert_eq!(
            format_qty(Qty {
                v: 2_540_000.0,
                dim: Dim::LENGTH
            }),
            "2.54mm"
        );
        assert_eq!(format_qty(Qty::plain(3.0)), "3");
    }

    #[test]
    fn lengths_are_exact_or_rejected() {
        assert_eq!(
            to_nm(Qty {
                v: 2.54 * 1e6,
                dim: Dim::LENGTH
            }),
            Ok(2_540_000)
        );
        assert_eq!(
            to_nm(Qty {
                v: 10.0 * 25_400.0,
                dim: Dim::LENGTH
            }),
            Ok(254_000)
        );
        assert!(to_nm(Qty {
            v: 0.1,
            dim: Dim::LENGTH
        })
        .unwrap_err()
        .contains("whole number"));
        assert!(to_nm(Qty::plain(5.0))
            .unwrap_err()
            .contains("expected a length"));
        assert!(to_nm(Qty {
            v: 1e15,
            dim: Dim::LENGTH
        })
        .is_err());
    }
}
