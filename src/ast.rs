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
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit(_, span) => *span,
            Expr::Ident(_, span) => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
        }
    }
}

pub enum Stmt {
    /// `let NAME = <expr>;` - a compile-time constant. Resolved away
    /// entirely during semantic analysis; never reaches codegen.
    Let {
        name: String,
        value: Expr,
        span: Span,
    },

    /// `$syscall(nr, a0, a1, a2, a3, a4, a5);`
    ///
    /// The sole compiler intrinsic today. Named syscall wrappers
    /// (`write`, `read`, `exit`, ...) will be reintroduced later as
    /// ordinary standard-library functions built on top of this.
    Syscall { args: [Expr; 7], span: Span },
}

pub struct FunctionDef {
    pub name: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

pub struct Program {
    pub functions: Vec<FunctionDef>,
}
