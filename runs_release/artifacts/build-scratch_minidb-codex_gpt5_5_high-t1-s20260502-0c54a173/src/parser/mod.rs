pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Token, TokenKind, lex};
use crate::parser::ast::*;
use crate::types::{ColumnType, Value};

pub fn parse_sql(sql: &str) -> Result<Vec<Statement>> {
    let tokens = lex(sql)?;
    Parser { tokens, pos: 0 }.parse_statements()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        while !self.at_eof() {
            if self.eat_symbol(';') {
                continue;
            }
            statements.push(self.parse_statement()?);
            let _ = self.eat_symbol(';');
        }
        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        if self.eat_keyword("CREATE") {
            if self.eat_keyword("TABLE") {
                return self.parse_create_table();
            }
            self.expect_keyword("INDEX")?;
            return self.parse_create_index();
        }
        if self.eat_keyword("DROP") {
            if self.eat_keyword("TABLE") {
                return Ok(Statement::DropTable {
                    name: self.expect_ident()?,
                });
            }
            self.expect_keyword("INDEX")?;
            return Ok(Statement::DropIndex {
                name: self.expect_ident()?,
            });
        }
        if self.eat_keyword("INSERT") {
            return self.parse_insert();
        }
        if self.eat_keyword("SELECT") {
            return self.parse_select();
        }
        if self.eat_keyword("UPDATE") {
            return self.parse_update();
        }
        if self.eat_keyword("DELETE") {
            return self.parse_delete();
        }
        if self.eat_keyword("BEGIN") {
            return Ok(Statement::Begin);
        }
        if self.eat_keyword("COMMIT") {
            return Ok(Statement::Commit);
        }
        if self.eat_keyword("ROLLBACK") {
            return Ok(Statement::Rollback);
        }
        self.err("expected statement")
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect_symbol('(')?;
        let mut columns = Vec::new();
        let mut checks = Vec::new();
        loop {
            if self.eat_keyword("CHECK") {
                self.expect_symbol('(')?;
                checks.push(self.parse_expr()?);
                self.expect_symbol(')')?;
            } else {
                columns.push(self.parse_column_def()?);
            }
            if self.eat_symbol(',') {
                continue;
            }
            break;
        }
        self.expect_symbol(')')?;
        Ok(Statement::CreateTable {
            name,
            columns,
            checks,
        })
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let ty = if self.eat_keyword("INT") || self.eat_keyword("BIGINT") {
            ColumnType::Int
        } else if self.eat_keyword("REAL") {
            ColumnType::Real
        } else if self.eat_keyword("TEXT") {
            ColumnType::Text
        } else if self.eat_keyword("BOOL") {
            ColumnType::Bool
        } else {
            return self.err("expected column type");
        };
        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = Value::Null;
        loop {
            if self.eat_keyword("PRIMARY") {
                self.expect_keyword("KEY")?;
                primary_key = true;
                not_null = true;
                unique = true;
            } else if self.eat_keyword("NOT") {
                self.expect_keyword("NULL")?;
                not_null = true;
            } else if self.eat_keyword("UNIQUE") {
                unique = true;
            } else if self.eat_keyword("DEFAULT") {
                default = self.parse_literal()?;
            } else {
                break;
            }
        }
        Ok(ColumnDef {
            name,
            ty,
            primary_key,
            not_null,
            unique,
            default,
        })
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect_keyword("ON")?;
        let table = self.expect_ident()?;
        self.expect_symbol('(')?;
        let column = self.expect_ident()?;
        self.expect_symbol(')')?;
        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_keyword("INTO")?;
        let table = self.expect_ident()?;
        let columns = if self.eat_symbol('(') {
            let cols = self.parse_ident_list()?;
            self.expect_symbol(')')?;
            Some(cols)
        } else {
            None
        };
        self.expect_keyword("VALUES")?;
        self.expect_symbol('(')?;
        let values = self.parse_expr_list(')')?;
        self.expect_symbol(')')?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select(&mut self) -> Result<Statement> {
        let projection = if self.eat_symbol('*') {
            vec![SelectItem::Wildcard]
        } else {
            let mut items = Vec::new();
            loop {
                let expr = self.parse_expr()?;
                let alias = if self.eat_keyword("AS") {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                items.push(SelectItem::Expr { expr, alias });
                if self.eat_symbol(',') {
                    continue;
                }
                break;
            }
            items
        };
        self.expect_keyword("FROM")?;
        let from = self.parse_table_ref()?;
        let selection = if self.eat_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.eat_keyword("ORDER") {
            self.expect_keyword("BY")?;
            loop {
                let expr = self.parse_expr()?;
                let asc = if self.eat_keyword("DESC") {
                    false
                } else {
                    let _ = self.eat_keyword("ASC");
                    true
                };
                order_by.push(OrderBy { expr, asc });
                if self.eat_symbol(',') {
                    continue;
                }
                break;
            }
        }
        let limit = if self.eat_keyword("LIMIT") {
            let n = self.expect_u64()?;
            let offset = if self.eat_keyword("OFFSET") {
                self.expect_u64()?
            } else {
                0
            };
            Some(Limit { limit: n, offset })
        } else {
            None
        };
        Ok(Statement::Select(Select {
            projection,
            from,
            selection,
            order_by,
            limit,
        }))
    }

    fn parse_table_ref(&mut self) -> Result<TableRef> {
        let left = self.expect_ident()?;
        if self.eat_keyword("INNER") {
            self.expect_keyword("JOIN")?;
            let right = self.expect_ident()?;
            self.expect_keyword("ON")?;
            let on = self.parse_expr()?;
            Ok(TableRef::Join { left, right, on })
        } else {
            Ok(TableRef::Table(left))
        }
    }

    fn parse_update(&mut self) -> Result<Statement> {
        let table = self.expect_ident()?;
        self.expect_keyword("SET")?;
        let mut assignments = Vec::new();
        loop {
            let column = self.expect_ident()?;
            self.expect_op("=")?;
            let expr = self.parse_expr()?;
            assignments.push((column, expr));
            if self.eat_symbol(',') {
                continue;
            }
            break;
        }
        let selection = if self.eat_keyword("WHERE") {
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
        self.expect_keyword("FROM")?;
        let table = self.expect_ident()?;
        let selection = if self.eat_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, selection })
    }

    fn parse_ident_list(&mut self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        loop {
            out.push(self.expect_ident()?);
            if self.eat_symbol(',') {
                continue;
            }
            break;
        }
        Ok(out)
    }

    fn parse_expr_list(&mut self, terminator: char) -> Result<Vec<Expr>> {
        let mut out = Vec::new();
        if self.check_symbol(terminator) {
            return Ok(out);
        }
        loop {
            out.push(self.parse_expr()?);
            if self.eat_symbol(',') {
                continue;
            }
            break;
        }
        Ok(out)
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        while self.eat_keyword("OR") {
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
        while self.eat_keyword("AND") {
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
        if self.eat_keyword("NOT") {
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
        if self.eat_keyword("IS") {
            let not = self.eat_keyword("NOT");
            self.expect_keyword("NULL")?;
            expr = Expr::IsNull {
                expr: Box::new(expr),
                not,
            };
        } else if let Some(op) = self.eat_compare_op() {
            let rhs = self.parse_concat()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut expr = self.parse_add()?;
        while self.eat_op("||") {
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
            let op = if self.eat_op("+") {
                Some(BinaryOp::Add)
            } else if self.eat_op("-") {
                Some(BinaryOp::Sub)
            } else {
                None
            };
            if let Some(op) = op {
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
            let op = if self.eat_symbol('*') || self.eat_op("*") {
                Some(BinaryOp::Mul)
            } else if self.eat_op("/") {
                Some(BinaryOp::Div)
            } else {
                None
            };
            if let Some(op) = op {
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
        if self.eat_op("-") {
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        if self.eat_symbol('(') {
            let expr = self.parse_expr()?;
            self.expect_symbol(')')?;
            return Ok(expr);
        }
        if self.eat_keyword("NULL") {
            return Ok(Expr::Literal(Value::Null));
        }
        if self.eat_keyword("TRUE") {
            return Ok(Expr::Literal(Value::Bool(true)));
        }
        if self.eat_keyword("FALSE") {
            return Ok(Expr::Literal(Value::Bool(false)));
        }
        if let Some(value) = self.eat_number()? {
            return Ok(Expr::Literal(value));
        }
        if let Some(value) = self.eat_string() {
            return Ok(Expr::Literal(Value::Text(value)));
        }
        let name = self.expect_ident()?;
        if self.eat_symbol('(') {
            let args = self.parse_expr_list(')')?;
            self.expect_symbol(')')?;
            return Ok(Expr::Function { name, args });
        }
        if self.eat_symbol('.') {
            let col = self.expect_ident()?;
            Ok(Expr::Column {
                table: Some(name),
                name: col,
            })
        } else {
            Ok(Expr::Column { table: None, name })
        }
    }

    fn parse_literal(&mut self) -> Result<Value> {
        match self.parse_unary()? {
            Expr::Literal(v) => Ok(v),
            Expr::Unary {
                op: UnaryOp::Neg,
                expr,
            } => match *expr {
                Expr::Literal(Value::Int(v)) => Ok(Value::Int(-v)),
                Expr::Literal(Value::Real(v)) => Ok(Value::Real(-v)),
                _ => self.err("expected literal"),
            },
            _ => self.err("expected literal"),
        }
    }

    fn eat_compare_op(&mut self) -> Option<BinaryOp> {
        for (text, op) in [
            ("=", BinaryOp::Eq),
            ("<>", BinaryOp::Ne),
            ("<=", BinaryOp::Le),
            (">=", BinaryOp::Ge),
            ("<", BinaryOp::Lt),
            (">", BinaryOp::Gt),
        ] {
            if self.eat_op(text) {
                return Some(op);
            }
        }
        None
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance().kind.clone() {
            TokenKind::Ident(s) => Ok(s),
            TokenKind::Keyword(s) if !is_reserved_for_identifier(&s) => Ok(s.to_ascii_lowercase()),
            other => self.err_at_prev(format!("expected identifier, got {}", other.text())),
        }
    }

    fn expect_u64(&mut self) -> Result<u64> {
        let token = self.advance().clone();
        if let TokenKind::Number(s) = token.kind {
            return s.parse::<u64>().map_err(|_| {
                MiniDbError::parse("expected unsigned integer", token.line, token.column)
            });
        }
        Err(MiniDbError::parse(
            "expected unsigned integer",
            token.line,
            token.column,
        ))
    }

    fn eat_number(&mut self) -> Result<Option<Value>> {
        if let TokenKind::Number(s) = self.peek().kind.clone() {
            self.pos += 1;
            if s.contains('.') {
                let v = s.parse::<f64>().map_err(|_| {
                    MiniDbError::parse(
                        "invalid real literal",
                        self.previous().line,
                        self.previous().column,
                    )
                })?;
                Ok(Some(Value::Real(v)))
            } else {
                let v = s.parse::<i64>().map_err(|_| {
                    MiniDbError::parse(
                        "invalid integer literal",
                        self.previous().line,
                        self.previous().column,
                    )
                })?;
                Ok(Some(Value::Int(v)))
            }
        } else {
            Ok(None)
        }
    }

    fn eat_string(&mut self) -> Option<String> {
        if let TokenKind::String(s) = self.peek().kind.clone() {
            self.pos += 1;
            Some(s)
        } else {
            None
        }
    }

    fn eat_keyword(&mut self, kw: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Keyword(s) if s == kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<()> {
        if self.eat_keyword(kw) {
            Ok(())
        } else {
            self.err(format!("expected keyword {kw}"))
        }
    }

    fn eat_symbol(&mut self, ch: char) -> bool {
        if matches!(&self.peek().kind, TokenKind::Symbol(c) if *c == ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn check_symbol(&self, ch: char) -> bool {
        matches!(&self.peek().kind, TokenKind::Symbol(c) if *c == ch)
    }

    fn expect_symbol(&mut self, ch: char) -> Result<()> {
        if self.eat_symbol(ch) {
            Ok(())
        } else {
            self.err(format!("expected symbol {ch}"))
        }
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Op(s) if s == op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_op(&mut self, op: &str) -> Result<()> {
        if self.eat_op(op) {
            Ok(())
        } else {
            self.err(format!("expected operator {op}"))
        }
    }

    fn advance(&mut self) -> &Token {
        if !self.at_eof() {
            self.pos += 1;
        }
        self.previous()
    }

    fn previous(&self) -> &Token {
        let idx = self.pos.saturating_sub(1);
        &self.tokens[idx]
    }

    fn peek(&self) -> &Token {
        let idx = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[idx]
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T> {
        let token = self.peek();
        Err(MiniDbError::parse(message, token.line, token.column))
    }

    fn err_at_prev<T>(&self, message: impl Into<String>) -> Result<T> {
        let token = self.previous();
        Err(MiniDbError::parse(message, token.line, token.column))
    }
}

fn is_reserved_for_identifier(s: &str) -> bool {
    matches!(
        s,
        "CREATE"
            | "TABLE"
            | "DROP"
            | "INDEX"
            | "ON"
            | "INSERT"
            | "INTO"
            | "VALUES"
            | "SELECT"
            | "FROM"
            | "WHERE"
            | "ORDER"
            | "BY"
            | "LIMIT"
            | "UPDATE"
            | "SET"
            | "DELETE"
            | "BEGIN"
            | "COMMIT"
            | "ROLLBACK"
            | "NULL"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_create_and_select() {
        let stmts = parse_sql("create table users (id int primary key, name text default 'x'); select * from users where id = 1;").unwrap();
        assert_eq!(stmts.len(), 2);
    }
}
