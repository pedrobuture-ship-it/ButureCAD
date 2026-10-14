//! ISO 10303-21 ("Part 21", the STEP physical file) **syntax** reader: header and the entity
//! instance list (`#id = TYPE(parameters);`). It does not interpret geometry; parameters are kept
//! as raw text. Input is untrusted: malformed files return errors, never panic.

use crate::backend::McadError;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// Upper-case entity type; `"(COMPLEX)"` for a complex instance `#1=(A() B());`.
    pub name: String,
    /// Raw text between the outermost parentheses.
    pub params: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Part21 {
    /// Header entities: `FILE_DESCRIPTION`, `FILE_NAME`, `FILE_SCHEMA`, … with raw parameters.
    pub header: Vec<(String, String)>,
    /// Schema names from `FILE_SCHEMA`.
    pub schemas: Vec<String>,
    pub entities: BTreeMap<u32, Entity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationProtocol {
    Ap203,
    Ap214,
    Ap242,
    Other,
}

fn malformed(detail: impl Into<String>) -> McadError {
    McadError::Malformed {
        format: "STEP",
        detail: detail.into(),
    }
}

/// Removes `/* … */` comments (outside strings) and splits at `;` outside strings.
fn statements(text: &str) -> Result<Vec<String>, McadError> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_str, mut in_comment) = (false, false);
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if in_comment {
            if c == '*' && it.peek() == Some(&'/') {
                it.next();
                in_comment = false;
            }
            continue;
        }
        if in_str {
            cur.push(c);
            if c == '\'' {
                if it.peek() == Some(&'\'') {
                    cur.push('\'');
                    it.next();
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        match c {
            '/' if it.peek() == Some(&'*') => {
                it.next();
                in_comment = true;
            }
            '\'' => {
                in_str = true;
                cur.push(c);
            }
            ';' => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if in_str {
        return Err(malformed("unterminated string"));
    }
    if in_comment {
        return Err(malformed("unterminated comment"));
    }
    if !cur.trim().is_empty() {
        return Err(malformed("missing ';' at the end of the file"));
    }
    Ok(out)
}

/// `NAME( … )` → (name, inner text). Parentheses must balance outside strings.
fn split_call(s: &str) -> Result<(String, String), McadError> {
    let open = s
        .find('(')
        .ok_or_else(|| malformed(format!("expected '(' in '{}'", clip(s))))?;
    if !s.ends_with(')') {
        return Err(malformed(format!(
            "expected ')' at the end of '{}'",
            clip(s)
        )));
    }
    let (mut depth, mut in_str) = (0i32, false);
    let mut chars = s[open..].chars().peekable();
    let mut idx = open;
    while let Some(c) = chars.next() {
        idx += c.len_utf8();
        if in_str {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    idx += 1;
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        match c {
            '\'' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 || (depth == 0 && idx != s.len()) {
                    return Err(malformed(format!(
                        "unbalanced parentheses in '{}'",
                        clip(s)
                    )));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(malformed(format!(
            "unbalanced parentheses in '{}'",
            clip(s)
        )));
    }
    Ok((
        s[..open].trim().to_ascii_uppercase(),
        s[open + 1..s.len() - 1].trim().to_string(),
    ))
}

fn clip(s: &str) -> String {
    s.chars().take(40).collect()
}

/// Quoted strings inside a parameter list.
fn quoted(params: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut it = params.chars().peekable();
    while let Some(c) = it.next() {
        if in_str {
            if c == '\'' {
                if it.peek() == Some(&'\'') {
                    cur.push('\'');
                    it.next();
                } else {
                    in_str = false;
                    out.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push(c);
            }
        } else if c == '\'' {
            in_str = true;
        }
    }
    out
}

pub fn parse(text: &str) -> Result<Part21, McadError> {
    let stmts = statements(text)?;
    let mut it = stmts.into_iter();
    if it.next().as_deref() != Some("ISO-10303-21") {
        return Err(malformed("missing ISO-10303-21 start marker"));
    }
    let mut p = Part21::default();
    #[derive(PartialEq)]
    enum Sec {
        Out,
        Header,
        Data,
        Done,
    }
    let mut sec = Sec::Out;
    for s in it {
        let up = s.to_ascii_uppercase();
        match (up.as_str(), &sec) {
            ("HEADER", Sec::Out) => sec = Sec::Header,
            ("DATA", Sec::Out) => sec = Sec::Data,
            ("ENDSEC", Sec::Header | Sec::Data) => sec = Sec::Out,
            ("END-ISO-10303-21", Sec::Out) => sec = Sec::Done,
            _ if up.starts_with("DATA") && sec == Sec::Out => {
                // `DATA(name, (schemas))` of ISO 10303-21 edition 3: ignored but accepted.
                sec = Sec::Data;
            }
            _ => match sec {
                Sec::Header => {
                    let (name, params) = split_call(&s)?;
                    if name == "FILE_SCHEMA" {
                        p.schemas = quoted(&params);
                    }
                    p.header.push((name, params));
                }
                Sec::Data => {
                    let (lhs, rhs) = s
                        .split_once('=')
                        .ok_or_else(|| malformed(format!("expected '=' in '{}'", clip(&s))))?;
                    let id: u32 = lhs
                        .trim()
                        .strip_prefix('#')
                        .and_then(|n| n.parse().ok())
                        .ok_or_else(|| malformed(format!("bad entity id '{}'", clip(lhs))))?;
                    let rhs = rhs.trim();
                    let ent = if rhs.starts_with('(') {
                        let (_, params) = split_call(rhs)?;
                        Entity {
                            name: "(COMPLEX)".into(),
                            params,
                        }
                    } else {
                        let (name, params) = split_call(rhs)?;
                        Entity { name, params }
                    };
                    if p.entities.insert(id, ent).is_some() {
                        return Err(malformed(format!("duplicate entity #{id}")));
                    }
                }
                Sec::Out | Sec::Done => {
                    return Err(malformed(format!(
                        "unexpected '{}' outside a section",
                        clip(&s)
                    )))
                }
            },
        }
    }
    if sec != Sec::Done {
        return Err(malformed("missing END-ISO-10303-21"));
    }
    if p.schemas.is_empty() {
        return Err(malformed("the header has no FILE_SCHEMA"));
    }
    Ok(p)
}

impl Part21 {
    pub fn protocol(&self) -> ApplicationProtocol {
        let s = self.schemas.join(" ").to_ascii_uppercase();
        if s.contains("AP242") || s.contains("MANAGED_MODEL_BASED_3D_ENGINEERING") {
            ApplicationProtocol::Ap242
        } else if s.contains("AUTOMOTIVE_DESIGN") || s.contains("AP214") {
            ApplicationProtocol::Ap214
        } else if s.contains("CONFIG_CONTROL_DESIGN") || s.contains("AP203") {
            ApplicationProtocol::Ap203
        } else {
            ApplicationProtocol::Other
        }
    }

    pub fn count(&self, name: &str) -> usize {
        self.entities.values().filter(|e| e.name == name).count()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SAMPLE: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('a; b'),'2;1');
FILE_NAME('x''y.step','2026',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
/* a comment; with semicolon */
#1 = CARTESIAN_POINT('',(0.,0.,0.));
#2 = MANIFOLD_SOLID_BREP('b',#1);
#3 = ( GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('','') );
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn parses_header_and_entities() {
        let p = parse(SAMPLE).unwrap();
        assert_eq!(p.protocol(), ApplicationProtocol::Ap214);
        assert_eq!(p.entities.len(), 3);
        assert_eq!(p.count("MANIFOLD_SOLID_BREP"), 1);
        assert_eq!(p.entities[&3].name, "(COMPLEX)");
        assert_eq!(p.header[0].1, "('a; b'),'2;1'");
        assert!(p.header.iter().any(|(n, _)| n == "FILE_NAME"));
    }

    #[test]
    fn malformed_input_is_an_error() {
        for bad in [
            "",
            "ISO-10303-21;",
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\n#1 = A(;\nENDSEC;\nEND-ISO-10303-21;",
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\n#1 = A('x);\nENDSEC;\nEND-ISO-10303-21;",
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\n#1 = A();\n#1 = B();\nENDSEC;\nEND-ISO-10303-21;",
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;",
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\nfoo = A();\nENDSEC;\nEND-ISO-10303-21;",
        ] {
            assert!(parse(bad).is_err(), "should fail: {bad:?}");
        }
    }
}
