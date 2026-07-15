use crate::{
    ast,
    sema::{CheckedExpr, CheckedFunction, CheckedProgram, CheckedStmt},
};

/// A virtual register: one SSA-like "slot" holding a single value. There's
/// an unlimited supply of these during lowering; a later register
/// allocator maps them down to the handful of real ARM registers.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct VReg(pub u32);

pub enum Instr {
    Const {
        dst: VReg,
        value: i64,
    },
    Copy {
        dst: VReg,
        src: VReg,
    },
    Unary {
        dst: VReg,
        op: ast::UnaryOp,
        src: VReg,
    },
    Binary {
        dst: VReg,
        op: ast::BinOp,
        lhs: VReg,
        rhs: VReg,
    },
    Compare {
        dst: VReg,
        op: ast::CompareOp,
        lhs: VReg,
        rhs: VReg,
    },

    LoadLocal {
        dst: VReg,
        offset: usize,
    },
    StoreLocal {
        offset: usize,
        src: VReg,
    },
    LoadIndexed {
        dst: VReg,
        base_offset: usize,
        index: VReg,
        elem_size: usize,
    },
    StoreIndexed {
        base_offset: usize,
        index: VReg,
        elem_size: usize,
        src: VReg,
    },
    AddressOf {
        dst: VReg,
        offset: usize,
    },
    Load {
        dst: VReg,
        addr: VReg,
    }, // *p (read)
    Store {
        addr: VReg,
        src: VReg,
    }, // *p = v (write)

    Syscall {
        dst: VReg,
        args: [VReg; 7],
    },
    Call {
        dst: Option<VReg>,
        name: String,
        args: Vec<VReg>,
    },

    Label(String),
    Jump(String),
    JumpIfZero {
        cond: VReg,
        label: String,
    },
    JumpIfNotZero {
        cond: VReg,
        label: String,
    },
    Return(Option<VReg>),
}

pub struct Function {
    pub name: String,
    pub is_entry: bool,
    pub param_offsets: Vec<usize>,
    pub frame_size: usize,
    pub vreg_count: u32,
    pub body: Vec<Instr>,
}

pub struct Program {
    pub functions: Vec<Function>,
}

struct Lowering {
    next_vreg: u32,
    label_counter: usize,
    body: Vec<Instr>,
}

impl Lowering {
    fn new() -> Self {
        Lowering {
            next_vreg: 0,
            label_counter: 0,
            body: Vec::new(),
        }
    }

    fn new_vreg(&mut self) -> VReg {
        let v = VReg(self.next_vreg);
        self.next_vreg += 1;
        v
    }

    fn new_label(&mut self, prefix: &str) -> String {
        let label = format!(".L{}{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    fn emit(&mut self, instr: Instr) {
        self.body.push(instr);
    }

    fn lower_stmt(&mut self, stmt: &CheckedStmt) {
        match stmt {
            CheckedStmt::Store { offset, value } => {
                let src = self.lower_expr(value);
                self.emit(Instr::StoreLocal {
                    offset: *offset,
                    src,
                });
            }

            CheckedStmt::Expr(value) => {
                self.lower_expr(value); // side effects only, result discarded
            }

            CheckedStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond_v = self.lower_expr(cond);
                let end_label = self.new_label("if_end");

                if let Some(else_body) = else_body {
                    let else_label = self.new_label("if_else");
                    self.emit(Instr::JumpIfZero {
                        cond: cond_v,
                        label: else_label.clone(),
                    });
                    for s in then_body {
                        self.lower_stmt(s);
                    }
                    self.emit(Instr::Jump(end_label.clone()));
                    self.emit(Instr::Label(else_label));
                    for s in else_body {
                        self.lower_stmt(s);
                    }
                } else {
                    self.emit(Instr::JumpIfZero {
                        cond: cond_v,
                        label: end_label.clone(),
                    });
                    for s in then_body {
                        self.lower_stmt(s);
                    }
                }

                self.emit(Instr::Label(end_label));
            }

            CheckedStmt::While { cond, body } => {
                let start_label = self.new_label("while_start");
                let end_label = self.new_label("while_end");

                self.emit(Instr::Label(start_label.clone()));
                let cond_v = self.lower_expr(cond);
                self.emit(Instr::JumpIfZero {
                    cond: cond_v,
                    label: end_label.clone(),
                });

                for s in body {
                    self.lower_stmt(s);
                }

                self.emit(Instr::Jump(start_label));
                self.emit(Instr::Label(end_label));
            }

            CheckedStmt::Return(value) => {
                let v = value.as_ref().map(|e| self.lower_expr(e));
                self.emit(Instr::Return(v));
            }

            CheckedStmt::StoreThroughPointer { address, value } => {
                let addr = self.lower_expr(address);
                let src = self.lower_expr(value);
                self.emit(Instr::Store { addr, src });
            }

            CheckedStmt::StoreIndexed {
                base_offset,
                index,
                elem_size,
                value,
            } => {
                let index_v = self.lower_expr(index);
                let src = self.lower_expr(value);
                self.emit(Instr::StoreIndexed {
                    base_offset: *base_offset,
                    index: index_v,
                    elem_size: *elem_size,
                    src,
                });
            }
        }
    }

    fn lower_expr(&mut self, expr: &CheckedExpr) -> VReg {
        match expr {
            CheckedExpr::Const(value) => {
                let dst = self.new_vreg();
                self.emit(Instr::Const { dst, value: *value });
                dst
            }

            CheckedExpr::Local(offset) => {
                let dst = self.new_vreg();
                self.emit(Instr::LoadLocal {
                    dst,
                    offset: *offset,
                });
                dst
            }

            CheckedExpr::Unary { op, operand } => {
                let src = self.lower_expr(operand);
                let dst = self.new_vreg();
                self.emit(Instr::Unary { dst, op: *op, src });
                dst
            }

            CheckedExpr::Binary { op, lhs, rhs } => {
                let lhs_v = self.lower_expr(lhs);
                let rhs_v = self.lower_expr(rhs);
                let dst = self.new_vreg();
                self.emit(Instr::Binary {
                    dst,
                    op: *op,
                    lhs: lhs_v,
                    rhs: rhs_v,
                });
                dst
            }

            CheckedExpr::Compare { op, lhs, rhs } => {
                let lhs_v = self.lower_expr(lhs);
                let rhs_v = self.lower_expr(rhs);
                let dst = self.new_vreg();
                self.emit(Instr::Compare {
                    dst,
                    op: *op,
                    lhs: lhs_v,
                    rhs: rhs_v,
                });
                dst
            }

            CheckedExpr::Logical { op, lhs, rhs } => match op {
                ast::LogicalOp::And => {
                    let dst = self.new_vreg();
                    let false_label = self.new_label("and_false");
                    let end_label = self.new_label("and_end");

                    let lhs_v = self.lower_expr(lhs);
                    self.emit(Instr::JumpIfZero {
                        cond: lhs_v,
                        label: false_label.clone(),
                    });

                    let rhs_v = self.lower_expr(rhs);
                    self.emit(Instr::Copy { dst, src: rhs_v });
                    self.emit(Instr::Jump(end_label.clone()));

                    self.emit(Instr::Label(false_label));
                    self.emit(Instr::Const { dst, value: 0 });

                    self.emit(Instr::Label(end_label));
                    dst
                }
                ast::LogicalOp::Or => {
                    let dst = self.new_vreg();
                    let true_label = self.new_label("or_true");
                    let end_label = self.new_label("or_end");

                    let lhs_v = self.lower_expr(lhs);
                    self.emit(Instr::JumpIfNotZero {
                        cond: lhs_v,
                        label: true_label.clone(),
                    });

                    let rhs_v = self.lower_expr(rhs);
                    self.emit(Instr::Copy { dst, src: rhs_v });
                    self.emit(Instr::Jump(end_label.clone()));

                    self.emit(Instr::Label(true_label));
                    self.emit(Instr::Const { dst, value: 1 });

                    self.emit(Instr::Label(end_label));
                    dst
                }
            },

            CheckedExpr::Syscall { args } => {
                let arg_vregs: Vec<VReg> = args.iter().map(|a| self.lower_expr(a)).collect();
                let args: [VReg; 7] = arg_vregs
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("syscall always has exactly 7 args"));
                let dst = self.new_vreg();
                self.emit(Instr::Syscall { dst, args });
                dst
            }

            CheckedExpr::Call { name, args } => {
                let arg_vregs: Vec<VReg> = args.iter().map(|a| self.lower_expr(a)).collect();
                let dst = self.new_vreg();
                self.emit(Instr::Call {
                    dst: Some(dst),
                    name: name.clone(),
                    args: arg_vregs,
                });
                dst
            }

            CheckedExpr::AddressOf(offset) => {
                let dst = self.new_vreg();
                self.emit(Instr::AddressOf {
                    dst,
                    offset: *offset,
                });
                dst
            }

            CheckedExpr::Deref(inner) => {
                let addr = self.lower_expr(inner);
                let dst = self.new_vreg();
                self.emit(Instr::Load { dst, addr });
                dst
            }

            CheckedExpr::Index {
                base_offset,
                index,
                elem_size,
            } => {
                let index_v = self.lower_expr(index);
                let dst = self.new_vreg();
                self.emit(Instr::LoadIndexed {
                    dst,
                    base_offset: *base_offset,
                    index: index_v,
                    elem_size: *elem_size,
                });
                dst
            }
        }
    }
}

pub fn lower(program: &CheckedProgram) -> Program {
    Program {
        functions: program.functions.iter().map(lower_function).collect(),
    }
}

fn lower_function(function: &CheckedFunction) -> Function {
    let mut lowering = Lowering::new();
    for stmt in &function.body {
        lowering.lower_stmt(stmt);
    }

    Function {
        name: function.name.clone(),
        is_entry: function.name == "main",
        param_offsets: function.param_offsets.clone(),
        frame_size: function.frame_size,
        vreg_count: lowering.next_vreg,
        body: lowering.body,
    }
}
