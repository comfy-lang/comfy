//! Semantic analysis: resolves names, evaluates compile-time constants,
//! and produces a `CheckedProgram` for codegen to consume. This is where
//! `let` constants get folded away entirely - codegen never sees them.

use std::collections::HashMap;

use crate::{ast, diag::Diagnostic};

pub struct CheckedProgram {
    pub functions: Vec<CheckedFunction>,
}

pub struct CheckedFunction {
    pub name: String,
    /// Total bytes to reserve on the stack for this function's `mut`
    /// locals (each currently a 4-byte word, matching an ARM32 register).
    pub frame_size: usize,
    pub body: Vec<CheckedStmt>,
}

/// An expression after semantic analysis: either fully resolved to a
/// compile-time constant, or a small tree of runtime operations that
/// codegen still needs to emit real instructions for.
pub enum CheckedExpr {
    Const(i64),
    /// Load from a `mut` local's stack slot, `offset` bytes below the
    /// frame pointer.
    Local(usize),
    Unary {
        op: ast::UnaryOp,
        operand: Box<CheckedExpr>,
    },
    Binary {
        op: ast::BinOp,
        lhs: Box<CheckedExpr>,
        rhs: Box<CheckedExpr>,
    },
}

pub enum CheckedStmt {
    /// Store a value into a `mut` local's stack slot - used for both a
    /// `let mut` initializer and a later `NAME = ...;` reassignment.
    Store {
        offset: usize,
        value: CheckedExpr,
    },
    Syscall {
        args: [CheckedExpr; 7],
    },
}

pub fn check(program: &ast::Program) -> Result<CheckedProgram, Diagnostic> {
    let mut functions = Vec::new();
    for function in &program.functions {
        functions.push(check_function(function)?);
    }
    Ok(CheckedProgram { functions })
}

/// What a name currently in scope refers to.
enum Symbol {
    /// A plain `let` - fully known at compile time, substituted inline
    /// wherever it's used. Takes up no stack space.
    Const(i64),
    /// A `let mut` - lives at a fixed offset below the frame pointer.
    Local(usize),
}

fn check_function(function: &ast::FunctionDef) -> Result<CheckedFunction, Diagnostic> {
    let mut symbols: HashMap<String, i64> = HashMap::new();
    let mut body = Vec::new();

    for stmt in &function.body {
        match stmt {
            ast::Stmt::Let { name, value, span } => {
                if symbols.contains_key(name) {
                    return Err(Diagnostic::error(
                        format!("'{}' is already declared", name),
                        *span,
                    ));
                }
                let value = eval_const(value, &symbols)?;
                symbols.insert(name.clone(), value);
            }

            ast::Stmt::Syscall { args, .. } => {
                let mut resolved = [0i64; 7];
                for (i, arg) in args.iter().enumerate() {
                    resolved[i] = eval_const(arg, &symbols)?;
                }
                body.push(CheckedStmt::Syscall { args: resolved });
            }
        }
    }

    Ok(CheckedFunction {
        name: function.name.clone(),
        body,
    })
}

fn eval_const(expr: &ast::Expr, symbols: &HashMap<String, i64>) -> Result<i64, Diagnostic> {
    match expr {
        ast::Expr::IntLit(value, _) => Ok(*value),

        ast::Expr::Ident(name, span) => symbols
            .get(name)
            .copied()
            .ok_or_else(|| Diagnostic::error(format!("undefined name '{}'", name), *span)),

        ast::Expr::Unary { op, operand, span } => {
            let value = eval_const(operand, symbols)?;
            match op {
                ast::UnaryOp::Neg => value
                    .checked_neg()
                    .ok_or_else(|| Diagnostic::error("negation overflows a 64-bit integer", *span)),
            }
        }

        ast::Expr::Binary { op, lhs, rhs, span } => {
            let lhs_value = eval_const(lhs, symbols)?;
            let rhs_value = eval_const(rhs, symbols)?;

            match op {
                ast::BinOp::Add => lhs_value
                    .checked_add(rhs_value)
                    .ok_or_else(|| Diagnostic::error("addition overflows a 64-bit integer", *span)),
                ast::BinOp::Sub => lhs_value.checked_sub(rhs_value).ok_or_else(|| {
                    Diagnostic::error("subtraction overflows a 64-bit integer", *span)
                }),
                ast::BinOp::Mul => lhs_value.checked_mul(rhs_value).ok_or_else(|| {
                    Diagnostic::error("multiplication overflows a 64-bit integer", *span)
                }),
                ast::BinOp::Div => {
                    if rhs_value == 0 {
                        return Err(Diagnostic::error("division by zero", *span));
                    }
                    lhs_value.checked_div(rhs_value).ok_or_else(|| {
                        Diagnostic::error("division overflows a 64-bit integer", *span)
                    })
                }
                ast::BinOp::Rem => {
                    if rhs_value == 0 {
                        return Err(Diagnostic::error("division by zero (in '%')", *span));
                    }
                    lhs_value.checked_rem(rhs_value).ok_or_else(|| {
                        Diagnostic::error("remainder overflows a 64-bit integer", *span)
                    })
                }
            }
        }
    }
}
