pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Lexer, Token, TokenWithPos};
use crate::types::{DataType, Value};
use ast::*;

pub struct Parser {
    tokens: Vec<TokenWithPos>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<TokenWithPos>) -> Self {
        Parser { tokens, pos: 0 }
    }

    pub fn parse_from_str(input: &str) -> Result<Vec<Statement>> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        parser.parse_statements()
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof)
    }

    fn peek_pos(&self) -> (usize, usize) {
        self.tokens
            .get(self.pos)
            .map(|t| (t.line, t.col))
            .unwrap_or((0, 0))
    }

    fn advance(&mut self) -> &Token {
        let tok = self
            .tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        let (line, col) = self.peek_pos();
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(MiniDbError::parse(
                format!("expected {:?}, got {:?}", expected, self.peek()),
                line,
                col,
            ))
        }
    }

    fn expect_identifier(&mut self) -> Result<String> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Identifier(s) => {
                self.advance();
                Ok(s)
            }
            // Allow keywords as identifiers in some contexts
            Token::Int => {
                self.advance();
                Ok("int".to_string())
            }
            Token::Text => {
                self.advance();
                Ok("text".to_string())
            }
            Token::Index => {
                self.advance();
                Ok("index".to_string())
            }
            Token::Key => {
                self.advance();
                Ok("key".to_string())
            }
            tok => Err(MiniDbError::parse(
                format!("expected identifier, got {:?}", tok),
                line,
                col,
            )),
        }
    }

    fn check(&self, tok: &Token) -> bool {
        self.peek() == tok
    }

    fn consume_if(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while !self.check(&Token::Eof) {
            // Skip extra semicolons
            while self.consume_if(&Token::Semicolon) {}
            if self.check(&Token::Eof) {
                break;
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            // Consume trailing semicolon if present
            self.consume_if(&Token::Semicolon);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Select | Token::Distinct => Ok(Statement::Select(self.parse_select()?)),
            Token::Insert => Ok(Statement::Insert(self.parse_insert()?)),
            Token::Update => Ok(Statement::Update(self.parse_update()?)),
            Token::Delete => Ok(Statement::Delete(self.parse_delete()?)),
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Begin => {
                self.advance();
                Ok(Statement::Begin)
            }
            Token::Commit => {
                self.advance();
                Ok(Statement::Commit)
            }
            Token::Rollback => {
                self.advance();
                Ok(Statement::Rollback)
            }
            Token::MetaSchema => {
                self.advance();
                Ok(Statement::MetaSchema)
            }
            Token::MetaTables => {
                self.advance();
                Ok(Statement::MetaTables)
            }
            Token::MetaIndexes => {
                self.advance();
                Ok(Statement::MetaIndexes)
            }
            Token::MetaExit => {
                self.advance();
                Ok(Statement::MetaExit)
            }
            Token::MetaFormat => {
                self.advance();
                let fmt = self.expect_identifier()?;
                Ok(Statement::MetaFormat(fmt))
            }
            tok => Err(MiniDbError::parse(
                format!("unexpected token {:?}", tok),
                line,
                col,
            )),
        }
    }

    fn parse_select(&mut self) -> Result<SelectStmt> {
        self.expect(&Token::Select)?;
        let distinct = self.consume_if(&Token::Distinct);

        let projections = self.parse_select_items()?;

        let from = if self.consume_if(&Token::From) {
            Some(self.parse_from_item()?)
        } else {
            None
        };

        let where_clause = if self.consume_if(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.check(&Token::Order) {
            self.advance();
            self.expect(&Token::By)?;
            self.parse_order_by()?
        } else {
            Vec::new()
        };

        let limit = if self.consume_if(&Token::Limit) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let offset = if self.consume_if(&Token::Offset) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(SelectStmt {
            distinct,
            projections,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_select_items(&mut self) -> Result<Vec<SelectItem>> {
        let mut items = Vec::new();
        loop {
            if self.check(&Token::Star) {
                self.advance();
                items.push(SelectItem::Wildcard);
            } else {
                let expr = self.parse_expr()?;
                // Check for table.* pattern
                let alias = if self.consume_if(&Token::As) {
                    Some(self.expect_identifier()?)
                } else if let Token::Identifier(_) = self.peek() {
                    let name = self.expect_identifier()?;
                    Some(name)
                } else {
                    None
                };
                items.push(SelectItem::Expr { expr, alias });
            }
            if !self.consume_if(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_from_item(&mut self) -> Result<FromItem> {
        let name = self.expect_identifier()?;
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_identifier()?)
        } else if let Token::Identifier(_) = self.peek() {
            Some(self.expect_identifier()?)
        } else {
            None
        };

        let mut left = FromItem::Table { name, alias };

        // Handle JOINs
        loop {
            let join_type = if self.consume_if(&Token::Join) {
                JoinType::Inner
            } else if self.consume_if(&Token::Inner) {
                self.expect(&Token::Join)?;
                JoinType::Inner
            } else if self.consume_if(&Token::Left) {
                let _ = self.consume_if(&Token::Outer);
                self.expect(&Token::Join)?;
                JoinType::Left
            } else if self.consume_if(&Token::Right) {
                let _ = self.consume_if(&Token::Outer);
                self.expect(&Token::Join)?;
                JoinType::Right
            } else if self.consume_if(&Token::Cross) {
                self.expect(&Token::Join)?;
                JoinType::Cross
            } else {
                break;
            };

            let right_name = self.expect_identifier()?;
            let right_alias = if self.consume_if(&Token::As) {
                Some(self.expect_identifier()?)
            } else if let Token::Identifier(_) = self.peek() {
                Some(self.expect_identifier()?)
            } else {
                None
            };
            let right = FromItem::Table {
                name: right_name,
                alias: right_alias,
            };

            let condition = if self.consume_if(&Token::On) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            left = FromItem::Join {
                left: Box::new(left),
                right: Box::new(right),
                join_type,
                condition,
            };
        }

        Ok(left)
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let ascending = if self.consume_if(&Token::Desc) {
                false
            } else {
                let _ = self.consume_if(&Token::Asc);
                true
            };
            items.push(OrderByItem { expr, ascending });
            if !self.consume_if(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_insert(&mut self) -> Result<InsertStmt> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;
        let table = self.expect_identifier()?;

        let columns = if self.consume_if(&Token::LParen) {
            if !self.check(&Token::RParen) {
                let mut cols = Vec::new();
                loop {
                    cols.push(self.expect_identifier()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Some(cols)
            } else {
                self.expect(&Token::RParen)?;
                Some(Vec::new())
            }
        } else {
            None
        };

        self.expect(&Token::Values)?;

        let mut all_values = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let mut row_values = Vec::new();
            if !self.check(&Token::RParen) {
                loop {
                    row_values.push(self.parse_expr()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
            all_values.push(row_values);
            if !self.consume_if(&Token::Comma) {
                break;
            }
        }

        Ok(InsertStmt {
            table,
            columns,
            values: all_values,
        })
    }

    fn parse_update(&mut self) -> Result<UpdateStmt> {
        self.expect(&Token::Update)?;
        let table = self.expect_identifier()?;
        self.expect(&Token::Set)?;

        let mut assignments = Vec::new();
        loop {
            let col = self.expect_identifier()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((col, expr));
            if !self.consume_if(&Token::Comma) {
                break;
            }
        }

        let where_clause = if self.consume_if(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(UpdateStmt {
            table,
            assignments,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<DeleteStmt> {
        self.expect(&Token::Delete)?;
        self.expect(&Token::From)?;
        let table = self.expect_identifier()?;

        let where_clause = if self.consume_if(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(DeleteStmt {
            table,
            where_clause,
        })
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect(&Token::Create)?;
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Table => {
                self.advance();
                let if_not_exists = self.parse_if_not_exists();
                let name = self.expect_identifier()?;
                self.expect(&Token::LParen)?;
                let (columns, constraints) = self.parse_column_defs()?;
                self.expect(&Token::RParen)?;
                Ok(Statement::CreateTable(CreateTableStmt {
                    name,
                    columns,
                    constraints,
                    if_not_exists,
                }))
            }
            Token::Index | Token::Unique => {
                let unique = self.consume_if(&Token::Unique);
                self.expect(&Token::Index)?;
                let if_not_exists = self.parse_if_not_exists();
                let name = self.expect_identifier()?;
                self.expect(&Token::On)?;
                let table = self.expect_identifier()?;
                self.expect(&Token::LParen)?;
                let mut columns = Vec::new();
                loop {
                    columns.push(self.expect_identifier()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Ok(Statement::CreateIndex(CreateIndexStmt {
                    name,
                    table,
                    columns,
                    unique,
                    if_not_exists,
                }))
            }
            tok => Err(MiniDbError::parse(
                format!("expected TABLE or INDEX after CREATE, got {:?}", tok),
                line,
                col,
            )),
        }
    }

    fn parse_if_not_exists(&mut self) -> bool {
        if self.check(&Token::Identifier("IF".to_string())) || {
            // Check for IF keyword (it's not in our keywords list, so it's an Identifier)
            if let Token::Identifier(s) = self.peek() {
                s.eq_ignore_ascii_case("IF")
            } else {
                false
            }
        } {
            self.advance();
            if let Token::Not = self.peek() {
                self.advance();
                if let Token::Identifier(s) = self.peek() {
                    if s.eq_ignore_ascii_case("EXISTS") {
                        self.advance();
                        return true;
                    }
                }
            }
        }
        false
    }

    fn parse_if_exists(&mut self) -> bool {
        if let Token::Identifier(s) = self.peek() {
            if s.eq_ignore_ascii_case("IF") {
                self.advance();
                if let Token::Identifier(s2) = self.peek() {
                    if s2.eq_ignore_ascii_case("EXISTS") {
                        self.advance();
                        return true;
                    }
                }
            }
        }
        false
    }

    fn parse_column_defs(&mut self) -> Result<(Vec<ColumnDef>, Vec<TableConstraint>)> {
        let mut columns = Vec::new();
        let mut constraints = Vec::new();

        loop {
            if self.check(&Token::RParen) {
                break;
            }

            // Check for table constraints
            if self.check(&Token::Primary) {
                self.advance();
                self.expect(&Token::Key)?;
                self.expect(&Token::LParen)?;
                let mut cols = Vec::new();
                loop {
                    cols.push(self.expect_identifier()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                constraints.push(TableConstraint::PrimaryKey(cols));
            } else if self.check(&Token::Check) {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                constraints.push(TableConstraint::Check(expr));
            } else if self.check(&Token::Unique) {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut cols = Vec::new();
                loop {
                    cols.push(self.expect_identifier()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                constraints.push(TableConstraint::Unique(cols));
            } else {
                // Column definition
                let col = self.parse_column_def()?;
                columns.push(col);
            }

            if !self.consume_if(&Token::Comma) {
                break;
            }
        }

        Ok((columns, constraints))
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_identifier()?;
        let data_type = self.parse_data_type()?;

        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;

        // Parse column constraints
        loop {
            match self.peek() {
                Token::Primary => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    primary_key = true;
                    not_null = true;
                }
                Token::Not => {
                    self.advance();
                    self.expect(&Token::Null)?;
                    not_null = true;
                }
                Token::Unique => {
                    self.advance();
                    unique = true;
                }
                Token::Default => {
                    self.advance();
                    default = Some(self.parse_primary_expr()?);
                }
                _ => break,
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

    fn parse_data_type(&mut self) -> Result<DataType> {
        let (line, col) = self.peek_pos();
        let dt = match self.peek().clone() {
            Token::Int => {
                self.advance();
                DataType::Int
            }
            Token::BigInt => {
                self.advance();
                DataType::BigInt
            }
            Token::Real => {
                self.advance();
                DataType::Real
            }
            Token::Text => {
                self.advance();
                DataType::Text
            }
            Token::Bool => {
                self.advance();
                DataType::Bool
            }
            tok => {
                return Err(MiniDbError::parse(
                    format!("expected data type, got {:?}", tok),
                    line,
                    col,
                ))
            }
        };
        // Consume optional precision/length: e.g. VARCHAR(255)
        if self.check(&Token::LParen) {
            self.advance();
            self.parse_expr()?; // consume but ignore
            self.expect(&Token::RParen)?;
        }
        Ok(dt)
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect(&Token::Drop)?;
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Table => {
                self.advance();
                let if_exists = self.parse_if_exists();
                let name = self.expect_identifier()?;
                Ok(Statement::DropTable(DropTableStmt { name, if_exists }))
            }
            Token::Index => {
                self.advance();
                let if_exists = self.parse_if_exists();
                let name = self.expect_identifier()?;
                Ok(Statement::DropIndex(DropIndexStmt { name, if_exists }))
            }
            tok => Err(MiniDbError::parse(
                format!("expected TABLE or INDEX after DROP, got {:?}", tok),
                line,
                col,
            )),
        }
    }

    // Expression parsing with precedence climbing
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_not_expr()?;
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_not_expr()?;
            left = Expr::BinOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not_expr(&mut self) -> Result<Expr> {
        if self.consume_if(&Token::Not) {
            let expr = self.parse_not_expr()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            })
        } else {
            self.parse_comparison_expr()
        }
    }

    fn parse_comparison_expr(&mut self) -> Result<Expr> {
        let left = self.parse_additive_expr()?;

        let op = match self.peek() {
            Token::Eq => BinOp::Eq,
            Token::Neq => BinOp::Neq,
            Token::Lt => BinOp::Lt,
            Token::Le => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::Ge => BinOp::Ge,
            Token::Is => {
                self.advance();
                let negated = self.consume_if(&Token::Not);
                self.expect(&Token::Null)?;
                return Ok(Expr::IsNull {
                    expr: Box::new(left),
                    negated,
                });
            }
            _ => return Ok(left),
        };

        self.advance();
        let right = self.parse_additive_expr()?;
        Ok(Expr::BinOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_additive_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative_expr()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Concat => BinOp::Concat,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative_expr()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary_expr()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary_expr()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary_expr(&mut self) -> Result<Expr> {
        if self.consume_if(&Token::Minus) {
            let expr = self.parse_primary_expr()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary_expr()
        }
    }

    fn parse_primary_expr(&mut self) -> Result<Expr> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::IntLiteral(i) => {
                self.advance();
                Ok(Expr::Literal(Value::Int(i)))
            }
            Token::RealLiteral(r) => {
                self.advance();
                Ok(Expr::Literal(Value::Real(r)))
            }
            Token::StringLiteral(s) => {
                self.advance();
                Ok(Expr::Literal(Value::Text(s)))
            }
            Token::BoolLiteral(b) => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(b)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }
            Token::Star => {
                self.advance();
                Ok(Expr::Wildcard)
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Identifier(name) => {
                self.advance();
                // Check for function call
                if self.check(&Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            if self.check(&Token::Star) {
                                self.advance();
                                args.push(Expr::Wildcard);
                            } else {
                                args.push(self.parse_expr()?);
                            }
                            if !self.consume_if(&Token::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::FunctionCall {
                        name: name.to_uppercase(),
                        args,
                    })
                } else if self.check(&Token::Dot) {
                    self.advance();
                    // Could be table.column or table.*
                    if self.check(&Token::Star) {
                        self.advance();
                        Ok(Expr::QualifiedWildcard(name))
                    } else {
                        let col = self.expect_identifier()?;
                        Ok(Expr::Column(ColumnRef {
                            table: Some(name),
                            column: col,
                        }))
                    }
                } else {
                    Ok(Expr::Column(ColumnRef {
                        table: None,
                        column: name,
                    }))
                }
            }
            tok => Err(MiniDbError::parse(
                format!("unexpected token in expression: {:?}", tok),
                line,
                col,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let stmts = Parser::parse_from_str("SELECT * FROM users WHERE id = 1;").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Statement::Select(_)));
    }

    #[test]
    fn test_parse_create_table() {
        let sql =
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL, score REAL DEFAULT 0.0);";
        let stmts = Parser::parse_from_str(sql).unwrap();
        assert_eq!(stmts.len(), 1);
        if let Statement::CreateTable(ct) = &stmts[0] {
            assert_eq!(ct.name, "t");
            assert_eq!(ct.columns.len(), 3);
            assert!(ct.columns[0].primary_key);
            assert!(ct.columns[1].not_null);
        } else {
            panic!("expected CreateTable");
        }
    }

    #[test]
    fn test_parse_insert() {
        let sql = "INSERT INTO t (a, b) VALUES (1, 'hello');";
        let stmts = Parser::parse_from_str(sql).unwrap();
        assert!(matches!(&stmts[0], Statement::Insert(_)));
    }

    #[test]
    fn test_parse_multiple_statements() {
        let sql = "BEGIN; INSERT INTO t VALUES (1); COMMIT;";
        let stmts = Parser::parse_from_str(sql).unwrap();
        assert_eq!(stmts.len(), 3);
        assert!(matches!(&stmts[0], Statement::Begin));
        assert!(matches!(&stmts[1], Statement::Insert(_)));
        assert!(matches!(&stmts[2], Statement::Commit));
    }

    #[test]
    fn test_parse_order_by_limit() {
        let sql = "SELECT id FROM t ORDER BY id DESC LIMIT 10 OFFSET 5;";
        let stmts = Parser::parse_from_str(sql).unwrap();
        if let Statement::Select(s) = &stmts[0] {
            assert_eq!(s.order_by.len(), 1);
            assert!(!s.order_by[0].ascending);
            assert!(s.limit.is_some());
            assert!(s.offset.is_some());
        } else {
            panic!("expected Select");
        }
    }

    #[test]
    fn test_parse_join() {
        let sql = "SELECT a.id, b.name FROM a INNER JOIN b ON a.id = b.a_id;";
        let stmts = Parser::parse_from_str(sql).unwrap();
        if let Statement::Select(s) = &stmts[0] {
            assert!(matches!(s.from, Some(FromItem::Join { .. })));
        }
    }
}
