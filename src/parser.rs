use crate::ast::{
    AssignTarget, BinOp, CompareOp, Expr, FieldDef, FunctionDef, LogicalOp, Param, Program, Stmt,
    StructDef, TypeName, UnaryOp,
};
use crate::diag::{Diagnostic, Span};
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: &[Token]) -> Result<Program, Diagnostic> {
    Parser {
        tokens,
        pos: 0,
        no_struct_literal: false,
    }
    .parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    no_struct_literal: bool,
}

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let mut structs = Vec::new();
        let mut functions = Vec::new();

        while !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Struct) {
                structs.push(self.parse_struct_def()?);
            } else {
                functions.push(self.parse_function()?);
            }
        }

        if functions.is_empty() {
            return Err(Diagnostic::error(
                "expected at least a `fn main()`",
                self.span(),
            ));
        }

        Ok(Program { structs, functions })
    }

    /// Runs `f` with bare struct literals forbidden - used while parsing an
    /// `if`/`while` condition, since `if p == Point { ... }` would otherwise
    /// be ambiguous with the following block.
    fn disallowing_struct_literal<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let previous = self.no_struct_literal;
        self.no_struct_literal = true;
        let result = f(self);
        self.no_struct_literal = previous;
        result
    }

    /// Runs `f` with the restriction lifted again - used whenever we enter
    /// an unambiguous bracketed/parenthesized sub-context (so struct
    /// literals still work *inside* parens/calls/brackets even within an
    /// `if`/`while` condition).
    fn allowing_struct_literal<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let previous = self.no_struct_literal;
        self.no_struct_literal = false;
        let result = f(self);
        self.no_struct_literal = previous;
        result
    }

    fn parse_function(&mut self) -> Result<FunctionDef, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::Fn)?;

        let name = self.expect_ident()?;

        self.expect(&TokenKind::LParen)?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
            params.push(self.parse_param()?);
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RParen)?;

        let return_type = if self.at(&TokenKind::Arrow) {
            self.advance();
            Some(self.parse_type_name()?)
        } else {
            None
        };

        let (body, end) = self.parse_block()?;

        Ok(FunctionDef {
            name,
            params,
            return_type,
            body,
            span: start.to(end),
        })
    }

    fn parse_struct_def(&mut self) -> Result<StructDef, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let field_start = self.span();
            let field_name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_type_name()?;
            let field_end = ty.span();
            fields.push(FieldDef {
                name: field_name,
                ty,
                span: field_start.to(field_end),
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.span();
        self.expect(&TokenKind::RBrace)?;

        Ok(StructDef {
            name,
            fields,
            span: start.to(end),
        })
    }

    fn parse_param(&mut self) -> Result<Param, Diagnostic> {
        let start = self.span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_type_name()?;
        let end = ty.span();
        Ok(Param {
            name,
            ty,
            span: start.to(end),
        })
    }

    fn parse_type_name(&mut self) -> Result<TypeName, Diagnostic> {
        let start = self.span();
        if self.at(&TokenKind::Star) {
            self.advance();
            let inner = self.parse_type_name()?;
            let span = start.to(inner.span());
            return Ok(TypeName::Pointer(Box::new(inner), span));
        }
        if self.at(&TokenKind::LBracket) {
            self.advance();
            let elem = self.parse_type_name()?;
            self.expect(&TokenKind::Semicolon)?;

            let len_span = self.span();
            let len = match &self.current().kind {
                TokenKind::Int(n) if *n > 0 => {
                    let n = *n;
                    self.advance();
                    n as u32
                }
                other => {
                    return Err(Diagnostic::error(
                        format!(
                            "expected a positive integer array length, found {:?}",
                            other
                        ),
                        len_span,
                    ));
                }
            };

            let end = self.span();
            self.expect(&TokenKind::RBracket)?;
            return Ok(TypeName::Array(Box::new(elem), len, start.to(end)));
        }
        let span = self.span();
        let name = self.expect_ident()?;
        Ok(TypeName::Named(name, span))
    }

    fn parse_block(&mut self) -> Result<(Vec<Stmt>, Span), Diagnostic> {
        self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            stmts.push(self.parse_stmt()?);
        }
        let end = self.span();
        self.expect(&TokenKind::RBrace)?;
        Ok((stmts, end))
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.current().kind {
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::Return => self.parse_return_stmt(),
            _ => self.parse_expr_or_assign_stmt(),
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::Let)?;

        let mutable = if self.at(&TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };

        let name = self.expect_ident()?;
        self.expect(&TokenKind::Equals)?;
        let value = self.parse_expr()?;
        let end = self.span();
        self.expect(&TokenKind::Semicolon)?;

        Ok(Stmt::Let {
            name,
            mutable,
            value,
            span: start.to(end),
        })
    }

    fn parse_expr_or_assign_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        let expr = self.parse_expr()?;

        if self.at(&TokenKind::Equals) {
            self.advance();
            let target = Self::expr_to_assign_target(expr)?;
            let value = self.parse_expr()?;
            let end = self.span();
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt::Assign {
                target,
                value,
                span: start.to(end),
            });
        }

        let end = self.span();
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Expr {
            value: expr,
            span: start.to(end),
        })
    }

    fn expr_to_assign_target(expr: Expr) -> Result<AssignTarget, Diagnostic> {
        match expr {
            Expr::Ident(name, _) => Ok(AssignTarget::Name(name)),
            Expr::Unary {
                op: UnaryOp::Deref,
                operand,
                ..
            } => Ok(AssignTarget::Deref(*operand)),
            Expr::Index { base, index, .. } => Ok(AssignTarget::Index {
                base: *base,
                index: *index,
            }),
            Expr::Field { base, field, .. } => Ok(AssignTarget::Field { base: *base, field }),
            other => {
                let span = other.span();
                Err(Diagnostic::error("invalid assignment target", span))
            }
        }
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::If)?;
        let cond = self.disallowing_struct_literal(|p| p.parse_expr())?;
        let (then_body, mut end) = self.parse_block()?;

        let else_body = if self.at(&TokenKind::Else) {
            self.advance();
            if self.at(&TokenKind::If) {
                let nested = self.parse_if_stmt()?;
                end = nested.span();
                Some(vec![nested])
            } else {
                let (body, block_end) = self.parse_block()?;
                end = block_end;
                Some(body)
            }
        } else {
            None
        };

        Ok(Stmt::If {
            cond,
            then_body,
            else_body,
            span: start.to(end),
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::While)?;
        let cond = self.disallowing_struct_literal(|p| p.parse_expr())?;
        let (body, end) = self.parse_block()?;

        Ok(Stmt::While {
            cond,
            body,
            span: start.to(end),
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::Return)?;

        let value = if self.at(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };

        let end = self.span();
        self.expect(&TokenKind::Semicolon)?;

        Ok(Stmt::Return {
            value,
            span: start.to(end),
        })
    }

    fn parse_syscall_args(&mut self, name: String, start: Span) -> Result<Expr, Diagnostic> {
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
            args.push(self.allowing_struct_literal(|p| p.parse_expr())?);
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.span();
        self.expect(&TokenKind::RParen)?;

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
        Ok(Expr::Syscall {
            args: Box::new(args),
            span: start.to(end),
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_and()?;
        while self.at(&TokenKind::OrOr) {
            self.advance();
            let rhs = self.parse_and()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Logical {
                op: LogicalOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_compare()?;
        while self.at(&TokenKind::AndAnd) {
            self.advance();
            let rhs = self.parse_compare()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Logical {
                op: LogicalOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_compare(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_binary(0)?;

        if let Some(op) = self.peek_compare_op() {
            self.advance();
            let rhs = self.parse_binary(0)?;
            let span = lhs.span().to(rhs.span());
            return Ok(Expr::Compare {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            });
        }

        Ok(lhs)
    }

    fn peek_compare_op(&self) -> Option<CompareOp> {
        match self.current().kind {
            TokenKind::EqEq => Some(CompareOp::Eq),
            TokenKind::NotEq => Some(CompareOp::Ne),
            TokenKind::Lt => Some(CompareOp::Lt),
            TokenKind::Le => Some(CompareOp::Le),
            TokenKind::Gt => Some(CompareOp::Gt),
            TokenKind::Ge => Some(CompareOp::Ge),
            _ => None,
        }
    }

    /// Precedence climbing: consumes a unary term, then repeatedly folds in
    /// any following binary operator whose precedence is >= `min_prec`.
    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_unary()?;

        while let Some((op, prec)) = self.peek_binop() {
            if prec < min_prec {
                break;
            }
            self.advance(); // consume the operator token

            // `prec + 1` (not `prec`) makes this left-associative: it stops
            // the recursive call from swallowing another operator of the
            // *same* precedence, leaving it for this loop to pick up next.
            let rhs = self.parse_binary(prec + 1)?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }

        Ok(lhs)
    }

    fn peek_binop(&self) -> Option<(BinOp, u8)> {
        match self.current().kind {
            TokenKind::Plus => Some((BinOp::Add, 1)),
            TokenKind::Minus => Some((BinOp::Sub, 1)),
            TokenKind::Star => Some((BinOp::Mul, 2)),
            TokenKind::Slash => Some((BinOp::Div, 2)),
            TokenKind::Percent => Some((BinOp::Rem, 2)),
            _ => None,
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        if self.at(&TokenKind::Minus) {
            let start = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            let span = start.to(operand.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
                span,
            });
        }
        if self.at(&TokenKind::Bang) {
            let start = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            let span = start.to(operand.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span,
            });
        }
        if self.at(&TokenKind::Star) {
            let start = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            let span = start.to(operand.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Deref,
                operand: Box::new(operand),
                span,
            });
        }
        if self.at(&TokenKind::Amp) {
            let start = self.span();
            self.advance();
            let ident_span = self.span();
            let name = self.expect_ident()?;
            return Ok(Expr::AddressOf {
                name,
                span: start.to(ident_span),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.at(&TokenKind::LBracket) {
                let start = expr.span();
                self.advance();
                let index = self.allowing_struct_literal(|p| p.parse_expr())?;
                let end = self.span();
                self.expect(&TokenKind::RBracket)?;
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                    span: start.to(end),
                };
            } else if self.at(&TokenKind::Dot) {
                let start = expr.span();
                self.advance();
                let field_span = self.span();
                let field = self.expect_ident()?;
                expr = Expr::Field {
                    base: Box::new(expr),
                    field,
                    span: start.to(field_span),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
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
                if self.at(&TokenKind::LParen) {
                    self.parse_call_args(name, span)
                } else if self.at(&TokenKind::LBrace) && !self.no_struct_literal {
                    self.parse_struct_lit(name, span)
                } else {
                    Ok(Expr::Ident(name, span))
                }
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.allowing_struct_literal(|p| p.parse_expr())?;
                self.expect(&TokenKind::RParen)?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.at(&TokenKind::RBracket) {
                    elements.push(self.allowing_struct_literal(|p| p.parse_expr())?);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let end = self.span();
                self.expect(&TokenKind::RBracket)?;

                if elements.is_empty() {
                    return Err(Diagnostic::error(
                        "array literal cannot be empty",
                        span.to(end),
                    ));
                }

                Ok(Expr::ArrayLit {
                    elements,
                    span: span.to(end),
                })
            }
            TokenKind::Intrinsic(name) => {
                let name = name.clone();
                self.advance();
                self.parse_syscall_args(name, span)
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::BoolLit(true, span))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::BoolLit(false, span))
            }
            other => Err(Diagnostic::error(
                format!("expected an expression, found {:?}", other),
                span,
            )),
        }
    }

    fn parse_call_args(&mut self, name: String, start: Span) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::LParen)?;

        let mut args = Vec::new();
        while !self.at(&TokenKind::RParen) {
            args.push(self.allowing_struct_literal(|p| p.parse_expr())?);
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.span();
        self.expect(&TokenKind::RParen)?;

        Ok(Expr::Call {
            name,
            args,
            span: start.to(end),
        })
    }

    fn parse_struct_lit(&mut self, name: String, start: Span) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let field_name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let value = self.allowing_struct_literal(|p| p.parse_expr())?;
            fields.push((field_name, value));
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.span();
        self.expect(&TokenKind::RBrace)?;

        Ok(Expr::StructLit {
            name,
            fields,
            span: start.to(end),
        })
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
