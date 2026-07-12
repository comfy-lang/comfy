//! Code generation. A `Backend` turns a checked `Program` into target
//! assembly text. Only ARM32 exists today; the trait exists so x86_64 and
//! others can be added later without touching the frontend.

use crate::ast::Program;

pub mod arm32;

pub trait Backend {
    /// Human-readable name, used in CLI messages.
    fn name(&self) -> &'static str;

    /// Emit a complete assembly file for `program`.
    fn emit(&self, program: &Program) -> String;
}
