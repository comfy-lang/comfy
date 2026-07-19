//! IR-level optimization passes, run after lowering and before codegen.

use crate::ast;
use crate::ir::{Function, Instr, Program};
use std::collections::{HashMap, HashSet};

pub fn optimize(program: &mut Program) {
    for function in &mut program.functions {
        optimize_function(function);
    }
}

fn optimize_function(function: &mut Function) {
    // Each pass can expose new opportunities for the others (e.g. folding
    // a Binary into a Const can make its old operands unused), so iterate
    // to a fixpoint.
    loop {
        let mut changed = false;
        changed |= propagate_constants(&mut function.body, function.vreg_count);
        changed |= remove_unused_pure_instrs(&mut function.body);
        changed |= remove_unreachable_code(&mut function.body);
        if !changed {
            break;
        }
    }
}

fn propagate_constants(body: &mut [Instr], vreg_count: u32) -> bool {
    let mut known_vregs: Vec<Option<i64>> = vec![None; vreg_count as usize];
    let mut known_locals: HashMap<usize, i64> = HashMap::new();
    let mut changed = false;

    for instr in body.iter_mut() {
        match instr {
            Instr::Const { dst, value } => {
                known_vregs[dst.0 as usize] = Some(*value);
            }

            Instr::Copy { dst, src } => {
                if let Some(value) = known_vregs[src.0 as usize] {
                    known_vregs[dst.0 as usize] = Some(value);
                    *instr = Instr::Const { dst: *dst, value };
                    changed = true;
                }
            }

            Instr::Unary { dst, op, src } => {
                if let Some(value) = known_vregs[src.0 as usize]
                    && let Some(folded) = fold_unary(*op, value)
                {
                    known_vregs[dst.0 as usize] = Some(folded);
                    *instr = Instr::Const {
                        dst: *dst,
                        value: folded,
                    };
                    changed = true;
                }
            }

            Instr::Binary { dst, op, lhs, rhs } => {
                if let (Some(l), Some(r)) =
                    (known_vregs[lhs.0 as usize], known_vregs[rhs.0 as usize])
                    && let Some(folded) = fold_binary(*op, l, r)
                {
                    known_vregs[dst.0 as usize] = Some(folded);
                    *instr = Instr::Const {
                        dst: *dst,
                        value: folded,
                    };
                    changed = true;
                }
            }

            Instr::Compare { dst, op, lhs, rhs } => {
                if let (Some(l), Some(r)) =
                    (known_vregs[lhs.0 as usize], known_vregs[rhs.0 as usize])
                {
                    let result = fold_compare(*op, l, r);
                    known_vregs[dst.0 as usize] = Some(result);
                    *instr = Instr::Const {
                        dst: *dst,
                        value: result,
                    };
                    changed = true;
                }
            }

            Instr::LoadLocal { dst, offset } => {
                if let Some(&value) = known_locals.get(offset) {
                    known_vregs[dst.0 as usize] = Some(value);
                    *instr = Instr::Const { dst: *dst, value };
                    changed = true;
                }
            }

            Instr::StoreLocal { offset, src } => match known_vregs[src.0 as usize] {
                Some(value) => {
                    known_locals.insert(*offset, value);
                }
                None => {
                    known_locals.remove(offset);
                }
            },

            // Control-flow join we can't reason about with a flat
            // instruction list - forget everything we assumed about locals.
            Instr::Label(_) => known_locals.clear(),

            // Writes through a raw pointer or an unchecked index could touch
            // any local in the frame; a call or syscall could write through
            // a pointer argument internally. None of these are analyzable
            // here, so conservatively forget every tracked local.
            Instr::Store { .. }
            | Instr::StoreIndexed { .. }
            | Instr::Call { .. }
            | Instr::Syscall { .. } => {
                known_locals.clear();
            }

            _ => {}
        }
    }

    changed
}

/// Whether a value fits in the 32-bit registers arm32 actually has - the
/// same constraint `sema::to_checked_const` enforces on literals.
fn fits_i32(value: i64) -> bool {
    i32::try_from(value).is_ok()
}

fn fold_unary(op: ast::UnaryOp, value: i64) -> Option<i64> {
    match op {
        ast::UnaryOp::Neg => value.checked_neg().filter(|v| fits_i32(*v)),
        ast::UnaryOp::Not => Some(value ^ 1),
        ast::UnaryOp::Deref => None, // never appears as Instr::Unary
    }
}

fn fold_binary(op: ast::BinOp, lhs: i64, rhs: i64) -> Option<i64> {
    let result = match op {
        ast::BinOp::Add => lhs.checked_add(rhs)?,
        ast::BinOp::Sub => lhs.checked_sub(rhs)?,
        ast::BinOp::Mul => lhs.checked_mul(rhs)?,
        // sema rejects runtime division/remainder outright, so a Binary
        // instruction with these ops can never reach the IR.
        ast::BinOp::Div | ast::BinOp::Rem => return None,
    };
    fits_i32(result).then_some(result)
}

fn fold_compare(op: ast::CompareOp, lhs: i64, rhs: i64) -> i64 {
    let result = match op {
        ast::CompareOp::Eq => lhs == rhs,
        ast::CompareOp::Ne => lhs != rhs,
        ast::CompareOp::Lt => lhs < rhs,
        ast::CompareOp::Le => lhs <= rhs,
        ast::CompareOp::Gt => lhs > rhs,
        ast::CompareOp::Ge => lhs >= rhs,
    };
    result as i64
}

/// Drops any pure instruction whose result is never read by a later
/// instruction in the function.
fn remove_unused_pure_instrs(body: &mut Vec<Instr>) -> bool {
    let before = body.len();
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

    body.len() != before
}

/// Drops instructions that can never execute: anything between an
/// unconditional `Jump`/`Return` and the next `Label`.
fn remove_unreachable_code(body: &mut Vec<Instr>) -> bool {
    let before = body.len();
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
    body.len() != before
}
