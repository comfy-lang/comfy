use crate::{
    frontend::parser::{AstNode, Parser},
    frontend::tokenizer::Token,
};

pub fn parse_sys_write(parser: &mut Parser) -> AstNode {
    parser.consume(Token::Syscall("write".to_string()));

    parser.consume(Token::ParentOpen);

    let fd = parse_int_or_identifier(parser, "file descriptor");

    parser.consume(Token::Comma);

    let write_data = match parser.current_token() {
        Token::StrContainer(s) => {
            parser.consume(Token::StrContainer(s.clone()));
            Token::StrContainer(s)
        }
        Token::Identifier(id) => {
            parser.consume(Token::Identifier(id.clone()));
            Token::Identifier(id)
        }
        _ => panic!("Expected write data (string or identifier)"),
    };

    parser.consume(Token::ParentClose);

    AstNode::Write(fd, write_data)
}

pub fn parse_sys_read(parser: &mut Parser) -> AstNode {
    parser.consume(Token::Syscall("read".to_string()));

    parser.consume(Token::ParentOpen);

    let fd = parse_int_or_identifier(parser, "file descriptor");

    parser.consume(Token::Comma);

    let buffer = match parser.current_token() {
        Token::Identifier(id) => id,
        _ => panic!("Expected buffer identifier"),
    };

    parser.consume(Token::Identifier(buffer.clone()));
    parser.consume(Token::ParentClose);

    AstNode::Read(fd, buffer)
}

pub fn parse_sys_exit(parser: &mut Parser) -> AstNode {
    parser.consume(Token::Syscall("exit".to_string()));

    parser.consume(Token::ParentOpen);

    let code = parse_int_or_identifier(parser, "exit code");

    parser.consume(Token::ParentClose);
    AstNode::Exit(code)
}

pub fn parse_sys_open(parser: &mut Parser) -> AstNode {
    parser.consume(Token::Syscall("open".to_string()));

    parser.consume(Token::ParentOpen);

    let filename = match parser.current_token() {
        Token::StrContainer(s) => s,
        Token::Identifier(id) => id,
        _ => panic!("Expected filename (string)"),
    };
    parser.consume(Token::StrContainer(filename.clone()));

    parser.consume(Token::Comma);

    let flags = match parser.current_token() {
        Token::Int32Container(n) => {
            parser.consume(Token::Int32Container(n));
            n
        }
        Token::Int16Container(n) => {
            parser.consume(Token::Int16Container(n));
            i32::from(n)
        }
        Token::Int8Container(n) => {
            parser.consume(Token::Int8Container(n));
            i32::from(n)
        }
        _ => panic!("Expected flags (number)"),
    };

    parser.consume(Token::Comma);

    let mode = match parser.current_token() {
        Token::Int32Container(n) => {
            parser.consume(Token::Int32Container(n));
            n
        }
        Token::Int16Container(n) => {
            parser.consume(Token::Int16Container(n));
            i32::from(n)
        }
        Token::Int8Container(n) => {
            parser.consume(Token::Int8Container(n));
            i32::from(n)
        }
        _ => panic!("Expected mode (number)"),
    };

    parser.consume(Token::ParentClose);

    AstNode::Open(filename, flags as usize, mode as usize)
}

pub fn parse_sys_sysinfo(parser: &mut Parser) -> AstNode {
    parser.consume(Token::Syscall("sysinfo".to_string()));

    parser.consume(Token::ParentOpen);

    let buffer = match parser.current_token() {
        Token::Identifier(id) => id,
        _ => panic!("Expected buffer identifier"),
    };

    parser.consume(Token::Identifier(buffer.clone()));
    parser.consume(Token::ParentClose);

    AstNode::Sysinfo(buffer)
}

fn parse_int_or_identifier(parser: &mut Parser, context: &str) -> Token {
    match parser.current_token() {
        token @ Token::Int32Container(_)
        | token @ Token::Int16Container(_)
        | token @ Token::Int8Container(_)
        | token @ Token::Identifier(_) => {
            parser.consume(token.clone());
            token.clone()
        }
        _ => panic!("Expected {} (number or identifier)", context),
    }
}
