//! Abstract syntax tree for the (currently very small) comfyc grammar.

use crate::diag::Span;

pub struct Program {
    pub functions: Vec<FunctionDef>,
}

pub struct FunctionDef {
    pub name: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

pub enum Stmt {
    /// `$syscall(nr, a0, a1, a2, a3, a4, a5);`
    ///
    /// The sole compiler intrinsic today. Named syscall wrappers
    /// (`write`, `read`, `exit`, ...) will be reintroduced later as
    /// ordinary standard-library functions built on top of this.
    Syscall { args: [i64; 7], span: Span },
}
