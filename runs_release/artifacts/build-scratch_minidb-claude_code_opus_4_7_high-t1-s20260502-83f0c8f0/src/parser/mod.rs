//! SQL parser. Consumes tokens emitted by the lexer and produces an AST.
//!
//! Designed as a recursive-descent / Pratt hybrid: statements have their own
//! parsers, and expressions are parsed via operator precedence climbing.

pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{LocatedToken, Token};
use crate::types::{ColumnDef, DataType, Value};
use ast::*;

/// Parse a single statement string. Returns the AST or an error.
pub fn parse_statement(sql: &str) -> Result<Statement> {
    let mut p = Parser::from_sql(sql)?;
    let stmt = p.parse_statement()?;
    p.expect_terminator()?;
    Ok(stmt)
}

/// Parse a sequence of semicolon-separated statements.
pub fn parse_program(sql: &str) -> Result<Vec<Statement>> {
    let mut p = Parser::from_sql(sql)?;
    let mut out = Vec::new();
    loop {
        // skip leading semicolons / EOF
        while matches!(p.peek().token, Token::Semicolon) {
            p.bump();
        }
        if matches!(p.peek().token, Token::Eof) {
            break;
        }
        let stmt = p.parse_statement()?;
        out.push(stmt);
        match p.peek().token {
            Token::Semicolon => {
                p.bump();
            }
            Token::Eof => break,
            _ => {
                let lt = p.peek().clone();
                return Err(MiniDbError::parse(
                    format!("expected ';' or end of input, got {:?}", lt.token),
                    lt.line,
                    lt.column,
                ));
            }
        }
    }
    Ok(out)
}

pub struct Parser {
    tokens: Vec<LocatedToken>,
    pos: usize,
}

impl Parser {
    pub fn from_sql(sql: &str) -> Result<Self> {
        let tokens = crate::lexer::Lexer::new(sql).tokenize_all()?;
        Ok(Parser { tokens, pos: 0 })
    }

    fn peek(&self) -> &LocatedToken {
        &self.tokens[self.pos]
    }

    fn peek_at(&self, offset: usize) -> Option<&LocatedToken> {
        self.tokens.get(self.pos + offset)
    }

    fn bump(&mut self) -> LocatedToken {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, t: Token, what: &str) -> Result<LocatedToken> {
        if self.peek().token == t {
            Ok(self.bump())
        } else {
            let cur = self.peek().clone();
            Err(MiniDbError::parse(
                format!("expected {}, got {:?}", what, cur.token),
                cur.line,
                cur.column,
            ))
        }
    }

    fn expect_terminator(&mut self) -> Result<()> {
        match self.peek().token {
            Token::Semicolon | Token::Eof => Ok(()),
            _ => {
                let cur = self.peek().clone();
                Err(MiniDbError::parse(
                    format!("expected end of statement, got {:?}", cur.token),
                    cur.line,
                    cur.column,
                ))
            }
        }
    }

    fn expect_identifier(&mut self, what: &str) -> Result<String> {
        let cur = self.bump();
        match cur.token {
            Token::Identifier(s) => Ok(s),
            other => Err(MiniDbError::parse(
                format!("expected {} (identifier), got {:?}", what, other),
                cur.line,
                cur.column,
            )),
        }
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        let cur = self.peek().clone();
        match cur.token {
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Insert => self.parse_insert(),
            Token::Select => self.parse_select_statement(),
            Token::Update => self.parse_update(),
            Token::Delete => self.parse_delete(),
            Token::Begin => {
                self.bump();
                Ok(Statement::Begin)
            }
            Token::Commit => {
                self.bump();
                Ok(Statement::Commit)
            }
            Token::Rollback => {
                self.bump();
                Ok(Statement::Rollback)
            }
            other => Err(MiniDbError::parse(
                format!("unexpected token at start of statement: {:?}", other),
                cur.line,
                cur.column,
            )),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect(Token::Create, "CREATE")?;
        match self.peek().token {
            Token::Table => self.parse_create_table(),
            Token::Index => self.parse_create_index(),
            _ => {
                let cur = self.peek().clone();
                Err(MiniDbError::parse(
                    format!("expected TABLE or INDEX, got {:?}", cur.token),
                    cur.line,
                    cur.column,
                ))
            }
        }
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect(Token::Drop, "DROP")?;
        match self.peek().token {
            Token::Table => {
                self.bump();
                let if_exists = self.consume_if_exists();
                let name = self.expect_identifier("table name")?;
                Ok(Statement::DropTable(DropTable { name, if_exists }))
            }
            Token::Index => {
                self.bump();
                let if_exists = self.consume_if_exists();
                let name = self.expect_identifier("index name")?;
                Ok(Statement::DropIndex(DropIndex { name, if_exists }))
            }
            _ => {
                let cur = self.peek().clone();
                Err(MiniDbError::parse(
                    format!("expected TABLE or INDEX, got {:?}", cur.token),
                    cur.line,
                    cur.column,
                ))
            }
        }
    }

    fn consume_if_exists(&mut self) -> bool {
        if let Token::Identifier(s) = &self.peek().token {
            if s.eq_ignore_ascii_case("IF") {
                let saved = self.pos;
                self.bump();
                if self.peek().token == Token::Not {
                    // IF NOT EXISTS pattern is for create, not drop. Backtrack.
                    self.pos = saved;
                    return false;
                }
                if let Token::Identifier(s2) = &self.peek().token {
                    if s2.eq_ignore_ascii_case("EXISTS") {
                        self.bump();
                        return true;
                    }
                }
                self.pos = saved;
            }
        }
        false
    }

    fn consume_if_not_exists(&mut self) -> bool {
        if let Token::Identifier(s) = &self.peek().token {
            if s.eq_ignore_ascii_case("IF") {
                let saved = self.pos;
                self.bump();
                if self.peek().token == Token::Not {
                    self.bump();
                    if let Token::Identifier(s2) = &self.peek().token {
                        if s2.eq_ignore_ascii_case("EXISTS") {
                            self.bump();
                            return true;
                        }
                    }
                }
                self.pos = saved;
            }
        }
        false
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        self.expect(Token::Table, "TABLE")?;
        let if_not_exists = self.consume_if_not_exists();
        let name = self.expect_identifier("table name")?;
        self.expect(Token::LParen, "(")?;
        let mut columns = Vec::new();
        let mut checks = Vec::new();
        loop {
            // Table-level CHECK constraint
            if self.peek().token == Token::Check {
                self.bump();
                self.expect(Token::LParen, "(")?;
                let e = self.parse_expr()?;
                self.expect(Token::RParen, ")")?;
                checks.push(e);
            } else {
                let col = self.parse_column_def()?;
                columns.push(col);
            }
            match self.peek().token {
                Token::Comma => {
                    self.bump();
                }
                Token::RParen => {
                    self.bump();
                    break;
                }
                _ => {
                    let cur = self.peek().clone();
                    return Err(MiniDbError::parse(
                        format!("expected ',' or ')', got {:?}", cur.token),
                        cur.line,
                        cur.column,
                    ));
                }
            }
        }
        Ok(Statement::CreateTable(CreateTable {
            name,
            columns,
            checks,
            if_not_exists,
        }))
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_identifier("column name")?;
        let type_name = self.expect_identifier("type name")?;
        let data_type = DataType::parse(&type_name)?;
        let mut col = ColumnDef::new(name, data_type);

        loop {
            match self.peek().token {
                Token::Primary => {
                    self.bump();
                    self.expect(Token::Key, "KEY")?;
                    col.primary_key = true;
                    col.not_null = true;
                    col.unique = true;
                }
                Token::Not => {
                    self.bump();
                    self.expect(Token::Null, "NULL")?;
                    col.not_null = true;
                }
                Token::Unique => {
                    self.bump();
                    col.unique = true;
                }
                Token::Default => {
                    self.bump();
                    let v = self.parse_literal_value()?;
                    col.default = Some(v);
                }
                Token::Check => {
                    self.bump();
                    self.expect(Token::LParen, "(")?;
                    // column-level CHECK currently parsed and ignored beyond the column scope
                    // (implementation could attach this to the column).
                    let _ = self.parse_expr()?;
                    self.expect(Token::RParen, ")")?;
                }
                _ => break,
            }
        }
        Ok(col)
    }

    fn parse_literal_value(&mut self) -> Result<Value> {
        let cur = self.bump();
        match cur.token {
            Token::IntLit(n) => Ok(Value::Int(n)),
            Token::RealLit(r) => Ok(Value::Real(r)),
            Token::StringLit(s) => Ok(Value::Text(s)),
            Token::True => Ok(Value::Bool(true)),
            Token::False => Ok(Value::Bool(false)),
            Token::Null => Ok(Value::Null),
            Token::Minus => match self.bump().token {
                Token::IntLit(n) => Ok(Value::Int(-n)),
                Token::RealLit(r) => Ok(Value::Real(-r)),
                other => Err(MiniDbError::parse(
                    format!("expected numeric literal after '-', got {:?}", other),
                    cur.line,
                    cur.column,
                )),
            },
            other => Err(MiniDbError::parse(
                format!("expected literal, got {:?}", other),
                cur.line,
                cur.column,
            )),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.expect(Token::Index, "INDEX")?;
        let if_not_exists = self.consume_if_not_exists();
        let name = self.expect_identifier("index name")?;
        self.expect(Token::On, "ON")?;
        let table = self.expect_identifier("table name")?;
        self.expect(Token::LParen, "(")?;
        let column = self.expect_identifier("column name")?;
        self.expect(Token::RParen, ")")?;
        Ok(Statement::CreateIndex(CreateIndex {
            name,
            table,
            column,
            if_not_exists,
        }))
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect(Token::Insert, "INSERT")?;
        self.expect(Token::Into, "INTO")?;
        let table = self.expect_identifier("table name")?;
        let columns = if self.peek().token == Token::LParen {
            self.bump();
            let mut cs = Vec::new();
            loop {
                cs.push(self.expect_identifier("column name")?);
                match self.peek().token {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let cur = self.peek().clone();
                        return Err(MiniDbError::parse(
                            format!("expected ',' or ')', got {:?}", cur.token),
                            cur.line,
                            cur.column,
                        ));
                    }
                }
            }
            Some(cs)
        } else {
            None
        };
        self.expect(Token::Values, "VALUES")?;
        let mut rows = Vec::new();
        loop {
            self.expect(Token::LParen, "(")?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                match self.peek().token {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let cur = self.peek().clone();
                        return Err(MiniDbError::parse(
                            format!("expected ',' or ')', got {:?}", cur.token),
                            cur.line,
                            cur.column,
                        ));
                    }
                }
            }
            rows.push(row);
            if self.peek().token == Token::Comma {
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

    fn parse_select_statement(&mut self) -> Result<Statement> {
        let s = self.parse_select()?;
        Ok(Statement::Select(s))
    }

    fn parse_select(&mut self) -> Result<Select> {
        self.expect(Token::Select, "SELECT")?;
        let mut projections = Vec::new();
        loop {
            if self.peek().token == Token::Star {
                self.bump();
                projections.push(Projection::Star);
            } else if matches!(self.peek().token, Token::Identifier(_))
                && self.peek_at(1).map(|t| &t.token) == Some(&Token::Dot)
                && self.peek_at(2).map(|t| &t.token) == Some(&Token::Star)
            {
                let qual = self.expect_identifier("table qualifier")?;
                self.bump(); // .
                self.bump(); // *
                projections.push(Projection::QualifiedStar(qual));
            } else {
                let expr = self.parse_expr()?;
                let alias = if self.peek().token == Token::As {
                    self.bump();
                    Some(self.expect_identifier("alias")?)
                } else if let Token::Identifier(_) = self.peek().token {
                    // implicit alias: only consume if next token is a known stop-word boundary
                    // (we keep this conservative: explicit AS only)
                    None
                } else {
                    None
                };
                projections.push(Projection::Expr { expr, alias });
            }
            if self.peek().token == Token::Comma {
                self.bump();
            } else {
                break;
            }
        }

        let from = if self.peek().token == Token::From {
            self.bump();
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        let where_clause = if self.peek().token == Token::Where {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let mut order_by = Vec::new();
        if self.peek().token == Token::Order {
            self.bump();
            self.expect(Token::By, "BY")?;
            loop {
                let expr = self.parse_expr()?;
                let descending = match self.peek().token {
                    Token::Asc => {
                        self.bump();
                        false
                    }
                    Token::Desc => {
                        self.bump();
                        true
                    }
                    _ => false,
                };
                order_by.push(OrderItem { expr, descending });
                if self.peek().token == Token::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
        }

        let mut limit = None;
        let mut offset = None;
        if self.peek().token == Token::Limit {
            self.bump();
            let v = self.parse_int_literal("LIMIT count")?;
            limit = Some(v);
            if self.peek().token == Token::Offset {
                self.bump();
                let o = self.parse_int_literal("OFFSET count")?;
                offset = Some(o);
            }
        }

        Ok(Select {
            projections,
            from,
            r#where: where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_int_literal(&mut self, what: &str) -> Result<i64> {
        let cur = self.bump();
        match cur.token {
            Token::IntLit(n) => Ok(n),
            other => Err(MiniDbError::parse(
                format!("expected integer for {}, got {:?}", what, other),
                cur.line,
                cur.column,
            )),
        }
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let mut left = self.parse_from_atom()?;
        loop {
            let mut have_join = false;
            if self.peek().token == Token::Inner
                && self.peek_at(1).map(|t| &t.token) == Some(&Token::Join)
            {
                self.bump();
                self.bump();
                have_join = true;
            } else if self.peek().token == Token::Join {
                self.bump();
                have_join = true;
            }
            if !have_join {
                break;
            }
            let right = self.parse_from_atom()?;
            self.expect(Token::On, "ON")?;
            let on = self.parse_expr()?;
            left = FromClause::Join {
                left: Box::new(left),
                right: Box::new(right),
                on,
            };
        }
        Ok(left)
    }

    fn parse_from_atom(&mut self) -> Result<FromClause> {
        let name = self.expect_identifier("table name")?;
        let alias = if self.peek().token == Token::As {
            self.bump();
            Some(self.expect_identifier("alias")?)
        } else if let Token::Identifier(s) = &self.peek().token {
            // implicit alias
            let s = s.clone();
            self.bump();
            Some(s)
        } else {
            None
        };
        Ok(FromClause::Table { name, alias })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect(Token::Update, "UPDATE")?;
        let table = self.expect_identifier("table name")?;
        self.expect(Token::Set, "SET")?;
        let mut assignments = Vec::new();
        loop {
            let col = self.expect_identifier("column name")?;
            self.expect(Token::Eq, "=")?;
            let expr = self.parse_expr()?;
            assignments.push((col, expr));
            if self.peek().token == Token::Comma {
                self.bump();
            } else {
                break;
            }
        }
        let where_clause = if self.peek().token == Token::Where {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update(Update {
            table,
            assignments,
            r#where: where_clause,
        }))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect(Token::Delete, "DELETE")?;
        self.expect(Token::From, "FROM")?;
        let table = self.expect_identifier("table name")?;
        let where_clause = if self.peek().token == Token::Where {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete(Delete {
            table,
            r#where: where_clause,
        }))
    }

    // ---- expression parser (precedence climbing) ----
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek().token == Token::Or {
            self.bump();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.peek().token == Token::And {
            self.bump();
            let right = self.parse_not()?;
            left = Expr::BinaryOp {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.peek().token == Token::Not {
            self.bump();
            let inner = self.parse_not()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(inner),
            });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let left = self.parse_additive()?;
        // IS NULL / IS NOT NULL
        if self.peek().token == Token::Is {
            self.bump();
            let negated = if self.peek().token == Token::Not {
                self.bump();
                true
            } else {
                false
            };
            self.expect(Token::Null, "NULL")?;
            return Ok(if negated {
                Expr::IsNotNull(Box::new(left))
            } else {
                Expr::IsNull(Box::new(left))
            });
        }
        let op = match self.peek().token {
            Token::Eq => BinaryOp::Eq,
            Token::NotEq => BinaryOp::NotEq,
            Token::Lt => BinaryOp::Lt,
            Token::LtEq => BinaryOp::LtEq,
            Token::Gt => BinaryOp::Gt,
            Token::GtEq => BinaryOp::GtEq,
            _ => return Ok(left),
        };
        self.bump();
        let right = self.parse_additive()?;
        Ok(Expr::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().token {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                Token::ConcatOp => BinaryOp::Concat,
                _ => break,
            };
            self.bump();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().token {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => break,
            };
            self.bump();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek().token == Token::Minus {
            self.bump();
            let inner = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(inner),
            });
        }
        if self.peek().token == Token::Plus {
            self.bump();
            return self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let cur = self.peek().clone();
        match cur.token {
            Token::IntLit(n) => {
                self.bump();
                Ok(Expr::Literal(Value::Int(n)))
            }
            Token::RealLit(r) => {
                self.bump();
                Ok(Expr::Literal(Value::Real(r)))
            }
            Token::StringLit(s) => {
                self.bump();
                Ok(Expr::Literal(Value::Text(s)))
            }
            Token::True => {
                self.bump();
                Ok(Expr::Literal(Value::Bool(true)))
            }
            Token::False => {
                self.bump();
                Ok(Expr::Literal(Value::Bool(false)))
            }
            Token::Null => {
                self.bump();
                Ok(Expr::Literal(Value::Null))
            }
            Token::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(Token::RParen, ")")?;
                Ok(e)
            }
            Token::Identifier(name) => {
                self.bump();
                if self.peek().token == Token::Dot {
                    self.bump();
                    let col = self.expect_identifier("column")?;
                    Ok(Expr::Column {
                        qualifier: Some(name),
                        name: col,
                    })
                } else if self.peek().token == Token::LParen {
                    self.bump();
                    let mut args = Vec::new();
                    if self.peek().token != Token::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.peek().token == Token::Comma {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::RParen, ")")?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    Ok(Expr::Column {
                        qualifier: None,
                        name,
                    })
                }
            }
            other => Err(MiniDbError::parse(
                format!("unexpected token in expression: {:?}", other),
                cur.line,
                cur.column,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ast::*;
    use super::*;

    #[test]
    fn parse_create_simple() {
        let s = parse_statement("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL)").unwrap();
        match s {
            Statement::CreateTable(ct) => {
                assert_eq!(ct.name, "t");
                assert_eq!(ct.columns.len(), 2);
                assert!(ct.columns[0].primary_key);
                assert!(ct.columns[1].not_null);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_select_with_join() {
        let s =
            parse_statement("SELECT a.x, b.y FROM a INNER JOIN b ON a.id = b.aid WHERE a.x > 1 ORDER BY a.x DESC LIMIT 5 OFFSET 2")
                .unwrap();
        match s {
            Statement::Select(sel) => {
                assert_eq!(sel.projections.len(), 2);
                assert!(sel.r#where.is_some());
                assert_eq!(sel.order_by.len(), 1);
                assert!(sel.order_by[0].descending);
                assert_eq!(sel.limit, Some(5));
                assert_eq!(sel.offset, Some(2));
                assert!(matches!(sel.from, Some(FromClause::Join { .. })));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_insert_named() {
        let s = parse_statement("INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y')").unwrap();
        match s {
            Statement::Insert(ins) => {
                assert_eq!(ins.columns.as_ref().unwrap().len(), 2);
                assert_eq!(ins.rows.len(), 2);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_program_multiple() {
        let prog = parse_program("BEGIN; INSERT INTO t VALUES (1); COMMIT;").unwrap();
        assert_eq!(prog.len(), 3);
    }

    #[test]
    fn parse_is_null() {
        let s = parse_statement("SELECT * FROM t WHERE x IS NOT NULL").unwrap();
        match s {
            Statement::Select(sel) => {
                assert!(matches!(sel.r#where, Some(Expr::IsNotNull(_))));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_function_call() {
        let s = parse_statement("SELECT LENGTH(name) FROM t").unwrap();
        match s {
            Statement::Select(sel) => match &sel.projections[0] {
                Projection::Expr {
                    expr: Expr::FunctionCall { name, args },
                    ..
                } => {
                    assert_eq!(name.to_uppercase(), "LENGTH");
                    assert_eq!(args.len(), 1);
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
