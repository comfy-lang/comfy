//! ARM32 (armv7, EABI) backend.
//!
//! Emits GNU-assembler syntax targeting `arm-linux-gnueabihf`. Syscalls use
//! the standard EABI convention: syscall number in `r7`, up to 6 arguments
//! in `r0`-`r5`, trapped with `svc #0`.
//!
//! Runtime expressions are evaluated with a simple stack machine: every
//! `emit_expr` call leaves its result on top of the stack. This is not
//! optimal codegen (a real register allocator comes later, once there's an
//! IR), but it's simple and obviously correct.

use crate::ast;
use crate::codegen::Backend;
use crate::sema::{CheckedExpr, CheckedFunction, CheckedProgram, CheckedStmt};

pub struct Arm32Backend;

impl Backend for Arm32Backend {
    fn name(&self) -> &'static str {
        "arm32"
    }

    fn emit(&self, program: &CheckedProgram) -> String {
        let mut out = String::new();
        out.push_str(".global _start\n");
        out.push_str(".section .text\n");

        for function in &program.functions {
            emit_function(&mut out, function);
        }

        out
    }
}

fn emit_function(out: &mut String, function: &CheckedFunction) {
    let label = if function.name == "main" {
        "_start"
    } else {
        &function.name
    };
    out.push_str(&format!("{}:\n", label));

    if function.frame_size > 0 {
        let aligned = (function.frame_size + 7) & !7;
        out.push_str("\tmov fp, sp\n");
        out.push_str(&format!("\tsub sp, sp, #{}\n", aligned));
    }

    for stmt in &function.body {
        emit_stmt(out, stmt);
    }
}

fn emit_stmt(out: &mut String, stmt: &CheckedStmt) {
    match stmt {
        CheckedStmt::Store { offset, value } => {
            emit_expr(out, value);
            out.push_str("\tpop {r0}\n");
            out.push_str(&format!("\tstr r0, [fp, #-{}]\n", offset));
        }

        CheckedStmt::Expr(value) => {
            emit_expr(out, value);
            out.push_str("\tpop {r0}\n"); // discard the result, we only wanted the side effect
        }
    }
}

/// Evaluates `expr`, leaving the result on top of the stack.
fn emit_expr(out: &mut String, expr: &CheckedExpr) {
    match expr {
        CheckedExpr::Const(value) => {
            out.push_str(&format!("\tldr r0, ={}\n", value));
            out.push_str("\tpush {r0}\n");
        }

        CheckedExpr::Local(offset) => {
            out.push_str(&format!("\tldr r0, [fp, #-{}]\n", offset));
            out.push_str("\tpush {r0}\n");
        }

        CheckedExpr::Unary { op, operand } => {
            emit_expr(out, operand);
            out.push_str("\tpop {r0}\n");
            match op {
                ast::UnaryOp::Neg => out.push_str("\trsb r0, r0, #0\n"),
            }
            out.push_str("\tpush {r0}\n");
        }

        CheckedExpr::Binary { op, lhs, rhs } => {
            emit_expr(out, lhs);
            emit_expr(out, rhs);
            out.push_str("\tpop {r1}\n");
            out.push_str("\tpop {r0}\n");
            match op {
                ast::BinOp::Add => out.push_str("\tadd r0, r0, r1\n"),
                ast::BinOp::Sub => out.push_str("\tsub r0, r0, r1\n"),
                ast::BinOp::Mul => out.push_str("\tmul r0, r1, r0\n"),
                ast::BinOp::Div | ast::BinOp::Rem => {
                    unreachable!("division/remainder should have been rejected in sema")
                }
            }
            out.push_str("\tpush {r0}\n");
        }

        CheckedExpr::Syscall { args } => {
            for arg in args.iter() {
                emit_expr(out, arg);
            }
            out.push_str("\tpop {r5}\n");
            out.push_str("\tpop {r4}\n");
            out.push_str("\tpop {r3}\n");
            out.push_str("\tpop {r2}\n");
            out.push_str("\tpop {r1}\n");
            out.push_str("\tpop {r0}\n");
            out.push_str("\tpop {r7}\n");
            out.push_str("\tsvc #0\n");
            out.push_str("\tpush {r0}\n"); // leave the syscall's return value on the stack
        }
    }
}
