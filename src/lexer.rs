//! Lexer: converts source text into a stream of spanned tokens.

use crate::diag::{Diagnostic, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Fn,
    Ident(String),
    Int(i64),
    /// A `$name` intrinsic, e.g. `$syscall`.
    Intrinsic(String),

    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semicolon,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i] as char;

        match ch {
            ' ' | '\t' | '\r' | '\n' => {
                i += 1;
            }

            '/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }

            '(' => push(&mut tokens, TokenKind::LParen, &mut i, 1),
            ')' => push(&mut tokens, TokenKind::RParen, &mut i, 1),
            '{' => push(&mut tokens, TokenKind::LBrace, &mut i, 1),
            '}' => push(&mut tokens, TokenKind::RBrace, &mut i, 1),
            ',' => push(&mut tokens, TokenKind::Comma, &mut i, 1),
            ';' => push(&mut tokens, TokenKind::Semicolon, &mut i, 1),

            '$' => {
                let start = i;
                i += 1;
                let name_start = i;
                while i < bytes.len() && is_ident_continue(bytes[i] as char) {
                    i += 1;
                }
                if i == name_start {
                    return Err(Diagnostic::error(
                        "expected an identifier after '$'",
                        Span::new(start, i),
                    ));
                }
                tokens.push(Token {
                    kind: TokenKind::Intrinsic(source[name_start..i].to_string()),
                    span: Span::new(start, i),
                });
            }

            '-' if bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit()) => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                let text = &source[start..i];
                let value = text.parse::<i64>().map_err(|_| {
                    Diagnostic::error(
                        format!("invalid integer literal '{}'", text),
                        Span::new(start, i),
                    )
                })?;
                tokens.push(Token { kind: TokenKind::Int(value), span: Span::new(start, i) });
            }

            c if c.is_ascii_digit() => {
                let start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                let text = &source[start..i];
                let value = text.parse::<i64>().map_err(|_| {
                    Diagnostic::error(
                        format!("invalid integer literal '{}'", text),
                        Span::new(start, i),
                    )
                })?;
                tokens.push(Token { kind: TokenKind::Int(value), span: Span::new(start, i) });
            }

            c if is_ident_start(c) => {
                let start = i;
                while i < bytes.len() && is_ident_continue(bytes[i] as char) {
                    i += 1;
                }
                let text = &source[start..i];
                let kind = match text {
                    "fn" => TokenKind::Fn,
                    _ => TokenKind::Ident(text.to_string()),
                };
                tokens.push(Token { kind, span: Span::new(start, i) });
            }

            other => {
                return Err(Diagnostic::error(
                    format!("unexpected character '{}'", other),
                    Span::new(i, i + 1),
                ));
            }
        }
    }

    tokens.push(Token { kind: TokenKind::Eof, span: Span::new(source.len(), source.len()) });
    Ok(tokens)
}

fn push(tokens: &mut Vec<Token>, kind: TokenKind, i: &mut usize, len: usize) {
    let start = *i;
    *i += len;
    tokens.push(Token { kind, span: Span::new(start, *i) });
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
