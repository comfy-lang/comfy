//! Recursive-descent parser.
//!
//! Grammar so far: a single `fn main() { ... }` containing `let` constant
//! declarations and `$syscall(...)` statements. Expressions are still just
//! atoms (int literals or identifiers) - arithmetic comes in a later step.

use crate::ast::{Expr, FunctionDef, Program, Stmt};
use crate::diag::{Diagnostic, Span};
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: &[Token]) -> Result<Program, Diagnostic> {
    Parser { tokens, pos: 0 }.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let mut functions = Vec::new();

        while !self.at(&TokenKind::Eof) {
            functions.push(self.parse_function()?);
        }

        if functions.is_empty() {
            return Err(Diagnostic::error(
                "expected at least a `fn main()`",
                self.span(),
            ));
        }

        Ok(Program { functions })
    }

    fn parse_function(&mut self) -> Result<FunctionDef, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::Fn)?;

        let name = self.expect_ident()?;
        if name != "main" {
            return Err(Diagnostic::error(
                format!(
                    "function '{}' is not supported yet - only a single `fn main()` is allowed for now",
                    name
                ),
                start,
            ));
        }

        self.expect(&TokenKind::LParen)?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::LBrace)?;

        let mut body = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            body.push(self.parse_stmt()?);
        }
        let end = self.span();
        self.expect(&TokenKind::RBrace)?;

        Ok(FunctionDef {
            name,
            body,
            span: start.to(end),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.current().kind {
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::Intrinsic(_) => self.parse_syscall_stmt(),
            _ => Err(Diagnostic::error(
                format!("expected a statement, found {:?}", self.current().kind),
                self.span(),
            )),
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::Let)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Equals)?;
        let value = self.parse_expr()?;
        let end = self.span();
        self.expect(&TokenKind::Semicolon)?;

        Ok(Stmt::Let {
            name,
            value,
            span: start.to(end),
        })
    }

    fn parse_syscall_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();

        let name = match &self.current().kind {
            TokenKind::Intrinsic(name) => name.clone(),
            _ => unreachable!("caller already checked this is an intrinsic"),
        };
        self.advance();

        if name != "syscall" {
            return Err(Diagnostic::error(
                format!(
                    "unknown intrinsic '${}' - only '$syscall' exists so far",
                    name
                ),
                start,
            ));
        }

        self.expect(&TokenKind::LParen)?;

        let mut args = Vec::new();
        while !self.at(&TokenKind::RParen) {
            args.push(self.parse_expr()?);
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.span();
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Semicolon)?;

        if args.len() != 7 {
            return Err(Diagnostic::error(
                format!(
                    "'$syscall' expects exactly 7 arguments (nr, a0, a1, a2, a3, a4, a5), found {}",
                    args.len()
                ),
                start.to(end),
            ));
        }

        let args: [Expr; 7] = args
            .try_into()
            .unwrap_or_else(|_| unreachable!("length checked above"));
        Ok(Stmt::Syscall {
            args,
            span: start.to(end),
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.span();
        match &self.current().kind {
            TokenKind::Int(value) => {
                let value = *value;
                self.advance();
                Ok(Expr::IntLit(value, span))
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::Ident(name, span))
            }
            other => Err(Diagnostic::error(
                format!("expected an expression, found {:?}", other),
                span,
            )),
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn span(&self) -> Span {
        self.current().span
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if !matches!(tok.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        tok
    }

    fn at(&self, kind: &TokenKind) -> bool {
        &self.current().kind == kind
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<(), Diagnostic> {
        if self.at(kind) {
            self.advance();
            Ok(())
        } else {
            Err(Diagnostic::error(
                format!("expected {:?}, found {:?}", kind, self.current().kind),
                self.span(),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, Diagnostic> {
        match &self.current().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            other => Err(Diagnostic::error(
                format!("expected an identifier, found {:?}", other),
                self.span(),
            )),
        }
    }
}
