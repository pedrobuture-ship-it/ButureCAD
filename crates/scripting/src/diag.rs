//! Diagnostics: every problem has a severity, a stable code, a message and a source position.

use serde::Serialize;
use std::fmt;

/// 1-based line and column; `len` is the length in characters (at least 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

impl Span {
    pub fn new(line: u32, col: u32, len: u32) -> Self {
        Self {
            line,
            col,
            len: len.max(1),
        }
    }

    /// From the start of `self` to the end of `other` when on the same line, else just `self`.
    pub fn to(self, other: Span) -> Span {
        if self.line == other.line && other.col + other.len >= self.col {
            Span::new(self.line, self.col, other.col + other.len - self.col)
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable identifier (`E-syntax`, `E-dimension`, `W-unused`, …).
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(
            f,
            "{}:{}: {sev}[{}]: {}",
            self.span.line, self.span.col, self.code, self.message
        )
    }
}
