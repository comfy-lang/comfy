//! Abstract syntax tree for the (currently very small) comfyc grammar.

use crate::diag::Span;

pub struct TypeName {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Copy)]
pub enum UnaryOp {
    Neg, // -x
    Not, // !x
}

#[derive(Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Copy)]
pub enum CompareOp {
    Eq, // ==
    Ne, // !=
    Lt, // <
    Le, // <=
    Gt, // >
    Ge, // >=
}

#[derive(Clone, Copy)]
pub enum LogicalOp {
    And,
    Or,
}

/// An expression that (for now) only ever appears where a constant value
/// is expected: a `let` initializer, or a `$syscall` argument.
pub enum Expr {
    IntLit(i64, Span),
    BoolLit(bool, Span),
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
    Compare {
        op: CompareOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Logical {
        op: LogicalOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Syscall {
        args: Box<[Expr; 7]>,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit(_, span) => *span,
            Expr::BoolLit(_, span) => *span,
            Expr::Ident(_, span) => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Compare { span, .. } => *span,
            Expr::Logical { span, .. } => *span,
            Expr::Syscall { span, .. } => *span,
            Expr::Call { span, .. } => *span,
        }
    }
}

pub enum Stmt {
    Let {
        name: String,
        mutable: bool,
        value: Expr,
        span: Span,
    },

    Assign {
        name: String,
        value: Expr,
        span: Span,
    },

    Expr {
        value: Expr,
        span: Span,
    },

    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        /// `else if` is just sugar for an `else` block containing a single
        /// nested `If` - no separate AST node needed.
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },

    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::Expr { span, .. } => *span,
            Stmt::If { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::Return { span, .. } => *span,
        }
    }
}

pub struct Param {
    pub name: String,
    pub ty: TypeName,
    /// Kept for future diagnostics (e.g. pointing at just this parameter
    /// in a type-mismatch error); not read yet.
    #[allow(dead_code)]
    pub span: Span,
}

pub struct FunctionDef {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeName>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

pub struct Program {
    pub functions: Vec<FunctionDef>,
}
