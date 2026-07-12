//! Semantic analysis: resolves names, evaluates compile-time constants,
//! and produces a `CheckedProgram` for codegen to consume. This is where
//! `let` constants get folded away entirely - codegen never sees them.

use crate::{
    ast,
    diag::{Diagnostic, Span},
};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Bool => write!(f, "bool"),
        }
    }
}

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
    Compare {
        op: ast::CompareOp,
        lhs: Box<CheckedExpr>,
        rhs: Box<CheckedExpr>,
    },
    Syscall {
        args: Box<[CheckedExpr; 7]>,
    },
}

pub enum CheckedStmt {
    Store {
        offset: usize,
        value: CheckedExpr,
    },
    Expr(CheckedExpr),
    If {
        cond: CheckedExpr,
        then_body: Vec<CheckedStmt>,
        else_body: Option<Vec<CheckedStmt>>,
    },
    While {
        cond: CheckedExpr,
        body: Vec<CheckedStmt>,
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
    Const(i64, Ty),
    Local(usize, Ty),
}

struct Ctx {
    symbols: HashMap<String, Symbol>,
    frame_size: usize,
}

fn check_function(function: &ast::FunctionDef) -> Result<CheckedFunction, Diagnostic> {
    let mut ctx = Ctx {
        symbols: HashMap::new(),
        frame_size: 0,
    };
    let body = check_block(&function.body, &mut ctx)?;
    Ok(CheckedFunction {
        name: function.name.clone(),
        frame_size: ctx.frame_size,
        body,
    })
}

fn check_block(stmts: &[ast::Stmt], ctx: &mut Ctx) -> Result<Vec<CheckedStmt>, Diagnostic> {
    let mut body = Vec::new();

    for stmt in stmts {
        match stmt {
            ast::Stmt::Let {
                name,
                mutable,
                value,
                span,
            } => {
                if ctx.symbols.contains_key(name) {
                    return Err(Diagnostic::error(
                        format!("'{}' is already declared", name),
                        *span,
                    ));
                }

                let (value, ty) = lower_expr(value, &ctx.symbols)?;

                if *mutable {
                    ctx.frame_size += 4;
                    let offset = ctx.frame_size;
                    ctx.symbols.insert(name.clone(), Symbol::Local(offset, ty));
                    body.push(CheckedStmt::Store { offset, value });
                } else {
                    match value {
                        CheckedExpr::Const(v) => {
                            ctx.symbols.insert(name.clone(), Symbol::Const(v, ty));
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
                let (offset, expected_ty) = match ctx.symbols.get(name) {
                    Some(Symbol::Local(offset, ty)) => (*offset, *ty),
                    Some(Symbol::Const(_, _)) => {
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

                let (value, ty) = lower_expr(value, &ctx.symbols)?;
                if ty != expected_ty {
                    return Err(Diagnostic::error(
                        format!(
                            "cannot assign a '{}' to '{}', which is a '{}'",
                            ty, name, expected_ty
                        ),
                        *span,
                    ));
                }
                body.push(CheckedStmt::Store { offset, value });
            }

            ast::Stmt::Expr { value, .. } => {
                let (value, _ty) = lower_expr(value, &ctx.symbols)?;
                body.push(CheckedStmt::Expr(value));
            }

            ast::Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                let cond_span = cond.span();
                let (cond, cond_ty) = lower_expr(cond, &ctx.symbols)?;
                if cond_ty != Ty::Bool {
                    return Err(Diagnostic::error(
                        format!(
                            "'if' condition must be a bool, found '{}' - comfy doesn't implicitly convert integers to booleans; try a comparison like 'x != 0'",
                            cond_ty
                        ),
                        cond_span,
                    ));
                }

                let then_body = check_block(then_body, ctx)?;
                let else_body = match else_body {
                    Some(stmts) => Some(check_block(stmts, ctx)?),
                    None => None,
                };

                body.push(CheckedStmt::If {
                    cond,
                    then_body,
                    else_body,
                });
            }

            ast::Stmt::While {
                cond,
                body: while_body,
                ..
            } => {
                let cond_span = cond.span();
                let (cond, cond_ty) = lower_expr(cond, &ctx.symbols)?;
                if cond_ty != Ty::Bool {
                    return Err(Diagnostic::error(
                        format!(
                            "'while' condition must be a bool, found '{}' - comfy doesn't implicitly convert integers to booleans; try a comparison like 'x != 0'",
                            cond_ty
                        ),
                        cond_span,
                    ));
                }

                let checked_body = check_block(while_body, ctx)?;
                body.push(CheckedStmt::While {
                    cond,
                    body: checked_body,
                });
            }
        }
    }

    Ok(body)
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
) -> Result<(CheckedExpr, Ty), Diagnostic> {
    match expr {
        ast::Expr::IntLit(value, span) => Ok((to_checked_const(*value, *span)?, Ty::Int)),

        ast::Expr::BoolLit(value, _) => Ok((CheckedExpr::Const(*value as i64), Ty::Bool)),

        ast::Expr::Ident(name, span) => match symbols.get(name) {
            Some(Symbol::Const(value, ty)) => Ok((CheckedExpr::Const(*value), *ty)),
            Some(Symbol::Local(offset, ty)) => Ok((CheckedExpr::Local(*offset), *ty)),
            None => Err(Diagnostic::error(
                format!("undefined name '{}'", name),
                *span,
            )),
        },

        ast::Expr::Unary { op, operand, span } => {
            let (operand, ty) = lower_expr(operand, symbols)?;
            if ty != Ty::Int {
                return Err(Diagnostic::error(
                    format!("cannot negate a '{}' - '-' only applies to integers", ty),
                    *span,
                ));
            }
            match (op, operand) {
                (ast::UnaryOp::Neg, CheckedExpr::Const(value)) => {
                    let folded = value.checked_neg().ok_or_else(|| {
                        Diagnostic::error("negation overflows a 64-bit integer", *span)
                    })?;
                    Ok((to_checked_const(folded, *span)?, Ty::Int))
                }
                (ast::UnaryOp::Neg, operand) => Ok((
                    CheckedExpr::Unary {
                        op: ast::UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    Ty::Int,
                )),
            }
        }

        ast::Expr::Binary { op, lhs, rhs, span } => {
            let (lhs, lhs_ty) = lower_expr(lhs, symbols)?;
            let (rhs, rhs_ty) = lower_expr(rhs, symbols)?;

            if lhs_ty != Ty::Int || rhs_ty != Ty::Int {
                return Err(Diagnostic::error(
                    "arithmetic only works on integers",
                    *span,
                ));
            }

            match (lhs, rhs) {
                (CheckedExpr::Const(lhs_value), CheckedExpr::Const(rhs_value)) => {
                    Ok((fold_const(*op, lhs_value, rhs_value, *span)?, Ty::Int))
                }
                (lhs, rhs) => match op {
                    ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul => Ok((
                        CheckedExpr::Binary {
                            op: *op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        Ty::Int,
                    )),
                    ast::BinOp::Div | ast::BinOp::Rem => Err(Diagnostic::error(
                        "division and remainder are only supported between compile-time constants right now (arm32 has no hardware divide instruction)",
                        *span,
                    )),
                },
            }
        }

        ast::Expr::Compare { op, lhs, rhs, span } => {
            let (lhs, lhs_ty) = lower_expr(lhs, symbols)?;
            let (rhs, rhs_ty) = lower_expr(rhs, symbols)?;

            if lhs_ty != Ty::Int || rhs_ty != Ty::Int {
                return Err(Diagnostic::error(
                    "comparisons only work on integers right now",
                    *span,
                ));
            }

            match (lhs, rhs) {
                (CheckedExpr::Const(lhs_value), CheckedExpr::Const(rhs_value)) => {
                    let result = match op {
                        ast::CompareOp::Eq => lhs_value == rhs_value,
                        ast::CompareOp::Ne => lhs_value != rhs_value,
                        ast::CompareOp::Lt => lhs_value < rhs_value,
                        ast::CompareOp::Le => lhs_value <= rhs_value,
                        ast::CompareOp::Gt => lhs_value > rhs_value,
                        ast::CompareOp::Ge => lhs_value >= rhs_value,
                    };
                    Ok((CheckedExpr::Const(result as i64), Ty::Bool))
                }
                (lhs, rhs) => Ok((
                    CheckedExpr::Compare {
                        op: *op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    Ty::Bool,
                )),
            }
        }

        ast::Expr::Syscall { args, .. } => {
            let mut lowered = Vec::with_capacity(7);
            for arg in args.iter() {
                let (value, ty) = lower_expr(arg, symbols)?;
                if ty != Ty::Int {
                    return Err(Diagnostic::error(
                        format!("'$syscall' arguments must be integers, found a '{}'", ty),
                        arg.span(),
                    ));
                }
                lowered.push(value);
            }
            let lowered: [CheckedExpr; 7] = lowered
                .try_into()
                .unwrap_or_else(|_| unreachable!("syscall always has exactly 7 args"));
            Ok((
                CheckedExpr::Syscall {
                    args: Box::new(lowered),
                },
                Ty::Int,
            ))
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
