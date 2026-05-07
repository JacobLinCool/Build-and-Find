pub mod ast;

use crate::error::{MiniDbError, Position};
use crate::lexer::{Token, TokenWithPos};
use crate::types::Value;

pub struct Parser {
    tokens: Vec<TokenWithPos>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<TokenWithPos>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof)
    }

    fn peek_pos(&self) -> Position {
        self.tokens
            .get(self.pos)
            .map(|t| Position {
                line: t.line,
                column: t.column,
            })
            .unwrap_or(Position { line: 0, column: 0 })
    }

    fn advance(&mut self) -> Token {
        let tok = self
            .tokens
            .get(self.pos)
            .map(|t| t.token.clone())
            .unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), MiniDbError> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) != std::mem::discriminant(expected) {
            Err(MiniDbError::Syntax(
                format!("Expected {}, got {}", expected, tok),
                Some(self.peek_pos()),
            ))
        } else {
            Ok(())
        }
    }

    fn expect_ident(&mut self) -> Result<String, MiniDbError> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            tok => Err(MiniDbError::Syntax(
                format!("Expected identifier, got {}", tok),
                Some(self.peek_pos()),
            )),
        }
    }

    fn match_token(&mut self, expected: &Token) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<ast::Statement>, MiniDbError> {
        let mut stmts = Vec::new();
        loop {
            self.skip_semicolons();
            if matches!(self.peek(), Token::Eof) {
                break;
            }
            stmts.push(self.parse_statement()?);
        }
        Ok(stmts)
    }

    fn skip_semicolons(&mut self) {
        while self.match_token(&Token::Semicolon) {}
    }

    fn parse_statement(&mut self) -> Result<ast::Statement, MiniDbError> {
        let stmt = match self.peek() {
            Token::Select => ast::Statement::Select(self.parse_select()?),
            Token::Insert => ast::Statement::Insert(self.parse_insert()?),
            Token::Update => ast::Statement::Update(self.parse_update()?),
            Token::Delete => ast::Statement::Delete(self.parse_delete()?),
            Token::Create => self.parse_create()?,
            Token::Drop => self.parse_drop()?,
            Token::Begin => {
                self.advance();
                ast::Statement::Begin
            }
            Token::Commit => {
                self.advance();
                ast::Statement::Commit
            }
            Token::Rollback => {
                self.advance();
                ast::Statement::Rollback
            }
            tok => {
                return Err(MiniDbError::Syntax(
                    format!("Unexpected token: {}", tok),
                    Some(self.peek_pos()),
                ))
            }
        };
        self.skip_semicolons();
        Ok(stmt)
    }

    fn parse_select(&mut self) -> Result<ast::Select, MiniDbError> {
        self.advance(); // SELECT
        let distinct = self.match_token(&Token::Distinct);
        let projection = self.parse_projection()?;
        self.expect(&Token::From)?;
        let from = self.parse_from()?;
        let where_clause = if self.match_token(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let order_by = if self.match_token(&Token::Order) {
            self.expect(&Token::By)?;
            Some(self.parse_order_by()?)
        } else {
            None
        };
        let limit = if self.match_token(&Token::Limit) {
            match self.advance() {
                Token::Integer(n) => Some(n as usize),
                tok => {
                    return Err(MiniDbError::Syntax(
                        format!("Expected integer after LIMIT, got {}", tok),
                        Some(self.peek_pos()),
                    ))
                }
            }
        } else {
            None
        };
        let offset = if self.match_token(&Token::Offset) {
            match self.advance() {
                Token::Integer(n) => Some(n as usize),
                tok => {
                    return Err(MiniDbError::Syntax(
                        format!("Expected integer after OFFSET, got {}", tok),
                        Some(self.peek_pos()),
                    ))
                }
            }
        } else {
            None
        };
        Ok(ast::Select {
            distinct,
            projection,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_projection(&mut self) -> Result<Vec<ast::Expr>, MiniDbError> {
        if self.match_token(&Token::StarOp) {
            return Ok(vec![ast::Expr::Star]);
        }
        let mut cols = Vec::new();
        cols.push(self.parse_expr()?);
        while self.match_token(&Token::Comma) {
            cols.push(self.parse_expr()?);
        }
        Ok(cols)
    }

    fn parse_from(&mut self) -> Result<ast::FromClause, MiniDbError> {
        let table = self.expect_ident()?;
        let alias = if self.match_token(&Token::As) {
            Some(self.expect_ident()?)
        } else if matches!(self.peek(), Token::Ident(_)) {
            // Check if it's an alias (not a keyword)
            Some(self.expect_ident()?)
        } else {
            None
        };
        if self.match_token(&Token::Inner) {
            self.expect(&Token::Join)?;
            let right_table = self.expect_ident()?;
            let right_alias = if self.match_token(&Token::As) {
                Some(self.expect_ident()?)
            } else if matches!(self.peek(), Token::Ident(_)) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            self.expect(&Token::On)?;
            let on = self.parse_expr()?;
            return Ok(ast::FromClause::Join(ast::JoinClause {
                left_table: table,
                left_alias: alias,
                right_table,
                right_alias,
                on,
            }));
        }
        Ok(ast::FromClause::Table(ast::TableRef { name: table, alias }))
    }

    fn parse_order_by(&mut self) -> Result<Vec<ast::OrderByItem>, MiniDbError> {
        let mut items = Vec::new();
        items.push(ast::OrderByItem {
            expr: self.parse_expr()?,
            desc: self.parse_sort_direction(),
        });
        while self.match_token(&Token::Comma) {
            items.push(ast::OrderByItem {
                expr: self.parse_expr()?,
                desc: self.parse_sort_direction(),
            });
        }
        Ok(items)
    }

    fn parse_sort_direction(&mut self) -> bool {
        if self.match_token(&Token::Desc) {
            true
        } else {
            self.match_token(&Token::Asc);
            false
        }
    }

    fn parse_insert(&mut self) -> Result<ast::Insert, MiniDbError> {
        self.advance(); // INSERT
        self.expect(&Token::Into)?;
        let table = self.expect_ident()?;
        let columns = if self.match_token(&Token::LParen) {
            let mut cols = Vec::new();
            cols.push(self.expect_ident()?);
            while self.match_token(&Token::Comma) {
                cols.push(self.expect_ident()?);
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };
        self.expect(&Token::Values)?;
        self.expect(&Token::LParen)?;
        let mut values = Vec::new();
        values.push(self.parse_literal_value()?);
        while self.match_token(&Token::Comma) {
            values.push(self.parse_literal_value()?);
        }
        self.expect(&Token::RParen)?;
        Ok(ast::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_literal_value(&mut self) -> Result<ast::Expr, MiniDbError> {
        match self.peek().clone() {
            Token::Null => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Null))
            }
            Token::Integer(v) => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Int(v)))
            }
            Token::Float(v) => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Real(v)))
            }
            Token::String(s) => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Text(s)))
            }
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::Integer(v) => {
                        self.advance();
                        Ok(ast::Expr::Literal(Value::Int(-v)))
                    }
                    Token::Float(v) => {
                        self.advance();
                        Ok(ast::Expr::Literal(Value::Real(-v)))
                    }
                    _ => Err(MiniDbError::Syntax(
                        "Expected number after minus".into(),
                        Some(self.peek_pos()),
                    )),
                }
            }
            _ => self.parse_expr(),
        }
    }

    fn parse_update(&mut self) -> Result<ast::Update, MiniDbError> {
        self.advance(); // UPDATE
        let table = self.expect_ident()?;
        self.expect(&Token::Set)?;
        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push(ast::Assignment { column: col, expr });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        let where_clause = if self.match_token(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(ast::Update {
            table,
            assignments,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<ast::Delete, MiniDbError> {
        self.advance(); // DELETE
        self.expect(&Token::From)?;
        let table = self.expect_ident()?;
        let where_clause = if self.match_token(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(ast::Delete {
            table,
            where_clause,
        })
    }

    fn parse_create(&mut self) -> Result<ast::Statement, MiniDbError> {
        self.advance(); // CREATE
        if self.match_token(&Token::Table) {
            let name = self.expect_ident()?;
            self.expect(&Token::LParen)?;
            let mut columns: Vec<ast::ColumnDef> = Vec::new();
            loop {
                if self.match_token(&Token::RParen) {
                    break;
                }
                if !columns.is_empty() {
                    self.expect(&Token::Comma)?;
                }
                // Check for table-level constraints
                if matches!(self.peek(), Token::Primary) {
                    self.advance();
                    self.expect(&Token::Key)?;
                    self.expect(&Token::LParen)?;
                    let pk_col = self.expect_ident()?;
                    self.expect(&Token::RParen)?;
                    // Find the column and mark it as primary key
                    if let Some(col) = columns.iter_mut().find(|c| c.name == pk_col) {
                        col.primary_key = true;
                        col.not_null = true;
                    }
                    continue;
                }
                let col = self.parse_column_def()?;
                columns.push(col);
            }
            // Parse CHECK constraints after column definitions
            let mut check_constraints = Vec::new();
            // Already consumed RParen above
            Ok(ast::Statement::CreateTable(ast::CreateTable {
                name,
                columns,
                check_constraints,
            }))
        } else if self.match_token(&Token::Index) {
            let name = self.expect_ident()?;
            self.expect(&Token::On)?;
            let table = self.expect_ident()?;
            self.expect(&Token::LParen)?;
            let column = self.expect_ident()?;
            self.expect(&Token::RParen)?;
            Ok(ast::Statement::CreateIndex(ast::CreateIndex {
                name,
                table,
                column,
            }))
        } else {
            Err(MiniDbError::Syntax(
                "Expected TABLE or INDEX after CREATE".into(),
                Some(self.peek_pos()),
            ))
        }
    }

    fn parse_column_def(&mut self) -> Result<ast::ColumnDef, MiniDbError> {
        let name = self.expect_ident()?;
        let col_type = match self.advance() {
            Token::Ident(t) => {
                let upper = t.to_uppercase();
                match upper.as_str() {
                    "INT" | "INTEGER" => ast::ColumnType::Int,
                    "BIGINT" => ast::ColumnType::BigInt,
                    "REAL" | "FLOAT" | "DOUBLE" => ast::ColumnType::Real,
                    "TEXT" | "VARCHAR" | "STRING" => ast::ColumnType::Text,
                    "BOOL" | "BOOLEAN" => ast::ColumnType::Bool,
                    _ => {
                        return Err(MiniDbError::Syntax(
                            format!("Unknown type: {}", t),
                            Some(self.peek_pos()),
                        ))
                    }
                }
            }
            tok => {
                return Err(MiniDbError::Syntax(
                    format!("Expected type, got {}", tok),
                    Some(self.peek_pos()),
                ))
            }
        };
        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;

        loop {
            if self.match_token(&Token::Primary) {
                self.expect(&Token::Key)?;
                primary_key = true;
                not_null = true;
                continue;
            }
            if self.match_token(&Token::Not) {
                self.expect(&Token::Null)?;
                not_null = true;
                continue;
            }
            if self.match_token(&Token::Unique) {
                unique = true;
                continue;
            }
            if self.match_token(&Token::Default) {
                default = Some(match self.peek().clone() {
                    Token::Null => {
                        self.advance();
                        Value::Null
                    }
                    Token::Integer(v) => {
                        self.advance();
                        Value::Int(v)
                    }
                    Token::Float(v) => {
                        self.advance();
                        Value::Real(v)
                    }
                    Token::String(s) => {
                        self.advance();
                        Value::Text(s)
                    }
                    Token::Minus => {
                        self.advance();
                        match self.peek().clone() {
                            Token::Integer(v) => {
                                self.advance();
                                Value::Int(-v)
                            }
                            Token::Float(v) => {
                                self.advance();
                                Value::Real(-v)
                            }
                            _ => {
                                return Err(MiniDbError::Syntax(
                                    "Expected number after minus in DEFAULT".into(),
                                    Some(self.peek_pos()),
                                ))
                            }
                        }
                    }
                    tok => {
                        return Err(MiniDbError::Syntax(
                            format!("Expected literal value after DEFAULT, got {}", tok),
                            Some(self.peek_pos()),
                        ))
                    }
                });
                continue;
            }
            break;
        }

        Ok(ast::ColumnDef {
            name,
            col_type,
            primary_key,
            not_null,
            unique,
            default,
        })
    }

    fn parse_drop(&mut self) -> Result<ast::Statement, MiniDbError> {
        self.advance(); // DROP
        if self.match_token(&Token::Table) {
            let name = self.expect_ident()?;
            Ok(ast::Statement::DropTable(name))
        } else if self.match_token(&Token::Index) {
            let name = self.expect_ident()?;
            Ok(ast::Statement::DropIndex(name))
        } else {
            Err(MiniDbError::Syntax(
                "Expected TABLE or INDEX after DROP".into(),
                Some(self.peek_pos()),
            ))
        }
    }

    fn parse_expr(&mut self) -> Result<ast::Expr, MiniDbError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<ast::Expr, MiniDbError> {
        let mut left = self.parse_and()?;
        while self.match_token(&Token::Or) {
            let right = self.parse_and()?;
            left = ast::Expr::BinaryOp {
                op: ast::BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<ast::Expr, MiniDbError> {
        let mut left = self.parse_not()?;
        while self.match_token(&Token::And) {
            let right = self.parse_not()?;
            left = ast::Expr::BinaryOp {
                op: ast::BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<ast::Expr, MiniDbError> {
        if self.match_token(&Token::Not) {
            let expr = self.parse_not()?;
            Ok(ast::Expr::UnaryOp {
                op: ast::UnaryOp::Not,
                expr: Box::new(expr),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<ast::Expr, MiniDbError> {
        let mut left = self.parse_concat()?;
        loop {
            if self.match_token(&Token::Eq) {
                let right = self.parse_concat()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Eq,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Ne) {
                let right = self.parse_concat()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Ne,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Lt) {
                let right = self.parse_concat()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Lt,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Le) {
                let right = self.parse_concat()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Le,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Gt) {
                let right = self.parse_concat()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Gt,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Ge) {
                let right = self.parse_concat()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Ge,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Is) {
                if self.match_token(&Token::Not) {
                    self.expect(&Token::Null)?;
                    left = ast::Expr::IsNotNull(Box::new(left));
                } else {
                    self.expect(&Token::Null)?;
                    left = ast::Expr::IsNull(Box::new(left));
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_concat(&mut self) -> Result<ast::Expr, MiniDbError> {
        let mut left = self.parse_additive()?;
        while self.match_token(&Token::Concat) {
            let right = self.parse_additive()?;
            left = ast::Expr::BinaryOp {
                op: ast::BinaryOp::Concat,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<ast::Expr, MiniDbError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            if self.match_token(&Token::Plus) {
                let right = self.parse_multiplicative()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Add,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Minus) {
                let right = self.parse_multiplicative()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Sub,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<ast::Expr, MiniDbError> {
        let mut left = self.parse_unary()?;
        loop {
            if self.match_token(&Token::StarOp) {
                let right = self.parse_unary()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Mul,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Slash) {
                let right = self.parse_unary()?;
                left = ast::Expr::BinaryOp {
                    op: ast::BinaryOp::Div,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<ast::Expr, MiniDbError> {
        if self.match_token(&Token::Minus) {
            let expr = self.parse_unary()?;
            Ok(ast::Expr::UnaryOp {
                op: ast::UnaryOp::Neg,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<ast::Expr, MiniDbError> {
        match self.peek().clone() {
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Null => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Null))
            }
            Token::Integer(v) => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Int(v)))
            }
            Token::Float(v) => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Real(v)))
            }
            Token::String(s) => {
                self.advance();
                Ok(ast::Expr::Literal(Value::Text(s)))
            }
            Token::Ident(name) => {
                self.advance();
                // Check for function call
                if self.match_token(&Token::LParen) {
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        args.push(self.parse_expr()?);
                        while self.match_token(&Token::Comma) {
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(ast::Expr::Function {
                        name: name.to_uppercase(),
                        args,
                    })
                } else if self.match_token(&Token::Dot) {
                    let col = self.expect_ident()?;
                    Ok(ast::Expr::ColumnRef {
                        table: Some(name),
                        column: col,
                    })
                } else {
                    Ok(ast::Expr::ColumnRef {
                        table: None,
                        column: name,
                    })
                }
            }
            tok => Err(MiniDbError::Syntax(
                format!("Unexpected token: {}", tok),
                Some(self.peek_pos()),
            )),
        }
    }
}

pub fn parse(input: &str) -> Result<Vec<ast::Statement>, MiniDbError> {
    let tokens = crate::lexer::tokenize(input).map_err(|e| MiniDbError::Parse(e))?;
    let mut parser = Parser::new(tokens);
    parser.parse_statements()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select_all() {
        let stmts = parse("SELECT * FROM users;").unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            ast::Statement::Select(sel) => {
                assert_eq!(sel.projection.len(), 1);
                assert!(matches!(sel.projection[0], ast::Expr::Star));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_where_clause() {
        let stmts = parse("SELECT * FROM users WHERE id > 5 AND name = 'alice';").unwrap();
        match &stmts[0] {
            ast::Statement::Select(sel) => {
                assert!(sel.where_clause.is_some());
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmts = parse("INSERT INTO users (name, age) VALUES ('alice', 30);").unwrap();
        match &stmts[0] {
            ast::Statement::Insert(ins) => {
                assert_eq!(ins.table, "users");
                assert_eq!(ins.columns.as_ref().unwrap().len(), 2);
                assert_eq!(ins.values.len(), 2);
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let stmts = parse("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);").unwrap();
        match &stmts[0] {
            ast::Statement::CreateTable(ct) => {
                assert_eq!(ct.name, "users");
                assert_eq!(ct.columns.len(), 2);
                assert!(ct.columns[0].primary_key);
                assert!(ct.columns[1].not_null);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmts = parse("UPDATE users SET name = 'bob' WHERE id = 1;").unwrap();
        match &stmts[0] {
            ast::Statement::Update(u) => {
                assert_eq!(u.table, "users");
                assert_eq!(u.assignments.len(), 1);
                assert!(u.where_clause.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmts = parse("DELETE FROM users WHERE id = 1;").unwrap();
        match &stmts[0] {
            ast::Statement::Delete(d) => {
                assert_eq!(d.table, "users");
                assert!(d.where_clause.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_create_index() {
        let stmts = parse("CREATE INDEX idx_name ON users (name);").unwrap();
        match &stmts[0] {
            ast::Statement::CreateIndex(ci) => {
                assert_eq!(ci.name, "idx_name");
                assert_eq!(ci.table, "users");
                assert_eq!(ci.column, "name");
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_begin_commit() {
        let stmts = parse("BEGIN; INSERT INTO t VALUES (1); COMMIT;").unwrap();
        assert_eq!(stmts.len(), 3);
        assert!(matches!(stmts[0], ast::Statement::Begin));
        assert!(matches!(stmts[2], ast::Statement::Commit));
    }

    #[test]
    fn test_parse_multiple_statements() {
        let stmts = parse("SELECT * FROM t; SELECT * FROM t; SELECT * FROM t;").unwrap();
        assert_eq!(stmts.len(), 3);
    }
}
