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
    loop {
        let mut changed = false;
        changed |= propagate_constants(&mut function.body, function.vreg_count);
        changed |= fold_branches(&mut function.body, function.vreg_count);
        changed |= remove_unused_pure_instrs(&mut function.body);
        changed |= remove_unreachable_code(&mut function.body);
        if !changed {
            break;
        }
    }
}

fn propagate_constants(body: &mut [Instr], vreg_count: u32) -> bool {
    let multi = multiply_defined_vregs(body);
    let mut known_vregs: Vec<Option<i64>> = vec![None; vreg_count as usize];
    let mut known_locals: HashMap<usize, i64> = HashMap::new();
    let mut changed = false;

    for instr in body.iter_mut() {
        match instr {
            Instr::Const { dst, value } => {
                if !multi.contains(&dst.0) {
                    known_vregs[dst.0 as usize] = Some(*value);
                }
            }

            Instr::Copy { dst, src } => {
                if !multi.contains(&dst.0)
                    && let Some(value) = known_vregs[src.0 as usize]
                {
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

enum BranchDecision {
    AlwaysJump(String),
    NeverJump,
    Unknown,
}

/// Simplifies a conditional jump whose condition is a compile-time constant
/// into either an unconditional `Jump` or nothing at all.
fn fold_branches(body: &mut Vec<Instr>, vreg_count: u32) -> bool {
    let multi = multiply_defined_vregs(body);
    let mut known: Vec<Option<i64>> = vec![None; vreg_count as usize];
    let mut changed = false;
    let mut i = 0;

    while i < body.len() {
        let decision = match &body[i] {
            Instr::Const { dst, value } => {
                if !multi.contains(&dst.0) {
                    known[dst.0 as usize] = Some(*value);
                }
                BranchDecision::Unknown
            }
            Instr::JumpIfZero { cond, label } => match known[cond.0 as usize] {
                Some(0) => BranchDecision::AlwaysJump(label.clone()),
                Some(_) => BranchDecision::NeverJump,
                None => BranchDecision::Unknown,
            },
            Instr::JumpIfNotZero { cond, label } => match known[cond.0 as usize] {
                Some(v) if v != 0 => BranchDecision::AlwaysJump(label.clone()),
                Some(_) => BranchDecision::NeverJump,
                None => BranchDecision::Unknown,
            },
            _ => BranchDecision::Unknown,
        };

        match decision {
            BranchDecision::AlwaysJump(label) => {
                body[i] = Instr::Jump(label);
                changed = true;
                i += 1;
            }
            BranchDecision::NeverJump => {
                body.remove(i);
                changed = true;
            }
            BranchDecision::Unknown => {
                i += 1;
            }
        }
    }

    changed
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

fn remove_unreachable_code(body: &mut Vec<Instr>) -> bool {
    let before = body.len();

    // A label only "rescues" code out of the unreachable state if it's
    // actually targeted by some jump in this function - otherwise reaching
    // it would require falling through from code already proven dead.
    let mut referenced: HashSet<String> = HashSet::new();
    for instr in body.iter() {
        match instr {
            Instr::Jump(label)
            | Instr::JumpIfZero { label, .. }
            | Instr::JumpIfNotZero { label, .. } => {
                referenced.insert(label.clone());
            }
            _ => {}
        }
    }

    let mut result = Vec::with_capacity(body.len());
    let mut unreachable = false;

    for instr in body.drain(..) {
        match &instr {
            Instr::Label(name) => {
                if referenced.contains(name) {
                    unreachable = false;
                }
                if !unreachable {
                    result.push(instr);
                }
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

/// Virtual registers written by more than one instruction. Almost every
/// vreg has exactly one definition site (lowering always allocates a fresh
/// one via `new_vreg()`) - the one exception is `&&`/`||`, which reuse a
/// single `dst` across two instructions on two different control-flow
/// paths. A flat, flow-insensitive forward scan can't safely reason about
/// those: which instruction "wins" depends on which path executes, so such
/// a vreg must never be treated as provably constant.
fn multiply_defined_vregs(body: &[Instr]) -> HashSet<u32> {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut multi: HashSet<u32> = HashSet::new();
    for instr in body {
        if let Some(dst) = instr.def()
            && !seen.insert(dst.0)
        {
            multi.insert(dst.0);
        }
    }
    multi
}
