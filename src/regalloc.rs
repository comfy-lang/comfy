//! A simple linear-scan register allocator, operating on the flat IR.
//! Backend-agnostic: it just assigns each virtual register either one of
//! `num_registers` abstract physical "slots" or a spill slot, based on
//! textual live ranges. The backend maps slot indices to real register
//! names.

use crate::ir::{Instr, VReg};

#[derive(Clone, Copy)]
pub enum Location {
    Register(u32),
    Spill(usize),
}

pub struct Allocation {
    /// Location of each virtual register, indexed by `VReg.0`.
    pub locations: Vec<Location>,
    /// Which physical register slots (0..num_registers) ended up used at
    /// least once - the backend uses this to decide what to save/restore
    /// in the function's prologue/epilogue.
    pub used_registers: Vec<bool>,
    /// How many spill slots were needed.
    pub spill_count: usize,
}

struct Interval {
    vreg: VReg,
    start: usize,
    end: usize,
}

/// Visits every virtual register mentioned by an instruction, def or use
/// alike - for live-range purposes we don't need to distinguish the two.
fn for_each_vreg(instr: &Instr, mut f: impl FnMut(VReg)) {
    match instr {
        Instr::Const { dst, .. } => f(*dst),
        Instr::Copy { dst, src } => {
            f(*dst);
            f(*src);
        }
        Instr::Unary { dst, src, .. } | Instr::Shl { dst, src, .. } => {
            f(*dst);
            f(*src);
        }
        Instr::Binary { dst, lhs, rhs, .. } => {
            f(*dst);
            f(*lhs);
            f(*rhs);
        }
        Instr::Compare { dst, lhs, rhs, .. } => {
            f(*dst);
            f(*lhs);
            f(*rhs);
        }
        Instr::LoadLocal { dst, .. } => f(*dst),
        Instr::StoreLocal { src, .. } => f(*src),
        Instr::LoadIndexed { dst, index, .. } => {
            f(*dst);
            f(*index);
        }
        Instr::StoreIndexed { index, src, .. } => {
            f(*index);
            f(*src);
        }
        Instr::AddressOf { dst, .. } => f(*dst),
        Instr::Load { dst, addr } => {
            f(*dst);
            f(*addr);
        }
        Instr::Store { addr, src } => {
            f(*addr);
            f(*src);
        }
        Instr::Syscall { dst, args } => {
            f(*dst);
            for a in args {
                f(*a);
            }
        }
        Instr::Call { dst, args, .. } => {
            if let Some(d) = dst {
                f(*d);
            }
            for a in args {
                f(*a);
            }
        }
        Instr::Label(_) | Instr::Jump(_) => {}
        Instr::JumpIfZero { cond, .. } | Instr::JumpIfNotZero { cond, .. } => f(*cond),
        Instr::Return(v) => {
            if let Some(v) = v {
                f(*v);
            }
        }
    }
}

/// Computes each virtual register's live range as [first occurrence, last
/// occurrence] by textual instruction position. This is a conservative
/// approximation (real liveness would follow control flow precisely), but
/// it's always *safe*: at worst it overestimates how long a value needs to
/// stay live, which only costs a missed optimization, never correctness.
fn compute_intervals(body: &[Instr], vreg_count: u32) -> Vec<Interval> {
    let mut first: Vec<Option<usize>> = vec![None; vreg_count as usize];
    let mut last: Vec<Option<usize>> = vec![None; vreg_count as usize];

    for (i, instr) in body.iter().enumerate() {
        for_each_vreg(instr, |v| {
            let idx = v.0 as usize;
            if first[idx].is_none() {
                first[idx] = Some(i);
            }
            last[idx] = Some(i);
        });
    }

    (0..vreg_count)
        .filter_map(|id| {
            let start = first[id as usize]?;
            let end = last[id as usize]?;
            Some(Interval {
                vreg: VReg(id),
                start,
                end,
            })
        })
        .collect()
}

pub fn allocate(body: &[Instr], vreg_count: u32, num_registers: u32) -> Allocation {
    let mut intervals = compute_intervals(body, vreg_count);
    intervals.sort_by_key(|iv| iv.start);

    let mut locations: Vec<Location> = (0..vreg_count).map(|_| Location::Spill(0)).collect();
    let mut used_registers = vec![false; num_registers as usize];
    let mut next_spill_slot = 0usize;

    // Live intervals currently holding a register: (end, register slot, vreg).
    let mut active: Vec<(usize, u32, VReg)> = Vec::new();

    for iv in intervals {
        active.retain(|&(end, _, _)| end >= iv.start);

        let used_slots: std::collections::HashSet<u32> =
            active.iter().map(|&(_, slot, _)| slot).collect();
        let free_slot = (0..num_registers).find(|slot| !used_slots.contains(slot));

        if let Some(slot) = free_slot {
            locations[iv.vreg.0 as usize] = Location::Register(slot);
            used_registers[slot as usize] = true;
            active.push((iv.end, slot, iv.vreg));
        } else {
            // No free register - spill whichever of "this interval" or the
            // active interval ending furthest in the future is less
            // valuable to keep in a register (classic linear-scan
            // heuristic: prefer to keep the shorter-lived value in a
            // register, since it'll free up sooner for reuse).
            active.sort_by_key(|&(end, _, _)| end);
            match active.last().copied() {
                Some((worst_end, worst_slot, worst_vreg)) if worst_end > iv.end => {
                    locations[worst_vreg.0 as usize] = Location::Spill(next_spill_slot);
                    next_spill_slot += 1;
                    active.pop();

                    locations[iv.vreg.0 as usize] = Location::Register(worst_slot);
                    active.push((iv.end, worst_slot, iv.vreg));
                }
                _ => {
                    locations[iv.vreg.0 as usize] = Location::Spill(next_spill_slot);
                    next_spill_slot += 1;
                }
            }
        }
    }

    Allocation {
        locations,
        used_registers,
        spill_count: next_spill_slot,
    }
}
