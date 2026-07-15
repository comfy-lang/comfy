//! IR-level optimization passes, run after lowering and before codegen.

use crate::ast;
use crate::ir::{Function, Instr, Program};
use std::collections::HashSet;

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

/// Tracks which virtual registers are known, at this point in the program,
/// to hold a specific compile-time-constant value, and rewrites any
/// instruction whose inputs are all constant into a plain `Const`. Safe to
/// run unconditionally: virtual registers are assigned exactly once, so a
/// fact "vN == constant" is true for the rest of the function once recorded
/// - unlike a stack slot, a VReg can never be overwritten with something
/// else later.
fn propagate_constants(body: &mut [Instr], vreg_count: u32) -> bool {
    let mut known: Vec<Option<i64>> = vec![None; vreg_count as usize];
    let mut changed = false;

    for instr in body.iter_mut() {
        match instr {
            Instr::Const { dst, value } => {
                known[dst.0 as usize] = Some(*value);
            }

            Instr::Copy { dst, src } => {
                if let Some(value) = known[src.0 as usize] {
                    known[dst.0 as usize] = Some(value);
                    *instr = Instr::Const { dst: *dst, value };
                    changed = true;
                }
            }

            Instr::Unary { dst, op, src } => {
                if let Some(value) = known[src.0 as usize]
                    && let Some(folded) = fold_unary(*op, value)
                {
                    known[dst.0 as usize] = Some(folded);
                    *instr = Instr::Const {
                        dst: *dst,
                        value: folded,
                    };
                    changed = true;
                }
            }

            Instr::Binary { dst, op, lhs, rhs } => {
                if let (Some(l), Some(r)) = (known[lhs.0 as usize], known[rhs.0 as usize])
                    && let Some(folded) = fold_binary(*op, l, r)
                {
                    known[dst.0 as usize] = Some(folded);
                    *instr = Instr::Const {
                        dst: *dst,
                        value: folded,
                    };
                    changed = true;
                }
            }

            Instr::Compare { dst, op, lhs, rhs } => {
                if let (Some(l), Some(r)) = (known[lhs.0 as usize], known[rhs.0 as usize]) {
                    let result = fold_compare(*op, l, r);
                    known[dst.0 as usize] = Some(result);
                    *instr = Instr::Const {
                        dst: *dst,
                        value: result,
                    };
                    changed = true;
                }
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
