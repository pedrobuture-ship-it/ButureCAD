//! Bus naming: `PREFIX[first..last]`, e.g. `D[0..7]` → `D0` … `D7`.

use thiserror::Error;

/// Upper bound on bus width (guards against hostile or accidental `D[0..99999999]`).
pub const MAX_BUS_WIDTH: u32 = 4096;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum BusNameError {
    #[error("not a bus name (expected PREFIX[first..last])")]
    NotABus,
    #[error("bus range must be two non-negative integers with first <= last")]
    BadRange,
    #[error("bus is wider than {MAX_BUS_WIDTH} signals")]
    TooWide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusDef {
    pub prefix: String,
    pub first: u32,
    pub last: u32,
}

impl BusDef {
    pub fn parse(name: &str) -> Result<BusDef, BusNameError> {
        let name = name.trim();
        let open = name.find('[').ok_or(BusNameError::NotABus)?;
        let inner = name[open + 1..]
            .strip_suffix(']')
            .ok_or(BusNameError::NotABus)?;
        let (a, b) = inner.split_once("..").ok_or(BusNameError::BadRange)?;
        let (first, last) = match (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
            (Ok(a), Ok(b)) if a <= b => (a, b),
            _ => return Err(BusNameError::BadRange),
        };
        if last - first >= MAX_BUS_WIDTH {
            return Err(BusNameError::TooWide);
        }
        let prefix = name[..open].to_owned();
        if prefix.is_empty() {
            return Err(BusNameError::NotABus);
        }
        Ok(BusDef {
            prefix,
            first,
            last,
        })
    }

    /// Signal names `PREFIX{n}` in ascending order.
    pub fn members(&self) -> Vec<String> {
        (self.first..=self.last)
            .map(|n| format!("{}{n}", self.prefix))
            .collect()
    }

    pub fn width(&self) -> u32 {
        self.last - self.first + 1
    }
}

/// Does the text look like a bus name (contains a `[`)? Used to separate bus labels from net labels.
pub fn looks_like_bus(name: &str) -> bool {
    name.contains('[')
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_expands() {
        let b = BusDef::parse("D[0..3]").unwrap();
        assert_eq!(b.members(), vec!["D0", "D1", "D2", "D3"]);
        assert_eq!(b.width(), 4);
        assert_eq!(
            BusDef::parse(" ADDR[4..4] ").map(|b| b.members()),
            Ok(vec!["ADDR4".to_string()])
        );
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(BusDef::parse("D0"), Err(BusNameError::NotABus));
        assert_eq!(BusDef::parse("[0..3]"), Err(BusNameError::NotABus));
        assert_eq!(BusDef::parse("D[3..0]"), Err(BusNameError::BadRange));
        assert_eq!(BusDef::parse("D[a..b]"), Err(BusNameError::BadRange));
        assert_eq!(BusDef::parse("D[0..3"), Err(BusNameError::NotABus));
        assert_eq!(BusDef::parse("D[0..99999]"), Err(BusNameError::TooWide));
        assert_eq!(BusDef::parse("D[-1..3]"), Err(BusNameError::BadRange));
    }
}
