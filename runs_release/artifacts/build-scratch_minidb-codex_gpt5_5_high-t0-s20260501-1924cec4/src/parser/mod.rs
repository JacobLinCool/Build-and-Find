pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::parser::ast::*;
use crate::types::{DataType, Value};

pub fn parse_sql(sql: &str) -> Result<Vec<Statement>> {
    let tokens = Lexer::new(sql).tokenize()?;
    Parser::new(tokens).parse_statements()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        self.skip_semicolons();
        while !self.is_eof() {
            statements.push(self.parse_statement()?);
            self.skip_semicolons();
        }
        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        if self.consume_symbol('.') {
            return self.parse_meta();
        }
        if self.consume_keyword("CREATE") {
            if self.consume_keyword("TABLE") {
                self.parse_create_table()
            } else if self.consume_keyword("INDEX") {
                self.parse_create_index()
            } else {
                self.err_here("expected TABLE or INDEX after CREATE")
            }
        } else if self.consume_keyword("DROP") {
            if self.consume_keyword("TABLE") {
                Ok(Statement::DropTable {
                    name: self.parse_name()?,
                })
            } else if self.consume_keyword("INDEX") {
                Ok(Statement::DropIndex {
                    name: self.parse_name()?,
                })
            } else {
                self.err_here("expected TABLE or INDEX after DROP")
            }
        } else if self.consume_keyword("INSERT") {
            self.parse_insert()
        } else if self.consume_keyword("SELECT") {
            self.parse_select().map(Statement::Select)
        } else if self.consume_keyword("UPDATE") {
            self.parse_update()
        } else if self.consume_keyword("DELETE") {
            self.parse_delete()
        } else if self.consume_keyword("BEGIN") {
            Ok(Statement::Begin)
        } else if self.consume_keyword("COMMIT") {
            Ok(Statement::Commit)
        } else if self.consume_keyword("ROLLBACK") {
            Ok(Statement::Rollback)
        } else {
            self.err_here("expected SQL statement")
        }
    }

    fn parse_meta(&mut self) -> Result<Statement> {
        let name = self.parse_name()?.to_ascii_lowercase();
        match name.as_str() {
            "schema" => Ok(Statement::Meta(MetaCommand::Schema)),
            "tables" => Ok(Statement::Meta(MetaCommand::Tables)),
            "indexes" => Ok(Statement::Meta(MetaCommand::Indexes)),
            "exit" | "quit" => Ok(Statement::Meta(MetaCommand::Exit)),
            "format" => {
                let kind = self.parse_name()?.to_ascii_lowercase();
                Ok(Statement::Meta(MetaCommand::Format(kind)))
            }
            _ => self.err_here("unknown meta-command"),
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        let name = self.parse_name()?;
        self.expect_symbol('(')?;
        let mut columns = Vec::new();
        let mut checks = Vec::new();
        loop {
            if self.consume_keyword("CHECK") {
                self.expect_symbol('(')?;
                let expr = self.parse_expr()?;
                self.expect_symbol(')')?;
                checks.push(expr);
            } else {
                columns.push(self.parse_column_def()?);
            }
            if self.consume_symbol(',') {
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
        let name = self.parse_name()?;
        let data_type = self.parse_type()?;
        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;
        loop {
            if self.consume_keyword("PRIMARY") {
                self.expect_keyword("KEY")?;
                primary_key = true;
                not_null = true;
                unique = true;
            } else if self.consume_keyword("NOT") {
                self.expect_keyword("NULL")?;
                not_null = true;
            } else if self.consume_keyword("UNIQUE") {
                unique = true;
            } else if self.consume_keyword("DEFAULT") {
                default = Some(self.parse_expr()?);
            } else {
                break;
            }
        }
        Ok(ColumnDef {
            name,
            data_type,
            primary_key,
            not_null,
            unique,
            default,
        })
    }

    fn parse_type(&mut self) -> Result<DataType> {
        if self.consume_keyword("INT") || self.consume_keyword("BIGINT") {
            Ok(DataType::Int)
        } else if self.consume_keyword("REAL") {
            Ok(DataType::Real)
        } else if self.consume_keyword("TEXT") {
            Ok(DataType::Text)
        } else if self.consume_keyword("BOOL") {
            Ok(DataType::Bool)
        } else {
            self.err_here("expected column type")
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        let name = self.parse_name()?;
        self.expect_keyword("ON")?;
        let table = self.parse_name()?;
        self.expect_symbol('(')?;
        let column = self.parse_name()?;
        self.expect_symbol(')')?;
        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_keyword("INTO")?;
        let table = self.parse_name()?;
        let columns = if self.consume_symbol('(') {
            let mut cols = Vec::new();
            loop {
                cols.push(self.parse_name()?);
                if self.consume_symbol(',') {
                    continue;
                }
                break;
            }
            self.expect_symbol(')')?;
            Some(cols)
        } else {
            None
        };
        self.expect_keyword("VALUES")?;
        self.expect_symbol('(')?;
        let values = self.parse_expr_list(')')?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select(&mut self) -> Result<SelectStmt> {
        let mut projection = Vec::new();
        loop {
            if self.consume_operator("*") {
                projection.push(SelectItem::Wildcard);
            } else {
                let expr = self.parse_expr()?;
                let alias = if self.consume_keyword("AS") {
                    Some(self.parse_name()?)
                } else {
                    None
                };
                projection.push(SelectItem::Expr { expr, alias });
            }
            if self.consume_symbol(',') {
                continue;
            }
            break;
        }
        self.expect_keyword("FROM")?;
        let from = self.parse_from()?;
        let where_clause = if self.consume_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.consume_keyword("ORDER") {
            self.expect_keyword("BY")?;
            loop {
                let expr = self.parse_expr()?;
                let asc = if self.consume_keyword("DESC") {
                    false
                } else {
                    self.consume_keyword("ASC");
                    true
                };
                order_by.push(OrderBy { expr, asc });
                if self.consume_symbol(',') {
                    continue;
                }
                break;
            }
        }
        let limit = if self.consume_keyword("LIMIT") {
            Some(self.parse_usize()?)
        } else {
            None
        };
        let offset = if self.consume_keyword("OFFSET") {
            Some(self.parse_usize()?)
        } else {
            None
        };
        Ok(SelectStmt {
            projection,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_from(&mut self) -> Result<From> {
        let left = self.parse_name()?;
        let saw_inner = if self.consume_keyword("INNER") {
            self.expect_keyword("JOIN")?;
            true
        } else {
            false
        };
        if saw_inner || self.consume_keyword("JOIN") {
            let right = self.parse_name()?;
            self.expect_keyword("ON")?;
            let on = self.parse_expr()?;
            Ok(From::Join { left, right, on })
        } else {
            Ok(From::Table { name: left })
        }
    }

    fn parse_update(&mut self) -> Result<Statement> {
        let table = self.parse_name()?;
        self.expect_keyword("SET")?;
        let mut assignments = Vec::new();
        loop {
            let column = self.parse_name()?;
            self.expect_operator("=")?;
            let expr = self.parse_expr()?;
            assignments.push((column, expr));
            if self.consume_symbol(',') {
                continue;
            }
            break;
        }
        let where_clause = if self.consume_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_keyword("FROM")?;
        let table = self.parse_name()?;
        let where_clause = if self.consume_keyword("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete {
            table,
            where_clause,
        })
    }

    fn parse_expr_list(&mut self, end: char) -> Result<Vec<Expr>> {
        let mut exprs = Vec::new();
        if self.consume_symbol(end) {
            return Ok(exprs);
        }
        loop {
            exprs.push(self.parse_expr()?);
            if self.consume_symbol(',') {
                continue;
            }
            self.expect_symbol(end)?;
            break;
        }
        Ok(exprs)
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        while self.consume_keyword("OR") {
            let right = self.parse_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut expr = self.parse_not()?;
        while self.consume_keyword("AND") {
            let right = self.parse_not()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.consume_keyword("NOT") {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_not()?),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut expr = self.parse_concat()?;
        if self.consume_keyword("IS") {
            let negated = self.consume_keyword("NOT");
            self.expect_keyword("NULL")?;
            expr = Expr::IsNull {
                expr: Box::new(expr),
                negated,
            };
        } else if let Some(op) = self.consume_comparison_operator() {
            let right = self.parse_concat()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut expr = self.parse_add()?;
        while self.consume_operator("||") {
            let right = self.parse_add()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Concat,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut expr = self.parse_mul()?;
        loop {
            let op = if self.consume_operator("+") {
                Some(BinaryOp::Add)
            } else if self.consume_operator("-") {
                Some(BinaryOp::Sub)
            } else {
                None
            };
            if let Some(op) = op {
                let right = self.parse_mul()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
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
            let op = if self.consume_operator("*") {
                Some(BinaryOp::Mul)
            } else if self.consume_operator("/") {
                Some(BinaryOp::Div)
            } else {
                None
            };
            if let Some(op) = op {
                let right = self.parse_unary()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.consume_operator("-") {
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            })
        } else if self.consume_operator("+") {
            Ok(Expr::Unary {
                op: UnaryOp::Pos,
                expr: Box::new(self.parse_unary()?),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Number(n) => {
                self.pos += 1;
                if n.contains('.') {
                    let value = n.parse::<f64>().map_err(|_| {
                        MiniDbError::parse("invalid real literal", token.line, token.column)
                    })?;
                    Ok(Expr::Literal(Value::Real(value)))
                } else {
                    let value = n.parse::<i64>().map_err(|_| {
                        MiniDbError::parse("invalid integer literal", token.line, token.column)
                    })?;
                    Ok(Expr::Literal(Value::Int(value)))
                }
            }
            TokenKind::String(s) => {
                self.pos += 1;
                Ok(Expr::Literal(Value::Text(s)))
            }
            TokenKind::Keyword(k) if k == "NULL" => {
                self.pos += 1;
                Ok(Expr::Literal(Value::Null))
            }
            TokenKind::Keyword(k) if k == "TRUE" || k == "FALSE" => {
                self.pos += 1;
                Ok(Expr::Literal(Value::Bool(k == "TRUE")))
            }
            TokenKind::Ident(_) | TokenKind::Keyword(_) => {
                let mut parts = vec![self.parse_name()?];
                while self.consume_symbol('.') {
                    parts.push(self.parse_name()?);
                }
                if self.consume_symbol('(') {
                    let args = self.parse_expr_list(')')?;
                    let name = parts.join(".").to_ascii_uppercase();
                    Ok(Expr::Function { name, args })
                } else {
                    Ok(Expr::Identifier(parts))
                }
            }
            TokenKind::Symbol('(') => {
                self.pos += 1;
                let expr = self.parse_expr()?;
                self.expect_symbol(')')?;
                Ok(expr)
            }
            _ => self.err_here("expected expression"),
        }
    }

    fn parse_usize(&mut self) -> Result<usize> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Number(n) => {
                self.pos += 1;
                n.parse::<usize>().map_err(|_| {
                    MiniDbError::parse("expected non-negative integer", token.line, token.column)
                })
            }
            _ => self.err_here("expected non-negative integer"),
        }
    }

    fn parse_name(&mut self) -> Result<String> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Ident(v) => {
                self.pos += 1;
                Ok(v)
            }
            TokenKind::Keyword(v) => {
                self.pos += 1;
                Ok(v.to_ascii_lowercase())
            }
            _ => self.err_here("expected identifier"),
        }
    }

    fn consume_comparison_operator(&mut self) -> Option<BinaryOp> {
        for (text, op) in [
            ("=", BinaryOp::Eq),
            ("<>", BinaryOp::Ne),
            ("!=", BinaryOp::Ne),
            ("<=", BinaryOp::Le),
            (">=", BinaryOp::Ge),
            ("<", BinaryOp::Lt),
            (">", BinaryOp::Gt),
        ] {
            if self.consume_operator(text) {
                return Some(op);
            }
        }
        None
    }

    fn consume_keyword(&mut self, expected: &str) -> bool {
        match &self.current().kind {
            TokenKind::Keyword(k) if k == expected => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<()> {
        if self.consume_keyword(expected) {
            Ok(())
        } else {
            self.err_here(format!("expected {expected}"))
        }
    }

    fn consume_symbol(&mut self, expected: char) -> bool {
        match &self.current().kind {
            TokenKind::Symbol(ch) if *ch == expected => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn expect_symbol(&mut self, expected: char) -> Result<()> {
        if self.consume_symbol(expected) {
            Ok(())
        } else {
            self.err_here(format!("expected {expected}"))
        }
    }

    fn consume_operator(&mut self, expected: &str) -> bool {
        match &self.current().kind {
            TokenKind::Operator(op) if op == expected => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn expect_operator(&mut self, expected: &str) -> Result<()> {
        if self.consume_operator(expected) {
            Ok(())
        } else {
            self.err_here(format!("expected {expected}"))
        }
    }

    fn skip_semicolons(&mut self) {
        while self.consume_symbol(';') {}
    }

    fn current(&self) -> &Token {
        let idx = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[idx]
    }

    fn is_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn err_here<T>(&self, message: impl Into<String>) -> Result<T> {
        let token = self.current();
        Err(MiniDbError::parse(message, token.line, token.column))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_create_and_insert() {
        let stmts = parse_sql(
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT DEFAULT 'x'); INSERT INTO t VALUES (1, 'a');",
        )
        .unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn parses_select_join_order_limit() {
        let stmts = parse_sql(
            "SELECT t.id, UPPER(name) FROM t INNER JOIN u ON t.id = u.id WHERE id >= 1 ORDER BY name DESC LIMIT 2 OFFSET 1;",
        )
        .unwrap();
        assert!(matches!(&stmts[0], Statement::Select(_)));
    }
}
