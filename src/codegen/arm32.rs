//! ARM32 (armv7, EABI) backend.
//!
//! Emits GNU-assembler syntax targeting `arm-linux-gnueabihf`. Syscalls use
//! the standard EABI convention: syscall number in `r7`, up to 6 arguments
//! in `r0`-`r5`, trapped with `svc #0`.

use crate::codegen::Backend;
use crate::sema::{CheckedProgram, CheckedStmt};

pub struct Arm32Backend;

const ARG_REGS: [&str; 6] = ["r0", "r1", "r2", "r3", "r4", "r5"];

impl Backend for Arm32Backend {
    fn name(&self) -> &'static str {
        "arm32"
    }

    fn emit(&self, program: &CheckedProgram) -> String {
        let mut out = String::new();
        out.push_str(".global _start\n");
        out.push_str(".section .text\n");

        for function in &program.functions {
            let label = if function.name == "main" {
                "_start"
            } else {
                &function.name
            };
            out.push_str(&format!("{}:\n", label));

            for stmt in &function.body {
                emit_stmt(&mut out, stmt);
            }
        }

        out
    }
}

fn emit_stmt(out: &mut String, stmt: &CheckedStmt) {
    match stmt {
        CheckedStmt::Syscall { args, .. } => {
            let [nr, a0, a1, a2, a3, a4, a5] = *args;
            out.push_str(&format!("\tldr r7, ={}\n", nr));
            for (reg, value) in ARG_REGS.iter().zip([a0, a1, a2, a3, a4, a5]) {
                out.push_str(&format!("\tldr {}, ={}\n", reg, value));
            }
            out.push_str("\tsvc #0\n");
        }
    }
}
