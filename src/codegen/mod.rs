use crate::ir::Program;

pub mod arm32;

pub trait Backend {
    fn name(&self) -> &'static str;
    fn emit(&self, program: &Program) -> String;
}
