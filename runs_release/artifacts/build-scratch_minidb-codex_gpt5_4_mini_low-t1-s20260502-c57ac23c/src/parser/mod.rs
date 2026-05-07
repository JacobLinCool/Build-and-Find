pub mod ast;

use crate::error::MiniDbError;
use crate::lexer::{Token, TokenKind};
use crate::types::{ColumnType, Value};

use ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    idx: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, idx: 0 }
    }
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.idx].kind
    }
    fn next(&mut self) -> &Token {
        let t = &self.tokens[self.idx];
        self.idx += 1;
        t
    }
    fn eat_keyword(&mut self, kw: &str) -> bool {
        matches!(self.peek(), TokenKind::Keyword(k) if k == kw) && {
            self.idx += 1;
            true
        }
    }
    fn expect_keyword(&mut self, kw: &str) -> Result<(), MiniDbError> {
        if self.eat_keyword(kw) {
            Ok(())
        } else {
            Err(self.err(format!("expected {kw}")))
        }
    }
    fn expect_ident(&mut self) -> Result<String, MiniDbError> {
        match self.next().kind.clone() {
            TokenKind::Ident(s) | TokenKind::Keyword(s) => Ok(s),
            _ => Err(self.err("expected identifier")),
        }
    }
    fn err(&self, msg: impl Into<String>) -> MiniDbError {
        let t = &self.tokens[self.idx.min(self.tokens.len() - 1)];
        MiniDbError::Parse {
            line: t.line,
            column: t.column,
            message: msg.into(),
        }
    }

    pub fn parse_script(&mut self) -> Result<Vec<Statement>, MiniDbError> {
        let mut out = Vec::new();
        while !matches!(self.peek(), TokenKind::EOF) {
            out.push(self.parse_statement()?);
            if matches!(self.peek(), TokenKind::Semi) {
                self.idx += 1;
            } else if !matches!(self.peek(), TokenKind::EOF) {
                return Err(self.err("expected ';'"));
            }
        }
        Ok(out)
    }

    fn parse_statement(&mut self) -> Result<Statement, MiniDbError> {
        match self.peek() {
            TokenKind::Keyword(k) if k == "CREATE" => self.parse_create(),
            TokenKind::Keyword(k) if k == "DROP" => self.parse_drop(),
            TokenKind::Keyword(k) if k == "INSERT" => self.parse_insert(),
            TokenKind::Keyword(k) if k == "SELECT" => self.parse_select(),
            TokenKind::Keyword(k) if k == "UPDATE" => self.parse_update(),
            TokenKind::Keyword(k) if k == "DELETE" => self.parse_delete(),
            TokenKind::Keyword(k) if k == "BEGIN" => {
                self.idx += 1;
                Ok(Statement::Begin)
            }
            TokenKind::Keyword(k) if k == "COMMIT" => {
                self.idx += 1;
                Ok(Statement::Commit)
            }
            TokenKind::Keyword(k) if k == "ROLLBACK" => {
                self.idx += 1;
                Ok(Statement::Rollback)
            }
            _ => Err(self.err("unexpected token")),
        }
    }
    fn parse_drop(&mut self) -> Result<Statement, MiniDbError> {
        self.expect_keyword("DROP")?;
        if self.eat_keyword("TABLE") {
            Ok(Statement::DropTable {
                name: self.expect_ident()?,
            })
        } else if self.eat_keyword("INDEX") {
            Ok(Statement::DropIndex {
                name: self.expect_ident()?,
            })
        } else {
            Err(self.err("expected TABLE or INDEX"))
        }
    }
    fn parse_create(&mut self) -> Result<Statement, MiniDbError> {
        self.expect_keyword("CREATE")?;
        if self.eat_keyword("TABLE") {
            self.parse_create_table()
        } else if self.eat_keyword("INDEX") {
            self.parse_create_index()
        } else {
            Err(self.err("expected TABLE or INDEX"))
        }
    }
    fn parse_create_table(&mut self) -> Result<Statement, MiniDbError> {
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut cols = Vec::new();
        loop {
            if self.eat_keyword("CHECK") {
                self.expect(TokenKind::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                cols.push(TableElement::Check(expr));
            } else {
                let col_name = self.expect_ident()?;
                let ty = match self.next().kind.clone() {
                    TokenKind::Keyword(k)
                        if ["INT", "BIGINT", "REAL", "TEXT", "BOOL"].contains(&k.as_str()) =>
                    {
                        ColumnType::from_keyword(&k)
                    }
                    _ => return Err(self.err("expected column type")),
                };
                let mut c = ColumnDef::new(col_name, ty);
                loop {
                    if self.eat_keyword("PRIMARY") {
                        self.expect_keyword("KEY")?;
                        c.primary_key = true;
                    } else if self.eat_keyword("NOT") {
                        self.expect_keyword("NULL")?;
                        c.not_null = true;
                    } else if self.eat_keyword("UNIQUE") {
                        c.unique = true;
                    } else if self.eat_keyword("DEFAULT") {
                        c.default = Some(self.parse_literal()?);
                    } else {
                        break;
                    }
                }
                cols.push(TableElement::Column(c));
            }
            if self.eat(TokenKind::Comma) {
                continue;
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(Statement::CreateTable {
            name,
            elements: cols,
        })
    }
    fn parse_create_index(&mut self) -> Result<Statement, MiniDbError> {
        let name = self.expect_ident()?;
        self.expect_keyword("ON")?;
        let table = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let column = self.expect_ident()?;
        self.expect(TokenKind::RParen)?;
        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }
    fn parse_insert(&mut self) -> Result<Statement, MiniDbError> {
        self.expect_keyword("INSERT")?;
        self.expect_keyword("INTO")?;
        let table = self.expect_ident()?;
        let columns = if self.eat(TokenKind::LParen) {
            let mut v = Vec::new();
            loop {
                v.push(self.expect_ident()?);
                if self.eat(TokenKind::Comma) {
                    continue;
                }
                break;
            }
            self.expect(TokenKind::RParen)?;
            Some(v)
        } else {
            None
        };
        self.expect_keyword("VALUES")?;
        self.expect(TokenKind::LParen)?;
        let mut values = Vec::new();
        loop {
            values.push(self.parse_expr()?);
            if self.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
        self.expect(TokenKind::RParen)?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }
    fn parse_update(&mut self) -> Result<Statement, MiniDbError> {
        self.expect_keyword("UPDATE")?;
        let table = self.expect_ident()?;
        self.expect_keyword("SET")?;
        let mut assigns = Vec::new();
        loop {
            let c = self.expect_ident()?;
            self.expect(TokenKind::Eq)?;
            let e = self.parse_expr()?;
            assigns.push((c, e));
            if self.eat(TokenKind::Comma) {
                continue;
            }
            break;
        }
        let filter = if self.eat_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments: assigns,
            filter,
        })
    }
    fn parse_delete(&mut self) -> Result<Statement, MiniDbError> {
        self.expect_keyword("DELETE")?;
        self.expect_keyword("FROM")?;
        let table = self.expect_ident()?;
        let filter = if self.eat_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, filter })
    }
    fn parse_select(&mut self) -> Result<Statement, MiniDbError> {
        self.expect_keyword("SELECT")?;
        let mut items = Vec::new();
        if self.eat(TokenKind::Star) {
            items.push(SelectItem::Star);
        } else {
            loop {
                items.push(SelectItem::Expr(self.parse_expr()?));
                if self.eat(TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }
        self.expect_keyword("FROM")?;
        let from = self.expect_ident()?;
        let join = if self.eat_keyword("INNER") {
            self.expect_keyword("JOIN")?;
            let right = self.expect_ident()?;
            self.expect_keyword("ON")?;
            let on = self.parse_expr()?;
            Some(Join { right, on })
        } else {
            None
        };
        let filter = if self.eat_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let order_by = if self.eat_keyword("ORDER") {
            self.expect_keyword("BY")?;
            let expr = self.parse_expr()?;
            let desc = self.eat_keyword("DESC");
            if !desc {
                self.eat_keyword("ASC");
            }
            Some(OrderBy { expr, desc })
        } else {
            None
        };
        let limit = if self.eat_keyword("LIMIT") {
            let n = self.parse_int_literal()?;
            let offset = if self.eat_keyword("OFFSET") {
                Some(self.parse_int_literal()?)
            } else {
                None
            };
            Some((n, offset.unwrap_or(0)))
        } else {
            None
        };
        Ok(Statement::Select {
            items,
            from,
            join,
            filter,
            order_by,
            limit,
        })
    }
    fn parse_expr(&mut self) -> Result<Expr, MiniDbError> {
        self.parse_or()
    }
    fn parse_or(&mut self) -> Result<Expr, MiniDbError> {
        let mut e = self.parse_and()?;
        while self.eat_keyword("OR") {
            e = Expr::Binary {
                left: Box::new(e),
                op: BinaryOp::Or,
                right: Box::new(self.parse_and()?),
            }
        }
        Ok(e)
    }
    fn parse_and(&mut self) -> Result<Expr, MiniDbError> {
        let mut e = self.parse_cmp()?;
        while self.eat_keyword("AND") {
            e = Expr::Binary {
                left: Box::new(e),
                op: BinaryOp::And,
                right: Box::new(self.parse_cmp()?),
            }
        }
        Ok(e)
    }
    fn parse_cmp(&mut self) -> Result<Expr, MiniDbError> {
        let mut e = self.parse_add()?;
        loop {
            e = match self.peek() {
                TokenKind::Eq => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Eq,
                        right: Box::new(self.parse_add()?),
                    }
                }
                TokenKind::Neq => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Neq,
                        right: Box::new(self.parse_add()?),
                    }
                }
                TokenKind::Lt => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Lt,
                        right: Box::new(self.parse_add()?),
                    }
                }
                TokenKind::Lte => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Lte,
                        right: Box::new(self.parse_add()?),
                    }
                }
                TokenKind::Gt => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Gt,
                        right: Box::new(self.parse_add()?),
                    }
                }
                TokenKind::Gte => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Gte,
                        right: Box::new(self.parse_add()?),
                    }
                }
                TokenKind::Keyword(k) if k == "IS" => {
                    self.idx += 1;
                    let neg = self.eat_keyword("NOT");
                    self.expect_keyword("NULL")?;
                    Expr::IsNull {
                        expr: Box::new(e),
                        negated: neg,
                    }
                }
                _ => break,
            };
        }
        Ok(e)
    }
    fn parse_add(&mut self) -> Result<Expr, MiniDbError> {
        let mut e = self.parse_mul()?;
        loop {
            e = match self.peek() {
                TokenKind::Plus => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Add,
                        right: Box::new(self.parse_mul()?),
                    }
                }
                TokenKind::Minus => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Sub,
                        right: Box::new(self.parse_mul()?),
                    }
                }
                TokenKind::PipePipe => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Concat,
                        right: Box::new(self.parse_mul()?),
                    }
                }
                _ => break,
            };
        }
        Ok(e)
    }
    fn parse_mul(&mut self) -> Result<Expr, MiniDbError> {
        let mut e = self.parse_unary()?;
        loop {
            e = match self.peek() {
                TokenKind::Star => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Mul,
                        right: Box::new(self.parse_unary()?),
                    }
                }
                TokenKind::Slash => {
                    self.idx += 1;
                    Expr::Binary {
                        left: Box::new(e),
                        op: BinaryOp::Div,
                        right: Box::new(self.parse_unary()?),
                    }
                }
                _ => break,
            };
        }
        Ok(e)
    }
    fn parse_unary(&mut self) -> Result<Expr, MiniDbError> {
        if self.eat_keyword("NOT") {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_unary()?),
            })
        } else if matches!(self.peek(), TokenKind::Minus) {
            self.idx += 1;
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            })
        } else {
            self.parse_primary()
        }
    }
    fn parse_primary(&mut self) -> Result<Expr, MiniDbError> {
        match self.next().kind.clone() {
            TokenKind::Number(n) => {
                if n.contains('.') {
                    Ok(Expr::Literal(Value::Real(
                        n.parse().map_err(|_| self.err("bad number"))?,
                    )))
                } else {
                    Ok(Expr::Literal(Value::Int(
                        n.parse().map_err(|_| self.err("bad number"))?,
                    )))
                }
            }
            TokenKind::String(s) => Ok(Expr::Literal(Value::Text(s))),
            TokenKind::Keyword(k) if k == "NULL" => Ok(Expr::Literal(Value::Null)),
            TokenKind::Keyword(k)
                if ["LOWER", "UPPER", "LENGTH", "SUBSTR", "ABS", "COALESCE"]
                    .contains(&k.as_str()) =>
            {
                self.expect(TokenKind::LParen)?;
                let mut args = Vec::new();
                if !matches!(self.peek(), TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.eat(TokenKind::Comma) {
                            continue;
                        }
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
                Ok(Expr::Func { name: k, args })
            }
            TokenKind::Ident(s) | TokenKind::Keyword(s) => {
                if self.eat(TokenKind::Dot) {
                    let col = self.expect_ident()?;
                    Ok(Expr::Column {
                        table: Some(s),
                        name: col,
                    })
                } else {
                    Ok(Expr::Column {
                        table: None,
                        name: s,
                    })
                }
            }
            TokenKind::Star => Ok(Expr::Wildcard),
            TokenKind::LParen => {
                let e = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(e)
            }
            _ => Err(self.err("unexpected expression")),
        }
    }
    fn parse_literal(&mut self) -> Result<Value, MiniDbError> {
        match self.next().kind.clone() {
            TokenKind::Number(n) if n.contains('.') => {
                Ok(Value::Real(n.parse().map_err(|_| self.err("bad number"))?))
            }
            TokenKind::Number(n) => Ok(Value::Int(n.parse().map_err(|_| self.err("bad number"))?)),
            TokenKind::String(s) => Ok(Value::Text(s)),
            TokenKind::Keyword(k) if k == "NULL" => Ok(Value::Null),
            TokenKind::Keyword(k) if k == "BOOL" => Ok(Value::Bool(true)),
            _ => Err(self.err("expected literal")),
        }
    }
    fn parse_int_literal(&mut self) -> Result<usize, MiniDbError> {
        match self.next().kind.clone() {
            TokenKind::Number(n) => n.parse().map_err(|_| self.err("expected integer")),
            _ => Err(self.err("expected integer")),
        }
    }
    fn eat(&mut self, kind: TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(&kind) {
            self.idx += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: TokenKind) -> Result<(), MiniDbError> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(self.err("unexpected token"))
        }
    }
}

impl ColumnType {
    fn from_keyword(k: &str) -> Self {
        match k {
            "INT" | "BIGINT" => ColumnType::Int,
            "REAL" => ColumnType::Real,
            "TEXT" => ColumnType::Text,
            "BOOL" => ColumnType::Bool,
            _ => ColumnType::Text,
        }
    }
}
