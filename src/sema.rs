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
    Array(Box<Ty>, u32),
    Struct(String),
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Bool => write!(f, "bool"),
            Ty::Unit => write!(f, "()"),
            Ty::Pointer(inner) => write!(f, "*{}", inner),
            Ty::Array(elem, len) => write!(f, "[{}; {}]", elem, len),
            Ty::Struct(name) => write!(f, "{}", name),
        }
    }
}

fn resolve_type(
    ty: &ast::TypeName,
    struct_names: &std::collections::HashSet<String>,
) -> Result<Ty, Diagnostic> {
    match ty {
        ast::TypeName::Named(name, span) => match name.as_str() {
            "int" => Ok(Ty::Int),
            "bool" => Ok(Ty::Bool),
            other if struct_names.contains(other) => Ok(Ty::Struct(other.to_string())),
            other => Err(Diagnostic::error(
                format!("unknown type '{}'", other),
                *span,
            )),
        },
        ast::TypeName::Pointer(inner, _) => {
            Ok(Ty::Pointer(Box::new(resolve_type(inner, struct_names)?)))
        }
        ast::TypeName::Array(elem, len, _) => {
            Ok(Ty::Array(Box::new(resolve_type(elem, struct_names)?), *len))
        }
    }
}

/// Resolves an expression that denotes an addressable "place" - a plain
/// local variable, or a chain of field accesses off one - to its absolute
/// stack offset, type, and whether the underlying variable is `mut`.
/// Index expressions aren't handled here yet (`arr[i].field` is a
/// follow-up), so this only covers `name` and `name.a.b.c` chains.
fn resolve_place(expr: &ast::Expr, ctx: &Ctx<'_>) -> Result<(usize, Ty, bool), Diagnostic> {
    match expr {
        ast::Expr::Ident(name, span) => match ctx.symbols.get(name) {
            Some(Symbol::Local(offset, ty, mutable)) => Ok((*offset, ty.clone(), *mutable)),
            Some(Symbol::Const(_, _)) => Err(Diagnostic::error(
                format!(
                    "'{}' is a compile-time constant with no memory location; use 'let mut' instead",
                    name
                ),
                *span,
            )),
            None => Err(Diagnostic::error(
                format!("undefined name '{}'", name),
                *span,
            )),
        },

        ast::Expr::Field { base, field, span } => {
            let (base_offset, base_ty, mutable) = resolve_place(base, ctx)?;
            match base_ty {
                Ty::Struct(struct_name) => {
                    let info = &ctx.structs[&struct_name];
                    let (_, field_ty, field_offset) = info
                        .fields
                        .iter()
                        .find(|(n, _, _)| n == field)
                        .ok_or_else(|| {
                            Diagnostic::error(
                                format!("struct '{}' has no field '{}'", struct_name, field),
                                *span,
                            )
                        })?;
                    Ok((base_offset + *field_offset, field_ty.clone(), mutable))
                }
                other => Err(Diagnostic::error(
                    format!("cannot access field '{}' on a '{}'", field, other),
                    *span,
                )),
            }
        }

        other => Err(Diagnostic::error(
            "only a plain variable or a chain of field accesses is supported here right now",
            other.span(),
        )),
    }
}

/// Lowers an array or struct literal that's the direct initializer of a
/// `let`, flattening it into `Store`s at `base_offset + <field/element
/// offset>`. We have no runtime representation of a whole array/struct
/// *value* yet - only places (memory locations) for them - so this only
/// ever runs at the top of a `let`, recursing into nested composites.
fn lower_composite_literal(
    expr: &ast::Expr,
    base_offset: usize,
    ctx: &Ctx<'_>,
    body: &mut Vec<CheckedStmt>,
) -> Result<Ty, Diagnostic> {
    match expr {
        ast::Expr::ArrayLit { elements, span } => {
            let mut checked_elements = Vec::with_capacity(elements.len());
            let mut elem_ty: Option<Ty> = None;
            for el in elements {
                let (checked, ty) = lower_expr(el, ctx)?;
                match &elem_ty {
                    None => elem_ty = Some(ty.clone()),
                    Some(expected) if *expected != ty => {
                        return Err(Diagnostic::error(
                            format!(
                                "array elements must all have the same type - expected '{}', found '{}'",
                                expected, ty
                            ),
                            el.span(),
                        ));
                    }
                    _ => {}
                }
                checked_elements.push(checked);
            }
            let elem_ty =
                elem_ty.ok_or_else(|| Diagnostic::error("array literal cannot be empty", *span))?;
            let len = checked_elements.len() as u32;
            let elem_size = size_of(&elem_ty, ctx.structs);

            for (i, elem_value) in checked_elements.into_iter().enumerate() {
                body.push(CheckedStmt::Store {
                    offset: base_offset + i * elem_size,
                    value: elem_value,
                });
            }

            Ok(Ty::Array(Box::new(elem_ty), len))
        }

        ast::Expr::StructLit { name, fields, span } => {
            let info = ctx
                .structs
                .get(name)
                .ok_or_else(|| Diagnostic::error(format!("unknown struct '{}'", name), *span))?;

            let mut remaining: HashMap<&str, &ast::Expr> =
                fields.iter().map(|(n, v)| (n.as_str(), v)).collect();
            if remaining.len() != fields.len() {
                return Err(Diagnostic::error(
                    format!("duplicate field in struct literal for '{}'", name),
                    *span,
                ));
            }

            for (field_name, field_ty, field_offset) in &info.fields {
                let value_expr = remaining.remove(field_name.as_str()).ok_or_else(|| {
                    Diagnostic::error(
                        format!(
                            "missing field '{}' in literal for struct '{}'",
                            field_name, name
                        ),
                        *span,
                    )
                })?;

                let value_ty =
                    lower_composite_literal(value_expr, base_offset + *field_offset, ctx, body)?;

                if value_ty != *field_ty {
                    return Err(Diagnostic::error(
                        format!(
                            "field '{}' of struct '{}' expects '{}', found '{}'",
                            field_name, name, field_ty, value_ty
                        ),
                        value_expr.span(),
                    ));
                }
            }

            if let Some((extra_name, extra_expr)) = remaining.into_iter().next() {
                return Err(Diagnostic::error(
                    format!("struct '{}' has no field '{}'", name, extra_name),
                    extra_expr.span(),
                ));
            }

            Ok(Ty::Struct(name.clone()))
        }

        other => {
            let (value, ty) = lower_expr(other, ctx)?;
            body.push(CheckedStmt::Store {
                offset: base_offset,
                value,
            });
            Ok(ty)
        }
    }
}

fn size_of(ty: &Ty, structs: &HashMap<String, StructInfo>) -> usize {
    match ty {
        Ty::Unit => 0,
        Ty::Int | Ty::Bool | Ty::Pointer(_) => 4,
        Ty::Array(elem, len) => size_of(elem, structs) * (*len as usize),
        Ty::Struct(name) => structs.get(name).map(|info| info.size).unwrap_or(0),
    }
}

/// A struct's field layout: name, type, and byte offset, in declaration
/// order, plus the struct's total size.
pub struct StructInfo {
    pub fields: Vec<(String, Ty, usize)>,
    pub size: usize,
}

/// Resolves every struct's field types and computes byte offsets/total
/// size for each. Struct-typed fields are computed recursively (so nested
/// structs work), with a cycle check: a struct can't contain itself *by
/// value*, directly or indirectly (that's infinite size) - only through a
/// pointer, which is always 4 bytes regardless of what it points to.
fn build_struct_registry(
    structs: &[ast::StructDef],
    struct_names: &std::collections::HashSet<String>,
) -> Result<HashMap<String, StructInfo>, Diagnostic> {
    let mut field_types: HashMap<String, Vec<(String, Ty)>> = HashMap::new();
    let mut spans: HashMap<String, Span> = HashMap::new();

    for s in structs {
        let mut seen = std::collections::HashSet::new();
        let mut fields = Vec::with_capacity(s.fields.len());
        for field in &s.fields {
            if !seen.insert(field.name.clone()) {
                return Err(Diagnostic::error(
                    format!(
                        "field '{}' is already defined in struct '{}'",
                        field.name, s.name
                    ),
                    field.span,
                ));
            }
            fields.push((field.name.clone(), resolve_type(&field.ty, struct_names)?));
        }
        field_types.insert(s.name.clone(), fields);
        spans.insert(s.name.clone(), s.span);
    }

    let mut layouts: HashMap<String, StructInfo> = HashMap::new();
    let mut in_progress = std::collections::HashSet::new();
    for name in field_types.keys().cloned().collect::<Vec<_>>() {
        compute_struct_layout(&name, &field_types, &spans, &mut layouts, &mut in_progress)?;
    }

    Ok(layouts)
}

fn compute_struct_layout(
    name: &str,
    field_types: &HashMap<String, Vec<(String, Ty)>>,
    spans: &HashMap<String, Span>,
    layouts: &mut HashMap<String, StructInfo>,
    in_progress: &mut std::collections::HashSet<String>,
) -> Result<(), Diagnostic> {
    if layouts.contains_key(name) {
        return Ok(());
    }
    if !in_progress.insert(name.to_string()) {
        return Err(Diagnostic::error(
            format!(
                "recursive type '{}' has infinite size - try '*{}' (a pointer) instead",
                name, name
            ),
            spans[name],
        ));
    }

    let mut offset = 0usize;
    let mut resolved_fields = Vec::new();
    for (field_name, field_ty) in &field_types[name] {
        if let Ty::Struct(inner_name) = field_ty {
            compute_struct_layout(inner_name, field_types, spans, layouts, in_progress)?;
        }
        let field_size = size_of(field_ty, layouts);
        resolved_fields.push((field_name.clone(), field_ty.clone(), offset));
        offset += field_size;
    }

    in_progress.remove(name);
    layouts.insert(
        name.to_string(),
        StructInfo {
            fields: resolved_fields,
            size: offset,
        },
    );
    Ok(())
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
    Index {
        base_offset: usize,
        index: Box<CheckedExpr>,
        elem_size: usize,
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
    Return(Option<CheckedExpr>),
    StoreThroughPointer {
        address: CheckedExpr,
        value: CheckedExpr,
    },
    StoreIndexed {
        base_offset: usize,
        index: CheckedExpr,
        elem_size: usize,
        value: CheckedExpr,
    },
}

pub fn check(program: &ast::Program) -> Result<CheckedProgram, Diagnostic> {
    let mut struct_names = std::collections::HashSet::new();
    for s in &program.structs {
        if !struct_names.insert(s.name.clone()) {
            return Err(Diagnostic::error(
                format!("struct '{}' is already defined", s.name),
                s.span,
            ));
        }
    }

    let structs = build_struct_registry(&program.structs, &struct_names)?;

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
            let ty = resolve_type(&param.ty, &struct_names)?;
            if matches!(ty, Ty::Array(_, _) | Ty::Struct(_)) {
                return Err(Diagnostic::error(
                    format!(
                        "'{}' can't be a parameter type yet - arrays and structs aren't supported as parameters or return values; pass a pointer instead",
                        ty
                    ),
                    param.span,
                ));
            }
            params.push(ty);
        }

        let return_type = match &function.return_type {
            Some(ty_name) => {
                let ty = resolve_type(ty_name, &struct_names)?;
                if matches!(ty, Ty::Array(_, _) | Ty::Struct(_)) {
                    return Err(Diagnostic::error(
                        format!(
                            "'{}' can't be a return type yet - arrays and structs aren't supported as parameters or return values; return a pointer instead",
                            ty
                        ),
                        ty_name.span(),
                    ));
                }
                ty
            }
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
        functions.push(check_function(function, &sigs, &structs)?);
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
    structs: &'a HashMap<String, StructInfo>,
    return_type: Ty,
    is_entry: bool,
}

fn check_function(
    function: &ast::FunctionDef,
    sigs: &HashMap<String, FunctionSig>,
    structs: &HashMap<String, StructInfo>,
) -> Result<CheckedFunction, Diagnostic> {
    let sig = &sigs[&function.name];

    let mut ctx = Ctx {
        symbols: HashMap::new(),
        frame_size: 0,
        sigs,
        structs,
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

                if matches!(
                    value,
                    ast::Expr::ArrayLit { .. } | ast::Expr::StructLit { .. }
                ) {
                    let base_offset = ctx.frame_size + 4;
                    let ty = lower_composite_literal(value, base_offset, ctx, &mut body)?;
                    ctx.frame_size += size_of(&ty, ctx.structs);

                    ctx.symbols
                        .insert(name.clone(), Symbol::Local(base_offset, ty, *mutable));
                    continue;
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

                ast::AssignTarget::Index { base, index } => {
                    let (base_offset, base_ty, is_mutable) = resolve_place(base, ctx)?;
                    let elem_ty = match base_ty {
                        Ty::Array(elem_ty, _) => *elem_ty,
                        other => {
                            return Err(Diagnostic::error(
                                format!(
                                    "cannot index into a '{}' - only arrays can be indexed",
                                    other
                                ),
                                *span,
                            ));
                        }
                    };

                    if !is_mutable {
                        return Err(Diagnostic::error(
                            "cannot assign to an array element - the array is not declared 'mut'",
                            *span,
                        ));
                    }

                    let index_span = index.span();
                    let (checked_index, index_ty) = lower_expr(index, ctx)?;
                    if index_ty != Ty::Int {
                        return Err(Diagnostic::error(
                            format!("array index must be an 'int', found '{}'", index_ty),
                            index_span,
                        ));
                    }

                    let (checked_value, value_ty) = lower_expr(value, ctx)?;
                    if value_ty != elem_ty {
                        return Err(Diagnostic::error(
                            format!(
                                "cannot assign a '{}' to an element of type '{}'",
                                value_ty, elem_ty
                            ),
                            *span,
                        ));
                    }

                    body.push(CheckedStmt::StoreIndexed {
                        base_offset,
                        index: checked_index,
                        elem_size: size_of(&elem_ty, ctx.structs),
                        value: checked_value,
                    });
                }

                ast::AssignTarget::Field { base, field } => {
                    let (base_offset, base_ty, mutable) = resolve_place(base, ctx)?;
                    let struct_name = match base_ty {
                        Ty::Struct(name) => name,
                        other => {
                            return Err(Diagnostic::error(
                                format!("cannot access field '{}' on a '{}'", field, other),
                                *span,
                            ));
                        }
                    };

                    if !mutable {
                        return Err(Diagnostic::error(
                            format!(
                                "cannot assign to a field of '{}' - it is not declared 'mut'",
                                struct_name
                            ),
                            *span,
                        ));
                    }

                    let info = &ctx.structs[&struct_name];
                    let (_, field_ty, field_offset) = info
                        .fields
                        .iter()
                        .find(|(n, _, _)| n == field)
                        .ok_or_else(|| {
                            Diagnostic::error(
                                format!("struct '{}' has no field '{}'", struct_name, field),
                                *span,
                            )
                        })?;
                    let offset = base_offset + *field_offset;
                    let field_ty = field_ty.clone();

                    let (value, value_ty) = lower_expr(value, ctx)?;
                    if value_ty != field_ty {
                        return Err(Diagnostic::error(
                            format!(
                                "cannot assign a '{}' to field '{}', which is a '{}'",
                                value_ty, field, field_ty
                            ),
                            *span,
                        ));
                    }

                    body.push(CheckedStmt::Store { offset, value });
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
            Some(Symbol::Local(offset, ty, _)) => {
                if matches!(ty, Ty::Array(_, _) | Ty::Struct(_)) {
                    return Err(Diagnostic::error(
                        format!(
                            "cannot use the whole '{}' value of '{}' directly yet - access an individual field or element instead",
                            ty, name
                        ),
                        *span,
                    ));
                }
                Ok((CheckedExpr::Local(*offset), ty.clone()))
            }
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

        ast::Expr::ArrayLit { span, .. } | ast::Expr::StructLit { span, .. } => {
            Err(Diagnostic::error(
                "array/struct literals are only allowed as the direct initializer of a 'let' binding right now",
                *span,
            ))
        }

        ast::Expr::Field { .. } => {
            let (offset, ty, _mutable) = resolve_place(expr, ctx)?;
            if matches!(ty, Ty::Array(_, _) | Ty::Struct(_)) {
                return Err(Diagnostic::error(
                    format!(
                        "cannot use the whole '{}' value directly yet - access an individual field or element instead",
                        ty
                    ),
                    expr.span(),
                ));
            }
            Ok((CheckedExpr::Local(offset), ty))
        }

        ast::Expr::Index { base, index, span } => {
            let (base_offset, base_ty, _mutable) = resolve_place(base, ctx)?;
            let elem_ty = match base_ty {
                Ty::Array(elem_ty, _) => *elem_ty,
                other => {
                    return Err(Diagnostic::error(
                        format!(
                            "cannot index into a '{}' - only arrays can be indexed",
                            other
                        ),
                        *span,
                    ));
                }
            };

            let index_span = index.span();
            let (checked_index, index_ty) = lower_expr(index, ctx)?;
            if index_ty != Ty::Int {
                return Err(Diagnostic::error(
                    format!("array index must be an 'int', found '{}'", index_ty),
                    index_span,
                ));
            }

            let elem_size = size_of(&elem_ty, ctx.structs);
            Ok((
                CheckedExpr::Index {
                    base_offset,
                    index: Box::new(checked_index),
                    elem_size,
                },
                elem_ty,
            ))
        }
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
