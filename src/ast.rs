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

    Expr {
        value: Expr,
        /// Kept for future diagnostics (e.g. warning on a useless bare
        /// expression statement); not read yet.
        #[allow(dead_code)]
        span: Span,
    },
}

pub struct FunctionDef {
    pub name: String,
    pub body: Vec<Stmt>,
    /// Kept for future diagnostics (e.g. pointing at the whole function
    /// signature); not read yet.
    #[allow(dead_code)]
    pub span: Span,
}

pub struct Program {
    pub functions: Vec<FunctionDef>,
}
