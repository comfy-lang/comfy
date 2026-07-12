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
        let mut emitter = Emitter {
            out: String::new(),
            label_counter: 0,
        };

        emitter.out.push_str(".global _start\n");
        emitter.out.push_str(".section .text\n");

        for function in &program.functions {
            emitter.emit_function(function);
        }

        emitter.out
    }
}

/// Bundles the output buffer with a monotonic counter for generating
/// unique, deterministic labels (`.Lif_end0`, `.Lwhile_start1`, ...) as
/// `if`/`while` are walked.
struct Emitter {
    out: String,
    label_counter: usize,
}

impl Emitter {
    fn new_label(&mut self, prefix: &str) -> String {
        let label = format!(".L{}{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    fn emit_function(&mut self, function: &CheckedFunction) {
        let label = if function.name == "main" {
            "_start"
        } else {
            &function.name
        };
        self.out.push_str(&format!("{}:\n", label));

        if function.frame_size > 0 {
            let aligned = (function.frame_size + 7) & !7;
            self.out.push_str("\tmov fp, sp\n");
            self.out.push_str(&format!("\tsub sp, sp, #{}\n", aligned));
        }

        for stmt in &function.body {
            self.emit_stmt(stmt);
        }
    }

    fn emit_stmt(&mut self, stmt: &CheckedStmt) {
        match stmt {
            CheckedStmt::Store { offset, value } => {
                self.emit_expr(value);
                self.out.push_str("\tpop {r0}\n");
                self.out
                    .push_str(&format!("\tstr r0, [fp, #-{}]\n", offset));
            }

            CheckedStmt::Expr(value) => {
                self.emit_expr(value);
                self.out.push_str("\tpop {r0}\n"); // discard the result, we only wanted the side effect
            }

            CheckedStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                self.emit_expr(cond);
                self.out.push_str("\tpop {r0}\n");
                self.out.push_str("\tcmp r0, #0\n");

                let end_label = self.new_label("if_end");

                if let Some(else_body) = else_body {
                    let else_label = self.new_label("if_else");
                    self.out.push_str(&format!("\tbeq {}\n", else_label));
                    for stmt in then_body {
                        self.emit_stmt(stmt);
                    }
                    self.out.push_str(&format!("\tb {}\n", end_label));
                    self.out.push_str(&format!("{}:\n", else_label));
                    for stmt in else_body {
                        self.emit_stmt(stmt);
                    }
                } else {
                    self.out.push_str(&format!("\tbeq {}\n", end_label));
                    for stmt in then_body {
                        self.emit_stmt(stmt);
                    }
                }

                self.out.push_str(&format!("{}:\n", end_label));
            }

            CheckedStmt::While { cond, body } => {
                let start_label = self.new_label("while_start");
                let end_label = self.new_label("while_end");

                self.out.push_str(&format!("{}:\n", start_label));
                self.emit_expr(cond);
                self.out.push_str("\tpop {r0}\n");
                self.out.push_str("\tcmp r0, #0\n");
                self.out.push_str(&format!("\tbeq {}\n", end_label));

                for stmt in body {
                    self.emit_stmt(stmt);
                }

                self.out.push_str(&format!("\tb {}\n", start_label));
                self.out.push_str(&format!("{}:\n", end_label));
            }
        }
    }

    /// Evaluates `expr`, leaving the result on top of the stack.
    fn emit_expr(&mut self, expr: &CheckedExpr) {
        match expr {
            CheckedExpr::Const(value) => {
                self.out.push_str(&format!("\tldr r0, ={}\n", value));
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::Local(offset) => {
                self.out
                    .push_str(&format!("\tldr r0, [fp, #-{}]\n", offset));
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::Unary { op, operand } => {
                self.emit_expr(operand);
                self.out.push_str("\tpop {r0}\n");
                match op {
                    ast::UnaryOp::Neg => self.out.push_str("\trsb r0, r0, #0\n"),
                }
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::Binary { op, lhs, rhs } => {
                self.emit_expr(lhs);
                self.emit_expr(rhs);
                self.out.push_str("\tpop {r1}\n");
                self.out.push_str("\tpop {r0}\n");
                match op {
                    ast::BinOp::Add => self.out.push_str("\tadd r0, r0, r1\n"),
                    ast::BinOp::Sub => self.out.push_str("\tsub r0, r0, r1\n"),
                    ast::BinOp::Mul => self.out.push_str("\tmul r0, r1, r0\n"),
                    ast::BinOp::Div | ast::BinOp::Rem => {
                        unreachable!("division/remainder should have been rejected in sema")
                    }
                }
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::Compare { op, lhs, rhs } => {
                self.emit_expr(lhs);
                self.emit_expr(rhs);
                self.out.push_str("\tpop {r1}\n");
                self.out.push_str("\tpop {r0}\n");
                self.out.push_str("\tcmp r0, r1\n");
                self.out.push_str("\tmov r0, #0\n");
                let cond = match op {
                    ast::CompareOp::Eq => "moveq",
                    ast::CompareOp::Ne => "movne",
                    ast::CompareOp::Lt => "movlt",
                    ast::CompareOp::Le => "movle",
                    ast::CompareOp::Gt => "movgt",
                    ast::CompareOp::Ge => "movge",
                };
                self.out.push_str(&format!("\t{} r0, #1\n", cond));
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::Syscall { args } => {
                for arg in args.iter() {
                    self.emit_expr(arg);
                }
                self.out.push_str("\tpop {r5}\n");
                self.out.push_str("\tpop {r4}\n");
                self.out.push_str("\tpop {r3}\n");
                self.out.push_str("\tpop {r2}\n");
                self.out.push_str("\tpop {r1}\n");
                self.out.push_str("\tpop {r0}\n");
                self.out.push_str("\tpop {r7}\n");
                self.out.push_str("\tsvc #0\n");
                self.out.push_str("\tpush {r0}\n");
            }
        }
    }
}
