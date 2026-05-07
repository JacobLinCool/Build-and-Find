pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Keyword, Lexer, Token, TokenKind};
use crate::parser::ast::*;
use crate::types::ColumnType;

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    lookahead: Token,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Result<Self> {
        let mut lexer = Lexer::new(input);
        let lookahead = lexer.next_token()?;
        Ok(Self { lexer, lookahead })
    }

    fn advance(&mut self) -> Result<Token> {
        let current = self.lookahead.clone();
        self.lookahead = self.lexer.next_token()?;
        Ok(current)
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<Token> {
        match self.lookahead.kind.clone() {
            TokenKind::Keyword(k) if k == keyword => self.advance(),
            _ => Err(self.error_here(format!("expected keyword {:?}", keyword))),
        }
    }

    fn match_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.lookahead.kind, TokenKind::Keyword(k) if k == keyword)
    }

    fn match_kind(&self, kind: &TokenKind) -> bool {
        &self.lookahead.kind == kind
    }

    fn error_here(&self, message: impl Into<String>) -> MiniDbError {
        MiniDbError::parse(
            message,
            Some(self.lookahead.line),
            Some(self.lookahead.column),
        )
    }

    fn consume_if_keyword(&mut self, keyword: Keyword) -> Result<bool> {
        if self.match_keyword(keyword) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        while !matches!(self.lookahead.kind, TokenKind::Eof) {
            statements.push(self.parse_statement()?);
            if matches!(self.lookahead.kind, TokenKind::Semicolon) {
                self.advance()?;
            } else if !matches!(self.lookahead.kind, TokenKind::Eof) {
                return Err(self.error_here("expected ';' or end of input"));
            }
        }
        Ok(statements)
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        match self.lookahead.kind.clone() {
            TokenKind::Keyword(Keyword::Create) => self.parse_create(),
            TokenKind::Keyword(Keyword::Drop) => self.parse_drop(),
            TokenKind::Keyword(Keyword::Insert) => self.parse_insert(),
            TokenKind::Keyword(Keyword::Select) => self.parse_select().map(Statement::Select),
            TokenKind::Keyword(Keyword::Update) => self.parse_update(),
            TokenKind::Keyword(Keyword::Delete) => self.parse_delete(),
            TokenKind::Keyword(Keyword::Begin) => {
                self.advance()?;
                Ok(Statement::Begin)
            }
            TokenKind::Keyword(Keyword::Commit) => {
                self.advance()?;
                Ok(Statement::Commit)
            }
            TokenKind::Keyword(Keyword::Rollback) => {
                self.advance()?;
                Ok(Statement::Rollback)
            }
            _ => Err(self.error_here("expected SQL statement")),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Create)?;
        if self.match_keyword(Keyword::Table) {
            Ok(Statement::CreateTable(self.parse_create_table()?))
        } else if self.match_keyword(Keyword::Index) {
            self.parse_create_index()
        } else {
            Err(self.error_here("expected TABLE or INDEX after CREATE"))
        }
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Drop)?;
        if self.match_keyword(Keyword::Table) {
            self.advance()?;
            let name = self.parse_identifier()?;
            Ok(Statement::DropTable { name })
        } else if self.match_keyword(Keyword::Index) {
            self.advance()?;
            let name = self.parse_identifier()?;
            Ok(Statement::DropIndex { name })
        } else {
            Err(self.error_here("expected TABLE or INDEX after DROP"))
        }
    }

    fn parse_create_table(&mut self) -> Result<CreateTable> {
        self.expect_keyword(Keyword::Table)?;
        let name = self.parse_identifier()?;
        self.expect(TokenKind::LParen)?;
        let mut columns = Vec::new();
        let mut checks = Vec::new();
        loop {
            if self.match_keyword(Keyword::Check) {
                self.advance()?;
                self.expect(TokenKind::LParen)?;
                checks.push(self.parse_expr()?);
                self.expect(TokenKind::RParen)?;
            } else {
                columns.push(self.parse_column_definition()?);
            }
            if self.match_kind(&TokenKind::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(CreateTable {
            name,
            columns,
            checks,
        })
    }

    fn parse_column_definition(&mut self) -> Result<ColumnDefinition> {
        let name = self.parse_identifier()?;
        let col_type = self.parse_type_name()?;
        let mut not_null = false;
        let mut unique = false;
        let mut primary_key = false;
        let mut default = None;
        loop {
            if self.consume_if_keyword(Keyword::Primary)? {
                self.expect_keyword(Keyword::Key)?;
                primary_key = true;
            } else if self.consume_if_keyword(Keyword::Not)? {
                self.expect_keyword(Keyword::Null)?;
                not_null = true;
            } else if self.consume_if_keyword(Keyword::Unique)? {
                unique = true;
            } else if self.consume_if_keyword(Keyword::Default)? {
                default = Some(self.parse_literal_expr()?);
            } else {
                break;
            }
        }
        Ok(ColumnDefinition {
            name,
            col_type,
            not_null,
            unique,
            primary_key,
            default,
        })
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Index)?;
        let name = self.parse_identifier()?;
        self.expect_keyword(Keyword::On)?;
        let table = self.parse_identifier()?;
        self.expect(TokenKind::LParen)?;
        let column = self.parse_identifier()?;
        self.expect(TokenKind::RParen)?;
        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Insert)?;
        self.expect_keyword(Keyword::Into)?;
        let table = self.parse_identifier()?;
        let columns = if self.match_kind(&TokenKind::LParen) {
            self.advance()?;
            let mut cols = Vec::new();
            loop {
                cols.push(self.parse_identifier()?);
                if self.match_kind(&TokenKind::Comma) {
                    self.advance()?;
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
            Some(cols)
        } else {
            None
        };
        self.expect_keyword(Keyword::Values)?;
        self.expect(TokenKind::LParen)?;
        let mut values = Vec::new();
        loop {
            values.push(self.parse_expr()?);
            if self.match_kind(&TokenKind::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(Statement::Insert(Insert {
            table,
            columns,
            values,
        }))
    }

    fn parse_select(&mut self) -> Result<Select> {
        self.expect_keyword(Keyword::Select)?;
        let mut projection = Vec::new();
        loop {
            projection.push(self.parse_select_item()?);
            if self.match_kind(&TokenKind::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect_keyword(Keyword::From)?;
        let left = self.parse_table_ref()?;
        let join = if self.match_keyword(Keyword::Join) || self.match_keyword(Keyword::Inner) {
            if self.match_keyword(Keyword::Inner) {
                self.advance()?;
                self.expect_keyword(Keyword::Join)?;
            } else {
                self.advance()?;
            }
            let right = self.parse_table_ref()?;
            self.expect_keyword(Keyword::On)?;
            let on = self.parse_expr()?;
            Some(JoinClause { right, on })
        } else {
            None
        };
        let selection = if self.consume_if_keyword(Keyword::Where)? {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.consume_if_keyword(Keyword::Order)? {
            self.expect_keyword(Keyword::By)?;
            loop {
                let expr = self.parse_expr()?;
                let asc = if self.consume_if_keyword(Keyword::Desc)? {
                    false
                } else {
                    let _ = self.consume_if_keyword(Keyword::Asc)?;
                    true
                };
                order_by.push(OrderByExpr { expr, asc });
                if self.match_kind(&TokenKind::Comma) {
                    self.advance()?;
                } else {
                    break;
                }
            }
        }
        let limit = if self.consume_if_keyword(Keyword::Limit)? {
            let limit = self.parse_usize_literal()?;
            let offset = if self.consume_if_keyword(Keyword::Offset)? {
                self.parse_usize_literal()?
            } else {
                0
            };
            Some(LimitClause { limit, offset })
        } else {
            None
        };
        Ok(Select {
            projection,
            from: FromClause { left, join },
            selection,
            order_by,
            limit,
        })
    }

    fn parse_select_item(&mut self) -> Result<SelectItem> {
        if self.match_kind(&TokenKind::Star) {
            self.advance()?;
            Ok(SelectItem::Wildcard)
        } else if let TokenKind::Identifier(name) = self.lookahead.kind.clone() {
            self.advance()?;
            if self.match_kind(&TokenKind::LParen) {
                self.advance()?;
                let mut args = Vec::new();
                if !self.match_kind(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.match_kind(&TokenKind::Comma) {
                            self.advance()?;
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                Ok(SelectItem::Expr {
                    expr: Expr::Function { name, args },
                    alias: None,
                })
            } else if self.match_kind(&TokenKind::Dot) {
                self.advance()?;
                if self.match_kind(&TokenKind::Star) {
                    self.advance()?;
                    Ok(SelectItem::QualifiedWildcard(name))
                } else {
                    let column = self.parse_identifier()?;
                    Ok(SelectItem::Expr {
                        expr: Expr::ColumnRef {
                            table: Some(name),
                            column,
                        },
                        alias: None,
                    })
                }
            } else {
                Ok(SelectItem::Expr {
                    expr: Expr::ColumnRef {
                        table: None,
                        column: name,
                    },
                    alias: None,
                })
            }
        } else {
            let expr = self.parse_expr()?;
            let alias = if self.consume_if_keyword(Keyword::As)? {
                Some(self.parse_identifier()?)
            } else {
                None
            };
            Ok(SelectItem::Expr { expr, alias })
        }
    }

    fn parse_table_ref(&mut self) -> Result<TableRef> {
        let name = self.parse_identifier()?;
        let alias = if self.consume_if_keyword(Keyword::As)? {
            Some(self.parse_identifier()?)
        } else if matches!(self.lookahead.kind, TokenKind::Identifier(_)) {
            Some(self.parse_identifier()?)
        } else {
            None
        };
        Ok(TableRef { name, alias })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Update)?;
        let table = self.parse_identifier()?;
        self.expect_keyword(Keyword::Set)?;
        let mut assignments = Vec::new();
        loop {
            let column = self.parse_identifier()?;
            self.expect(TokenKind::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((column, expr));
            if self.match_kind(&TokenKind::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        let selection = if self.consume_if_keyword(Keyword::Where)? {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update(Update {
            table,
            assignments,
            selection,
        }))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Delete)?;
        self.expect_keyword(Keyword::From)?;
        let table = self.parse_identifier()?;
        let selection = if self.consume_if_keyword(Keyword::Where)? {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete(Delete { table, selection }))
    }

    fn parse_type_name(&mut self) -> Result<ColumnType> {
        match self.lookahead.kind.clone() {
            TokenKind::Keyword(Keyword::Int) => {
                self.advance()?;
                Ok(ColumnType::Int)
            }
            TokenKind::Keyword(Keyword::BigInt) => {
                self.advance()?;
                Ok(ColumnType::BigInt)
            }
            TokenKind::Keyword(Keyword::Real) => {
                self.advance()?;
                Ok(ColumnType::Real)
            }
            TokenKind::Keyword(Keyword::Text) => {
                self.advance()?;
                Ok(ColumnType::Text)
            }
            TokenKind::Keyword(Keyword::Bool) => {
                self.advance()?;
                Ok(ColumnType::Bool)
            }
            _ => Err(self.error_here("expected column type")),
        }
    }

    fn parse_identifier(&mut self) -> Result<String> {
        match self.lookahead.kind.clone() {
            TokenKind::Identifier(name) => {
                self.advance()?;
                Ok(name)
            }
            TokenKind::Keyword(keyword) => {
                let name = format!("{keyword:?}").to_ascii_lowercase();
                self.advance()?;
                Ok(name)
            }
            _ => Err(self.error_here("expected identifier")),
        }
    }

    fn parse_usize_literal(&mut self) -> Result<usize> {
        match self.lookahead.kind.clone() {
            TokenKind::Number(num) => {
                self.advance()?;
                num.parse::<usize>()
                    .map_err(|_| self.error_here("invalid integer"))
            }
            _ => Err(self.error_here("expected integer literal")),
        }
    }

    fn parse_literal_expr(&mut self) -> Result<Expr> {
        match self.lookahead.kind.clone() {
            TokenKind::Keyword(Keyword::Null) => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Null))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Bool(true)))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Bool(false)))
            }
            TokenKind::Number(num) => {
                self.advance()?;
                if num.contains('.') {
                    Ok(Expr::Literal(Literal::Real(
                        num.parse()
                            .map_err(|_| self.error_here("invalid real literal"))?,
                    )))
                } else {
                    Ok(Expr::Literal(Literal::Int(num.parse().map_err(|_| {
                        self.error_here("invalid integer literal")
                    })?)))
                }
            }
            TokenKind::String(s) => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Text(s)))
            }
            _ => Err(self.error_here("expected literal")),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        while self.consume_if_keyword(Keyword::Or)? {
            let rhs = self.parse_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut expr = self.parse_not()?;
        while self.consume_if_keyword(Keyword::And)? {
            let rhs = self.parse_not()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.consume_if_keyword(Keyword::Not)? {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_not()?),
            })
        } else {
            self.parse_compare()
        }
    }

    fn parse_compare(&mut self) -> Result<Expr> {
        let mut expr = self.parse_concat()?;
        loop {
            let op = match self.lookahead.kind.clone() {
                TokenKind::Eq => Some(BinaryOp::Eq),
                TokenKind::Neq => Some(BinaryOp::Neq),
                TokenKind::Lt => Some(BinaryOp::Lt),
                TokenKind::Lte => Some(BinaryOp::Lte),
                TokenKind::Gt => Some(BinaryOp::Gt),
                TokenKind::Gte => Some(BinaryOp::Gte),
                _ => None,
            };
            if let Some(op) = op {
                self.advance()?;
                let rhs = self.parse_concat()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(rhs),
                };
            } else if self.consume_if_keyword(Keyword::Is)? {
                let negated = self.consume_if_keyword(Keyword::Not)?;
                self.expect_keyword(Keyword::Null)?;
                expr = Expr::IsNull {
                    expr: Box::new(expr),
                    negated,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut expr = self.parse_add()?;
        while self.match_kind(&TokenKind::PipePipe) {
            self.advance()?;
            let rhs = self.parse_add()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Concat,
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut expr = self.parse_mul()?;
        loop {
            let op = match self.lookahead.kind.clone() {
                TokenKind::Plus => Some(BinaryOp::Add),
                TokenKind::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                self.advance()?;
                let rhs = self.parse_mul()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(rhs),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        loop {
            let op = match self.lookahead.kind.clone() {
                TokenKind::Star => Some(BinaryOp::Mul),
                TokenKind::Slash => Some(BinaryOp::Div),
                _ => None,
            };
            if let Some(op) = op {
                self.advance()?;
                let rhs = self.parse_unary()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(rhs),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.match_kind(&TokenKind::Minus) {
            self.advance()?;
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.lookahead.kind.clone() {
            TokenKind::Number(_)
            | TokenKind::String(_)
            | TokenKind::Keyword(Keyword::Null)
            | TokenKind::Keyword(Keyword::True)
            | TokenKind::Keyword(Keyword::False) => self.parse_literal_expr(),
            TokenKind::Identifier(_) | TokenKind::Keyword(_) => {
                let name = self.parse_identifier()?;
                if self.match_kind(&TokenKind::LParen) {
                    self.advance()?;
                    let mut args = Vec::new();
                    if !self.match_kind(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.match_kind(&TokenKind::Comma) {
                                self.advance()?;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    Ok(Expr::Function { name, args })
                } else if self.match_kind(&TokenKind::Dot) {
                    self.advance()?;
                    let column = self.parse_identifier()?;
                    Ok(Expr::ColumnRef {
                        table: Some(name),
                        column,
                    })
                } else {
                    Ok(Expr::ColumnRef {
                        table: None,
                        column: name,
                    })
                }
            }
            TokenKind::LParen => {
                self.advance()?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(self.error_here("expected expression")),
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token> {
        if self.lookahead.kind == kind {
            self.advance()
        } else {
            Err(self.error_here(format!("expected {:?}", kind)))
        }
    }
}

pub fn parse_sql(input: &str) -> Result<Vec<Statement>> {
    let mut parser = Parser::new(input)?;
    parser.parse_statements()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_create_table() {
        let statements =
            parse_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);").unwrap();
        assert_eq!(statements.len(), 1);
    }

    #[test]
    fn parses_select_with_where() {
        let statements = parse_sql("SELECT * FROM t WHERE id = 1;").unwrap();
        assert!(matches!(statements[0], Statement::Select(_)));
    }
}
