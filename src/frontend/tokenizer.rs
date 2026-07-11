#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Function,
    Identifier(String),
    Syscall(String),
    ParentOpen,
    ParentClose,
    CurlyOpen,
    CurlyClose,
    Comma,
    Semicolon,
    Colon,
    Equals,
    Mut,

    BracketOpen,
    BracketClose,
    InlineAsm,

    EOF,
    Unknown,

    // comfy base types
    Bool,
    Int8,
    Int16,
    Int32,
    Char,
    Str,

    // type containers
    BoolContainer(bool),
    CharContainer(char),
    StrContainer(String),
    Int8Container(i8),
    Int16Container(i16),
    Int32Container(i32),
}

pub fn tokenize(script: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut iter = script.chars().peekable();

    while let Some(ch) = iter.next() {
        match ch {
            ' ' | '\n' | '\t' => continue, // Skip whitespace

            '(' => tokens.push(Token::ParentOpen),
            ')' => tokens.push(Token::ParentClose),
            '{' => tokens.push(Token::CurlyOpen),
            '}' => tokens.push(Token::CurlyClose),
            ',' => tokens.push(Token::Comma),
            ';' => tokens.push(Token::Semicolon),
            ':' => tokens.push(Token::Colon),
            '=' => tokens.push(Token::Equals),
            '[' => tokens.push(Token::BracketOpen),
            ']' => tokens.push(Token::BracketClose),

            '$' => {
                let mut syscall = String::new();
                while let Some(&next_ch) = iter.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        syscall.push(iter.next().unwrap());
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Syscall(syscall));
            }

            '"' => {
                let mut string = String::new();
                while let Some(&next_ch) = iter.peek() {
                    if next_ch == '"' {
                        iter.next(); // Consume the closing quote
                        break;
                    } else {
                        string.push(iter.next().unwrap());
                    }
                }
                tokens.push(Token::StrContainer(string));
            }

            '/' => {
                if let Some(&next_ch) = iter.peek() {
                    if next_ch == '/' {
                        iter.next(); // Consume the second slash
                        while let Some(&next_ch) = iter.peek() {
                            if next_ch == '\n' {
                                break;
                            } else {
                                iter.next();
                            }
                        }
                    } else {
                        tokens.push(Token::Unknown);
                    }
                } else {
                    tokens.push(Token::Unknown);
                }
            }

            c if c.is_alphabetic() || c == '_' => {
                let mut identifier = c.to_string();
                while let Some(&next_ch) = iter.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        identifier.push(iter.next().unwrap());
                    } else {
                        break;
                    }
                }

                match identifier.as_str() {
                    "fn" => tokens.push(Token::Function),
                    "asm" => tokens.push(Token::InlineAsm),
                    "mut" => tokens.push(Token::Mut),
                    "true" => tokens.push(Token::BoolContainer(true)),
                    "false" => tokens.push(Token::BoolContainer(false)),
                    "bool" => tokens.push(Token::Bool),
                    "char" => tokens.push(Token::Char),
                    "str" => tokens.push(Token::Str),
                    "int8" => tokens.push(Token::Int8),
                    "int16" => tokens.push(Token::Int16),
                    "int32" => tokens.push(Token::Int32),
                    _ => tokens.push(Token::Identifier(identifier)),
                }
            }

            c if c.is_digit(10) => {
                let mut number = c.to_string();
                while let Some(&next_ch) = iter.peek() {
                    if next_ch.is_digit(10) {
                        number.push(iter.next().unwrap());
                    } else {
                        break;
                    }
                }

                let num_value = number.parse::<i32>().unwrap();
                match num_value {
                    n if n >= i8::MIN as i32 && n <= i8::MAX as i32 => {
                        tokens.push(Token::Int8Container(n as i8))
                    }
                    n if n >= i16::MIN as i32 && n <= i16::MAX as i32 => {
                        tokens.push(Token::Int16Container(n as i16))
                    }
                    _ => tokens.push(Token::Int32Container(num_value)),
                }
            }

            _ => tokens.push(Token::Unknown),
        }
    }

    tokens.push(Token::EOF);
    tokens
}
