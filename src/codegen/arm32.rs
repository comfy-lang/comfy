//! ARM32 (armv7, EABI) backend.
//!
//! Emits GNU-assembler syntax targeting `arm-linux-gnueabihf`. Consumes the
//! flat IR (`crate::ir`) rather than the checked expression tree directly.
//! Every virtual register gets its own dedicated stack spill slot for now -
//! this is intentionally "dumb" (equivalent in spirit to the old push/pop
//! stack machine), just restructured around the new IR. Real register
//! allocation (mapping hot virtual registers to `r4`-`r11` instead of
//! memory) is a follow-up step.

use crate::codegen::Backend;
use crate::ir::{self, VReg};
use crate::regalloc::Location;
use crate::{ast, regalloc};

/// Physical registers available for the linear-scan allocator to hand out.
/// Excluded: r0-r5 (used as internal scratch by various `Instr` cases,
/// e.g. syscalls need r0-r5 simultaneously), r7 (hard-wired syscall
/// number register), r11/fp (our frame pointer).
const ALLOCATABLE_REGS: [&str; 4] = ["r6", "r8", "r9", "r10"];

pub struct Arm32Backend;

impl Backend for Arm32Backend {
    fn name(&self) -> &'static str {
        "arm32"
    }

    fn emit(&self, program: &ir::Program) -> String {
        let mut emitter = Emitter {
            out: String::new(),
            return_label: None,
            vreg_base: 0,
            locations: Vec::new(),
            saved_regs: String::new(),
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

struct Emitter {
    out: String,
    return_label: Option<String>,
    vreg_base: usize,
    /// Where each virtual register currently lives, computed fresh per
    /// function by the register allocator.
    locations: Vec<Location>,
    /// The exact `push {...}`/`pop {...}` register list used in this
    /// function's prologue, so the epilogue can mirror it exactly.
    saved_regs: String,
}

impl Emitter {
    fn spill_offset(&self, slot: usize) -> usize {
        self.vreg_base + 4 * (slot + 1)
    }

    fn load(&mut self, reg: &str, v: VReg) {
        match self.locations[v.0 as usize] {
            Location::Register(slot) => {
                let src = ALLOCATABLE_REGS[slot as usize];
                if src != reg {
                    self.out.push_str(&format!("\tmov {}, {}\n", reg, src));
                }
            }
            Location::Spill(slot) => {
                let offset = self.spill_offset(slot);
                self.out
                    .push_str(&format!("\tldr {}, [fp, #-{}]\n", reg, offset));
            }
        }
    }

    fn store(&mut self, reg: &str, v: VReg) {
        match self.locations[v.0 as usize] {
            Location::Register(slot) => {
                let dst = ALLOCATABLE_REGS[slot as usize];
                if dst != reg {
                    self.out.push_str(&format!("\tmov {}, {}\n", dst, reg));
                }
            }
            Location::Spill(slot) => {
                let offset = self.spill_offset(slot);
                self.out
                    .push_str(&format!("\tstr {}, [fp, #-{}]\n", reg, offset));
            }
        }
    }

    fn emit_function(&mut self, function: &ir::Function) {
        let allocation = regalloc::allocate(
            &function.body,
            function.vreg_count,
            ALLOCATABLE_REGS.len() as u32,
        );
        self.vreg_base = function.frame_size;
        let total_frame = function.frame_size + 4 * allocation.spill_count;
        self.locations = allocation.locations;

        let used_regs: Vec<&str> = ALLOCATABLE_REGS
            .iter()
            .zip(&allocation.used_registers)
            .filter_map(|(reg, &used)| used.then_some(*reg))
            .collect();

        self.out
            .push_str(&format!("{}:\n", label_for(&function.name)));

        if function.is_entry {
            if total_frame > 0 {
                let aligned = (total_frame + 7) & !7;
                self.out.push_str("\tmov fp, sp\n");
                self.out.push_str(&format!("\tsub sp, sp, #{}\n", aligned));
            }
            self.return_label = None;
        } else {
            self.saved_regs = if used_regs.is_empty() {
                "fp, lr".to_string()
            } else {
                format!("{}, fp, lr", used_regs.join(", "))
            };
            self.out
                .push_str(&format!("\tpush {{{}}}\n", self.saved_regs));
            self.out.push_str("\tmov fp, sp\n");
            if total_frame > 0 {
                let aligned = (total_frame + 7) & !7;
                self.out.push_str(&format!("\tsub sp, sp, #{}\n", aligned));
            }
            self.return_label = Some(format!(".Lret_{}", function.name));
        }

        let arg_regs = ["r0", "r1", "r2", "r3"];
        for (reg, offset) in arg_regs.iter().zip(&function.param_offsets) {
            self.out
                .push_str(&format!("\tstr {}, [fp, #-{}]\n", reg, offset));
        }

        for instr in &function.body {
            self.emit_instr(instr);
        }

        if let Some(label) = self.return_label.clone() {
            self.out.push_str(&format!("{}:\n", label));
            self.out.push_str("\tmov sp, fp\n");
            self.out
                .push_str(&format!("\tpop {{{}}}\n", self.saved_regs));
            self.out.push_str("\tbx lr\n");
        }
    }

    fn emit_instr(&mut self, instr: &ir::Instr) {
        match instr {
            ir::Instr::Const { dst, value } => {
                self.out.push_str(&format!("\tldr r0, ={}\n", value));
                self.store("r0", *dst);
            }

            ir::Instr::Copy { dst, src } => {
                self.load("r0", *src);
                self.store("r0", *dst);
            }

            ir::Instr::Unary { dst, op, src } => {
                self.load("r0", *src);
                match op {
                    ast::UnaryOp::Neg => self.out.push_str("\trsb r0, r0, #0\n"),
                    ast::UnaryOp::Not => self.out.push_str("\teor r0, r0, #1\n"),
                    ast::UnaryOp::Deref => unreachable!("Deref lowers to ir::Instr::Load"),
                }
                self.store("r0", *dst);
            }

            ir::Instr::Binary { dst, op, lhs, rhs } => {
                self.load("r0", *lhs);
                self.load("r1", *rhs);
                match op {
                    ast::BinOp::Add => self.out.push_str("\tadd r0, r0, r1\n"),
                    ast::BinOp::Sub => self.out.push_str("\tsub r0, r0, r1\n"),
                    ast::BinOp::Mul => self.out.push_str("\tmul r0, r1, r0\n"),
                    ast::BinOp::Div | ast::BinOp::Rem => {
                        unreachable!("division/remainder should have been rejected in sema")
                    }
                }
                self.store("r0", *dst);
            }

            ir::Instr::Compare { dst, op, lhs, rhs } => {
                self.load("r0", *lhs);
                self.load("r1", *rhs);
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
                self.store("r0", *dst);
            }

            ir::Instr::LoadLocal { dst, offset } => {
                self.out
                    .push_str(&format!("\tldr r0, [fp, #-{}]\n", offset));
                self.store("r0", *dst);
            }

            ir::Instr::StoreLocal { offset, src } => {
                self.load("r0", *src);
                self.out
                    .push_str(&format!("\tstr r0, [fp, #-{}]\n", offset));
            }

            ir::Instr::LoadIndexed {
                dst,
                base_offset,
                index,
                elem_size,
            } => {
                self.load("r1", *index);
                self.out.push_str(&format!("\tmov r2, #{}\n", elem_size));
                self.out.push_str("\tmul r1, r2, r1\n");
                self.out
                    .push_str(&format!("\tsub r0, fp, #{}\n", base_offset));
                self.out.push_str("\tsub r0, r0, r1\n");
                self.out.push_str("\tldr r0, [r0]\n");
                self.store("r0", *dst);
            }

            ir::Instr::StoreIndexed {
                base_offset,
                index,
                elem_size,
                src,
            } => {
                self.load("r1", *index);
                self.load("r3", *src);
                self.out.push_str(&format!("\tmov r2, #{}\n", elem_size));
                self.out.push_str("\tmul r1, r2, r1\n");
                self.out
                    .push_str(&format!("\tsub r0, fp, #{}\n", base_offset));
                self.out.push_str("\tsub r0, r0, r1\n");
                self.out.push_str("\tstr r3, [r0]\n");
            }

            ir::Instr::AddressOf { dst, offset } => {
                self.out.push_str(&format!("\tsub r0, fp, #{}\n", offset));
                self.store("r0", *dst);
            }

            ir::Instr::Load { dst, addr } => {
                self.load("r0", *addr);
                self.out.push_str("\tldr r0, [r0]\n");
                self.store("r0", *dst);
            }

            ir::Instr::Store { addr, src } => {
                self.load("r0", *addr);
                self.load("r1", *src);
                self.out.push_str("\tstr r1, [r0]\n");
            }

            ir::Instr::Syscall { dst, args } => {
                self.load("r7", args[0]);
                let regs = ["r0", "r1", "r2", "r3", "r4", "r5"];
                for (reg, arg) in regs.iter().zip(&args[1..]) {
                    self.load(reg, *arg);
                }
                self.out.push_str("\tsvc #0\n");
                self.store("r0", *dst);
            }

            ir::Instr::Call { dst, name, args } => {
                let regs = ["r0", "r1", "r2", "r3"];
                for (reg, arg) in regs.iter().zip(args) {
                    self.load(reg, *arg);
                }
                self.out.push_str(&format!("\tbl {}\n", label_for(name)));
                if let Some(dst) = dst {
                    self.store("r0", *dst);
                }
            }

            ir::Instr::Label(label) => {
                self.out.push_str(&format!("{}:\n", label));
            }

            ir::Instr::Jump(label) => {
                self.out.push_str(&format!("\tb {}\n", label));
            }

            ir::Instr::JumpIfZero { cond, label } => {
                self.load("r0", *cond);
                self.out.push_str("\tcmp r0, #0\n");
                self.out.push_str(&format!("\tbeq {}\n", label));
            }

            ir::Instr::JumpIfNotZero { cond, label } => {
                self.load("r0", *cond);
                self.out.push_str("\tcmp r0, #0\n");
                self.out.push_str(&format!("\tbne {}\n", label));
            }

            ir::Instr::Return(value) => {
                if let Some(v) = value {
                    self.load("r0", *v);
                }
                let label = self
                    .return_label
                    .clone()
                    .expect("'return' should only appear inside a function body");
                self.out.push_str(&format!("\tb {}\n", label));
            }
        }
    }
}
