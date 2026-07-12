//! Semantic analysis: resolves names, evaluates compile-time constants,
//! and produces a `CheckedProgram` for codegen to consume. This is where
//! `let` constants get folded away entirely - codegen never sees them.

use std::collections::HashMap;

use crate::{
    ast,
    diag::{Diagnostic, Span},
};

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
    let mut symbols: HashMap<String, Symbol> = HashMap::new();
    let mut body = Vec::new();
    let mut frame_size: usize = 0;

    for stmt in &function.body {
        match stmt {
            ast::Stmt::Let {
                name,
                mutable,
                value,
                span,
            } => {
                if symbols.contains_key(name) {
                    return Err(Diagnostic::error(
                        format!("'{}' is already declared", name),
                        *span,
                    ));
                }

                let value = lower_expr(value, &symbols)?;

                if *mutable {
                    frame_size += 4;
                    let offset = frame_size;
                    symbols.insert(name.clone(), Symbol::Local(offset));
                    body.push(CheckedStmt::Store { offset, value });
                } else {
                    match value {
                        CheckedExpr::Const(v) => {
                            symbols.insert(name.clone(), Symbol::Const(v));
                        }
                        _ => {
                            return Err(Diagnostic::error(
                                format!(
                                    "'{}' is not a compile-time constant; use 'let mut' instead",
                                    name
                                ),
                                *span,
                            ));
                        }
                    }
                }
            }

            ast::Stmt::Assign { name, value, span } => {
                let offset = match symbols.get(name) {
                    Some(Symbol::Local(offset)) => *offset,
                    Some(Symbol::Const(_)) => {
                        return Err(Diagnostic::error(
                            format!("cannot assign to '{}' - it is a constant, not 'mut'", name),
                            *span,
                        ));
                    }
                    None => {
                        return Err(Diagnostic::error(
                            format!("undefined name '{}'", name),
                            *span,
                        ));
                    }
                };

                let value = lower_expr(value, &symbols)?;
                body.push(CheckedStmt::Store { offset, value });
            }

            ast::Stmt::Syscall { args, .. } => {
                let resolved = [
                    lower_expr(&args[0], &symbols)?,
                    lower_expr(&args[1], &symbols)?,
                    lower_expr(&args[2], &symbols)?,
                    lower_expr(&args[3], &symbols)?,
                    lower_expr(&args[4], &symbols)?,
                    lower_expr(&args[5], &symbols)?,
                    lower_expr(&args[6], &symbols)?,
                ];
                body.push(CheckedStmt::Syscall { args: resolved });
            }
        }
    }

    Ok(CheckedFunction {
        name: function.name.clone(),
        frame_size,
        body,
    })
}

/// Builds a `Const` node, checking the value actually fits in a 32-bit
/// register - arm32 has no 64-bit arithmetic, so anything that doesn't
/// fit is a compile error rather than a silent truncation at codegen time.
fn to_checked_const(value: i64, span: Span) -> Result<CheckedExpr, Diagnostic> {
    i32::try_from(value).map(|_| CheckedExpr::Const(value)).map_err(|_| {
        Diagnostic::error(
            format!(
                "value {} does not fit in a 32-bit register (arm32 only supports 32-bit integers)",
                value
            ),
            span,
        )
    })
}

fn lower_expr(
    expr: &ast::Expr,
    symbols: &HashMap<String, Symbol>,
) -> Result<CheckedExpr, Diagnostic> {
    match expr {
        ast::Expr::IntLit(value, span) => to_checked_const(*value, *span),

        ast::Expr::Ident(name, span) => match symbols.get(name) {
            Some(Symbol::Const(value)) => Ok(CheckedExpr::Const(*value)),
            Some(Symbol::Local(offset)) => Ok(CheckedExpr::Local(*offset)),
            None => Err(Diagnostic::error(
                format!("undefined name '{}'", name),
                *span,
            )),
        },

        ast::Expr::Unary { op, operand, span } => {
            let operand = lower_expr(operand, symbols)?;
            match (op, operand) {
                (ast::UnaryOp::Neg, CheckedExpr::Const(value)) => value
                    .checked_neg()
                    .ok_or_else(|| Diagnostic::error("negation overflows a 64-bit integer", *span))
                    .and_then(|v| to_checked_const(v, *span)),
                (ast::UnaryOp::Neg, operand) => Ok(CheckedExpr::Unary {
                    op: ast::UnaryOp::Neg,
                    operand: Box::new(operand),
                }),
            }
        }

        ast::Expr::Binary { op, lhs, rhs, span } => {
            let lhs = lower_expr(lhs, symbols)?;
            let rhs = lower_expr(rhs, symbols)?;

            match (lhs, rhs) {
                (CheckedExpr::Const(lhs_value), CheckedExpr::Const(rhs_value)) => {
                    fold_const(*op, lhs_value, rhs_value, *span)
                }
                (lhs, rhs) => match op {
                    ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul => {
                        Ok(CheckedExpr::Binary {
                            op: *op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        })
                    }
                    ast::BinOp::Div | ast::BinOp::Rem => Err(Diagnostic::error(
                        "division and remainder are only supported between compile-time constants right now (arm32 has no hardware divide instruction)",
                        *span,
                    )),
                },
            }
        }
    }
}

fn fold_const(op: ast::BinOp, lhs: i64, rhs: i64, span: Span) -> Result<CheckedExpr, Diagnostic> {
    let result = match op {
        ast::BinOp::Add => lhs
            .checked_add(rhs)
            .ok_or_else(|| Diagnostic::error("addition overflows a 64-bit integer", span)),
        ast::BinOp::Sub => lhs
            .checked_sub(rhs)
            .ok_or_else(|| Diagnostic::error("subtraction overflows a 64-bit integer", span)),
        ast::BinOp::Mul => lhs
            .checked_mul(rhs)
            .ok_or_else(|| Diagnostic::error("multiplication overflows a 64-bit integer", span)),
        ast::BinOp::Div => {
            if rhs == 0 {
                return Err(Diagnostic::error("division by zero", span));
            }
            lhs.checked_div(rhs)
                .ok_or_else(|| Diagnostic::error("division overflows a 64-bit integer", span))
        }
        ast::BinOp::Rem => {
            if rhs == 0 {
                return Err(Diagnostic::error("division by zero (in '%')", span));
            }
            lhs.checked_rem(rhs)
                .ok_or_else(|| Diagnostic::error("remainder overflows a 64-bit integer", span))
        }
    }?;
    to_checked_const(result, span)
}
