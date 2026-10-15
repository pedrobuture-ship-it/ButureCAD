//! The abstract syntax tree. Every node that can be the subject of a diagnostic carries a span.

use crate::diag::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Num(f64, Option<String>),
    Str(String),
    Bool(bool),
    Ident(String),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Unary(Op, Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

/// `LED` or `LED[i]` (the index is appended to the name: `LED3`).
#[derive(Debug, Clone, PartialEq)]
pub struct NameRef {
    pub base: String,
    pub index: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PinPart {
    Name(String),
    Number(String),
    /// `.(expr)`
    Computed(Expr),
}

/// `MCU.VDD`, `C1.1` (a pin) or a bare `GND` (a net; `pin` is `None`).
#[derive(Debug, Clone, PartialEq)]
pub struct PinRef {
    pub name: NameRef,
    pub pin: Option<(PinPart, Span)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub key: String,
    pub key_span: Span,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub span: Span,
    pub port: bool,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: String,
    pub span: Span,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Project(Expr),
    Board(Vec<Prop>),
    Component {
        name: NameRef,
        props: Vec<Prop>,
    },
    Net {
        name: NameRef,
        pins: Vec<PinRef>,
    },
    /// `a -> b -> c`: every endpoint joins one net.
    Connect(Vec<Vec<PinRef>>),
    Place {
        target: NameRef,
        at: Expr,
        rotate: Option<Expr>,
        side: Option<(String, Span)>,
    },
    Track {
        net: NameRef,
        layer: String,
        width: Expr,
        path: Vec<Expr>,
    },
    Via {
        net: NameRef,
        at: Expr,
        diameter: Option<Expr>,
        drill: Option<Expr>,
    },
    Zone {
        net: NameRef,
        layer: String,
        props: Vec<Prop>,
    },
    Rule(Vec<Prop>),
    Class {
        name: String,
        props: Vec<Prop>,
    },
    Let {
        name: String,
        value: Expr,
    },
    ParamDecl {
        name: String,
        value: Expr,
    },
    Fn {
        name: String,
        params: Vec<String>,
        body: Expr,
    },
    Module {
        name: String,
        params: Vec<Param>,
        body: Vec<Item>,
    },
    Use {
        name: NameRef,
        module: String,
        args: Vec<Arg>,
    },
    For {
        var: String,
        from: Expr,
        to: Expr,
        body: Vec<Item>,
    },
    If {
        cond: Expr,
        then: Vec<Item>,
        otherwise: Vec<Item>,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}
