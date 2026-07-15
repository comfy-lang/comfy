//! IR-level optimization passes, run after lowering and before codegen.

use crate::ir::{Function, Instr, Program};
use std::collections::HashSet;

pub fn optimize(program: &mut Program) {
    for function in &mut program.functions {
        eliminate_dead_code(function);
    }
}

fn eliminate_dead_code(function: &mut Function) {
    // Removing one dead instruction can make the instructions that fed it
    // dead too, so iterate to a fixpoint.
    loop {
        let before = function.body.len();
        remove_unused_pure_instrs(&mut function.body);
        remove_unreachable_code(&mut function.body);
        if function.body.len() == before {
            break;
        }
    }
}

/// Drops any pure instruction whose result is never read by a later
/// instruction in the function.
fn remove_unused_pure_instrs(body: &mut Vec<Instr>) {
    let mut used: HashSet<u32> = HashSet::new();
    for instr in body.iter() {
        instr.for_each_use(|v| {
            used.insert(v.0);
        });
    }

    body.retain(|instr| match instr.def() {
        Some(dst) if instr.is_pure() => used.contains(&dst.0),
        _ => true,
    });
}

/// Drops instructions that can never execute: anything between an
/// unconditional `Jump`/`Return` and the next `Label`.
fn remove_unreachable_code(body: &mut Vec<Instr>) {
    let mut result = Vec::with_capacity(body.len());
    let mut unreachable = false;

    for instr in body.drain(..) {
        match instr {
            Instr::Label(_) => {
                unreachable = false;
                result.push(instr);
            }
            _ if unreachable => {} // drop it
            Instr::Jump(_) | Instr::Return(_) => {
                unreachable = true;
                result.push(instr);
            }
            _ => result.push(instr),
        }
    }

    *body = result;
}
