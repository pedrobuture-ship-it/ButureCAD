//! KiCad netlist importer (`.net`, the S-expression "export" file).
//!
//! Basis: the netlist file format as described in KiCad's public file-format documentation
//! (`(export (version …) (design …) (components (comp (ref) (value) (footprint) (libsource …)
//! (property …)) …) (libparts …) (libraries …) (nets (net (code) (name) (node (ref) (pin)
//! [(pinfunction)] [(pintype)]) …)))`). It is an independent implementation of the *format*; no
//! KiCad source code is used. Result: components and connectivity. A netlist carries no placement
//! or board, so none is imported. **Not verified against files produced by a real KiCad here:** the
//! tests use fixtures written from the documented structure.

use super::{name_from_file, ImportOutcome, ImportReport, Importer};
use buturecad_eda_core::ir::{ComponentIr, Design, NetIr, PinRefIr};
use buturecad_geometry::Point;
use buturecad_schematic::{Direction, Graphic, Pin, PinType, Symbol};
use std::collections::{BTreeMap, BTreeSet};

pub struct KicadNetlist;

#[derive(Debug, Clone, PartialEq)]
pub enum Sx {
    Atom(String),
    List(Vec<Sx>),
}

impl Sx {
    fn head(&self) -> Option<&str> {
        match self {
            Sx::List(v) => match v.first() {
                Some(Sx::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    fn children(&self) -> &[Sx] {
        match self {
            Sx::List(v) if !v.is_empty() => &v[1..],
            _ => &[],
        }
    }
    /// `(name value)` child → its first atom.
    fn field(&self, name: &str) -> Option<&str> {
        self.children()
            .iter()
            .find(|c| c.head() == Some(name))
            .and_then(|c| match c.children().first() {
                Some(Sx::Atom(a)) => Some(a.as_str()),
                _ => None,
            })
    }
    fn sub(&self, name: &str) -> Option<&Sx> {
        self.children().iter().find(|c| c.head() == Some(name))
    }
}

/// Parses one S-expression. Depth and size are bounded (the input is untrusted).
pub fn parse_sx(text: &str) -> Result<Sx, String> {
    const MAX_DEPTH: usize = 64;
    let b: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut stack: Vec<Vec<Sx>> = Vec::new();
    let mut root: Option<Sx> = None;
    while i < b.len() {
        match b[i] {
            c if c.is_whitespace() => i += 1,
            '(' => {
                if stack.len() >= MAX_DEPTH {
                    return Err("nesting is too deep".into());
                }
                stack.push(Vec::new());
                i += 1;
            }
            ')' => {
                let list = stack.pop().ok_or("unmatched “)”")?;
                let node = Sx::List(list);
                match stack.last_mut() {
                    Some(top) => top.push(node),
                    None => {
                        if root.is_some() {
                            return Err("more than one top-level expression".into());
                        }
                        root = Some(node);
                    }
                }
                i += 1;
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                loop {
                    match b.get(i) {
                        None => return Err("unterminated string".into()),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => {
                            i += 1;
                            match b.get(i) {
                                Some('n') => s.push('\n'),
                                Some(c) => s.push(*c),
                                None => return Err("unterminated string".into()),
                            }
                            i += 1;
                        }
                        Some(c) => {
                            s.push(*c);
                            i += 1;
                        }
                    }
                }
                stack
                    .last_mut()
                    .ok_or("text outside the expression")?
                    .push(Sx::Atom(s));
            }
            _ => {
                let st = i;
                while i < b.len()
                    && !b[i].is_whitespace()
                    && b[i] != '('
                    && b[i] != ')'
                    && b[i] != '"'
                {
                    i += 1;
                }
                stack
                    .last_mut()
                    .ok_or("text outside the expression")?
                    .push(Sx::Atom(b[st..i].iter().collect()));
            }
        }
    }
    if !stack.is_empty() {
        return Err("missing “)” at the end of the file".into());
    }
    root.ok_or_else(|| "empty file".into())
}

fn pin_type(t: &str) -> PinType {
    match t {
        "power_in" => PinType::PowerInput,
        "power_out" => PinType::PowerOutput,
        "input" => PinType::Input,
        "output" => PinType::Output,
        "bidirectional" => PinType::Bidirectional,
        "tri_state" => PinType::TriState,
        "passive" => PinType::Passive,
        "open_collector" => PinType::OpenCollector,
        "open_emitter" => PinType::OpenEmitter,
        "no_connect" => PinType::NoConnect,
        _ => PinType::Unspecified,
    }
}

/// A rectangular symbol with the given pins (number, name, type), left/right alternating.
fn generated_symbol(name: &str, prefix: &str, pins: &[(String, String, PinType)]) -> Symbol {
    const G: i64 = 1_270_000;
    let mut s = Symbol::new(name, prefix);
    let half = pins.len().div_ceil(2) as i64;
    for (i, (num, pname, ty)) in pins.iter().enumerate() {
        let (side, row) = (i % 2, (i / 2) as i64);
        let y = (half - 1 - 2 * row) * G;
        let (x, dir) = if side == 0 {
            (-7 * G, Direction::Right)
        } else {
            (7 * G, Direction::Left)
        };
        s.pins.push(Pin {
            number: num.clone(),
            name: pname.clone(),
            kind: *ty,
            position: Point::new(x, y),
            orientation: dir,
            length: 2 * G,
            unit: 0,
            representation: 0,
        });
    }
    if let Ok(rect) = buturecad_geometry::Rectangle::new(
        Point::new(-5 * G, -(half * 2) * G),
        Point::new(5 * G, half * 2 * G),
    ) {
        s.graphics.push(
            Graphic::Rect {
                rect,
                filled: false,
                width: 150_000,
            }
            .into(),
        );
    }
    s
}

fn map_footprint(fp: &str) -> Option<String> {
    let name = fp.rsplit(':').next().unwrap_or(fp);
    let low = name.to_ascii_lowercase();
    // R_0603_1608Metric, C_0402_1005Metric, LED_0805_2012Metric …
    for code in ["0201", "0402", "0603", "0805", "1206"] {
        if low.contains(&format!("_{code}_")) || low.ends_with(&format!("_{code}")) {
            return Some(code.to_owned());
        }
    }
    if let Some(r) = low.strip_prefix("pinheader_") {
        let (rows, rest) = r.split_once('x')?;
        let pins: String = rest.chars().take_while(char::is_ascii_digit).collect();
        return match (rows, pins.parse::<u32>().ok()?) {
            ("1", n) => Some(format!("header{n}")),
            ("2", n) => Some(format!("header2x{n}")),
            _ => None,
        };
    }
    if let Some(r) = low.strip_prefix("soic-") {
        let n: String = r.chars().take_while(char::is_ascii_digit).collect();
        return Some(format!("soic{n}"));
    }
    None
}

fn default_symbol(reference: &str) -> Option<&'static str> {
    buturecad_eda_core::ir_apply::default_symbol(reference)
}

impl Importer for KicadNetlist {
    fn id(&self) -> &'static str {
        "kicad-netlist"
    }
    fn name(&self) -> &'static str {
        "KiCad netlist (.net)"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["net"]
    }
    fn basis(&self) -> &'static str {
        "KiCad's public netlist file-format description; independent implementation, no KiCad code."
    }
    fn sniff(&self, text: &str) -> bool {
        let t = text.trim_start();
        t.starts_with("(export") && t.contains("(components")
    }

    fn import(&self, text: &str, source_name: &str) -> ImportOutcome {
        let mut r = ImportReport::new("kicad-netlist", source_name);
        let root = match parse_sx(text) {
            Ok(x) if x.head() == Some("export") => x,
            Ok(_) => {
                r.seen(1);
                r.error("the file is an S-expression but not a KiCad netlist (no “export” root)");
                r.unsupported("file", source_name, "not a netlist export", 1);
                return ImportOutcome {
                    design: None,
                    symbols: Vec::new(),
                    report: r.finish(),
                };
            }
            Err(e) => {
                r.seen(1);
                r.error(format!("not readable as a KiCad netlist: {e}"));
                r.unsupported("file", source_name, "unreadable", 1);
                return ImportOutcome {
                    design: None,
                    symbols: Vec::new(),
                    report: r.finish(),
                };
            }
        };
        if let Some(v) = root.field("version") {
            if !matches!(v, "D" | "E") {
                r.warn(format!(
                    "netlist version “{v}” was not tested; the structure is read as version D/E"
                ));
            }
        }
        // Pass 1: nodes per component (pins that exist), nets.
        let mut pins_of: BTreeMap<String, BTreeMap<String, (String, PinType)>> = BTreeMap::new();
        let mut nets_raw: Vec<(String, Vec<(String, String)>)> = Vec::new();
        let mut known: BTreeSet<String> = BTreeSet::new();
        for sec in root.children() {
            if sec.head() == Some("components") {
                for c in sec.children() {
                    if let Some(rf) = c.field("ref") {
                        known.insert(rf.to_owned());
                    }
                }
            }
        }
        let mut components: Vec<ComponentIr> = Vec::new();
        let mut comp_lib: BTreeMap<String, (String, String)> = BTreeMap::new();
        for sec in root.children() {
            match sec.head() {
                Some("version") => {}
                Some("design") => {
                    r.seen(1);
                    r.ignored(
                        "design",
                        sec.field("source").unwrap_or("design"),
                        "source file, date and tool are provenance only",
                        1,
                    );
                }
                Some("components") => {
                    for c in sec.children() {
                        r.seen(1);
                        if c.head() != Some("comp") {
                            r.unsupported(
                                "record",
                                c.head().unwrap_or("?"),
                                "unexpected entry in components",
                                1,
                            );
                            continue;
                        }
                        let Some(reference) = c.field("ref") else {
                            r.error("a component has no (ref …)");
                            r.unsupported("component", "?", "no reference", 1);
                            continue;
                        };
                        if components.iter().any(|x| x.reference == reference) {
                            r.error(format!("the component {reference} appears twice"));
                            r.ignored("component", reference, "duplicate", 1);
                            continue;
                        }
                        let mut props: BTreeMap<String, String> = BTreeMap::new();
                        for p in c.children().iter().filter(|x| x.head() == Some("property")) {
                            if let (Some(k), Some(v)) = (p.field("name"), p.field("value")) {
                                props.insert(k.to_owned(), v.to_owned());
                            }
                        }
                        if let Some(ls) = c.sub("libsource") {
                            comp_lib.insert(
                                reference.to_owned(),
                                (
                                    ls.field("lib").unwrap_or("").to_owned(),
                                    ls.field("part").unwrap_or("").to_owned(),
                                ),
                            );
                            if let Some(d) = ls.field("description") {
                                props.insert("description".into(), d.to_owned());
                            }
                        }
                        let fp = c.field("footprint").unwrap_or("");
                        let footprint = match (fp, map_footprint(fp)) {
                            ("", _) => String::new(),
                            (_, Some(m)) => m,
                            (orig, None) => {
                                r.warn(format!("{reference}: the footprint “{orig}” cannot be generated; it is kept by name and needs a definition"));
                                orig.to_owned()
                            }
                        };
                        components.push(ComponentIr {
                            reference: reference.to_owned(),
                            value: c.field("value").unwrap_or("").to_owned(),
                            footprint,
                            symbol: String::new(),
                            properties: props,
                        });
                        r.converted(
                            "component",
                            reference,
                            &format!("{} ({})", c.field("value").unwrap_or(""), fp),
                        );
                    }
                }
                Some("libparts") | Some("libraries") => {
                    let n = sec.children().len();
                    r.seen(n);
                    if n > 0 {
                        r.ignored(sec.head().unwrap_or("?"), "library tables", "symbols are generated from the pins used in the nets, not copied from library definitions", n);
                    }
                }
                Some("nets") => {
                    for n in sec.children() {
                        r.seen(1);
                        if n.head() != Some("net") {
                            r.unsupported(
                                "record",
                                n.head().unwrap_or("?"),
                                "unexpected entry in nets",
                                1,
                            );
                            continue;
                        }
                        let name = n.field("name").unwrap_or("").to_owned();
                        let mut nodes = Vec::new();
                        for node in n.children().iter().filter(|x| x.head() == Some("node")) {
                            r.seen(1);
                            match (node.field("ref"), node.field("pin")) {
                                (Some(rf), Some(pin)) if known.contains(rf) => {
                                    let pname = node
                                        .field("pinfunction")
                                        .filter(|f| !f.is_empty())
                                        .unwrap_or(pin)
                                        .to_owned();
                                    pins_of.entry(rf.to_owned()).or_default().insert(
                                        pin.to_owned(),
                                        (pname, pin_type(node.field("pintype").unwrap_or(""))),
                                    );
                                    nodes.push((rf.to_owned(), pin.to_owned()));
                                    r.converted(
                                        "pin",
                                        &format!("{rf}.{pin}"),
                                        &format!("on net {name}"),
                                    );
                                }
                                (Some(rf), Some(pin)) => {
                                    r.error(format!("net {name}: the node {rf}.{pin} refers to a component that is not in the file"));
                                    r.unsupported(
                                        "pin",
                                        &format!("{rf}.{pin}"),
                                        "unknown component",
                                        1,
                                    );
                                }
                                _ => {
                                    r.error(format!(
                                        "net {name}: a node without (ref …) or (pin …)"
                                    ));
                                    r.unsupported("pin", "?", "incomplete node", 1);
                                }
                            }
                        }
                        nets_raw.push((name, nodes));
                    }
                }
                other => {
                    r.seen(1);
                    r.unsupported(
                        "section",
                        other.unwrap_or("?"),
                        "not a section this importer understands",
                        1,
                    );
                }
            }
        }
        // Pass 2: symbols, nets.
        let mut symbols: Vec<Symbol> = Vec::new();
        for c in &mut components {
            let used = pins_of.get(&c.reference);
            let two_pin = used.is_some_and(|u| u.keys().all(|k| k == "1" || k == "2"));
            let default = default_symbol(&c.reference).filter(|_| two_pin || used.is_none());
            match default {
                Some(s) => c.symbol = s.to_owned(),
                None => {
                    let prefix: String = c
                        .reference
                        .chars()
                        .take_while(|ch| ch.is_ascii_alphabetic())
                        .collect();
                    let (lib, part) = comp_lib.get(&c.reference).cloned().unwrap_or_default();
                    let mut pins: Vec<(String, String, PinType)> = used
                        .map(|u| {
                            u.iter()
                                .map(|(n, (nm, t))| (n.clone(), nm.clone(), *t))
                                .collect()
                        })
                        .unwrap_or_default();
                    pins.sort_by(|a, b| {
                        (a.0.parse::<u64>().unwrap_or(u64::MAX), &a.0)
                            .cmp(&(b.0.parse::<u64>().unwrap_or(u64::MAX), &b.0))
                    });
                    let sig: String = pins
                        .iter()
                        .map(|p| format!("{}={}", p.0, p.1))
                        .collect::<Vec<_>>()
                        .join(",");
                    let tag = format!(
                        "{}_{}_{:08x}",
                        if lib.is_empty() { "Imported" } else { &lib },
                        if part.is_empty() {
                            c.reference.as_str()
                        } else {
                            &part
                        },
                        sig.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b))
                            .wrapping_mul(16777619))
                    );
                    let id = format!("Imported:{}", tag.replace([' ', ':', '/'], "_"));
                    if !symbols.iter().any(|s| s.name == id) {
                        symbols.push(generated_symbol(&id, &prefix, &pins));
                    }
                    c.symbol = id;
                    r.warn(format!("{}: a symbol with {} pin(s) was generated from the pins used in the nets (unused pins do not exist on it)", c.reference, pins.len()));
                }
            }
        }
        let mut nets: Vec<NetIr> = Vec::new();
        for (name, nodes) in nets_raw {
            let single = nodes.len() == 1 && (name.starts_with("unconnected-") || name.is_empty());
            if single {
                // A pin that is not connected to anything: not a net.
                r.warn(format!(
                    "{}.{} is unconnected in the source",
                    nodes[0].0, nodes[0].1
                ));
                r.ignored("net", &name, "a single unconnected pin is not a net", 1);
                continue;
            }
            if nodes.is_empty() {
                r.ignored("net", &name, "no usable nodes", 1);
                continue;
            }
            r.converted("net", &name, &format!("{} node(s)", nodes.len()));
            nets.push(NetIr {
                name,
                pins: nodes
                    .into_iter()
                    .map(|(reference, pin)| PinRefIr { reference, pin })
                    .collect(),
            });
        }
        r.warn("a netlist has no placement or board outline: nothing of the board was imported");
        let design = Design {
            name: name_from_file(source_name),
            components,
            nets,
            ..Design::default()
        };
        ImportOutcome {
            design: Some(design),
            symbols,
            report: r.finish(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    pub(crate) const NET: &str = r#"(export (version "E")
  (design (source "/home/u/board.kicad_sch") (date "2026-01-01") (tool "Eeschema 8.0"))
  (components
    (comp (ref "R1") (value "10k") (footprint "Resistor_SMD:R_0603_1608Metric")
      (libsource (lib "Device") (part "R") (description "Resistor"))
      (property (name "Sheetname") (value "Root")) (sheetpath (names "/") (tstamps "/")) (tstamps "aaaa"))
    (comp (ref "C1") (value "100n") (footprint "Capacitor_SMD:C_0402_1005Metric") (libsource (lib "Device") (part "C")))
    (comp (ref "U1") (value "MCU") (footprint "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm") (libsource (lib "MCU") (part "STM")))
    (comp (ref "J1") (value "Conn") (footprint "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical") (libsource (lib "Connector") (part "Conn_01x02"))))
  (libparts (libpart (lib "Device") (part "R")) (libpart (lib "MCU") (part "STM")))
  (libraries (library (logical "Device") (uri "/x/Device.kicad_sym")))
  (nets
    (net (code "1") (name "GND") (node (ref "C1") (pin "2") (pintype "passive")) (node (ref "U1") (pin "2") (pinfunction "VSS") (pintype "power_in")))
    (net (code "2") (name "+3V3") (node (ref "R1") (pin "1") (pintype "passive")) (node (ref "C1") (pin "1")) (node (ref "U1") (pin "1") (pinfunction "VDD") (pintype "power_in")))
    (net (code "3") (name "Net-(R1-Pad2)") (node (ref "R1") (pin "2")) (node (ref "U1") (pin "3") (pinfunction "PA0") (pintype "bidirectional")))
    (net (code "4") (name "unconnected-(U1-Pad4)") (node (ref "U1") (pin "4") (pintype "bidirectional")))))
"#;

    #[test]
    fn the_s_expression_parser_is_strict_and_bounded() {
        assert!(parse_sx("(a (b \"c d\") e)").is_ok());
        for bad in ["", "(a", "a)", "(a) (b)", "(a \"x", &"(".repeat(200)] {
            assert!(parse_sx(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn components_nets_and_generated_symbols() {
        let o = KicadNetlist.import(NET, "board.net");
        let r = &o.report;
        assert!(!r.has_errors(), "{}", r.text());
        let d = o.design.unwrap();
        assert_eq!(d.components.len(), 4);
        let c = |n: &str| d.components.iter().find(|c| c.reference == n).unwrap();
        assert_eq!(
            (c("R1").footprint.as_str(), c("R1").symbol.as_str()),
            ("0603", "Buture:R")
        );
        assert_eq!(c("C1").footprint, "0402");
        assert_eq!(c("U1").footprint, "soic8");
        assert_eq!(
            c("J1").footprint,
            "header1x02".replace("1x02", "2"),
            "{:?}",
            c("J1").footprint
        );
        assert_eq!(c("R1").properties["Sheetname"], "Root");
        assert!(
            c("U1").symbol.starts_with("Imported:MCU_STM_"),
            "{}",
            c("U1").symbol
        );
        let sym = o.symbols.iter().find(|s| s.name == c("U1").symbol).unwrap();
        let names: Vec<_> = sym
            .pins
            .iter()
            .map(|p| (p.number.as_str(), p.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [("1", "VDD"), ("2", "VSS"), ("3", "PA0"), ("4", "4")]
        );
        assert_eq!(sym.pins[0].kind, PinType::PowerInput);
        assert_eq!(
            d.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["GND", "+3V3", "Net-(R1-Pad2)"],
            "the single-pin 'unconnected' net is not a net"
        );
        // Nothing disappears: library tables, design info, the unconnected net are all listed.
        assert_eq!(r.accounted(), r.records_seen);
        assert!(r
            .ignored
            .iter()
            .any(|i| i.kind == "libparts" && i.count == 2));
        assert!(r.ignored.iter().any(|i| i.kind == "design"));
        assert!(r.warnings.iter().any(|w| w.contains("U1.4 is unconnected")));
        assert!(r.warnings.iter().any(|w| w.contains("no placement")));
    }

    #[test]
    fn bad_files_report_errors_and_unknown_content_is_listed() {
        let o = KicadNetlist.import("(kicad_pcb (version 1))", "x.net");
        assert!(o.design.is_none() && o.report.has_errors());
        let o = KicadNetlist.import("(export (version \"E\") (components (comp (ref \"R1\") (value \"1\")) (comp (ref \"R1\"))) (nets (net (code \"1\") (name \"N\") (node (ref \"R9\") (pin \"1\")))) (mystery 1 2))", "x.net");
        let r = &o.report;
        assert!(
            r.errors.iter().any(|e| e.contains("appears twice"))
                && r.errors.iter().any(|e| e.contains("not in the file")),
            "{r:?}"
        );
        assert!(r.unsupported.iter().any(|i| i.name == "mystery"));
        assert_eq!(r.accounted(), r.records_seen);
        assert!(KicadNetlist.import("(export", "x.net").design.is_none());
        assert!(KicadNetlist.sniff(NET) && !KicadNetlist.sniff("hello"));
    }
}
