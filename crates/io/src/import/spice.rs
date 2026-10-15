//! SPICE netlist importer (`.cir`, `.sp`, `.net`, `.spice`).
//!
//! Basis: the SPICE netlist syntax as documented in the public ngspice and SPICE3 user manuals
//! (title line, element lines, `.model`, `+` continuation, `*` comments). Written from the format
//! description; no simulator code is used. Result: a schematic-level design (components and
//! nets); a netlist carries no placement, footprints or board.

use super::{name_from_file, ImportOutcome, ImportReport, Importer};
use buturecad_eda_core::ir::{ComponentIr, Design, NetIr, PinRefIr};
use std::collections::BTreeMap;

pub struct SpiceNetlist;

fn node(n: &str) -> String {
    if n == "0" || n.eq_ignore_ascii_case("gnd") {
        "GND".into()
    } else {
        n.to_owned()
    }
}

/// Logical lines (continuations joined, inline comments removed) with their first physical line.
fn logical(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if let Some(rest) = line.strip_prefix('+') {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(rest.trim());
                continue;
            }
        }
        out.push((i + 1, line.to_owned()));
    }
    out
}

fn strip_inline(line: &str) -> &str {
    let cut = [" ;", " $"].iter().filter_map(|m| line.find(m)).min();
    match cut {
        Some(i) => line[..i].trim_end(),
        None => line,
    }
}

impl Importer for SpiceNetlist {
    fn id(&self) -> &'static str {
        "spice"
    }
    fn name(&self) -> &'static str {
        "SPICE netlist"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["cir", "sp", "spice", "net"]
    }
    fn basis(&self) -> &'static str {
        "Public SPICE3/ngspice netlist syntax documentation; independent implementation."
    }
    fn sniff(&self, text: &str) -> bool {
        let t = text.to_ascii_lowercase();
        !t.trim_start().starts_with("(export")
            && (t.contains("\n.end")
                || t.contains("\n.model")
                || t.lines().skip(1).any(|l| {
                    matches!(l.chars().next(), Some('R' | 'C' | 'V' | 'r' | 'c' | 'v'))
                        && l.split_whitespace().count() >= 4
                }))
    }

    fn import(&self, text: &str, source_name: &str) -> ImportOutcome {
        let mut r = ImportReport::new("spice", source_name);
        let lines = logical(text);
        let mut models: BTreeMap<String, (String, bool)> = BTreeMap::new(); // name → (text, used)
        let mut elements: Vec<(usize, Vec<String>)> = Vec::new();
        let (mut comments, mut after_end, mut in_subckt) =
            (0usize, 0usize, None::<(String, usize)>);
        let mut ended = false;
        for (idx, (ln, raw)) in lines.iter().enumerate() {
            if idx == 0 {
                r.seen(1);
                r.converted("title", raw.trim(), "used as the project name");
                continue;
            }
            let line = strip_inline(raw.trim());
            if line.is_empty() {
                continue;
            }
            r.seen(1);
            if ended {
                after_end += 1;
                continue;
            }
            if let Some((name, n)) = in_subckt.as_mut() {
                *n += 1;
                if line.to_ascii_lowercase().starts_with(".ends") {
                    let (name, n) = (name.clone(), *n);
                    r.unsupported("subcircuit", &name, "subcircuit definitions are not imported (their lines are listed here, not converted)", n);
                    in_subckt = None;
                }
                continue;
            }
            if line.starts_with('*') {
                comments += 1;
                continue;
            }
            let toks: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
            let head = toks[0].to_ascii_lowercase();
            if head.starts_with('.') {
                match head.as_str() {
                    ".end" => {
                        ended = true;
                        r.ignored("directive", ".end", "end of the netlist", 1);
                    }
                    ".subckt" => in_subckt = Some((toks.get(1).cloned().unwrap_or_else(|| "?".into()), 1)),
                    ".model" => match toks.get(1) {
                        Some(name) => {
                            if models.insert(name.to_ascii_lowercase(), (line.to_owned(), false)).is_some() {
                                r.ignored("model", name, "defined earlier too; the later definition is used", 1);
                            }
                        }
                        None => {
                            r.error(format!("line {ln}: .model without a name"));
                            r.ignored("directive", ".model", "malformed", 1);
                        }
                    },
                    ".op" | ".dc" | ".ac" | ".tran" | ".noise" | ".tf" | ".sens" | ".disto" | ".pz" => {
                        r.unsupported("analysis", &toks[0], "analysis commands are not stored in the design; set them up in the Simulation tab", 1)
                    }
                    other => r.unsupported("directive", other, &format!("line {ln}: not supported by this importer: {line}"), 1),
                }
                continue;
            }
            elements.push((*ln, toks));
        }
        if comments > 0 {
            r.ignored(
                "comment",
                "comment lines",
                "comments carry no design data",
                comments,
            );
        }
        if after_end > 0 {
            r.unsupported(
                "line",
                "after .end",
                "everything after .end is ignored by SPICE and by this importer",
                after_end,
            );
        }
        if let Some((name, n)) = in_subckt {
            r.unsupported(
                "subcircuit",
                &name,
                "unterminated .subckt block (no .ends)",
                n,
            );
            r.error(format!("the .subckt “{name}” has no .ends"));
        }

        let mut components: Vec<ComponentIr> = Vec::new();
        let mut pins: BTreeMap<String, Vec<PinRefIr>> = BTreeMap::new();
        let mut names = std::collections::BTreeSet::new();
        for (ln, t) in &elements {
            let name = &t[0];
            if !names.insert(name.to_ascii_lowercase()) {
                r.error(format!("line {ln}: the element {name} is defined twice"));
                r.ignored("element", name, "duplicate name", 1);
                continue;
            }
            let kind = name
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase())
                .unwrap_or('?');
            let mut join = |nodes: &[(&String, &str)], comp: &str| {
                for (n, pin) in nodes {
                    pins.entry(node(n)).or_default().push(PinRefIr {
                        reference: comp.to_owned(),
                        pin: (*pin).to_owned(),
                    });
                }
            };
            match kind {
                'R' | 'C' | 'L' if t.len() >= 4 => {
                    let mut props = BTreeMap::new();
                    if t.len() > 4 {
                        props.insert("spice_extra".to_owned(), t[4..].join(" "));
                        r.warn(format!("{name}: extra parameters “{}” are kept in the property spice_extra but have no effect yet", t[4..].join(" ")));
                    }
                    components.push(ComponentIr {
                        reference: name.clone(),
                        value: t[3].clone(),
                        footprint: String::new(),
                        symbol: String::new(),
                        properties: props,
                    });
                    join(&[(&t[1], "1"), (&t[2], "2")], name);
                    r.converted("component", name, &format!("{kind} = {}", t[3]));
                }
                'V' | 'I' if t.len() >= 4 => {
                    components.push(ComponentIr {
                        reference: name.clone(),
                        value: t[3..].join(" "),
                        footprint: String::new(),
                        symbol: String::new(),
                        properties: BTreeMap::new(),
                    });
                    join(&[(&t[1], "1"), (&t[2], "2")], name);
                    r.converted("component", name, &format!("source {}", t[3..].join(" ")));
                }
                'D' if t.len() >= 4 => {
                    let m = t[3].to_ascii_lowercase();
                    let mut props = BTreeMap::new();
                    match models.get_mut(&m) {
                        Some((text, used)) => {
                            *used = true;
                            props.insert("Spice_Model".to_owned(), text.clone());
                        }
                        None => {
                            props.insert("Spice_Model".to_owned(), t[3].clone());
                            r.warn(format!(
                                "{name}: the model {} is not defined in the file",
                                t[3]
                            ));
                        }
                    }
                    if t.len() > 4 {
                        props.insert("spice_extra".to_owned(), t[4..].join(" "));
                    }
                    components.push(ComponentIr {
                        reference: name.clone(),
                        value: t[3].clone(),
                        footprint: String::new(),
                        symbol: String::new(),
                        properties: props,
                    });
                    join(&[(&t[1], "A"), (&t[2], "K")], name);
                    r.converted("component", name, &format!("diode, model {}", t[3]));
                }
                'R' | 'C' | 'L' | 'V' | 'I' | 'D' => {
                    r.error(format!("line {ln}: {name} has too few fields"));
                    r.ignored("element", name, "too few fields to convert", 1);
                }
                'Q' | 'M' | 'J' | 'Z' | 'X' | 'E' | 'F' | 'G' | 'H' | 'B' | 'K' | 'S' | 'W'
                | 'T' | 'O' | 'U' | 'A' | 'Y' => {
                    let why = match kind {
                        'Q' => "bipolar transistors have no built-in symbol yet",
                        'M' => "MOSFETs have no built-in symbol yet",
                        'X' => "subcircuit instances need the subcircuit, which is not imported",
                        'E' | 'F' | 'G' | 'H' | 'B' => {
                            "controlled and behavioural sources are not supported"
                        }
                        _ => "this element type is not supported",
                    };
                    r.unsupported(
                        "element",
                        name,
                        &format!("line {ln}: {why} ({})", t.join(" ")),
                        1,
                    );
                }
                other => r.unsupported(
                    "element",
                    name,
                    &format!(
                        "line {ln}: unknown element type “{other}” ({})",
                        t.join(" ")
                    ),
                    1,
                ),
            }
        }
        for (name, (text, used)) in &models {
            if *used {
                r.converted("model", name, "attached to the diode(s) that use it");
            } else {
                r.ignored(
                    "model",
                    name,
                    &format!(
                        "not used by any imported device ({})",
                        text.chars().take(60).collect::<String>()
                    ),
                    1,
                );
            }
        }
        r.warn("a SPICE netlist has no footprints or placement: assign footprints before updating the PCB");
        let design = Design {
            name: name_from_file(source_name),
            components,
            nets: pins
                .into_iter()
                .map(|(name, pins)| NetIr { name, pins })
                .collect(),
            ..Design::default()
        };
        ImportOutcome {
            design: Some(design),
            symbols: Vec::new(),
            report: r.finish(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const NETLIST: &str = "RC low-pass with a diode clamp\n* an input source\nV1 in 0 DC 0 AC 1\nR1 in out 1k tc=0.001\nC1 out 0 1u ; load\nD1 out 0 D1N4148\n.model D1N4148 D(IS=2.5e-9\n+ N=1.75 RS=0.57)\n.model UNUSED NPN(BF=100)\nQ1 out in 0 BC547\nX1 in out sub\n.subckt sub a b\nR9 a b 1\n.ends\n.param x=1\n.tran 1u 1m\n.end\nR99 never seen 1\n";

    #[test]
    fn converts_elements_models_and_accounts_for_everything() {
        let o = SpiceNetlist.import(NETLIST, "lowpass.cir");
        let r = &o.report;
        assert!(!r.has_errors(), "{}", r.text());
        let d = o.design.unwrap();
        assert_eq!(d.name, "lowpass");
        let refs: Vec<_> = d.components.iter().map(|c| c.reference.as_str()).collect();
        assert_eq!(refs, ["V1", "R1", "C1", "D1"]);
        let r1 = &d.components[1];
        assert_eq!(
            (r1.value.as_str(), r1.properties["spice_extra"].as_str()),
            ("1k", "tc=0.001"),
            "extra parameters are kept"
        );
        assert!(
            d.components[3].properties["Spice_Model"].contains("N=1.75 RS=0.57"),
            "continuation lines are joined"
        );
        let gnd = d.nets.iter().find(|n| n.name == "GND").unwrap();
        assert_eq!(gnd.pins.len(), 3, "node 0 is GND: V1-, C1-, D1 K");
        assert!(d
            .nets
            .iter()
            .find(|n| n.name == "out")
            .unwrap()
            .pins
            .iter()
            .any(|p| p.reference == "D1" && p.pin == "A"));
        // Unsupported things are listed, not dropped.
        let un: Vec<_> = r.unsupported.iter().map(|i| i.name.as_str()).collect();
        for want in ["Q1", "X1", "sub", ".param", ".tran", "after .end"] {
            assert!(un.contains(&want), "{want} in {un:?}");
        }
        assert!(
            r.ignored
                .iter()
                .any(|i| i.name == "UNUSED".to_ascii_lowercase())
                || r.ignored
                    .iter()
                    .any(|i| i.name.eq_ignore_ascii_case("unused"))
        );
        assert_eq!(r.accounted(), r.records_seen);
        assert!(
            r.warnings.iter().any(|w| w.contains("spice_extra"))
                && r.warnings.iter().any(|w| w.contains("footprints"))
        );
    }

    #[test]
    fn a_model_defined_twice_is_not_silently_dropped() {
        // Found by fuzzing: the first definition used to vanish without a trace.
        let o = SpiceNetlist.import(
            "t\nD1 a 0 M\n.model M D(IS=1e-9)\n.model M D(IS=2e-9)\n.end\n",
            "d.cir",
        );
        assert!(
            o.report
                .ignored
                .iter()
                .any(|i| i.kind == "model" && i.detail.contains("defined earlier")),
            "{}",
            o.report.text()
        );
        assert_eq!(o.report.accounted(), o.report.records_seen);
        assert!(o.design.unwrap().components[0].properties["Spice_Model"].contains("2e-9"));
    }

    #[test]
    fn malformed_input_is_reported_not_swallowed() {
        let o = SpiceNetlist.import("t\nR1 a\nR2 a b 1\nR2 b c 2\n.subckt x\n", "bad.cir");
        assert!(o.report.errors.iter().any(|e| e.contains("too few fields")));
        assert!(o.report.errors.iter().any(|e| e.contains("defined twice")));
        assert!(o.report.errors.iter().any(|e| e.contains("no .ends")));
        assert_eq!(o.report.accounted(), o.report.records_seen);
        assert_eq!(SpiceNetlist.import("", "empty.cir").report.records_seen, 0);
        assert!(!SpiceNetlist.sniff("(export (version \"E\"))"));
        assert!(SpiceNetlist.sniff("t\nR1 a b 1\n.end\n"));
    }
}
