//! Abstract syntax tree for the (currently very small) comfyc grammar.

use crate::diag::Span;

#[derive(Clone, Copy)]
pub enum UnaryOp {
    Neg, // -x
}

#[derive(Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// An expression that (for now) only ever appears where a constant value
/// is expected: a `let` initializer, or a `$syscall` argument.
pub enum Expr {
    IntLit(i64, Span),
    Ident(String, Span),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Syscall {
        args: Box<[Expr; 7]>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit(_, span) => *span,
            Expr::Ident(_, span) => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Syscall { span, .. } => *span,
        }
    }
}

pub enum Stmt {
    /// `let NAME = <expr>;` (constant) or `let mut NAME = <expr>;` (a real,
    /// stack-allocated, runtime local). Plain `let` is resolved away
    /// entirely during semantic analysis; `let mut` gets an actual stack
    /// slot and reaches codegen.
    Let {
        name: String,
        mutable: bool,
        value: Expr,
        span: Span,
    },

    /// `NAME = <expr>;` - reassigns an existing `mut` binding.
    Assign {
        name: String,
        value: Expr,
        span: Span,
    },

    /// A bare expression used as a statement, e.g. `$syscall(...);` where
    /// the result is discarded. Anything that isn't `let` or `NAME = ...`
    /// ends up here.
    Expr { value: Expr, span: Span },
}

pub struct FunctionDef {
    pub name: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

pub struct Program {
    pub functions: Vec<FunctionDef>,
}
