use crate::{
    backend::syscalls::{
        parse_sys_exit, parse_sys_open, parse_sys_read, parse_sys_sysinfo, parse_sys_write,
    },
    frontend::tokenizer::Token,
};

#[derive(Debug)]
pub enum AstNode {
    Program(Vec<AstNode>),
    _Identifier(String, Box<AstNode>),
    FunctionDefinition(String, Vec<AstNode>, Vec<AstNode>),
    VariableDeclaration(String, Box<AstNode>, Box<AstNode>),
    VariableBufferDeclaration(String, Box<AstNode>),
    VariableAssignment(String, Box<AstNode>),
    InlineAsm(Vec<String>),

    // Token::Bool, isMutable?
    Type(Token, bool),

    // syscall wrappers
    Syscall(String, Box<AstNode>),
    Write(Token, Token),
    Read(Token, String),
    Open(String, usize, usize),
    Exit(Token),
    Sysinfo(String),

    // internal value containers
    IBool(bool),
    IChar(char),
    IStr(String),
    IInt8(i8),
    IInt16(i16),
    IInt32(i32),
}

pub fn parse(tokens: Vec<Token>) -> AstNode {
    let mut parser = Parser::new(tokens);
    parser.parse()
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    fn parse(&mut self) -> AstNode {
        let mut statements = Vec::new();

        while self.current_token() != Token::EOF {
            let statement = self.parse_statement();
            statements.push(statement);
        }

        AstNode::Program(statements)
    }

    pub fn current_token(&self) -> Token {
        self.tokens[self.current].clone()
    }

    fn parse_function_definition(&mut self) -> AstNode {
        self.consume(Token::Function);

        let identifier = self.consume_identifier();

        self.consume(Token::ParentOpen);

        let mut parameters = Vec::new();
        while self.current_token() != Token::ParentClose {
            let parameter = self.parse_parameter();
            parameters.push(parameter);

            if self.current_token() == Token::Comma {
                self.consume(Token::Comma);
            }
        }
        self.consume(Token::ParentClose);

        self.consume(Token::CurlyOpen);

        let mut body = Vec::new();
        while self.current_token() != Token::CurlyClose {
            let statement = self.parse_statement();
            body.push(statement);
        }
        self.consume(Token::CurlyClose);

        AstNode::FunctionDefinition(identifier, parameters, body)
    }

    fn parse_syscall(&mut self, syscall: String) -> AstNode {
        let matched_syscall = match syscall.as_str() {
            "write" => parse_sys_write(self),
            "read" => parse_sys_read(self),
            "exit" => parse_sys_exit(self),
            "open" => parse_sys_open(self),
            "sysinfo" => parse_sys_sysinfo(self),
            _ => {
                panic!("Unknown syscall: {}", syscall);
            }
        };

        AstNode::Syscall(syscall, Box::new(matched_syscall))
    }

    fn parse_variable_declaration(&mut self) -> AstNode {
        let mut mutable = false;
        if self.current_token() == Token::Mut {
            self.consume(Token::Mut);
            mutable = true;
        }

        let var_type = self.current_token();
        self.consume(var_type.clone());

        let identifier = self.consume_identifier();

        if self.current_token() == Token::Semicolon {
            self.consume(Token::Semicolon);
            return AstNode::VariableBufferDeclaration(
                identifier,
                Box::new(AstNode::Type(var_type, mutable)),
            );
        }

        self.consume(Token::Equals);

        let value: AstNode = match self.current_token() {
            Token::BoolContainer(_)
            | Token::CharContainer(_)
            | Token::Int8Container(_)
            | Token::Int16Container(_)
            | Token::Int32Container(_)
            | Token::StrContainer(_) => self.parse_datatype(&var_type),
            Token::Syscall(sys) => self.parse_syscall(sys),
            _ => panic!(
                "Unsupported value in variable declaration: {:?}",
                self.current_token()
            ),
        };

        self.consume(Token::Semicolon);

        AstNode::VariableDeclaration(
            identifier,
            Box::new(value),
            Box::new(AstNode::Type(var_type, mutable)),
        )
    }

    fn parse_statement(&mut self) -> AstNode {
        let ast_node = match self.current_token() {
            Token::Function => self.parse_function_definition(),
            Token::Syscall(syscall) => {
                let node = self.parse_syscall(syscall);

                self.consume(Token::Semicolon);
                node
            }
            Token::Bool
            | Token::Char
            | Token::Str
            | Token::Int8
            | Token::Int16
            | Token::Int32
            | Token::Mut => self.parse_variable_declaration(),
            Token::Identifier(_) => self.parse_identifier_statement(),
            Token::InlineAsm => self.parse_inline_asm(),
            _ => {
                panic!("Expected a statement, found: {:?}", self.current_token())
            }
        };

        ast_node
    }

    fn parse_identifier_statement(&mut self) -> AstNode {
        let identifier = self.consume_identifier();

        match self.current_token() {
            // variable assignment
            Token::Equals => self.parse_variable_assignment(identifier),
            // function call
            Token::ParentOpen => panic!("Function call parsing not implemented yet"),
            _ => panic!("Expected '=' or '(', found: {:?}", self.current_token()),
        }
    }

    fn parse_variable_assignment(&mut self, identifier: String) -> AstNode {
        self.consume(Token::Equals);

        let value: AstNode = match self.current_token() {
            Token::BoolContainer(_) => self.parse_datatype(&Token::Bool),
            Token::CharContainer(_) => self.parse_datatype(&Token::Char),
            Token::Int8Container(_) => self.parse_datatype(&Token::Int8),
            Token::Int16Container(_) => self.parse_datatype(&Token::Int16),
            Token::Int32Container(_) => self.parse_datatype(&Token::Int32),
            Token::StrContainer(_) => self.parse_datatype(&Token::Str),
            _ => panic!(
                "Unsupported value in variable assignment: {:?}",
                self.current_token()
            ),
        };

        self.consume(Token::Semicolon);

        AstNode::VariableAssignment(identifier, Box::new(value))
    }

    fn parse_datatype(&mut self, var_type: &Token) -> AstNode {
        match var_type {
            Token::Bool => self.parse_bool(),
            Token::Char => self.parse_char(),
            Token::Int8 => self.parse_int8(),
            Token::Int16 => self.parse_int16(),
            Token::Int32 => self.parse_int32(),
            Token::Str => self.parse_str(),
            _ => panic!("Unsupported variable type: {:?}", var_type),
        }
    }

    fn parse_bool(&mut self) -> AstNode {
        let value = match self.current_token() {
            Token::BoolContainer(v) => v,
            _ => panic!(
                "Expected a boolean value, found: {:?}",
                self.current_token()
            ),
        };

        self.consume(Token::BoolContainer(value));
        AstNode::IBool(value)
    }

    fn parse_char(&mut self) -> AstNode {
        let value = match self.current_token() {
            Token::CharContainer(c) => c,
            _ => panic!(
                "Expected a character value, found: {:?}",
                self.current_token()
            ),
        };

        self.consume(Token::CharContainer(value));
        AstNode::IChar(value)
    }

    fn parse_int8(&mut self) -> AstNode {
        let value = match self.current_token() {
            Token::Int8Container(v) => v,
            _ => panic!(
                "Expected an 8-bit integer value, found: {:?}",
                self.current_token()
            ),
        };

        self.consume(Token::Int8Container(value));
        AstNode::IInt8(value)
    }

    fn parse_int16(&mut self) -> AstNode {
        let value = match self.current_token() {
            Token::Int8Container(v) => {
                self.consume(Token::Int8Container(v));
                v as i16
            }
            Token::Int16Container(v) => {
                self.consume(Token::Int16Container(v));
                v
            }
            _ => panic!(
                "Expected a 16-bit integer value, found: {:?}",
                self.current_token()
            ),
        };

        AstNode::IInt16(value)
    }

    fn parse_int32(&mut self) -> AstNode {
        let value = match self.current_token() {
            Token::Int8Container(v) => {
                self.consume(Token::Int8Container(v));
                v as i32
            }
            Token::Int16Container(v) => {
                self.consume(Token::Int16Container(v));
                v as i32
            }
            Token::Int32Container(v) => {
                self.consume(Token::Int32Container(v));
                v
            }
            _ => panic!(
                "Expected a 32-bit integer value, found: {:?}",
                self.current_token()
            ),
        };

        AstNode::IInt32(value)
    }

    fn parse_str(&mut self) -> AstNode {
        let value = match self.current_token() {
            Token::StrContainer(s) => s,
            _ => panic!("Expected a string value, found: {:?}", self.current_token()),
        };

        self.consume(Token::StrContainer(value.clone()));
        AstNode::IStr(value)
    }

    fn parse_parameter(&mut self) -> AstNode {
        let mut mutable = false;
        if self.current_token() == Token::Mut {
            self.consume(Token::Mut);
            mutable = true;
        }

        let param_type = self.current_token();

        if param_type == Token::Bool
            || param_type == Token::Char
            || param_type == Token::Str
            || param_type == Token::Int8
            || param_type == Token::Int16
            || param_type == Token::Int32
        {
            self.consume(param_type.clone());
        } else {
            panic!(
                "Expected a type definition for parameter, found: {:?}",
                param_type
            );
        }

        let identifier = self.consume_identifier();

        AstNode::VariableBufferDeclaration(identifier, Box::new(AstNode::Type(param_type, mutable)))
    }

    fn parse_inline_asm(&mut self) -> AstNode {
        self.consume(Token::InlineAsm);
        self.consume(Token::CurlyOpen);

        let mut asm_lines = Vec::new();

        while self.current_token() != Token::CurlyClose && self.current_token() != Token::EOF {
            if let Token::StrContainer(ref line) = self.current_token() {
                asm_lines.push(line.clone());
                self.consume(Token::StrContainer(line.clone()));
                if self.current_token() == Token::Comma {
                    self.consume(Token::Comma);
                }
            } else {
                panic!(
                    "Expected string in inline asm block, found: {:?}",
                    self.current_token()
                );
            }
        }

        self.consume(Token::CurlyClose);
        self.consume(Token::Semicolon);

        AstNode::InlineAsm(asm_lines)
    }

    fn consume_identifier(&mut self) -> String {
        if let Token::Identifier(id) = self.current_token() {
            self.consume(Token::Identifier(id.clone()));
            id
        } else {
            panic!("Expected identifier, found: {:?}", self.current_token());
        }
    }

    pub fn consume(&mut self, token: Token) -> Token {
        if self.current_token() == token {
            let consumed = self.tokens[self.current].clone();
            self.current += 1;
            consumed
        } else {
            panic!(
                "Expected token {:?}, found {:?}",
                token,
                self.current_token()
            );
        }
    }
}
