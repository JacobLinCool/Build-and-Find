pub mod ast;

use ast::*;

use crate::{
    catalog::{Column, TableSchema},
    error::{MiniDbError, Result},
    lexer::{Lexer, Token, TokenKind},
    types::{ColumnType, Value},
};

pub fn parse_script(input: &str) -> Result<Vec<Statement>> {
    let tokens = Lexer::new(input).tokenize()?;
    Parser { tokens, pos: 0 }.parse_all()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse_all(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while !self.eof() {
            if self.consume_symbol(';') {
                continue;
            }
            stmts.push(self.statement()?);
            self.consume_symbol(';');
        }
        Ok(stmts)
    }

    fn statement(&mut self) -> Result<Statement> {
        if let TokenKind::DotCommand(cmd) = self.peek().kind.clone() {
            self.pos += 1;
            let mut parts = cmd.split_whitespace();
            return match parts.next().map_or("", |p| p) {
                ".schema" => Ok(Statement::Meta(MetaCommand::Schema)),
                ".tables" => Ok(Statement::Meta(MetaCommand::Tables)),
                ".indexes" => Ok(Statement::Meta(MetaCommand::Indexes)),
                ".exit" => Ok(Statement::Meta(MetaCommand::Exit)),
                ".format" => Ok(Statement::Meta(MetaCommand::Format(
                    parts.next().map_or("table", |p| p).to_string(),
                ))),
                other => self.err(format!("unknown meta-command {other}")),
            };
        }
        let kw = self.expect_ident()?;
        match kw.as_str() {
            "create" => self.parse_create(),
            "drop" => self.parse_drop(),
            "insert" => self.parse_insert(),
            "select" => self.parse_select().map(Statement::Select),
            "update" => self.parse_update(),
            "delete" => self.parse_delete(),
            "begin" => Ok(Statement::Begin),
            "commit" => Ok(Statement::Commit),
            "rollback" => Ok(Statement::Rollback),
            _ => self.err(format!("unexpected statement '{kw}'")),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        if self.consume_kw("table") {
            let name = self.expect_ident()?;
            self.expect_symbol('(')?;
            let mut columns = Vec::new();
            let mut checks = Vec::new();
            while !self.consume_symbol(')') {
                if self.consume_kw("check") {
                    self.expect_symbol('(')?;
                    checks.push(self.expr()?);
                    self.expect_symbol(')')?;
                } else {
                    let cname = self.expect_ident()?;
                    let ty = match self.expect_ident()?.as_str() {
                        "int" | "bigint" => ColumnType::Int,
                        "real" => ColumnType::Real,
                        "text" => ColumnType::Text,
                        "bool" => ColumnType::Bool,
                        t => return self.err(format!("unknown type '{t}'")),
                    };
                    let mut col = Column {
                        name: cname,
                        ty,
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    };
                    loop {
                        if self.consume_kw("primary") {
                            self.expect_kw("key")?;
                            col.primary_key = true;
                            col.not_null = true;
                            col.unique = true;
                        } else if self.consume_kw("not") {
                            self.expect_kw("null")?;
                            col.not_null = true;
                        } else if self.consume_kw("unique") {
                            col.unique = true;
                        } else if self.consume_kw("default") {
                            col.default = Some(self.literal_expr()?);
                        } else {
                            break;
                        }
                    }
                    columns.push(col);
                }
                if !self.consume_symbol(',') {
                    self.expect_symbol(')')?;
                    break;
                }
            }
            Ok(Statement::CreateTable(TableSchema {
                name,
                columns,
                checks,
            }))
        } else if self.consume_kw("index") {
            let name = self.expect_ident()?;
            self.expect_kw("on")?;
            let table = self.expect_ident()?;
            self.expect_symbol('(')?;
            let column = self.expect_ident()?;
            self.expect_symbol(')')?;
            Ok(Statement::CreateIndex {
                name,
                table,
                column,
            })
        } else {
            self.err("expected TABLE or INDEX")
        }
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        if self.consume_kw("table") {
            Ok(Statement::DropTable(self.expect_ident()?))
        } else if self.consume_kw("index") {
            Ok(Statement::DropIndex(self.expect_ident()?))
        } else {
            self.err("expected TABLE or INDEX")
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_kw("into")?;
        let table = self.expect_ident()?;
        let columns = if self.consume_symbol('(') {
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if self.consume_symbol(')') {
                    break;
                }
                self.expect_symbol(',')?;
            }
            Some(cols)
        } else {
            None
        };
        self.expect_kw("values")?;
        self.expect_symbol('(')?;
        let mut values = Vec::new();
        loop {
            values.push(self.expr()?);
            if self.consume_symbol(')') {
                break;
            }
            self.expect_symbol(',')?;
        }
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select(&mut self) -> Result<Select> {
        let mut projection = Vec::new();
        loop {
            if self.consume_symbol('*') {
                projection.push(SelectItem::Star);
            } else {
                projection.push(SelectItem::Expr {
                    expr: self.expr()?,
                    alias: None,
                });
            }
            if !self.consume_symbol(',') {
                break;
            }
        }
        self.expect_kw("from")?;
        let left = self.expect_ident()?;
        let from = if self.consume_kw("inner") || self.consume_kw("join") {
            if !matches!(self.prev_ident().as_deref(), Some("join")) {
                self.expect_kw("join")?;
            }
            let right = self.expect_ident()?;
            self.expect_kw("on")?;
            From::Join {
                left,
                right,
                on: self.expr()?,
            }
        } else {
            From::Table(left)
        };
        let where_clause = if self.consume_kw("where") {
            Some(self.expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.consume_kw("order") {
            self.expect_kw("by")?;
            loop {
                let expr = self.expr()?;
                let desc = if self.consume_kw("desc") {
                    true
                } else {
                    self.consume_kw("asc");
                    false
                };
                order_by.push(OrderBy { expr, desc });
                if !self.consume_symbol(',') {
                    break;
                }
            }
        }
        let limit = if self.consume_kw("limit") {
            Some(self.expect_usize()?)
        } else {
            None
        };
        let offset = if self.consume_kw("offset") {
            Some(self.expect_usize()?)
        } else {
            None
        };
        Ok(Select {
            projection,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        let table = self.expect_ident()?;
        self.expect_kw("set")?;
        let mut assigns = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect_op("=")?;
            assigns.push((col, self.expr()?));
            if !self.consume_symbol(',') {
                break;
            }
        }
        let where_clause = if self.consume_kw("where") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assigns,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_kw("from")?;
        let table = self.expect_ident()?;
        let where_clause = if self.consume_kw("where") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Statement::Delete {
            table,
            where_clause,
        })
    }

    fn expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }
    fn parse_or(&mut self) -> Result<Expr> {
        self.bin_kw(Self::parse_and, "or", BinaryOp::Or)
    }
    fn parse_and(&mut self) -> Result<Expr> {
        self.bin_kw(Self::parse_cmp, "and", BinaryOp::And)
    }
    fn parse_cmp(&mut self) -> Result<Expr> {
        let mut e = self.parse_concat()?;
        if self.consume_kw("is") {
            let negated = self.consume_kw("not");
            self.expect_kw("null")?;
            e = Expr::IsNull {
                expr: Box::new(e),
                negated,
            };
        } else if let Some(op) = self.consume_cmp() {
            e = Expr::Binary {
                left: Box::new(e),
                op,
                right: Box::new(self.parse_concat()?),
            };
        }
        Ok(e)
    }
    fn parse_concat(&mut self) -> Result<Expr> {
        self.bin_op(Self::parse_add, "||", BinaryOp::Concat)
    }
    fn parse_add(&mut self) -> Result<Expr> {
        let mut e = self.parse_mul()?;
        loop {
            let op = if self.consume_op("+") {
                BinaryOp::Add
            } else if self.consume_op("-") {
                BinaryOp::Sub
            } else {
                break;
            };
            e = Expr::Binary {
                left: Box::new(e),
                op,
                right: Box::new(self.parse_mul()?),
            };
        }
        Ok(e)
    }
    fn parse_mul(&mut self) -> Result<Expr> {
        let mut e = self.parse_unary()?;
        loop {
            let op = if self.consume_symbol('*') {
                BinaryOp::Mul
            } else if self.consume_op("/") {
                BinaryOp::Div
            } else {
                break;
            };
            e = Expr::Binary {
                left: Box::new(e),
                op,
                right: Box::new(self.parse_unary()?),
            };
        }
        Ok(e)
    }
    fn parse_unary(&mut self) -> Result<Expr> {
        if self.consume_kw("not") {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_unary()?),
            })
        } else if self.consume_op("-") {
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            })
        } else {
            self.primary()
        }
    }
    fn primary(&mut self) -> Result<Expr> {
        if self.consume_symbol('(') {
            let e = self.expr()?;
            self.expect_symbol(')')?;
            return Ok(e);
        }
        let tok = self.next().clone();
        match tok.kind {
            TokenKind::Number(n) if n.contains('.') => {
                Ok(Expr::Literal(Value::Real(n.parse().map_err(|_| {
                    MiniDbError::parse("bad real", tok.line, tok.column)
                })?)))
            }
            TokenKind::Number(n) => {
                Ok(Expr::Literal(Value::Int(n.parse().map_err(|_| {
                    MiniDbError::parse("bad int", tok.line, tok.column)
                })?)))
            }
            TokenKind::String(s) => Ok(Expr::Literal(Value::Text(s))),
            TokenKind::Ident(s) => {
                let l = s.to_ascii_lowercase();
                if l == "null" {
                    return Ok(Expr::Literal(Value::Null));
                }
                if l == "true" || l == "false" {
                    return Ok(Expr::Literal(Value::Bool(l == "true")));
                }
                if self.consume_symbol('(') {
                    let mut args = Vec::new();
                    if !self.consume_symbol(')') {
                        loop {
                            args.push(self.expr()?);
                            if self.consume_symbol(')') {
                                break;
                            }
                            self.expect_symbol(',')?;
                        }
                    }
                    Ok(Expr::Function { name: l, args })
                } else if self.consume_symbol('.') {
                    Ok(Expr::Column {
                        table: Some(l),
                        name: self.expect_ident()?,
                    })
                } else {
                    Ok(Expr::Column {
                        table: None,
                        name: l,
                    })
                }
            }
            _ => self.err_at("expected expression", tok.line, tok.column),
        }
    }

    fn literal_expr(&mut self) -> Result<Value> {
        match self.expr()? {
            Expr::Literal(v) => Ok(v),
            _ => Err(MiniDbError::Sql("DEFAULT must be a literal".into())),
        }
    }
    fn bin_kw(&mut self, f: fn(&mut Self) -> Result<Expr>, kw: &str, op: BinaryOp) -> Result<Expr> {
        let mut e = f(self)?;
        while self.consume_kw(kw) {
            e = Expr::Binary {
                left: Box::new(e),
                op: op.clone(),
                right: Box::new(f(self)?),
            };
        }
        Ok(e)
    }
    fn bin_op(
        &mut self,
        f: fn(&mut Self) -> Result<Expr>,
        sym: &str,
        op: BinaryOp,
    ) -> Result<Expr> {
        let mut e = f(self)?;
        while self.consume_op(sym) {
            e = Expr::Binary {
                left: Box::new(e),
                op: op.clone(),
                right: Box::new(f(self)?),
            };
        }
        Ok(e)
    }
    fn consume_cmp(&mut self) -> Option<BinaryOp> {
        for (s, op) in [
            ("=", BinaryOp::Eq),
            ("<>", BinaryOp::Ne),
            ("<=", BinaryOp::Le),
            (">=", BinaryOp::Ge),
            ("<", BinaryOp::Lt),
            (">", BinaryOp::Gt),
        ] {
            if self.consume_op(s) {
                return Some(op);
            }
        }
        None
    }
    fn eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn next(&mut self) -> &Token {
        let p = self.pos;
        self.pos += 1;
        &self.tokens[p]
    }
    fn prev_ident(&self) -> Option<String> {
        match self.tokens.get(self.pos.saturating_sub(1)).map(|t| &t.kind) {
            Some(TokenKind::Ident(s)) => Some(s.to_ascii_lowercase()),
            _ => None,
        }
    }
    fn consume_kw(&mut self, kw: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(kw)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, kw: &str) -> Result<()> {
        if self.consume_kw(kw) {
            Ok(())
        } else {
            self.err(format!("expected {kw}"))
        }
    }
    fn expect_ident(&mut self) -> Result<String> {
        let tok = self.next().clone();
        match tok.kind {
            TokenKind::Ident(s) => Ok(s.to_ascii_lowercase()),
            _ => self.err_at("expected identifier", tok.line, tok.column),
        }
    }
    fn expect_usize(&mut self) -> Result<usize> {
        let tok = self.next().clone();
        match tok.kind {
            TokenKind::Number(s) => s
                .parse()
                .map_err(|_| MiniDbError::parse("expected integer", tok.line, tok.column)),
            _ => self.err_at("expected integer", tok.line, tok.column),
        }
    }
    fn consume_symbol(&mut self, c: char) -> bool {
        if self.peek().kind == TokenKind::Symbol(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_symbol(&mut self, c: char) -> Result<()> {
        if self.consume_symbol(c) {
            Ok(())
        } else {
            self.err(format!("expected '{c}'"))
        }
    }
    fn consume_op(&mut self, op: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Op(s) if s == op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_op(&mut self, op: &str) -> Result<()> {
        if self.consume_op(op) {
            Ok(())
        } else {
            self.err(format!("expected '{op}'"))
        }
    }
    fn err<T>(&self, msg: impl Into<String>) -> Result<T> {
        self.err_at(msg, self.peek().line, self.peek().column)
    }
    fn err_at<T>(&self, msg: impl Into<String>, line: usize, column: usize) -> Result<T> {
        Err(MiniDbError::parse(msg, line, column))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_create_and_select() {
        let s = parse_script("create table t (id int primary key, name text default 'x'); select * from t where id = 1;").unwrap();
        assert_eq!(s.len(), 2);
    }
}
