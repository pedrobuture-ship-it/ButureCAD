//! ButureScript under fire: damaged and random programs must produce diagnostics, never a panic,
//! and must finish quickly (the evaluator has step limits; this checks they hold).
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_scripting::{compile, CompileOptions};
use buturecad_testkit::{as_text, iterations, mutations, Rng};
use std::time::{Duration, Instant};

const CHAIN: &str = include_str!("../../../examples/chain.beda");
const PARAMETRIC: &str = r#"
param n = 4
param pitch = 2.54mm
fn x(i) = i * pitch + 5mm
module Decouple(port VDD, port GND, value = "100nF") {
  component C { value = value  footprint = "0402" }
  connect VDD -> C.1
  connect C.2 -> GND
}
board { outline = rect(0mm, 0mm, 50mm, 30mm) }
for i in 0..n {
  component LED[i] { value = "red" footprint = "0603" }
  component R[i] { value = "330" footprint = "0402" }
  connect R[i].2 -> LED[i].A
  connect LED[i].K -> GND
  place LED[i] at (x(i), 20mm)
  if i % 2 == 0 { use dec[i] = Decouple(VDD = "+3V3", GND = GND) } else { use dec[i] = Decouple(VDD = "+3V3", GND = GND, value = "1uF") }
}
track GND on F.Cu width 0.25mm path (1mm, 1mm) -> (10mm, 1mm)
via GND at (5mm, 5mm) diameter 0.8mm drill 0.4mm
zone GND on B.Cu { outline = rect(0mm,0mm,50mm,30mm) clearance = 0.3mm }
rule { clearance = 0.2mm }
class Power { width = 0.5mm  nets = [VCC, GND] }
"#;

fn check(src: &str) {
    let t = Instant::now();
    let out = compile(src, &CompileOptions::default());
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "took {:?} for {:?}",
        t.elapsed(),
        src.chars().take(200).collect::<String>()
    );
    // Either a design, or at least one error explaining why not.
    if out.design.is_none() {
        assert!(out.has_errors(), "no design and no error for {src:?}");
    }
    for d in &out.diagnostics {
        assert!(d.span.line >= 1 && d.span.col >= 1 && !d.message.is_empty());
    }
}

#[test]
fn mutated_programs_never_panic_or_hang() {
    for (seed, src) in [(1u64, CHAIN), (2, PARAMETRIC)] {
        for bad in mutations(src.as_bytes(), seed, iterations(1500)) {
            check(&as_text(&bad));
        }
    }
}

#[test]
fn random_token_soup_never_panics_or_hangs() {
    let words = [
        "component",
        "net",
        "connect",
        "place",
        "track",
        "via",
        "zone",
        "rule",
        "class",
        "board",
        "let",
        "param",
        "fn",
        "module",
        "use",
        "for",
        "in",
        "if",
        "else",
        "port",
        "at",
        "on",
        "width",
        "path",
        "rotate",
        "side",
        "R1",
        "C[i]",
        "U1.2",
        "GND",
        "\"+3V3\"",
        "10mm",
        "5V",
        "1e999",
        "0..1000000",
        "(",
        ")",
        "{",
        "}",
        "[",
        "]",
        ",",
        "->",
        "=",
        "==",
        "&&",
        "+",
        "-",
        "*",
        "/",
        "%",
        "!",
        ".",
        "..",
        "\"",
        "//",
        "x",
        "i",
        "f(x)",
        "rect(0mm,0mm,1mm,1mm)",
    ];
    let mut rng = Rng::new(99);
    for _ in 0..iterations(3000) {
        let n = 1 + rng.below(60);
        let src: Vec<&str> = (0..n).map(|_| words[rng.below(words.len())]).collect();
        check(&src.join(if rng.below(2) == 0 { " " } else { "\n" }));
    }
}

#[test]
fn hostile_shapes_are_bounded() {
    for src in [
        "let a = ".repeat(10_000) + "1",
        "(".repeat(50_000),
        format!("let x = {}1{}", "-".repeat(100_000), ""),
        format!("component R1 {{ value = \"{}\" }}", "x".repeat(2_000_000)),
        "for i in 0..100000 { for j in 0..100000 { for k in 0..100000 { let a = 1 } } }".to_owned(),
        "fn f(x) = f(x)\nlet y = f(1)".to_owned(),
        "module M() { use m = M() }\nuse a = M()".to_owned(),
        "let a = 1mm / 0".to_owned(),
        "board { outline = rect(0mm, 0mm, 1e300mm, 1e300mm) }".to_owned(),
        "\u{0}\u{1}\u{7f}\u{feff}€💥".to_owned(),
    ] {
        check(&src);
    }
}
