//! **ButureScript** (`.beda`): a declarative, unit-aware language for describing electronics as
//! code. Pipeline (see `docs/buturescript/README.md`):
//!
//! ```text
//! source ─ lexer ─▶ tokens ─ parser ─▶ AST ─ elaboration/semantic analysis ─▶ EDA IR
//!                                                              (diagnostics with line:column)
//! EDA IR ─ buturecad_eda_core::ir_apply (Command Engine) ─▶ schematic + board
//! ```
//!
//! The language knows nothing about rendering or the GUI: it produces the plain
//! [`buturecad_eda_core::ir::Design`], which the core applies through its commands. Files are
//! untrusted input: the evaluator has step, loop, recursion and module-depth limits and cannot
//! reach the file system, the network or native code.

pub mod ast;
pub mod diag;
pub mod eval;
pub mod footprints;
pub mod lexer;
pub mod parser;
pub mod value;

pub use diag::{Diagnostic, Severity, Span};
pub use footprints::resolve_footprint;

use buturecad_eda_core::ir::Design;
use buturecad_eda_core::ir_apply::{apply_design, ApplyError, ApplyReport};
use buturecad_eda_core::{CommandEngine, Project};
use std::collections::BTreeMap;

/// Overrides for `param` declarations: name → value written as in the language (`3.3V`).
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// `None` when there is at least one error.
    pub design: Option<Design>,
    /// Errors and warnings, sorted by position.
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Lex, parse, elaborate and validate `src`.
pub fn compile(src: &str, opts: &CompileOptions) -> CompileOutput {
    let (tokens, mut diags) = lexer::lex(src);
    let (program, parse_diags) = parser::parse(tokens);
    diags.extend(parse_diags);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        diags.sort_by_key(|d| (d.span.line, d.span.col));
        return CompileOutput {
            design: None,
            diagnostics: diags,
        };
    }
    // Parameter overrides are values, evaluated with the same rules.
    let mut overrides = BTreeMap::new();
    let mut pre = eval::Elaborator::new(BTreeMap::new());
    for (k, v) in &opts.params {
        let (t, d) = lexer::lex(v);
        let mut p = parser::parse_expr(t);
        match p.as_mut() {
            Ok(e) if d.is_empty() => match pre.eval(e) {
                Ok(val) => {
                    overrides.insert(k.clone(), val);
                }
                Err(()) => diags.push(Diagnostic::error(
                    "E-param",
                    Span::new(1, 1, 1),
                    format!("--param {k}={v}: not a valid value"),
                )),
            },
            _ => diags.push(Diagnostic::error(
                "E-param",
                Span::new(1, 1, 1),
                format!("--param {k}={v}: not a valid value"),
            )),
        }
    }
    let mut el = eval::Elaborator::new(overrides);
    el.run(&program);
    let (design, mut more) = el.into_design();
    diags.append(&mut more);
    diags.sort_by_key(|d| (d.span.line, d.span.col));
    let design = if diags.iter().any(|d| d.severity == Severity::Error) {
        None
    } else {
        design
    };
    CompileOutput {
        design,
        diagnostics: diags,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("the script has errors")]
    Script(Vec<Diagnostic>),
    #[error(transparent)]
    Apply(#[from] ApplyError),
    #[error("{0}")]
    Project(String),
}

pub struct Built {
    pub engine: CommandEngine,
    pub report: ApplyReport,
    pub diagnostics: Vec<Diagnostic>,
    pub design: Design,
}

/// Compiles `src` and generates a new project from it through the Command Engine. No GUI, no
/// window: this is the headless path used by tests, the CLI and automation.
pub fn build(
    src: &str,
    opts: &CompileOptions,
    name_override: Option<&str>,
) -> Result<Built, BuildError> {
    let out = compile(src, opts);
    let Some(design) = out.design else {
        return Err(BuildError::Script(out.diagnostics));
    };
    let name = name_override.unwrap_or(&design.name);
    let project = Project::new(name).map_err(|e| BuildError::Project(e.to_string()))?;
    let mut engine = CommandEngine::new(project);
    let report = apply_design(&mut engine, &design, &|n| resolve_footprint(n))?;
    Ok(Built {
        engine,
        report,
        diagnostics: out.diagnostics,
        design,
    })
}
