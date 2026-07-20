use crate::ir::Program;

pub mod arm32;

pub trait Backend {
    fn name(&self) -> &'static str;
    fn emit(&self, program: &Program) -> String;
}

/// A text-level peephole pass: deletes a `mov B, A` line that immediately
/// follows a `mov A, B` line, since the first already makes `A` and `B`
/// equal, making the second a no-op. Backend-agnostic (any register-based
/// backend built around a load/store abstraction like ours tends to
/// produce exactly this redundancy at the boundary between instructions).
pub fn strip_redundant_movs(asm: &str) -> String {
    let lines: Vec<&str> = asm.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;

    while i < lines.len() {
        if let Some((a, b)) = parse_mov(lines[i])
            && i + 1 < lines.len()
            && parse_mov(lines[i + 1]) == Some((b, a))
        {
            out.push(lines[i]);
            i += 2; // drop the redundant second mov
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }

    let mut result = out.join("\n");
    result.push('\n');
    result
}

/// Parses a `\tmov DST, SRC` line into `(dst, src)`. Returns `None` for
/// anything else, including a `mov` with an immediate operand (`mov r0,
/// #0`), which isn't a register-to-register move at all.
fn parse_mov(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix("mov ")?;
    let (dst, src) = rest.split_once(',')?;
    let src = src.trim();
    if src.starts_with('#') {
        return None;
    }
    Some((dst.trim(), src))
}
