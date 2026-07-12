use crate::sema::CheckedProgram;

pub mod arm32;

pub trait Backend {
    fn name(&self) -> &'static str;
    fn emit(&self, program: &CheckedProgram) -> String;
}
