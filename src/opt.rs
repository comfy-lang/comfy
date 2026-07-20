//! IR-level optimization passes, run after lowering and before codegen.

use crate::ast;
use crate::ir::{Function, Instr, Program, VReg};
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
        changed |= strength_reduce(&mut function.body, function.vreg_count);
        changed |= fold_branches(&mut function.body, function.vreg_count);
        changed |= eliminate_common_subexprs(&mut function.body);
        changed |= remove_dead_stores(&mut function.body);
        changed |= remove_unused_pure_instrs(&mut function.body);
        changed |= remove_unreachable_code(&mut function.body);
        changed |= convert_tail_calls(&mut function.body); // NEW
        if !changed {
            break;
        }
    }
}

fn propagate_constants(body: &mut [Instr], vreg_count: u32) -> bool {
    let multi = multiply_defined_vregs(body);
    let mut known_vregs: Vec<Option<i64>> = vec![None; vreg_count as usize];
    let mut known_locals: HashMap<usize, i64> = HashMap::new();
    let mut known_local_vregs: HashMap<usize, VReg> = HashMap::new();
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
                } else if let Some(&src) = known_local_vregs.get(offset) {
                    let dst = *dst;
                    let offset = *offset;
                    *instr = Instr::Copy { dst, src };
                    known_local_vregs.insert(offset, dst);
                    changed = true;
                } else {
                    known_local_vregs.insert(*offset, *dst);
                }
            }

            Instr::StoreLocal { offset, src } => {
                match known_vregs[src.0 as usize] {
                    Some(value) => {
                        known_locals.insert(*offset, value);
                    }
                    None => {
                        known_locals.remove(offset);
                    }
                }
                known_local_vregs.insert(*offset, *src);
            }

            // Control-flow join we can't reason about with a flat
            // instruction list - forget everything we assumed about locals.
            Instr::Label(_) => {
                known_locals.clear();
                known_local_vregs.clear();
            }

            // Writes through a raw pointer or an unchecked index could touch
            // any local in the frame; a call or syscall could write through
            // a pointer argument internally. None of these are analyzable
            // here, so conservatively forget every tracked local.
            Instr::Store { .. }
            | Instr::StoreIndexed { .. }
            | Instr::Call { .. }
            | Instr::Syscall { .. } => {
                known_locals.clear();
                known_local_vregs.clear();
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
            Instr::Jump(_) | Instr::Return(_) | Instr::TailCall { .. } => {
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

/// Removes a `StoreLocal` whose value can never be observed: its offset's
/// address is never taken (so no pointer could alias it), no `LoadLocal`
/// anywhere in the function ever reads that offset, and it isn't within
/// the range of any array/struct-field accessed through a runtime index.
fn remove_dead_stores(body: &mut Vec<Instr>) -> bool {
    let before = body.len();

    let mut address_taken: HashSet<usize> = HashSet::new();
    let mut loaded: HashSet<usize> = HashSet::new();
    let mut indexed_base_offsets: Vec<usize> = Vec::new();

    for instr in body.iter() {
        match instr {
            Instr::AddressOf { offset, .. } => {
                address_taken.insert(*offset);
            }
            Instr::LoadLocal { offset, .. } => {
                loaded.insert(*offset);
            }
            Instr::LoadIndexed { base_offset, .. } | Instr::StoreIndexed { base_offset, .. } => {
                indexed_base_offsets.push(*base_offset);
            }
            _ => {}
        }
    }

    // Array elements sit at `base_offset + i * elem_size` for increasing
    // `i`, but the IR doesn't carry the array's length, so we can't know
    // each array's exact upper bound. Conservatively protect every offset
    // at or above the smallest indexed base seen - it might be an element
    // reached via a runtime index we can't reason about further.
    let indexed_cutoff = indexed_base_offsets.into_iter().min();

    body.retain(|instr| match instr {
        Instr::StoreLocal { offset, .. } => {
            address_taken.contains(offset)
                || loaded.contains(offset)
                || indexed_cutoff.is_some_and(|cutoff| *offset >= cutoff)
        }
        _ => true,
    });

    body.len() != before
}

#[derive(PartialEq, Eq, Hash)]
enum CseKey {
    Unary(ast::UnaryOp, u32),
    Binary(ast::BinOp, u32, u32),
    Compare(ast::CompareOp, u32, u32),
    Shl(u32, u32), // (resolved src, shift amount)
}

/// Resolves a vreg to the earliest vreg proven to hold the exact same
/// value, by following any `Copy` chain recorded in `canon`.
fn resolve(canon: &HashMap<u32, u32>, v: VReg) -> u32 {
    let mut cur = v.0;
    while let Some(&next) = canon.get(&cur) {
        cur = next;
    }
    cur
}

/// Replaces a pure computation with a `Copy` from an earlier instruction
/// that provably computed the exact same value from the exact same inputs.
///
/// Operand identity is checked through `canon`, a same-value chain built
/// from `Copy` instructions - in particular, the ones our own
/// store-to-load forwarding in `propagate_constants` introduces. Without
/// it, two reads of the same unmodified local would look like different
/// vregs and never match here.
///
/// A `Label` clears both caches: it's a control-flow join, and code
/// reached from a different path may never have executed the earlier
/// computation at all - reusing its vreg there would read an undefined
/// register. Nothing else needs to invalidate this pass's state: unlike
/// `known_locals` in `propagate_constants`, this only reasons about
/// register values (which, aside from the `&&`/`||` exception guarded by
/// `multi`, never change once defined), never memory - so a
/// `Store`/`Call`/`Syscall` in between can't affect it.
fn eliminate_common_subexprs(body: &mut Vec<Instr>) -> bool {
    let multi = multiply_defined_vregs(body);
    let mut canon: HashMap<u32, u32> = HashMap::new();
    let mut available: HashMap<CseKey, VReg> = HashMap::new();
    let mut changed = false;

    for instr in body.iter_mut() {
        match instr {
            Instr::Copy { dst, src } => {
                if !multi.contains(&dst.0) {
                    let root = resolve(&canon, *src);
                    canon.insert(dst.0, root);
                }
            }
            Instr::Unary { dst, op, src } => {
                let key = CseKey::Unary(*op, resolve(&canon, *src));
                if let Some(&existing) = available.get(&key) {
                    let dst = *dst;
                    *instr = Instr::Copy { dst, src: existing };
                    changed = true;
                } else {
                    available.insert(key, *dst);
                }
            }
            Instr::Binary { dst, op, lhs, rhs } => {
                let key = CseKey::Binary(*op, resolve(&canon, *lhs), resolve(&canon, *rhs));
                if let Some(&existing) = available.get(&key) {
                    let dst = *dst;
                    *instr = Instr::Copy { dst, src: existing };
                    changed = true;
                } else {
                    available.insert(key, *dst);
                }
            }
            Instr::Compare { dst, op, lhs, rhs } => {
                let key = CseKey::Compare(*op, resolve(&canon, *lhs), resolve(&canon, *rhs));
                if let Some(&existing) = available.get(&key) {
                    let dst = *dst;
                    *instr = Instr::Copy { dst, src: existing };
                    changed = true;
                } else {
                    available.insert(key, *dst);
                }
            }
            Instr::Shl { dst, src, shift } => {
                let key = CseKey::Shl(resolve(&canon, *src), *shift);
                if let Some(&existing) = available.get(&key) {
                    let dst = *dst;
                    *instr = Instr::Copy { dst, src: existing };
                    changed = true;
                } else {
                    available.insert(key, *dst);
                }
            }
            Instr::Label(_) => {
                available.clear();
                canon.clear();
            }
            _ => {}
        }
    }

    changed
}

enum Reduced {
    Const(i64),
    Copy(VReg),
    Shl(VReg, u32),
}

fn is_power_of_two(value: i64) -> bool {
    value > 0 && (value & (value - 1)) == 0
}

/// Given one operand's known constant `value` and the other (unknown)
/// operand `other`, decides how to rewrite `other * value` more cheaply -
/// or `None` if `value` isn't a special case worth rewriting.
fn reduce_mul(value: i64, other: VReg) -> Option<Reduced> {
    if value == 0 {
        Some(Reduced::Const(0))
    } else if value == 1 {
        Some(Reduced::Copy(other))
    } else if is_power_of_two(value) {
        Some(Reduced::Shl(other, value.trailing_zeros()))
    } else {
        None
    }
}

/// Rewrites a multiply by a compile-time-known power of two (or the
/// degenerate `*0`/`*1` cases) into a cheaper equivalent. ARM's `mul`
/// instruction can't take an immediate operand at all, so a multiply by a
/// runtime-unknown value against a literal already has to load that
/// literal into a register first - this saves an instruction whenever it
/// applies, not just cycles.
fn strength_reduce(body: &mut [Instr], vreg_count: u32) -> bool {
    let multi = multiply_defined_vregs(body);
    let mut known_vregs: Vec<Option<i64>> = vec![None; vreg_count as usize];
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
                }
            }
            Instr::Binary {
                dst,
                op: ast::BinOp::Mul,
                lhs,
                rhs,
            } => {
                let reduced = known_vregs[rhs.0 as usize]
                    .and_then(|v| reduce_mul(v, *lhs))
                    .or_else(|| known_vregs[lhs.0 as usize].and_then(|v| reduce_mul(v, *rhs)));

                if let Some(new_instr) = reduced {
                    let dst = *dst;
                    *instr = match new_instr {
                        Reduced::Const(value) => Instr::Const { dst, value },
                        Reduced::Copy(src) => Instr::Copy { dst, src },
                        Reduced::Shl(src, shift) => Instr::Shl { dst, src, shift },
                    };
                    changed = true;
                }
            }
            _ => {}
        }
    }

    changed
}

/// Collapses `Call` immediately followed by a `Return` of that exact
/// call's result (or a discarded-result call followed by a bare `Return`)
/// into a single `TailCall`. Since a `return` statement always ends the
/// function wherever it appears, any `Call` directly preceding one is
/// already in tail position - no further reachability analysis needed.
fn convert_tail_calls(body: &mut Vec<Instr>) -> bool {
    let mut changed = false;
    let mut i = 0;

    while i + 1 < body.len() {
        let is_tail_call = match (&body[i], &body[i + 1]) {
            (
                Instr::Call {
                    dst: Some(call_dst),
                    ..
                },
                Instr::Return(Some(ret_v)),
            ) => call_dst == ret_v,
            (Instr::Call { dst: None, .. }, Instr::Return(None)) => true,
            _ => false,
        };

        if is_tail_call {
            body.remove(i + 1); // the Return
            let call = body.remove(i);
            let Instr::Call { name, args, .. } = call else {
                unreachable!("`is_tail_call` only true when body[i] was Instr::Call")
            };
            body.insert(i, Instr::TailCall { name, args });
            changed = true;
        }

        i += 1;
    }

    changed
}
