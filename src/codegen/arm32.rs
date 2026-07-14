//! ARM32 (armv7, EABI) backend.
//!
//! Emits GNU-assembler syntax targeting `arm-linux-gnueabihf`. Syscalls use
//! the standard EABI convention: syscall number in `r7`, up to 6 arguments
//! in `r0`-`r5`, trapped with `svc #0`. Function calls follow AAPCS: up to
//! 4 integer arguments in `r0`-`r3`, return value in `r0`.
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
            return_label: None,
        };

        emitter.out.push_str(".global _start\n");
        emitter.out.push_str(".section .text\n");

        for function in &program.functions {
            emitter.emit_function(function);
        }

        emitter.out
    }
}

/// `main` is the process entry point (`_start`), not something anyone
/// calls - it never returns, so it gets no `push {fp, lr}` / epilogue,
/// unlike ordinary functions.
fn label_for(name: &str) -> &str {
    if name == "main" { "_start" } else { name }
}

/// Bundles the output buffer with a monotonic counter for generating
/// unique, deterministic labels (`.Lif_end0`, `.Lwhile_start1`, ...), plus
/// the current function's epilogue label for `return` to jump to.
struct Emitter {
    out: String,
    label_counter: usize,
    return_label: Option<String>,
}

impl Emitter {
    fn new_label(&mut self, prefix: &str) -> String {
        let label = format!(".L{}{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    fn emit_function(&mut self, function: &CheckedFunction) {
        let is_entry = function.name == "main";
        self.out
            .push_str(&format!("{}:\n", label_for(&function.name)));

        if is_entry {
            if function.frame_size > 0 {
                let aligned = (function.frame_size + 7) & !7;
                self.out.push_str("\tmov fp, sp\n");
                self.out.push_str(&format!("\tsub sp, sp, #{}\n", aligned));
            }
            self.return_label = None;
        } else {
            self.out.push_str("\tpush {fp, lr}\n");
            self.out.push_str("\tmov fp, sp\n");
            if function.frame_size > 0 {
                let aligned = (function.frame_size + 7) & !7;
                self.out.push_str(&format!("\tsub sp, sp, #{}\n", aligned));
            }
            self.return_label = Some(format!(".Lret_{}", function.name));
        }

        let arg_regs = ["r0", "r1", "r2", "r3"];
        for (reg, offset) in arg_regs.iter().zip(&function.param_offsets) {
            self.out
                .push_str(&format!("\tstr {}, [fp, #-{}]\n", reg, offset));
        }

        for stmt in &function.body {
            self.emit_stmt(stmt);
        }

        if let Some(label) = self.return_label.clone() {
            self.out.push_str(&format!("{}:\n", label));
            self.out.push_str("\tmov sp, fp\n");
            self.out.push_str("\tpop {fp, lr}\n");
            self.out.push_str("\tbx lr\n");
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

            CheckedStmt::Return(value) => {
                if let Some(value) = value {
                    self.emit_expr(value);
                    self.out.push_str("\tpop {r0}\n");
                }
                let label = self
                    .return_label
                    .clone()
                    .expect("'return' should only appear inside a function body");
                self.out.push_str(&format!("\tb {}\n", label));
            }

            CheckedStmt::StoreThroughPointer { address, value } => {
                self.emit_expr(address);
                self.emit_expr(value);
                self.out.push_str("\tpop {r1}\n"); // value (pushed last, popped first)
                self.out.push_str("\tpop {r0}\n"); // address
                self.out.push_str("\tstr r1, [r0]\n");
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
                    ast::UnaryOp::Not => self.out.push_str("\teor r0, r0, #1\n"),
                    ast::UnaryOp::Deref => {
                        unreachable!("Deref is lowered to CheckedExpr::Deref, not Unary")
                    }
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

            CheckedExpr::Logical { op, lhs, rhs } => match op {
                ast::LogicalOp::And => {
                    let false_label = self.new_label("and_false");
                    let end_label = self.new_label("and_end");

                    self.emit_expr(lhs);
                    self.out.push_str("\tpop {r0}\n");
                    self.out.push_str("\tcmp r0, #0\n");
                    self.out.push_str(&format!("\tbeq {}\n", false_label));

                    self.emit_expr(rhs);
                    self.out.push_str("\tpop {r0}\n");
                    self.out.push_str(&format!("\tb {}\n", end_label));

                    self.out.push_str(&format!("{}:\n", false_label));
                    self.out.push_str("\tmov r0, #0\n");

                    self.out.push_str(&format!("{}:\n", end_label));
                    self.out.push_str("\tpush {r0}\n");
                }
                ast::LogicalOp::Or => {
                    let true_label = self.new_label("or_true");
                    let end_label = self.new_label("or_end");

                    self.emit_expr(lhs);
                    self.out.push_str("\tpop {r0}\n");
                    self.out.push_str("\tcmp r0, #0\n");
                    self.out.push_str(&format!("\tbne {}\n", true_label));

                    self.emit_expr(rhs);
                    self.out.push_str("\tpop {r0}\n");
                    self.out.push_str(&format!("\tb {}\n", end_label));

                    self.out.push_str(&format!("{}:\n", true_label));
                    self.out.push_str("\tmov r0, #1\n");

                    self.out.push_str(&format!("{}:\n", end_label));
                    self.out.push_str("\tpush {r0}\n");
                }
            },

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

            CheckedExpr::Call { name, args } => {
                for arg in args {
                    self.emit_expr(arg);
                }
                for i in (0..args.len()).rev() {
                    self.out.push_str(&format!("\tpop {{r{}}}\n", i));
                }
                self.out.push_str(&format!("\tbl {}\n", label_for(name)));
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::AddressOf(offset) => {
                self.out.push_str(&format!("\tsub r0, fp, #{}\n", offset));
                self.out.push_str("\tpush {r0}\n");
            }

            CheckedExpr::Deref(inner) => {
                self.emit_expr(inner);
                self.out.push_str("\tpop {r0}\n");
                self.out.push_str("\tldr r0, [r0]\n");
                self.out.push_str("\tpush {r0}\n");
            }
        }
    }
}
