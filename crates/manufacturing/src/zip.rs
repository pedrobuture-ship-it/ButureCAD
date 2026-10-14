//! A minimal, deterministic ZIP writer (method "stored", no compression, fixed 1980-01-01
//! timestamps, no ZIP64). Gerber/CSV files are small; avoiding a dependency keeps output
//! byte-reproducible.

use crate::FabError;
use std::collections::BTreeSet;

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    !data.iter().fold(!0u32, |c, b| {
        table[((c ^ u32::from(*b)) & 0xFF) as usize] ^ (c >> 8)
    })
}

const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01

pub fn write(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, FabError> {
    let bad = |m: String| Err(FabError::Invalid(m));
    if files.len() > usize::from(u16::MAX) {
        return bad("too many files for a ZIP".into());
    }
    let mut seen = BTreeSet::new();
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in files {
        if name.is_empty()
            || name.starts_with('/')
            || name.contains("..")
            || name.contains('\\')
            || !seen.insert(name.as_str())
        {
            return bad(format!("unsafe or duplicate file name '{name}'"));
        }
        let (Ok(len), Ok(offset), Ok(nlen)) = (
            u32::try_from(data.len()),
            u32::try_from(out.len()),
            u16::try_from(name.len()),
        ) else {
            return bad("the archive exceeds the ZIP (non-64) limits".into());
        };
        let crc = crc32(data);
        let flags: u16 = 0x0800; // UTF-8 names
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        hdr.extend_from_slice(&20u16.to_le_bytes());
        hdr.extend_from_slice(&flags.to_le_bytes());
        hdr.extend_from_slice(&0u16.to_le_bytes());
        hdr.extend_from_slice(&DOS_TIME.to_le_bytes());
        hdr.extend_from_slice(&DOS_DATE.to_le_bytes());
        hdr.extend_from_slice(&crc.to_le_bytes());
        hdr.extend_from_slice(&len.to_le_bytes());
        hdr.extend_from_slice(&len.to_le_bytes());
        hdr.extend_from_slice(&nlen.to_le_bytes());
        hdr.extend_from_slice(&0u16.to_le_bytes());
        hdr.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&hdr);
        out.extend_from_slice(data);

        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]); // extra, comment, disk, int attr, ext attr
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let (Ok(cd_off), Ok(cd_len)) = (u32::try_from(out.len()), u32::try_from(central.len())) else {
        return bad("the archive exceeds the ZIP (non-64) limits".into());
    };
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    let n = files.len() as u16;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// Reads a stored ZIP written by [`write`] (test helper and integrity check).
pub fn read_stored(zip: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let u16at = |i: usize| {
        zip.get(i..i + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let u32at = |i: usize| {
        zip.get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let eocd = zip.len().checked_sub(22)?;
    if zip.get(eocd..eocd + 4)? != [0x50, 0x4b, 0x05, 0x06] {
        return None;
    }
    let count = u16at(eocd + 10)?;
    let mut p = u32at(eocd + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if zip.get(p..p + 4)? != [0x50, 0x4b, 0x01, 0x02] {
            return None;
        }
        let crc = u32at(p + 16)?;
        let len = u32at(p + 24)? as usize;
        let nlen = u16at(p + 28)?;
        let off = u32at(p + 42)? as usize;
        let name = String::from_utf8(zip.get(p + 46..p + 46 + nlen)?.to_vec()).ok()?;
        let lnlen = u16at(off + 26)?;
        let start = off + 30 + lnlen;
        let data = zip.get(start..start + len)?.to_vec();
        if crc32(&data) != crc {
            return None;
        }
        out.push((name, data));
        p += 46 + nlen;
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn round_trip_and_determinism() {
        let files = vec![
            ("a/b.txt".to_string(), b"hello".to_vec()),
            ("empty".to_string(), Vec::new()),
        ];
        let z = write(&files).unwrap();
        assert_eq!(z, write(&files).unwrap());
        assert_eq!(read_stored(&z).unwrap(), files);
    }

    #[test]
    fn unsafe_names_are_rejected() {
        for n in ["../x", "/x", "", "a\\b"] {
            assert!(write(&[(n.to_string(), vec![])]).is_err());
        }
        assert!(write(&[("a".into(), vec![]), ("a".into(), vec![])]).is_err());
    }
}
