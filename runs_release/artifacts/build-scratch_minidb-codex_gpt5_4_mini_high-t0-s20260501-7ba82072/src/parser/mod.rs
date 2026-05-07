pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::types::{ColumnConstraint, ColumnType, Literal, TableConstraint};
use ast::*;

pub fn parse(input: &str) -> Result<Vec<Statement>> {
    Parser::new(input)?.parse_statements()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(input: &str) -> Result<Self> {
        let mut tokens = Lexer::new(input).collect::<Result<Vec<_>>>()?;
        if !matches!(tokens.last().map(|t| &t.kind), Some(TokenKind::Eof)) {
            tokens.push(Token {
                kind: TokenKind::Eof,
                line: 1,
                column: 1,
            });
        }
        Ok(Self { tokens, pos: 0 })
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len().saturating_sub(1))]
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len().saturating_sub(1) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.current_kind_eq(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn current_kind_eq(&self, kind: &TokenKind) -> bool {
        use TokenKind::*;
        match (&self.current().kind, kind) {
            (Comma, Comma)
            | (Dot, Dot)
            | (Star, Star)
            | (LParen, LParen)
            | (RParen, RParen)
            | (Plus, Plus)
            | (Minus, Minus)
            | (Slash, Slash)
            | (Percent, Percent)
            | (PipePipe, PipePipe)
            | (Eq, Eq)
            | (Neq, Neq)
            | (Lt, Lt)
            | (Lte, Lte)
            | (Gt, Gt)
            | (Gte, Gte)
            | (Semicolon, Semicolon)
            | (Eof, Eof) => true,
            (Keyword(a), Keyword(b)) => a == b,
            (Ident(a), Ident(b)) => a == b,
            (Number(a), Number(b)) => a == b,
            (String(a), String(b)) => a == b,
            _ => false,
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<()> {
        if self.current_kind_eq(&kind) {
            self.advance();
            Ok(())
        } else {
            Err(self.error_here(format!("expected {:?}", kind)))
        }
    }

    fn error_here(&self, message: impl Into<String>) -> MiniDbError {
        MiniDbError::Parse {
            line: self.current().line,
            column: self.current().column,
            message: message.into(),
        }
    }

    fn parse_statements(mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            if self.eat(&TokenKind::Semicolon) {
                continue;
            }
            stmts.push(self.parse_statement()?);
            let _ = self.eat(&TokenKind::Semicolon);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        match &self.current().kind {
            TokenKind::Dot => self.parse_meta_command(),
            TokenKind::Keyword(k) if k == "CREATE" => self.parse_create(),
            TokenKind::Keyword(k) if k == "DROP" => self.parse_drop(),
            TokenKind::Keyword(k) if k == "INSERT" => self.parse_insert(),
            TokenKind::Keyword(k) if k == "SELECT" => self.parse_select(),
            TokenKind::Keyword(k) if k == "UPDATE" => self.parse_update(),
            TokenKind::Keyword(k) if k == "DELETE" => self.parse_delete(),
            TokenKind::Keyword(k) if k == "BEGIN" => {
                self.advance();
                Ok(Statement::Begin)
            }
            TokenKind::Keyword(k) if k == "COMMIT" => {
                self.advance();
                Ok(Statement::Commit)
            }
            TokenKind::Keyword(k) if k == "ROLLBACK" => {
                self.advance();
                Ok(Statement::Rollback)
            }
            _ => Err(self.error_here("expected statement")),
        }
    }

    fn parse_meta_command(&mut self) -> Result<Statement> {
        self.expect(TokenKind::Dot)?;
        let name = match &self.current().kind {
            TokenKind::Ident(s) | TokenKind::Keyword(s) => s.to_ascii_lowercase(),
            _ => return Err(self.error_here("expected meta command name")),
        };
        self.advance();
        match name.as_str() {
            "schema" => {
                let table = if matches!(
                    self.current().kind,
                    TokenKind::Ident(_) | TokenKind::Keyword(_)
                ) {
                    Some(self.parse_ident()?)
                } else {
                    None
                };
                Ok(Statement::Meta(MetaCommand::Schema { table }))
            }
            "tables" => Ok(Statement::Meta(MetaCommand::Tables)),
            "indexes" => Ok(Statement::Meta(MetaCommand::Indexes)),
            "format" => {
                let kind = self.parse_ident()?;
                Ok(Statement::Meta(MetaCommand::Format { kind }))
            }
            "exit" => Ok(Statement::Meta(MetaCommand::Exit)),
            _ => Err(self.error_here(format!("unknown meta command .{name}"))),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect_kw("CREATE")?;
        match &self.current().kind {
            TokenKind::Keyword(k) if k == "TABLE" => self.parse_create_table(),
            TokenKind::Keyword(k) if k == "INDEX" => self.parse_create_index(),
            _ => Err(self.error_here("expected TABLE or INDEX")),
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        self.expect_kw("TABLE")?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut columns = Vec::new();
        let mut table_constraints = Vec::new();
        loop {
            if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "CHECK") {
                self.advance();
                self.expect(TokenKind::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                table_constraints.push(TableConstraint::Check(expr));
            } else {
                columns.push(self.parse_column_def()?);
            }
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        self.expect(TokenKind::RParen)?;
        Ok(Statement::CreateTable {
            name,
            columns,
            table_constraints,
        })
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.parse_ident()?;
        let col_type = self.parse_column_type()?;
        let mut constraints = ColumnConstraint::new();
        loop {
            match &self.current().kind {
                TokenKind::Keyword(k) if k == "PRIMARY" => {
                    self.advance();
                    self.expect_kw("KEY")?;
                    constraints.primary_key = true;
                    constraints.not_null = true;
                    constraints.unique = true;
                }
                TokenKind::Keyword(k) if k == "NOT" => {
                    self.advance();
                    self.expect_kw("NULL")?;
                    constraints.not_null = true;
                }
                TokenKind::Keyword(k) if k == "UNIQUE" => {
                    self.advance();
                    constraints.unique = true;
                }
                TokenKind::Keyword(k) if k == "DEFAULT" => {
                    self.advance();
                    constraints.default = Some(self.parse_literal()?);
                }
                _ => break,
            }
        }
        Ok(ColumnDef {
            name,
            col_type,
            constraints,
        })
    }

    fn parse_column_type(&mut self) -> Result<ColumnType> {
        match &self.current().kind {
            TokenKind::Keyword(k) if k == "INT" || k == "BIGINT" => {
                self.advance();
                Ok(ColumnType::Int)
            }
            TokenKind::Keyword(k) if k == "REAL" => {
                self.advance();
                Ok(ColumnType::Real)
            }
            TokenKind::Keyword(k) if k == "TEXT" => {
                self.advance();
                Ok(ColumnType::Text)
            }
            TokenKind::Keyword(k) if k == "BOOL" => {
                self.advance();
                Ok(ColumnType::Bool)
            }
            _ => Err(self.error_here("expected column type")),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.expect_kw("INDEX")?;
        let name = self.parse_ident()?;
        self.expect_kw("ON")?;
        let table = self.parse_ident()?;
        self.expect(TokenKind::LParen)?;
        let column = self.parse_ident()?;
        self.expect(TokenKind::RParen)?;
        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect_kw("DROP")?;
        match &self.current().kind {
            TokenKind::Keyword(k) if k == "TABLE" => {
                self.advance();
                Ok(Statement::DropTable {
                    name: self.parse_ident()?,
                })
            }
            TokenKind::Keyword(k) if k == "INDEX" => {
                self.advance();
                Ok(Statement::DropIndex {
                    name: self.parse_ident()?,
                })
            }
            _ => Err(self.error_here("expected TABLE or INDEX")),
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_kw("INSERT")?;
        self.expect_kw("INTO")?;
        let table = self.parse_ident()?;
        let columns = if self.eat(&TokenKind::LParen) {
            let cols = self.parse_ident_list()?;
            self.expect(TokenKind::RParen)?;
            Some(cols)
        } else {
            None
        };
        self.expect_kw("VALUES")?;
        self.expect(TokenKind::LParen)?;
        let values = self.parse_expr_list()?;
        self.expect(TokenKind::RParen)?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.expect_kw("SELECT")?;
        let projection = self.parse_select_list()?;
        self.expect_kw("FROM")?;
        let from = self.parse_table_ref()?;
        let mut joins = Vec::new();
        while matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "INNER") {
            self.advance();
            self.expect_kw("JOIN")?;
            let table = self.parse_table_ref()?;
            self.expect_kw("ON")?;
            let on = self.parse_expr()?;
            joins.push(Join { table, on });
        }
        let selection = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "WHERE")
        {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        let order_by = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "ORDER") {
            self.advance();
            self.expect_kw("BY")?;
            Some(self.parse_order_by_list()?)
        } else {
            None
        };
        let limit = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "LIMIT") {
            self.advance();
            let value = self.parse_usize_literal()?;
            let offset = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "OFFSET")
            {
                self.advance();
                Some(self.parse_usize_literal()?)
            } else {
                None
            };
            Some(Limit {
                limit: value,
                offset: offset.unwrap_or(0),
            })
        } else {
            None
        };
        Ok(Statement::Select(SelectStmt {
            projection,
            from,
            joins,
            selection,
            order_by,
            limit,
        }))
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect_kw("UPDATE")?;
        let table = self.parse_ident()?;
        self.expect_kw("SET")?;
        let mut assignments = Vec::new();
        loop {
            let column = self.parse_ident()?;
            self.expect(TokenKind::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((column, expr));
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        let selection = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "WHERE")
        {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments,
            selection,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_kw("DELETE")?;
        self.expect_kw("FROM")?;
        let table = self.parse_ident()?;
        let selection = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "WHERE")
        {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, selection })
    }

    fn parse_table_ref(&mut self) -> Result<TableRef> {
        let name = self.parse_ident()?;
        let alias = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "AS") {
            self.advance();
            Some(self.parse_ident()?)
        } else if matches!(
            self.current().kind,
            TokenKind::Ident(_) | TokenKind::Keyword(_)
        ) {
            if !matches!(self.current().kind, TokenKind::Keyword(_)) {
                Some(self.parse_ident()?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(TableRef { name, alias })
    }

    fn parse_select_list(&mut self) -> Result<Vec<SelectItem>> {
        if self.eat(&TokenKind::Star) {
            return Ok(vec![SelectItem::Wildcard]);
        }
        let mut items = Vec::new();
        loop {
            if self.eat(&TokenKind::Star) {
                items.push(SelectItem::Wildcard);
            } else {
                let expr = self.parse_expr()?;
                let alias = if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "AS")
                {
                    self.advance();
                    Some(self.parse_ident()?)
                } else {
                    None
                };
                items.push(SelectItem::Expr { expr, alias });
            }
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        Ok(items)
    }

    fn parse_order_by_list(&mut self) -> Result<Vec<OrderBy>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let ascending = match &self.current().kind {
                TokenKind::Keyword(k) if k == "DESC" => {
                    self.advance();
                    false
                }
                TokenKind::Keyword(k) if k == "ASC" => {
                    self.advance();
                    true
                }
                _ => true,
            };
            items.push(OrderBy { expr, ascending });
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        Ok(items)
    }

    fn parse_expr_list(&mut self) -> Result<Vec<Expr>> {
        let mut values = Vec::new();
        loop {
            values.push(self.parse_expr()?);
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        Ok(values)
    }

    fn parse_ident_list(&mut self) -> Result<Vec<String>> {
        let mut values = Vec::new();
        loop {
            values.push(self.parse_ident()?);
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        Ok(values)
    }

    fn parse_ident(&mut self) -> Result<String> {
        match &self.current().kind {
            TokenKind::Ident(s) | TokenKind::Keyword(s) => {
                let ident = s.clone();
                self.advance();
                Ok(ident)
            }
            _ => Err(self.error_here("expected identifier")),
        }
    }

    fn parse_usize_literal(&mut self) -> Result<usize> {
        match &self.current().kind {
            TokenKind::Number(s) => {
                let value = s
                    .parse::<usize>()
                    .map_err(|_| self.error_here("expected integer"))?;
                self.advance();
                Ok(value)
            }
            _ => Err(self.error_here("expected integer")),
        }
    }

    fn parse_literal(&mut self) -> Result<Literal> {
        match &self.current().kind {
            TokenKind::Keyword(k) if k == "NULL" => {
                self.advance();
                Ok(Literal::Null)
            }
            TokenKind::Keyword(k) if k == "TRUE" => {
                self.advance();
                Ok(Literal::Bool(true))
            }
            TokenKind::Keyword(k) if k == "FALSE" => {
                self.advance();
                Ok(Literal::Bool(false))
            }
            TokenKind::String(s) => {
                let s = s.clone();
                self.advance();
                Ok(Literal::Text(s))
            }
            TokenKind::Number(s) => {
                let s = s.clone();
                self.advance();
                if s.contains('.') {
                    Ok(Literal::Real(
                        s.parse::<f64>().map_err(|_| self.error_here("bad real"))?,
                    ))
                } else {
                    Ok(Literal::Int(
                        s.parse::<i64>().map_err(|_| self.error_here("bad int"))?,
                    ))
                }
            }
            _ => Err(self.error_here("expected literal")),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        while matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "OR") {
            self.advance();
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
        while matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "AND") {
            self.advance();
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
        if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "NOT") {
            self.advance();
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
            let op = match &self.current().kind {
                TokenKind::Eq => Some(BinaryOp::Eq),
                TokenKind::Neq => Some(BinaryOp::Neq),
                TokenKind::Lt => Some(BinaryOp::Lt),
                TokenKind::Lte => Some(BinaryOp::Lte),
                TokenKind::Gt => Some(BinaryOp::Gt),
                TokenKind::Gte => Some(BinaryOp::Gte),
                TokenKind::Keyword(k) if k == "IS" => {
                    self.advance();
                    if matches!(self.current().kind, TokenKind::Keyword(ref k) if k == "NOT") {
                        self.advance();
                        self.expect_kw("NULL")?;
                        return Ok(Expr::Unary {
                            op: UnaryOp::IsNotNull,
                            expr: Box::new(expr),
                        });
                    } else {
                        self.expect_kw("NULL")?;
                        return Ok(Expr::Unary {
                            op: UnaryOp::IsNull,
                            expr: Box::new(expr),
                        });
                    }
                }
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let rhs = self.parse_concat()?;
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

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut expr = self.parse_add()?;
        while matches!(self.current().kind, TokenKind::PipePipe) {
            self.advance();
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
            let op = match self.current().kind {
                TokenKind::Plus => Some(BinaryOp::Add),
                TokenKind::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
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
            let op = match self.current().kind {
                TokenKind::Star => Some(BinaryOp::Mul),
                TokenKind::Slash => Some(BinaryOp::Div),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
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
        match &self.current().kind {
            TokenKind::Minus => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            TokenKind::Plus => {
                self.advance();
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match &self.current().kind {
            TokenKind::Number(_) => Ok(Expr::Literal(self.parse_literal()?)),
            TokenKind::String(_) => Ok(Expr::Literal(self.parse_literal()?)),
            TokenKind::Keyword(k) if k == "NULL" || k == "TRUE" || k == "FALSE" => {
                Ok(Expr::Literal(self.parse_literal()?))
            }
            TokenKind::Star => {
                self.advance();
                Ok(Expr::Star)
            }
            TokenKind::Ident(_) | TokenKind::Keyword(_) => {
                let ident = self.parse_ident()?;
                if self.eat(&TokenKind::Dot) {
                    if self.eat(&TokenKind::Star) {
                        Ok(Expr::QualifiedStar { table: ident })
                    } else {
                        let column = self.parse_ident()?;
                        Ok(Expr::ColumnRef {
                            table: Some(ident),
                            column,
                        })
                    }
                } else if self.eat(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    if !self.eat(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.eat(&TokenKind::Comma) {
                                continue;
                            }
                            self.expect(TokenKind::RParen)?;
                            break;
                        }
                    }
                    Ok(Expr::Function { name: ident, args })
                } else {
                    Ok(Expr::ColumnRef {
                        table: None,
                        column: ident,
                    })
                }
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(self.error_here("expected expression")),
        }
    }

    fn expect_kw(&mut self, kw: &str) -> Result<()> {
        match &self.current().kind {
            TokenKind::Keyword(k) if k == kw => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error_here(format!("expected {kw}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_select() {
        let statements = parse("SELECT a FROM t WHERE a = 1;").unwrap();
        assert!(matches!(statements[0], Statement::Select(_)));
    }
}
