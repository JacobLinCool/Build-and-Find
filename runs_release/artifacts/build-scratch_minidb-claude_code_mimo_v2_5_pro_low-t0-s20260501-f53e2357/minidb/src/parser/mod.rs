pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::Token;
use crate::types::{ColumnType, Value};
use ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_all(input: &str) -> Result<Vec<Statement>> {
        let tokens = crate::lexer::Lexer::new(input).tokenize()?;
        let mut parser = Parser::new(tokens);
        let mut stmts = Vec::new();
        while !parser.check(&Token::Eof) {
            stmts.push(parser.parse_statement()?);
            if parser.check(&Token::Semicolon) {
                parser.advance();
            }
        }
        Ok(stmts)
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        // Check for meta-commands first
        if self.is_meta() {
            return self.parse_meta();
        }
        match self.peek() {
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Insert => self.parse_insert(),
            Token::Select => self.parse_select_stmt(),
            Token::Update => self.parse_update(),
            Token::Delete => self.parse_delete(),
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
            _ => Err(self.error("expected a statement")),
        }
    }

    fn is_meta(&self) -> bool {
        matches!(
            self.peek(),
            Token::DotSchema
                | Token::DotTables
                | Token::DotIndexes
                | Token::DotExit
                | Token::DotFormat
                | Token::DotHelp
        )
    }

    fn parse_meta(&mut self) -> Result<Statement> {
        match self.advance() {
            Token::DotSchema => Ok(Statement::Meta(MetaCommand::Schema)),
            Token::DotTables => Ok(Statement::Meta(MetaCommand::Tables)),
            Token::DotIndexes => Ok(Statement::Meta(MetaCommand::Indexes)),
            Token::DotExit => Ok(Statement::Meta(MetaCommand::Exit)),
            Token::DotFormat => {
                let kind = match self.peek().clone() {
                    Token::Ident(s) | Token::StringLit(s) => {
                        self.advance();
                        s
                    }
                    _ => return Err(self.error("expected format kind (table, csv, json)")),
                };
                Ok(Statement::Meta(MetaCommand::Format(kind)))
            }
            Token::DotHelp => Ok(Statement::Meta(MetaCommand::Help)),
            other => Err(self.error_at(&format!("unexpected meta-token {:?}", other))),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect(&Token::Create)?;
        // Check for CREATE INDEX
        if self.check(&Token::Index) {
            return self.parse_create_index();
        }
        self.expect(&Token::Table)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let mut columns = Vec::new();
        let mut constraints = Vec::new();
        while !self.check(&Token::RParen) {
            // Check for table-level constraint
            if self.check(&Token::Check) {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                constraints.push(TableConstraint::Check(expr));
            } else {
                columns.push(self.parse_column_def()?);
            }
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        self.expect(&Token::RParen)?;
        Ok(Statement::CreateTable {
            name,
            columns,
            constraints,
        })
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let col_type = self.parse_column_type()?;
        let mut def = ColumnDef {
            name,
            col_type,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        };
        loop {
            if self.check(&Token::Primary) {
                self.advance();
                self.expect(&Token::Key)?;
                def.primary_key = true;
                def.not_null = true;
                def.unique = true;
            } else if self.check(&Token::NotNull)
                || (self.check(&Token::Not) && self.peek_ahead(1) == Some(&Token::Null))
            {
                if self.check(&Token::Not) {
                    self.advance();
                    self.expect(&Token::Null)?;
                } else {
                    self.advance();
                }
                def.not_null = true;
            } else if self.check(&Token::Unique) {
                self.advance();
                def.unique = true;
            } else if self.check(&Token::Default) {
                self.advance();
                def.default = Some(self.parse_literal_value()?);
            } else {
                break;
            }
        }
        Ok(def)
    }

    fn parse_column_type(&mut self) -> Result<ColumnType> {
        match self.advance() {
            Token::Int | Token::Bigint => Ok(ColumnType::Int),
            Token::Real => Ok(ColumnType::Real),
            Token::Text => Ok(ColumnType::Text),
            Token::Bool => Ok(ColumnType::Bool),
            _ => Err(self.error("expected column type (INT, REAL, TEXT, BOOL)")),
        }
    }

    fn parse_literal_value(&mut self) -> Result<Value> {
        match self.peek() {
            Token::Null => {
                self.advance();
                Ok(Value::Null)
            }
            Token::IntegerLit(n) => {
                let v = *n;
                self.advance();
                Ok(Value::Int(v))
            }
            Token::RealLit(n) => {
                let v = *n;
                self.advance();
                Ok(Value::Real(v))
            }
            Token::StringLit(ref s) => {
                let v = s.clone();
                self.advance();
                Ok(Value::Text(v))
            }
            Token::BoolLit(b) => {
                let v = *b;
                self.advance();
                Ok(Value::Bool(v))
            }
            Token::Minus => {
                self.advance();
                match self.advance() {
                    Token::IntegerLit(n) => Ok(Value::Int(-n)),
                    Token::RealLit(n) => Ok(Value::Real(-n)),
                    _ => Err(self.error("expected number after '-'")),
                }
            }
            _ => Err(self.error("expected a literal value")),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.expect(&Token::Index)?;
        let index_name = self.expect_ident()?;
        self.expect(&Token::On)?;
        let table_name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let column_name = self.expect_ident()?;
        self.expect(&Token::RParen)?;
        Ok(Statement::CreateIndex {
            index_name,
            table_name,
            column_name,
        })
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect(&Token::Drop)?;
        if self.check(&Token::Table) {
            self.advance();
            let name = self.expect_ident()?;
            Ok(Statement::DropTable { name })
        } else if self.check(&Token::Index) {
            self.advance();
            let name = self.expect_ident()?;
            Ok(Statement::DropIndex { name })
        } else {
            Err(self.error("expected TABLE or INDEX after DROP"))
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;
        let table = self.expect_ident()?;
        let columns = if self.check(&Token::LParen) {
            self.advance();
            let mut cols = Vec::new();
            while !self.check(&Token::RParen) {
                cols.push(self.expect_ident()?);
                if self.check(&Token::Comma) {
                    self.advance();
                }
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };
        self.expect(&Token::Values)?;
        let mut values = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let mut row = Vec::new();
            while !self.check(&Token::RParen) {
                row.push(self.parse_expr()?);
                if self.check(&Token::Comma) {
                    self.advance();
                }
            }
            self.expect(&Token::RParen)?;
            values.push(row);
            if !self.check(&Token::Comma) {
                break;
            }
            self.advance();
        }
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select_stmt(&mut self) -> Result<Statement> {
        self.expect(&Token::Select)?;
        // Parse projection
        let mut projection = Vec::new();
        loop {
            if self.check(&Token::Star) {
                self.advance();
                projection.push(SelectItem::Star);
            } else {
                let expr = self.parse_expr()?;
                // Check for table.* pattern
                if self.check(&Token::Dot) && matches!(&expr, Expr::Column { .. }) {
                    self.advance();
                    if self.check(&Token::Star) {
                        self.advance();
                        if let Expr::Column { name, .. } = expr {
                            projection.push(SelectItem::QualifiedStar(name));
                            if self.check(&Token::Comma) {
                                self.advance();
                                continue;
                            }
                            break;
                        }
                    }
                }
                let alias = if self.check(&Token::As) {
                    self.advance();
                    Some(self.expect_ident()?)
                } else if let Token::Ident(_) = self.peek() {
                    // Check it's not a keyword that starts a clause
                    if !self.is_clause_keyword() {
                        Some(self.expect_ident()?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                projection.push(SelectItem::Expr { expr, alias });
            }
            if self.check(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        // FROM
        let from = if self.check(&Token::From) {
            self.advance();
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        // WHERE
        let where_clause = if self.check(&Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        // ORDER BY
        let mut order_by = Vec::new();
        if self.check(&Token::Order) {
            self.advance();
            self.expect(&Token::By)?;
            loop {
                let expr = self.parse_expr()?;
                let asc = if self.check(&Token::Desc) {
                    self.advance();
                    false
                } else if self.check(&Token::Asc) {
                    self.advance();
                    true
                } else {
                    true
                };
                order_by.push(OrderByItem { expr, asc });
                if self.check(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // LIMIT
        let limit = if self.check(&Token::Limit) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        // OFFSET
        let offset = if self.check(&Token::Offset) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Select(SelectStmt {
            projection,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        }))
    }

    fn is_clause_keyword(&self) -> bool {
        matches!(
            self.peek(),
            Token::From
                | Token::Where
                | Token::Order
                | Token::Limit
                | Token::Offset
                | Token::Join
                | Token::Inner
                | Token::Cross
                | Token::Left
                | Token::Right
                | Token::On
                | Token::As
                | Token::Semicolon
                | Token::Eof
        )
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let mut left = self.parse_table_ref()?;
        loop {
            if self.check(&Token::Cross) {
                self.advance();
                self.expect(&Token::Join)?;
                let right = self.parse_table_ref()?;
                left = FromClause::Join {
                    left: Box::new(left),
                    right: Box::new(right),
                    on: None,
                };
            } else if self.check(&Token::Inner) {
                self.advance();
                self.expect(&Token::Join)?;
                let right = self.parse_table_ref()?;
                let on = if self.check(&Token::On) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                left = FromClause::Join {
                    left: Box::new(left),
                    right: Box::new(right),
                    on,
                };
            } else if self.check(&Token::Join) {
                self.advance();
                let right = self.parse_table_ref()?;
                let on = if self.check(&Token::On) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                left = FromClause::Join {
                    left: Box::new(left),
                    right: Box::new(right),
                    on,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_table_ref(&mut self) -> Result<FromClause> {
        let name = self.expect_ident()?;
        let alias = if self.check(&Token::As) {
            self.advance();
            Some(self.expect_ident()?)
        } else if let Token::Ident(_) = self.peek() {
            if !self.is_clause_keyword() {
                Some(self.expect_ident()?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(FromClause::Table { name, alias })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect(&Token::Update)?;
        let table = self.expect_ident()?;
        self.expect(&Token::Set)?;
        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((col, expr));
            if self.check(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let where_clause = if self.check(&Token::Where) {
            self.advance();
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
        self.expect(&Token::Delete)?;
        self.expect(&Token::From)?;
        let table = self.expect_ident()?;
        let where_clause = if self.check(&Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete {
            table,
            where_clause,
        })
    }

    // Expression parsing (precedence climbing)
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.check(&Token::Not) {
            self.advance();
            let expr = self.parse_comparison()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_concat()?;
        loop {
            let op = match self.peek() {
                Token::Eq => BinaryOp::Eq,
                Token::Neq => BinaryOp::Neq,
                Token::Lt => BinaryOp::Lt,
                Token::Le => BinaryOp::Le,
                Token::Gt => BinaryOp::Gt,
                Token::Ge => BinaryOp::Ge,
                Token::Is => {
                    self.advance();
                    if self.check(&Token::Not) {
                        self.advance();
                        self.expect(&Token::Null)?;
                        return Ok(Expr::IsNotNull(Box::new(left)));
                    } else {
                        self.expect(&Token::Null)?;
                        return Ok(Expr::IsNull(Box::new(left)));
                    }
                }
                _ => break,
            };
            self.advance();
            let right = self.parse_concat()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while self.check(&Token::Concat) {
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Concat,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.check(&Token::Minus) {
            self.advance();
            let expr = self.parse_primary()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::IntegerLit(n) => {
                let v = n;
                self.advance();
                Ok(Expr::Literal(Value::Int(v)))
            }
            Token::RealLit(n) => {
                let v = n;
                self.advance();
                Ok(Expr::Literal(Value::Real(v)))
            }
            Token::StringLit(ref s) => {
                let v = s.clone();
                self.advance();
                Ok(Expr::Literal(Value::Text(v)))
            }
            Token::BoolLit(b) => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(b)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Ident(ref name) => {
                let name = name.clone();
                self.advance();
                // Check for function call
                if self.check(&Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.check(&Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Function {
                        name: name.to_uppercase(),
                        args,
                    })
                } else if self.check(&Token::Dot) {
                    // qualified column: table.col
                    self.advance();
                    let col = self.expect_ident()?;
                    Ok(Expr::Column {
                        table: Some(name),
                        name: col,
                    })
                } else {
                    Ok(Expr::Column { table: None, name })
                }
            }
            _ => Err(self.error("expected expression")),
        }
    }

    // Helpers
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_ahead(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn check(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(expected)
    }

    fn expect(&mut self, expected: &Token) -> Result<Token> {
        if self.check(expected) {
            Ok(self.advance())
        } else {
            Err(MiniDbError::Parse {
                message: format!(
                    "expected {}, found {}",
                    expected.description(),
                    self.peek().description()
                ),
                line: None,
                column: None,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            // Allow some keywords as identifiers for table/column names
            Token::Int | Token::Bigint | Token::Real | Token::Text | Token::Bool => {
                let s = format!("{:?}", self.peek());
                self.advance();
                Ok(s)
            }
            _ => Err(self.error("expected identifier")),
        }
    }

    fn error(&self, msg: &str) -> MiniDbError {
        MiniDbError::Parse {
            message: msg.to_string(),
            line: None,
            column: None,
        }
    }

    fn error_at(&self, msg: &str) -> MiniDbError {
        MiniDbError::Parse {
            message: msg.to_string(),
            line: None,
            column: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(input: &str) -> Statement {
        let stmts = Parser::parse_all(input).unwrap();
        assert_eq!(stmts.len(), 1, "expected 1 statement, got {}", stmts.len());
        stmts.into_iter().next().unwrap()
    }

    #[test]
    fn test_parse_create_table() {
        let stmt = parse_one("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);");
        match stmt {
            Statement::CreateTable { name, columns, .. } => {
                assert_eq!(name, "t");
                assert_eq!(columns.len(), 2);
                assert!(columns[0].primary_key);
                assert!(columns[1].not_null);
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn test_parse_select_star() {
        let stmt = parse_one("SELECT * FROM t;");
        match stmt {
            Statement::Select(s) => {
                assert_eq!(s.projection.len(), 1);
                assert!(matches!(s.projection[0], SelectItem::Star));
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_select_where() {
        let stmt = parse_one("SELECT a, b FROM t WHERE a > 10 ORDER BY b DESC LIMIT 5;");
        match stmt {
            Statement::Select(s) => {
                assert_eq!(s.projection.len(), 2);
                assert!(s.where_clause.is_some());
                assert_eq!(s.order_by.len(), 1);
                assert!(s.limit.is_some());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmt = parse_one("INSERT INTO t (id, name) VALUES (1, 'hello');");
        match stmt {
            Statement::Insert {
                table,
                columns,
                values,
            } => {
                assert_eq!(table, "t");
                assert!(columns.is_some());
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].len(), 2);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmt = parse_one("UPDATE t SET a = 1, b = 'x' WHERE id = 5;");
        match stmt {
            Statement::Update {
                table,
                assignments,
                where_clause,
            } => {
                assert_eq!(table, "t");
                assert_eq!(assignments.len(), 2);
                assert!(where_clause.is_some());
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmt = parse_one("DELETE FROM t WHERE id = 5;");
        match stmt {
            Statement::Delete {
                table,
                where_clause,
            } => {
                assert_eq!(table, "t");
                assert!(where_clause.is_some());
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_parse_transactions() {
        assert!(matches!(parse_one("BEGIN;"), Statement::Begin));
        assert!(matches!(parse_one("COMMIT;"), Statement::Commit));
        assert!(matches!(parse_one("ROLLBACK;"), Statement::Rollback));
    }

    #[test]
    fn test_parse_join() {
        let stmt = parse_one("SELECT * FROM a INNER JOIN b ON a.id = b.aid;");
        match stmt {
            Statement::Select(s) => {
                assert!(s.from.is_some());
                match s.from.unwrap() {
                    FromClause::Join { on, .. } => {
                        assert!(on.is_some());
                    }
                    _ => panic!("expected Join"),
                }
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_create_index() {
        let stmt = parse_one("CREATE INDEX idx_name ON t (col);");
        match stmt {
            Statement::CreateIndex {
                index_name,
                table_name,
                column_name,
            } => {
                assert_eq!(index_name, "idx_name");
                assert_eq!(table_name, "t");
                assert_eq!(column_name, "col");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_parse_functions() {
        let stmt = parse_one("SELECT LENGTH(name), UPPER(name) FROM t;");
        match stmt {
            Statement::Select(s) => {
                assert_eq!(s.projection.len(), 2);
            }
            _ => panic!("expected Select"),
        }
    }
}
