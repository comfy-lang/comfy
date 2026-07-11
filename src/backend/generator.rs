use crate::{
    backend::arm32::{
        asm::{
            ldr_label, mov_imm, store_syscall_return_value, syscall_1arg, syscall_2args,
            syscall_3args,
        },
        section,
        syscall_mapper::{Architecture, get_syscall_num_or_panic},
    },
    extra::utils::{generate_str_varname, get_bytes_from_type},
    frontend::{parser::AstNode, tokenizer::Token},
};

pub struct Generator {
    pub section_writer: section::SectionWriter,
    last_fun_name: String,
    arch: Architecture,
}

pub fn generate(ast_nodes: &AstNode, arch: Architecture) -> Generator {
    let mut generator = Generator::new(arch);
    generator.generate(ast_nodes);
    generator
}

impl Generator {
    pub fn new(arch: Architecture) -> Self {
        let self_data = Self {
            section_writer: section::SectionWriter::new(),
            last_fun_name: String::new(),
            arch,
        };

        self_data
    }

    fn generate(&mut self, ast: &AstNode) {
        match ast {
            AstNode::Program(statements) => {
                for stmt in statements {
                    self.generate(stmt);
                }
            }

            AstNode::FunctionDefinition(name, params, body) => {
                let fun_name = if name == "main" {
                    "_start".to_string()
                } else {
                    name.clone()
                };

                self.last_fun_name = fun_name.clone();
                self.section_writer.push_text(format!("{}:", fun_name));

                // Declare params in .bss
                for param in params {
                    self.generate(param);
                }

                for stmt in body {
                    self.generate(stmt);
                }
            }

            AstNode::VariableDeclaration(name, value, var_type) => {
                let label = format!("{}_{}", self.last_fun_name, name);
                let (value_type, is_mutable) = match &**var_type {
                    AstNode::Type(token, is_mutable) => (token, *is_mutable),
                    _ => panic!("Expected type node, found: {:?}", var_type),
                };

                match &**value {
                    AstNode::IBool(val) => self.store_num_variable(
                        &label,
                        if *val { 1 } else { 0 },
                        value_type,
                        is_mutable,
                    ),
                    AstNode::IChar(val) => {
                        self.store_num_variable(&label, *val as usize, value_type, is_mutable)
                    }
                    AstNode::IInt8(val) => {
                        self.store_num_variable(&label, *val as usize, value_type, is_mutable)
                    }
                    AstNode::IInt16(val) => {
                        self.store_num_variable(&label, *val as usize, value_type, is_mutable)
                    }
                    AstNode::IInt32(val) => {
                        self.store_num_variable(&label, *val as usize, value_type, is_mutable)
                    }
                    AstNode::IStr(val) => self.store_str_variable(&label, val, is_mutable),
                    _ => panic!(
                        "Unsupported value type in variable declaration: {:?}",
                        value
                    ),
                }
            }

            AstNode::VariableBufferDeclaration(name, var_type) => {
                let name = format!("{}_{}", self.last_fun_name, name);

                let (value_type, is_mutable) = match &**var_type {
                    AstNode::Type(token, is_mutable) => (token, *is_mutable),
                    _ => panic!("Expected type node, found: {:?}", var_type),
                };

                if !is_mutable {
                    panic!("Immutable buffers are not supported yet");
                }

                self.store_buffer(&name, value_type);
            }

            AstNode::VariableAssignment(name, value) => {
                let var_name = format!("{}_{}", self.last_fun_name, name);

                match &**value {
                    AstNode::IBool(val) => {
                        self.assign_num_variable(&name, if *val { 1 } else { 0 })
                    }
                    AstNode::IChar(val) => self.assign_num_variable(&var_name, *val as usize),
                    AstNode::IInt8(val) => self.assign_num_variable(&var_name, *val as usize),
                    AstNode::IInt16(val) => self.assign_num_variable(&var_name, *val as usize),
                    AstNode::IInt32(val) => self.assign_num_variable(&var_name, *val as usize),
                    _ => panic!("Unsupported value type in variable assignment: {:?}", value),
                };
            }

            AstNode::InlineAsm(asm_lines) => {
                self.section_writer.push_text("\t@ ===Inline Assembly===");
                for line in asm_lines {
                    self.section_writer.push_text(format!("\t{}", line));
                }
                self.section_writer.push_text("\t@ =====================\n");
            }

            AstNode::Syscall(name, inner) => match name.as_str() {
                "write" => self.generate_write(inner),
                "read" => self.generate_read(inner),
                "open" => self.generate_open(inner),
                "exit" => self.generate_exit(inner),
                "sysinfo" => self.generate_sysinfo(inner),
                _ => panic!("Unknown syscall: {}", name),
            },

            _ => panic!("Unsupported AST node in code generation: {:?}", ast),
        }
    }

    fn generate_write(&mut self, inner: &AstNode) {
        let (fd, data) = match inner {
            AstNode::Write(fd, token) => (fd, token),
            _ => panic!("Invalid write syscall inner node"),
        };

        let syscall_number: u32 = get_syscall_num_or_panic(self.arch, "write");
        let fd_str = match fd {
            Token::Int32Container(n) => n.to_string(),
            Token::Int16Container(n) => (i32::from(*n)).to_string(),
            Token::Int8Container(n) => (i32::from(*n)).to_string(),
            Token::Identifier(id) => format!("{}_{}", self.last_fun_name, id),
            _ => panic!("Unsupported file descriptor type: {:?}", fd),
        };

        match data {
            Token::StrContainer(s) => {
                let var = generate_str_varname();
                self.section_writer.push_rodata_str_with_len(&var, s);

                match fd {
                    Token::Int32Container(n) => {
                        let fd_val = *n as i32;
                        let instr = syscall_3args(
                            syscall_number,
                            &fd_val.to_string(),
                            &var,
                            &format!("{}_len", var),
                        );
                        self.section_writer.push_text(&instr);
                    }
                    Token::Int16Container(n) => {
                        let fd_val = i32::from(*n);
                        let instr = syscall_3args(
                            syscall_number,
                            &fd_val.to_string(),
                            &var,
                            &format!("{}_len", var),
                        );
                        self.section_writer.push_text(&instr);
                    }
                    Token::Int8Container(n) => {
                        let fd_val = i32::from(*n);
                        let instr = syscall_3args(
                            syscall_number,
                            &fd_val.to_string(),
                            &var,
                            &format!("{}_len", var),
                        );
                        self.section_writer.push_text(&instr);
                    }
                    Token::Identifier(id) => {
                        let label = format!("{}_{}", self.last_fun_name, id);

                        // !! Exception: Inline asm for simplicity and since we need
                        // to load the value into a register from a label
                        // TODO: Refactor this to use a more generic approach
                        self.section_writer.push_text(format!(
                            "\tmov r7, #{}\n\tldr r0, ={}\n\tldr r1, ={}\n\tldr r2, ={}_len\n\tsvc #0\n",
                            syscall_number, label, var, var
                        ));
                    }
                    _ => panic!("Unsupported file descriptor type: {:?}", fd),
                }
            }

            Token::Identifier(id) => {
                let label = format!("{}_{}", self.last_fun_name, id);
                let instr =
                    syscall_3args(syscall_number, &fd_str, &label, &format!("{}_len", label));
                self.section_writer.push_text(&instr);
            }

            _ => panic!("Unsupported write token: {:?}", data),
        }
        store_syscall_return_value(&mut self.section_writer.text);
    }

    fn generate_read(&mut self, inner: &AstNode) {
        let (fd, buffer) = match inner {
            AstNode::Read(fd, buffer) => (fd, buffer),
            _ => panic!("Invalid read syscall inner node"),
        };

        let syscall_number = get_syscall_num_or_panic(self.arch, "read");

        let fd_str = match fd {
            Token::Int32Container(n) => n.to_string(),
            Token::Int16Container(n) => (i32::from(*n)).to_string(),
            Token::Int8Container(n) => (i32::from(*n)).to_string(),
            Token::Identifier(id) => format!("{}_{}", self.last_fun_name, id),
            _ => panic!("Unsupported file descriptor type: {:?}", fd),
        };

        let label = format!("{}_{}", self.last_fun_name, buffer);

        let instr = syscall_2args(syscall_number, &fd_str, &label);
        self.section_writer.push_text(&instr);

        store_syscall_return_value(&mut self.section_writer.text);
    }

    fn generate_exit(&mut self, inner: &AstNode) {
        let code = match inner {
            AstNode::Exit(token) => token,
            _ => panic!("Invalid exit syscall inner node"),
        };

        let syscall_number = get_syscall_num_or_panic(self.arch, "exit");

        let asm = match code {
            Token::Int32Container(n) => syscall_1arg(syscall_number, &n.to_string()),
            Token::Int16Container(n) => syscall_1arg(syscall_number, &(i32::from(*n)).to_string()),
            Token::Int8Container(n) => syscall_1arg(syscall_number, &(i32::from(*n)).to_string()),
            Token::Identifier(id) => syscall_1arg(
                syscall_number,
                format!("{}_{}", self.last_fun_name, id).as_str(),
            ),
            _ => panic!("Unsupported exit code: {:?}", code),
        };

        self.section_writer.push_text(&asm);
    }

    fn store_num_variable(
        &mut self,
        label: &str,
        val: usize,
        value_type: &Token,
        is_mutable: bool,
    ) {
        if is_mutable {
            self.section_writer
                .declare_bss(&label, get_bytes_from_type(value_type));
            self.section_writer.push_text(ldr_label("r1", &label));
            self.section_writer.push_text(mov_imm("r0", val));
            self.section_writer.push_text("\tstr r0, [r1]");
            return;
        }

        self.section_writer.push_rodata_word(&label, val as i32);
    }

    fn assign_num_variable(&mut self, label: &str, val: usize) {
        self.section_writer.push_text(ldr_label("r0", &label));
        self.section_writer.push_text(mov_imm("r1", val));
        self.section_writer.push_text("str r1, [r0]");
    }

    fn store_buffer(&mut self, label: &str, value_type: &Token) {
        self.section_writer
            .declare_bss(&label, get_bytes_from_type(value_type));
    }

    fn store_str_variable(&mut self, label: &str, val: &str, is_mutable: bool) {
        if is_mutable {
            panic!("Mutable string variables are not supported yet");
        }

        self.section_writer.push_rodata_str_with_len(&label, val);
    }

    fn generate_open(&mut self, inner: &AstNode) {
        let (path, flags, mode) = match inner {
            AstNode::Open(path, flags, mode) => (path, flags, mode),
            _ => panic!("Invalid open syscall inner node"),
        };

        let syscall_number = get_syscall_num_or_panic(self.arch, "open");

        let var = generate_str_varname();
        self.section_writer.push_rodata_str_with_len(&var, path);

        let flags_str = flags.to_string();
        let mode_str = mode.to_string();

        let instr = syscall_3args(syscall_number, &var, &flags_str, &mode_str);
        self.section_writer.push_text(&instr);

        store_syscall_return_value(&mut self.section_writer.text);
    }

    fn generate_sysinfo(&mut self, inner: &AstNode) {
        let buffer = match inner {
            AstNode::Sysinfo(buffer) => buffer,
            _ => panic!("Invalid sysinfo syscall inner node"),
        };

        let syscall_number = get_syscall_num_or_panic(self.arch, "sysinfo");
        let label = format!("{}_{}", self.last_fun_name, buffer);

        let instr = syscall_1arg(syscall_number, &label);
        self.section_writer.push_text(&instr);

        store_syscall_return_value(&mut self.section_writer.text);
    }
}
