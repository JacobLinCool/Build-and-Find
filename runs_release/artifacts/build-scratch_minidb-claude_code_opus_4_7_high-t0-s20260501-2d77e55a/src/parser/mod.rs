//! SQL parser: builds an AST from a token stream.

pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Keyword, Lexer, Token, TokenKind};
use crate::types::{DataType, Value};
use ast::*;

pub fn parse(input: &str) -> Result<Vec<Statement>> {
    let tokens = Lexer::new(input).tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn expect_kw(&mut self, kw: Keyword) -> Result<()> {
        match self.peek_kind() {
            TokenKind::Keyword(k) if *k == kw => {
                self.bump();
                Ok(())
            }
            other => {
                let p = self.peek();
                Err(MiniDbError::parse(
                    format!("expected keyword {:?}, found {:?}", kw, other),
                    p.line,
                    p.column,
                ))
            }
        }
    }

    fn match_kw(&mut self, kw: Keyword) -> bool {
        if matches!(self.peek_kind(), TokenKind::Keyword(k) if *k == kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, want: &TokenKind, label: &str) -> Result<()> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(want) {
            self.bump();
            Ok(())
        } else {
            let p = self.peek();
            Err(MiniDbError::parse(
                format!("expected {}, found {:?}", label, p.kind),
                p.line,
                p.column,
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Ident(s) => {
                self.bump();
                Ok(s)
            }
            other => Err(MiniDbError::parse(
                format!("expected identifier, found {:?}", other),
                t.line,
                t.column,
            )),
        }
    }

    pub fn parse_program(&mut self) -> Result<Vec<Statement>> {
        let mut out = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::Eof) {
                break;
            }
            // Empty statements: just `;`
            if matches!(self.peek_kind(), TokenKind::Semicolon) {
                self.bump();
                continue;
            }
            let stmt = self.parse_statement()?;
            out.push(stmt);
            // Optional terminator
            if matches!(self.peek_kind(), TokenKind::Semicolon) {
                self.bump();
            }
        }
        Ok(out)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        // Meta-command: a leading '.' followed by an identifier.
        if matches!(self.peek_kind(), TokenKind::Dot) {
            self.bump();
            let name = match self.peek().clone().kind {
                TokenKind::Ident(s) => {
                    self.bump();
                    s
                }
                TokenKind::Keyword(kw) => {
                    self.bump();
                    format!("{:?}", kw).to_ascii_lowercase()
                }
                other => {
                    let p = self.peek();
                    return Err(MiniDbError::parse(
                        format!("expected meta-command name, found {:?}", other),
                        p.line,
                        p.column,
                    ));
                }
            };
            let arg = if let TokenKind::Ident(a) = self.peek_kind().clone() {
                self.bump();
                Some(a)
            } else {
                None
            };
            return Ok(Statement::Meta(MetaCommand {
                name: name.to_ascii_lowercase(),
                arg,
            }));
        }
        let kw = match self.peek_kind() {
            TokenKind::Keyword(k) => *k,
            other => {
                let p = self.peek();
                return Err(MiniDbError::parse(
                    format!("expected statement, found {:?}", other),
                    p.line,
                    p.column,
                ));
            }
        };
        match kw {
            Keyword::Select => self.parse_select().map(Statement::Select),
            Keyword::Insert => self.parse_insert(),
            Keyword::Update => self.parse_update(),
            Keyword::Delete => self.parse_delete(),
            Keyword::Create => {
                self.bump();
                if self.match_kw(Keyword::Table) {
                    self.parse_create_table()
                } else if self.match_kw(Keyword::Index) {
                    self.parse_create_index()
                } else {
                    let p = self.peek();
                    Err(MiniDbError::parse(
                        "expected TABLE or INDEX after CREATE",
                        p.line,
                        p.column,
                    ))
                }
            }
            Keyword::Drop => {
                self.bump();
                if self.match_kw(Keyword::Table) {
                    let name = self.expect_ident()?;
                    Ok(Statement::DropTable(DropTable { name }))
                } else if self.match_kw(Keyword::Index) {
                    let name = self.expect_ident()?;
                    Ok(Statement::DropIndex(DropIndex { name }))
                } else {
                    let p = self.peek();
                    Err(MiniDbError::parse(
                        "expected TABLE or INDEX after DROP",
                        p.line,
                        p.column,
                    ))
                }
            }
            Keyword::Begin => {
                self.bump();
                Ok(Statement::Begin)
            }
            Keyword::Commit => {
                self.bump();
                Ok(Statement::Commit)
            }
            Keyword::Rollback => {
                self.bump();
                Ok(Statement::Rollback)
            }
            other => {
                let p = self.peek();
                Err(MiniDbError::parse(
                    format!("unsupported statement: {:?}", other),
                    p.line,
                    p.column,
                ))
            }
        }
    }

    // --- DDL ---

    fn parse_create_table(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect_punct(&TokenKind::LParen, "(")?;
        let mut columns = Vec::new();
        let mut checks = Vec::new();
        loop {
            // Could be a column def or a table constraint (CHECK ...)
            if let TokenKind::Keyword(Keyword::Check) = self.peek_kind() {
                self.bump();
                self.expect_punct(&TokenKind::LParen, "(")?;
                let e = self.parse_expr()?;
                self.expect_punct(&TokenKind::RParen, ")")?;
                checks.push(e);
            } else {
                let col = self.parse_column_def()?;
                columns.push(col);
            }
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_punct(&TokenKind::RParen, ")")?;
        Ok(Statement::CreateTable(CreateTable {
            name,
            columns,
            checks,
        }))
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let dtype = self.parse_data_type()?;
        let mut not_null = false;
        let mut primary_key = false;
        let mut unique = false;
        let mut default = None;
        loop {
            match self.peek_kind() {
                TokenKind::Keyword(Keyword::Not) => {
                    self.bump();
                    self.expect_kw(Keyword::Null)?;
                    not_null = true;
                }
                TokenKind::Keyword(Keyword::Primary) => {
                    self.bump();
                    self.expect_kw(Keyword::Key)?;
                    primary_key = true;
                    not_null = true;
                }
                TokenKind::Keyword(Keyword::Unique) => {
                    self.bump();
                    unique = true;
                }
                TokenKind::Keyword(Keyword::Default) => {
                    self.bump();
                    let v = self.parse_literal_value()?;
                    default = Some(v);
                }
                _ => break,
            }
        }
        Ok(ColumnDef {
            name,
            dtype,
            not_null,
            primary_key,
            unique,
            default,
        })
    }

    fn parse_data_type(&mut self) -> Result<DataType> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Keyword(Keyword::IntKw) | TokenKind::Keyword(Keyword::BigInt) => {
                self.bump();
                Ok(DataType::Int)
            }
            TokenKind::Keyword(Keyword::RealKw) => {
                self.bump();
                Ok(DataType::Real)
            }
            TokenKind::Keyword(Keyword::TextKw) => {
                self.bump();
                Ok(DataType::Text)
            }
            TokenKind::Keyword(Keyword::BoolKw) | TokenKind::Keyword(Keyword::BooleanKw) => {
                self.bump();
                Ok(DataType::Bool)
            }
            other => Err(MiniDbError::parse(
                format!("expected type, found {:?}", other),
                t.line,
                t.column,
            )),
        }
    }

    fn parse_literal_value(&mut self) -> Result<Value> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::IntLit(i) => {
                self.bump();
                Ok(Value::Int(i))
            }
            TokenKind::RealLit(r) => {
                self.bump();
                Ok(Value::Real(r))
            }
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Value::Text(s))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Ok(Value::Bool(true))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Ok(Value::Bool(false))
            }
            TokenKind::Keyword(Keyword::Null) => {
                self.bump();
                Ok(Value::Null)
            }
            TokenKind::Minus => {
                self.bump();
                let v = self.parse_literal_value()?;
                match v {
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Real(r) => Ok(Value::Real(-r)),
                    _ => Err(MiniDbError::parse(
                        "negative literal must be numeric",
                        t.line,
                        t.column,
                    )),
                }
            }
            other => Err(MiniDbError::parse(
                format!("expected literal, found {:?}", other),
                t.line,
                t.column,
            )),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect_kw(Keyword::On)?;
        let table = self.expect_ident()?;
        self.expect_punct(&TokenKind::LParen, "(")?;
        let column = self.expect_ident()?;
        self.expect_punct(&TokenKind::RParen, ")")?;
        Ok(Statement::CreateIndex(CreateIndex {
            name,
            table,
            column,
        }))
    }

    // --- DML ---

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Insert)?;
        self.expect_kw(Keyword::Into)?;
        let table = self.expect_ident()?;
        let columns = if matches!(self.peek_kind(), TokenKind::LParen) {
            self.bump();
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect_punct(&TokenKind::RParen, ")")?;
            Some(cols)
        } else {
            None
        };
        self.expect_kw(Keyword::Values)?;
        let mut rows = Vec::new();
        loop {
            self.expect_punct(&TokenKind::LParen, "(")?;
            let mut row = Vec::new();
            loop {
                let e = self.parse_expr()?;
                row.push(e);
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect_punct(&TokenKind::RParen, ")")?;
            rows.push(row);
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(Statement::Insert(Insert {
            table,
            columns,
            rows,
        }))
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Update)?;
        let table = self.expect_ident()?;
        self.expect_kw(Keyword::Set)?;
        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect_punct(&TokenKind::Eq, "=")?;
            let e = self.parse_expr()?;
            assignments.push((col, e));
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update(Update {
            table,
            assignments,
            where_clause,
        }))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Delete)?;
        self.expect_kw(Keyword::From)?;
        let table = self.expect_ident()?;
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete(Delete {
            table,
            where_clause,
        }))
    }

    fn parse_select(&mut self) -> Result<Select> {
        self.expect_kw(Keyword::Select)?;
        let mut projections = Vec::new();
        loop {
            let item = self.parse_projection()?;
            projections.push(item);
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect_kw(Keyword::From)?;
        let from = self.parse_from()?;
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let order_by = if self.match_kw(Keyword::Order) {
            self.expect_kw(Keyword::By)?;
            let mut keys = Vec::new();
            loop {
                let e = self.parse_expr()?;
                let direction = if self.match_kw(Keyword::Desc) {
                    SortDir::Desc
                } else {
                    let _ = self.match_kw(Keyword::Asc);
                    SortDir::Asc
                };
                keys.push(OrderKey { expr: e, direction });
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            Some(keys)
        } else {
            None
        };
        let limit = if self.match_kw(Keyword::Limit) {
            let n = self.parse_int_literal()?;
            let offset = if self.match_kw(Keyword::Offset) {
                Some(self.parse_int_literal()?)
            } else {
                None
            };
            Some(LimitClause { limit: n, offset })
        } else {
            None
        };
        Ok(Select {
            projections,
            from,
            where_clause,
            order_by,
            limit,
        })
    }

    fn parse_int_literal(&mut self) -> Result<i64> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::IntLit(i) => {
                self.bump();
                Ok(i)
            }
            other => Err(MiniDbError::parse(
                format!("expected integer literal, found {:?}", other),
                t.line,
                t.column,
            )),
        }
    }

    fn parse_projection(&mut self) -> Result<Projection> {
        if matches!(self.peek_kind(), TokenKind::Star) {
            self.bump();
            return Ok(Projection::All);
        }
        // table.* form
        if let TokenKind::Ident(name) = self.peek_kind().clone() {
            if matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Dot)
            ) && matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(TokenKind::Star)
            ) {
                self.bump();
                self.bump();
                self.bump();
                return Ok(Projection::AllOf(name));
            }
        }
        let e = self.parse_expr()?;
        let alias = if self.match_kw(Keyword::As) {
            Some(self.expect_ident()?)
        } else if let TokenKind::Ident(name) = self.peek_kind().clone() {
            if !matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Comma)
                    | Some(TokenKind::Keyword(Keyword::From))
                    | Some(TokenKind::Eof)
                    | Some(TokenKind::Semicolon)
            ) {
                None
            } else {
                self.bump();
                Some(name)
            }
        } else {
            None
        };
        Ok(Projection::Expr { expr: e, alias })
    }

    fn parse_from(&mut self) -> Result<FromClause> {
        let first_name = self.expect_ident()?;
        let first_alias = self.parse_optional_alias()?;
        if matches!(
            self.peek_kind(),
            TokenKind::Keyword(Keyword::Inner) | TokenKind::Keyword(Keyword::Join)
        ) {
            // INNER JOIN or JOIN
            if self.match_kw(Keyword::Inner) {
                self.expect_kw(Keyword::Join)?;
            } else {
                self.expect_kw(Keyword::Join)?;
            }
            let right_name = self.expect_ident()?;
            let right_alias = self.parse_optional_alias()?;
            self.expect_kw(Keyword::On)?;
            let on = self.parse_expr()?;
            Ok(FromClause::Join(JoinClause {
                left_table: first_name,
                left_alias: first_alias,
                right_table: right_name,
                right_alias: right_alias,
                on,
            }))
        } else {
            Ok(FromClause::Table {
                name: first_name,
                alias: first_alias,
            })
        }
    }

    fn parse_optional_alias(&mut self) -> Result<Option<String>> {
        if self.match_kw(Keyword::As) {
            return Ok(Some(self.expect_ident()?));
        }
        // Bare identifier alias (only when followed by a token that doesn't look like part of a clause)
        if let TokenKind::Ident(name) = self.peek_kind().clone() {
            // We treat a bare ident before Inner/Join/Where/etc. as alias
            let next = self.tokens.get(self.pos + 1).map(|t| &t.kind);
            if matches!(
                next,
                Some(TokenKind::Keyword(Keyword::Inner))
                    | Some(TokenKind::Keyword(Keyword::Join))
                    | Some(TokenKind::Keyword(Keyword::Where))
                    | Some(TokenKind::Keyword(Keyword::Order))
                    | Some(TokenKind::Keyword(Keyword::Limit))
                    | Some(TokenKind::Eof)
                    | Some(TokenKind::Semicolon)
                    | Some(TokenKind::Keyword(Keyword::On))
            ) {
                self.bump();
                return Ok(Some(name));
            }
        }
        Ok(None)
    }

    // --- expressions ---
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_and()?;
        while self.match_kw(Keyword::Or) {
            let rhs = self.parse_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_not()?;
        while self.match_kw(Keyword::And) {
            let rhs = self.parse_not()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.match_kw(Keyword::Not) {
            let inner = self.parse_not()?;
            Ok(Expr::Not(Box::new(inner)))
        } else {
            self.parse_compare()
        }
    }

    fn parse_compare(&mut self) -> Result<Expr> {
        let lhs = self.parse_concat()?;
        // IS NULL / IS NOT NULL
        if let TokenKind::Keyword(Keyword::Is) = self.peek_kind() {
            self.bump();
            let negated = self.match_kw(Keyword::Not);
            self.expect_kw(Keyword::Null)?;
            let inner = Expr::IsNull(Box::new(lhs));
            return Ok(if negated {
                Expr::Not(Box::new(inner))
            } else {
                inner
            });
        }
        let op = match self.peek_kind() {
            TokenKind::Eq => Some(BinOp::Eq),
            TokenKind::NotEq => Some(BinOp::NotEq),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Le => Some(BinOp::Le),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::Ge => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let rhs = self.parse_concat()?;
            Ok(Expr::Binary {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_addsub()?;
        while matches!(self.peek_kind(), TokenKind::ConcatOp) {
            self.bump();
            let rhs = self.parse_addsub()?;
            lhs = Expr::Binary {
                op: BinOp::Concat,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_addsub(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_muldiv()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_muldiv()?;
            lhs = Expr::Binary {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_muldiv(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.peek_kind(), TokenKind::Minus) {
            self.bump();
            let inner = self.parse_unary()?;
            Ok(Expr::Neg(Box::new(inner)))
        } else if matches!(self.peek_kind(), TokenKind::Plus) {
            self.bump();
            self.parse_unary()
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::IntLit(i) => {
                self.bump();
                Ok(Expr::Literal(Value::Int(i)))
            }
            TokenKind::RealLit(r) => {
                self.bump();
                Ok(Expr::Literal(Value::Real(r)))
            }
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Expr::Literal(Value::Text(s)))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Ok(Expr::Literal(Value::Bool(true)))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Ok(Expr::Literal(Value::Bool(false)))
            }
            TokenKind::Keyword(Keyword::Null) => {
                self.bump();
                Ok(Expr::Literal(Value::Null))
            }
            TokenKind::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect_punct(&TokenKind::RParen, ")")?;
                Ok(e)
            }
            TokenKind::Ident(name) => {
                self.bump();
                // Function call?
                if matches!(self.peek_kind(), TokenKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek_kind(), TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek_kind(), TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_punct(&TokenKind::RParen, ")")?;
                    return Ok(Expr::Func {
                        name: name.to_ascii_uppercase(),
                        args,
                    });
                }
                // Qualified column?
                if matches!(self.peek_kind(), TokenKind::Dot) {
                    self.bump();
                    let col = self.expect_ident()?;
                    return Ok(Expr::Column {
                        table: Some(name),
                        name: col,
                    });
                }
                Ok(Expr::Column { table: None, name })
            }
            other => Err(MiniDbError::parse(
                format!("unexpected token in expression: {:?}", other),
                t.line,
                t.column,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_select_star() {
        let stmts = parse("SELECT * FROM t").unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                assert!(matches!(s.projections[0], Projection::All));
            }
            _ => panic!("expected select"),
        }
    }

    #[test]
    fn parses_create_table_with_constraints() {
        let stmts = parse(
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0, CHECK (age >= 0))",
        )
        .unwrap();
        match &stmts[0] {
            Statement::CreateTable(c) => {
                assert_eq!(c.columns.len(), 3);
                assert!(c.columns[0].primary_key);
                assert!(c.columns[1].not_null);
                assert!(matches!(c.columns[2].default, Some(Value::Int(0))));
                assert_eq!(c.checks.len(), 1);
            }
            _ => panic!("expected create table"),
        }
    }

    #[test]
    fn parses_insert_with_columns() {
        let stmts = parse("INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y')").unwrap();
        match &stmts[0] {
            Statement::Insert(i) => {
                assert_eq!(i.columns.as_ref().unwrap().len(), 2);
                assert_eq!(i.rows.len(), 2);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_select_with_join_and_order() {
        let stmts = parse(
            "SELECT a.id, b.name FROM a JOIN b ON a.id = b.aid WHERE a.id > 1 ORDER BY a.id DESC LIMIT 5 OFFSET 2",
        )
        .unwrap();
        match &stmts[0] {
            Statement::Select(s) => {
                assert!(matches!(s.from, FromClause::Join(_)));
                assert!(s.where_clause.is_some());
                assert_eq!(s.order_by.as_ref().unwrap().len(), 1);
                assert_eq!(s.limit.as_ref().unwrap().limit, 5);
                assert_eq!(s.limit.as_ref().unwrap().offset, Some(2));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_meta_command() {
        let stmts = parse(".tables").unwrap();
        match &stmts[0] {
            Statement::Meta(m) => assert_eq!(m.name, "tables"),
            _ => panic!(),
        }
    }

    #[test]
    fn parses_is_null() {
        let stmts = parse("SELECT * FROM t WHERE a IS NOT NULL").unwrap();
        match &stmts[0] {
            Statement::Select(s) => {
                assert!(s.where_clause.is_some());
            }
            _ => panic!(),
        }
    }
}
