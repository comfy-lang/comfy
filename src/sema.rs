//! Semantic analysis: resolves names, evaluates compile-time constants,
//! and produces a `CheckedProgram` for codegen to consume. This is where
//! `let` constants get folded away entirely - codegen never sees them.

use std::collections::HashMap;

use crate::{
    ast,
    diag::{Diagnostic, Span},
};

#[derive(Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
    Unit,
    Pointer(Box<Ty>),
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Bool => write!(f, "bool"),
            Ty::Unit => write!(f, "()"),
            Ty::Pointer(inner) => write!(f, "*{}", inner),
        }
    }
}

fn resolve_type(ty: &ast::TypeName) -> Result<Ty, Diagnostic> {
    match ty {
        ast::TypeName::Named(name, span) => match name.as_str() {
            "int" => Ok(Ty::Int),
            "bool" => Ok(Ty::Bool),
            other => Err(Diagnostic::error(
                format!("unknown type '{}'", other),
                *span,
            )),
        },
        ast::TypeName::Pointer(inner, _) => Ok(Ty::Pointer(Box::new(resolve_type(inner)?))),
    }
}

fn logical_op_str(op: ast::LogicalOp) -> &'static str {
    match op {
        ast::LogicalOp::And => "&&",
        ast::LogicalOp::Or => "||",
    }
}

pub struct CheckedProgram {
    pub functions: Vec<CheckedFunction>,
}

pub struct CheckedFunction {
    pub name: String,
    /// Stack slot offset for each parameter, in declared order - the
    /// prologue stores incoming `r0..r3` into these.
    pub param_offsets: Vec<usize>,
    pub frame_size: usize,
    pub body: Vec<CheckedStmt>,
}

pub enum CheckedExpr {
    Const(i64),
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
    Logical {
        op: ast::LogicalOp,
        lhs: Box<CheckedExpr>,
        rhs: Box<CheckedExpr>,
    },
    Syscall {
        args: Box<[CheckedExpr; 7]>,
    },
    Call {
        name: String,
        args: Vec<CheckedExpr>,
    },
    AddressOf(usize),
    Deref(Box<CheckedExpr>),
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
    Return(Option<CheckedExpr>),
    StoreThroughPointer {
        address: CheckedExpr,
        value: CheckedExpr,
    },
}

pub fn check(program: &ast::Program) -> Result<CheckedProgram, Diagnostic> {
    let mut sigs: HashMap<String, FunctionSig> = HashMap::new();

    for function in &program.functions {
        if sigs.contains_key(&function.name) {
            return Err(Diagnostic::error(
                format!("function '{}' is already defined", function.name),
                function.span,
            ));
        }

        if function.name == "main"
            && (!function.params.is_empty() || function.return_type.is_some())
        {
            return Err(Diagnostic::error(
                "'fn main()' cannot take parameters or return a value yet",
                function.span,
            ));
        }

        if function.params.len() > 4 {
            return Err(Diagnostic::error(
                format!(
                    "function '{}' has {} parameters, but only up to 4 are supported right now",
                    function.name,
                    function.params.len()
                ),
                function.span,
            ));
        }

        let mut params = Vec::with_capacity(function.params.len());
        for param in &function.params {
            params.push(resolve_type(&param.ty)?);
        }

        let return_type = match &function.return_type {
            Some(ty) => resolve_type(ty)?,
            None => Ty::Unit,
        };

        sigs.insert(
            function.name.clone(),
            FunctionSig {
                params,
                return_type,
            },
        );
    }

    if !sigs.contains_key("main") {
        return Err(Diagnostic::error(
            "expected a 'fn main()' function",
            program.functions[0].span,
        ));
    }

    let mut functions = Vec::new();
    for function in &program.functions {
        functions.push(check_function(function, &sigs)?);
    }

    Ok(CheckedProgram { functions })
}

struct FunctionSig {
    params: Vec<Ty>,
    return_type: Ty,
}

enum Symbol {
    Const(i64, Ty),
    Local(usize, Ty, bool), // offset, type, is_mutable
}

struct Ctx<'a> {
    symbols: HashMap<String, Symbol>,
    frame_size: usize,
    sigs: &'a HashMap<String, FunctionSig>,
    return_type: Ty,
    is_entry: bool,
}

fn check_function(
    function: &ast::FunctionDef,
    sigs: &HashMap<String, FunctionSig>,
) -> Result<CheckedFunction, Diagnostic> {
    let sig = &sigs[&function.name];

    let mut ctx = Ctx {
        symbols: HashMap::new(),
        frame_size: 0,
        sigs,
        return_type: sig.return_type.clone(),
        is_entry: function.name == "main",
    };

    let mut param_offsets = Vec::with_capacity(function.params.len());
    for (param, ty) in function.params.iter().zip(&sig.params) {
        ctx.frame_size += 4;
        let offset = ctx.frame_size;
        ctx.symbols
            .insert(param.name.clone(), Symbol::Local(offset, ty.clone(), true));

        param_offsets.push(offset);
    }

    let body = check_block(&function.body, &mut ctx)?;

    if ctx.return_type != Ty::Unit && !ends_with_return(&function.body) {
        return Err(Diagnostic::error(
            format!(
                "function '{}' must end with a 'return' statement (it returns '{}')",
                function.name, ctx.return_type
            ),
            function.span,
        ));
    }

    Ok(CheckedFunction {
        name: function.name.clone(),
        param_offsets,
        frame_size: ctx.frame_size,
        body,
    })
}

fn ends_with_return(stmts: &[ast::Stmt]) -> bool {
    matches!(stmts.last(), Some(ast::Stmt::Return { .. }))
}

fn check_block(stmts: &[ast::Stmt], ctx: &mut Ctx<'_>) -> Result<Vec<CheckedStmt>, Diagnostic> {
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

                let (value, ty) = lower_expr(value, ctx)?;
                if ty == Ty::Unit {
                    return Err(Diagnostic::error(
                        format!("cannot bind '{}' to a value of type '()'", name),
                        *span,
                    ));
                }

                if *mutable {
                    ctx.frame_size += 4;
                    let offset = ctx.frame_size;
                    ctx.symbols
                        .insert(name.clone(), Symbol::Local(offset, ty, true));
                    body.push(CheckedStmt::Store { offset, value });
                } else {
                    match value {
                        CheckedExpr::Const(v) => {
                            ctx.symbols.insert(name.clone(), Symbol::Const(v, ty));
                        }
                        _ => {
                            ctx.frame_size += 4;
                            let offset = ctx.frame_size;
                            ctx.symbols
                                .insert(name.clone(), Symbol::Local(offset, ty, false));
                            body.push(CheckedStmt::Store { offset, value });
                        }
                    }
                }
            }

            ast::Stmt::Assign {
                target,
                value,
                span,
            } => match target {
                ast::AssignTarget::Name(name) => {
                    let (offset, expected_ty) = match ctx.symbols.get(name) {
                        Some(Symbol::Local(offset, ty, true)) => (*offset, ty.clone()),
                        Some(Symbol::Local(_, _, false)) | Some(Symbol::Const(_, _)) => {
                            return Err(Diagnostic::error(
                                format!("cannot assign to '{}' - it is not declared 'mut'", name),
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

                    let (value, ty) = lower_expr(value, ctx)?;
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

                ast::AssignTarget::Deref(pointer_expr) => {
                    let pointer_span = pointer_expr.span();
                    let (address, pointer_ty) = lower_expr(pointer_expr, ctx)?;
                    let pointee_ty = match pointer_ty {
                        Ty::Pointer(inner) => *inner,
                        other => {
                            return Err(Diagnostic::error(
                                format!(
                                    "cannot dereference a '{}' - '*' only applies to pointers",
                                    other
                                ),
                                pointer_span,
                            ));
                        }
                    };

                    let (value, ty) = lower_expr(value, ctx)?;
                    if ty != pointee_ty {
                        return Err(Diagnostic::error(
                            format!(
                                "cannot store a '{}' through a pointer to '{}'",
                                ty, pointee_ty
                            ),
                            *span,
                        ));
                    }

                    body.push(CheckedStmt::StoreThroughPointer { address, value });
                }
            },

            ast::Stmt::Expr { value, .. } => {
                let (value, _ty) = lower_expr(value, ctx)?;
                body.push(CheckedStmt::Expr(value));
            }

            ast::Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                let cond_span = cond.span();
                let (cond, cond_ty) = lower_expr(cond, ctx)?;
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
                let (cond, cond_ty) = lower_expr(cond, ctx)?;
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

            ast::Stmt::Return { value, span } => {
                if ctx.is_entry {
                    return Err(Diagnostic::error(
                        "'return' cannot be used inside 'fn main()' - exit the program via '$syscall' instead",
                        *span,
                    ));
                }
                match value {
                    None => {
                        if ctx.return_type != Ty::Unit {
                            return Err(Diagnostic::error(
                                format!(
                                    "expected a 'return' value of type '{}', found a bare 'return'",
                                    ctx.return_type
                                ),
                                *span,
                            ));
                        }
                        body.push(CheckedStmt::Return(None));
                    }
                    Some(expr) => {
                        let (value, ty) = lower_expr(expr, ctx)?;
                        if ty != ctx.return_type {
                            return Err(Diagnostic::error(
                                format!(
                                    "'return' value has type '{}', expected '{}'",
                                    ty, ctx.return_type
                                ),
                                expr.span(),
                            ));
                        }
                        body.push(CheckedStmt::Return(Some(value)));
                    }
                }
            }
        }
    }

    Ok(body)
}

fn lower_expr(expr: &ast::Expr, ctx: &Ctx<'_>) -> Result<(CheckedExpr, Ty), Diagnostic> {
    match expr {
        ast::Expr::IntLit(value, span) => Ok((to_checked_const(*value, *span)?, Ty::Int)),

        ast::Expr::BoolLit(value, _) => Ok((CheckedExpr::Const(*value as i64), Ty::Bool)),

        ast::Expr::Ident(name, span) => match ctx.symbols.get(name) {
            Some(Symbol::Const(value, ty)) => Ok((CheckedExpr::Const(*value), ty.clone())),
            Some(Symbol::Local(offset, ty, _)) => Ok((CheckedExpr::Local(*offset), ty.clone())),
            None => Err(Diagnostic::error(
                format!("undefined name '{}'", name),
                *span,
            )),
        },

        ast::Expr::Unary { op, operand, span } => {
            let (operand, ty) = lower_expr(operand, ctx)?;
            match op {
                ast::UnaryOp::Neg => {
                    if ty != Ty::Int {
                        return Err(Diagnostic::error(
                            format!("cannot negate a '{}' - '-' only applies to integers", ty),
                            *span,
                        ));
                    }
                    match operand {
                        CheckedExpr::Const(value) => {
                            let folded = value.checked_neg().ok_or_else(|| {
                                Diagnostic::error("negation overflows a 64-bit integer", *span)
                            })?;
                            Ok((to_checked_const(folded, *span)?, Ty::Int))
                        }
                        operand => Ok((
                            CheckedExpr::Unary {
                                op: ast::UnaryOp::Neg,
                                operand: Box::new(operand),
                            },
                            Ty::Int,
                        )),
                    }
                }
                ast::UnaryOp::Not => {
                    if ty != Ty::Bool {
                        return Err(Diagnostic::error(
                            format!(
                                "cannot apply '!' to a '{}' - '!' only applies to booleans",
                                ty
                            ),
                            *span,
                        ));
                    }
                    match operand {
                        CheckedExpr::Const(value) => {
                            Ok((CheckedExpr::Const(if value == 0 { 1 } else { 0 }), Ty::Bool))
                        }
                        operand => Ok((
                            CheckedExpr::Unary {
                                op: ast::UnaryOp::Not,
                                operand: Box::new(operand),
                            },
                            Ty::Bool,
                        )),
                    }
                }
                ast::UnaryOp::Deref => match ty {
                    Ty::Pointer(inner_ty) => Ok((CheckedExpr::Deref(Box::new(operand)), *inner_ty)),
                    other => Err(Diagnostic::error(
                        format!(
                            "cannot dereference a '{}' - '*' only applies to pointers",
                            other
                        ),
                        *span,
                    )),
                },
            }
        }

        ast::Expr::Binary { op, lhs, rhs, span } => {
            let (lhs, lhs_ty) = lower_expr(lhs, ctx)?;
            let (rhs, rhs_ty) = lower_expr(rhs, ctx)?;

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
            let (lhs, lhs_ty) = lower_expr(lhs, ctx)?;
            let (rhs, rhs_ty) = lower_expr(rhs, ctx)?;

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

        ast::Expr::Logical { op, lhs, rhs, span } => {
            let (lhs, lhs_ty) = lower_expr(lhs, ctx)?;
            if lhs_ty != Ty::Bool {
                return Err(Diagnostic::error(
                    format!(
                        "'{}' only applies to booleans, found '{}' on the left",
                        logical_op_str(*op),
                        lhs_ty
                    ),
                    *span,
                ));
            }

            let (rhs, rhs_ty) = lower_expr(rhs, ctx)?;
            if rhs_ty != Ty::Bool {
                return Err(Diagnostic::error(
                    format!(
                        "'{}' only applies to booleans, found '{}' on the right",
                        logical_op_str(*op),
                        rhs_ty
                    ),
                    *span,
                ));
            }

            match (lhs, rhs) {
                (CheckedExpr::Const(lhs_value), CheckedExpr::Const(rhs_value)) => {
                    let result = match op {
                        ast::LogicalOp::And => lhs_value != 0 && rhs_value != 0,
                        ast::LogicalOp::Or => lhs_value != 0 || rhs_value != 0,
                    };
                    Ok((CheckedExpr::Const(result as i64), Ty::Bool))
                }
                (lhs, rhs) => Ok((
                    CheckedExpr::Logical {
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
                let (value, ty) = lower_expr(arg, ctx)?;
                if ty != Ty::Int && !matches!(ty, Ty::Pointer(_)) {
                    return Err(Diagnostic::error(
                        format!(
                            "'$syscall' arguments must be integers or pointers, found a '{}'",
                            ty
                        ),
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

        ast::Expr::Call { name, args, span } => {
            let sig = ctx.sigs.get(name).ok_or_else(|| {
                Diagnostic::error(format!("undefined function '{}'", name), *span)
            })?;

            if args.len() != sig.params.len() {
                return Err(Diagnostic::error(
                    format!(
                        "function '{}' expects {} argument(s), found {}",
                        name,
                        sig.params.len(),
                        args.len()
                    ),
                    *span,
                ));
            }

            let mut lowered = Vec::with_capacity(args.len());
            for (arg, expected_ty) in args.iter().zip(&sig.params) {
                let (value, ty) = lower_expr(arg, ctx)?;
                if ty != *expected_ty {
                    return Err(Diagnostic::error(
                        format!(
                            "argument to '{}' has type '{}', expected '{}'",
                            name, ty, expected_ty
                        ),
                        arg.span(),
                    ));
                }
                lowered.push(value);
            }

            Ok((
                CheckedExpr::Call {
                    name: name.clone(),
                    args: lowered,
                },
                sig.return_type.clone(),
            ))
        }

        ast::Expr::AddressOf { name, span } => match ctx.symbols.get(name) {
            Some(Symbol::Local(offset, ty, _)) => Ok((
                CheckedExpr::AddressOf(*offset),
                Ty::Pointer(Box::new(ty.clone())),
            )),
            Some(Symbol::Const(_, _)) => Err(Diagnostic::error(
                format!(
                    "cannot take the address of '{}' - it's a compile-time constant with no memory location; use 'let mut' instead",
                    name
                ),
                *span,
            )),
            None => Err(Diagnostic::error(
                format!("undefined name '{}'", name),
                *span,
            )),
        },
    }
}

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
