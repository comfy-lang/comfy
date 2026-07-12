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
    pub body: Vec<CheckedStmt>,
}

pub enum CheckedStmt {
    Syscall { args: [i64; 7] },
}

pub fn check(program: &ast::Program) -> Result<CheckedProgram, Diagnostic> {
    let mut functions = Vec::new();
    for function in &program.functions {
        functions.push(check_function(function)?);
    }
    Ok(CheckedProgram { functions })
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
    }
}
