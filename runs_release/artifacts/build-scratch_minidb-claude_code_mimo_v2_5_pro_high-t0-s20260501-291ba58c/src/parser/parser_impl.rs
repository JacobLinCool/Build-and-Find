use crate::error::{MiniDbError, Result};
use crate::lexer::Token;
use crate::parser::ast::*;
use crate::types::{ColumnType, Value};

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        let tok = self.advance();
        if &tok == expected {
            Ok(())
        } else {
            Err(MiniDbError::Syntax(format!(
                "expected {expected:?}, got {tok:?}"
            )))
        }
    }

    fn expect_identifier(&mut self) -> Result<String> {
        match self.advance() {
            Token::Identifier(s) => Ok(s),
            tok => Err(MiniDbError::Syntax(format!(
                "expected identifier, got {tok:?}"
            ))),
        }
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek().clone() {
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Insert => self.parse_insert(),
            Token::Select => self.parse_select(),
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
                let kind = self.expect_identifier()?;
                Ok(Statement::MetaFormat(kind))
            }
            tok => Err(MiniDbError::Syntax(format!("unexpected token: {tok:?}"))),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.advance(); // CREATE
        match self.peek().clone() {
            Token::Table => self.parse_create_table(),
            Token::Index => self.parse_create_index(),
            tok => Err(MiniDbError::Syntax(format!(
                "expected TABLE or INDEX after CREATE, got {tok:?}"
            ))),
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        self.advance(); // TABLE
        let name = self.expect_identifier()?;
        self.expect(&Token::LeftParen)?;

        let mut columns = Vec::new();
        let mut check = None;

        loop {
            if self.peek() == &Token::Check {
                self.advance(); // CHECK
                self.expect(&Token::LeftParen)?;
                check = Some(self.parse_or_expr()?);
                self.expect(&Token::RightParen)?;
            } else {
                columns.push(self.parse_column_def()?);
            }
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(&Token::RightParen)?;
        Ok(Statement::CreateTable(CreateTableStmt {
            name,
            columns,
            check,
        }))
    }

    fn parse_column_def(&mut self) -> Result<ColumnDefAst> {
        let name = self.expect_identifier()?;
        let col_type = self.parse_column_type()?;
        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;

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
                Token::NotNull => {
                    self.advance();
                    not_null = true;
                }
                Token::Unique => {
                    self.advance();
                    unique = true;
                }
                Token::Default => {
                    self.advance();
                    default = Some(self.parse_literal_value()?);
                }
                _ => break,
            }
        }

        Ok(ColumnDefAst {
            name,
            col_type,
            primary_key,
            not_null,
            unique,
            default,
        })
    }

    fn parse_column_type(&mut self) -> Result<ColumnType> {
        match self.advance() {
            Token::Int | Token::Bigint => Ok(ColumnType::Int),
            Token::Real => Ok(ColumnType::Real),
            Token::Text => Ok(ColumnType::Text),
            Token::Bool => Ok(ColumnType::Bool),
            tok => Err(MiniDbError::Syntax(format!(
                "expected column type, got {tok:?}"
            ))),
        }
    }

    fn parse_literal_value(&mut self) -> Result<Value> {
        match self.peek().clone() {
            Token::IntegerLiteral(i) => {
                self.advance();
                Ok(Value::Int(i))
            }
            Token::RealLiteral(r) => {
                self.advance();
                Ok(Value::Real(r))
            }
            Token::StringLiteral(s) => {
                self.advance();
                Ok(Value::Text(s))
            }
            Token::True => {
                self.advance();
                Ok(Value::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Value::Bool(false))
            }
            Token::Null => {
                self.advance();
                Ok(Value::Null)
            }
            Token::Minus => {
                self.advance();
                match self.advance() {
                    Token::IntegerLiteral(i) => Ok(Value::Int(-i)),
                    Token::RealLiteral(r) => Ok(Value::Real(-r)),
                    tok => Err(MiniDbError::Syntax(format!(
                        "expected number after '-', got {tok:?}"
                    ))),
                }
            }
            tok => Err(MiniDbError::Syntax(format!(
                "expected literal value, got {tok:?}"
            ))),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.advance(); // INDEX
        let name = self.expect_identifier()?;
        self.expect(&Token::On)?;
        let table = self.expect_identifier()?;
        self.expect(&Token::LeftParen)?;
        let column = self.expect_identifier()?;
        self.expect(&Token::RightParen)?;
        Ok(Statement::CreateIndex(CreateIndexStmt {
            name,
            table,
            column,
        }))
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.advance(); // DROP
        match self.peek().clone() {
            Token::Table => {
                self.advance();
                let name = self.expect_identifier()?;
                Ok(Statement::DropTable(DropTableStmt { name }))
            }
            Token::Index => {
                self.advance();
                let name = self.expect_identifier()?;
                Ok(Statement::DropIndex(DropIndexStmt { name }))
            }
            tok => Err(MiniDbError::Syntax(format!(
                "expected TABLE or INDEX after DROP, got {tok:?}"
            ))),
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.advance(); // INSERT
        self.expect(&Token::Into)?;
        let table = self.expect_identifier()?;

        let columns = if self.peek() == &Token::LeftParen {
            self.advance();
            let mut cols = vec![self.expect_identifier()?];
            while self.peek() == &Token::Comma {
                self.advance();
                cols.push(self.expect_identifier()?);
            }
            self.expect(&Token::RightParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;

        let mut values_rows = Vec::new();
        loop {
            self.expect(&Token::LeftParen)?;
            let mut row = vec![self.parse_expr()?];
            while self.peek() == &Token::Comma {
                self.advance();
                row.push(self.parse_expr()?);
            }
            self.expect(&Token::RightParen)?;
            values_rows.push(row);
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Statement::Insert(InsertStmt {
            table,
            columns,
            values: values_rows,
        }))
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.advance(); // SELECT

        let projections = self.parse_projections()?;
        let from = if self.peek() == &Token::From {
            self.advance();
            Some(self.parse_from()?)
        } else {
            None
        };

        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_or_expr()?)
        } else {
            None
        };

        let order_by = if self.peek() == &Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            Some(self.parse_order_by()?)
        } else {
            None
        };

        let limit = if self.peek() == &Token::Limit {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let offset = if self.peek() == &Token::Offset {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Select(SelectStmt {
            projections,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        }))
    }

    fn parse_projections(&mut self) -> Result<Vec<Projection>> {
        let mut projs = Vec::new();
        loop {
            if self.peek() == &Token::Star {
                self.advance();
                projs.push(Projection::Star);
            } else {
                let expr = self.parse_expr()?;
                let alias = if self.peek() == &Token::As {
                    self.advance();
                    Some(self.expect_identifier()?)
                } else if let Token::Identifier(_) = self.peek() {
                    // Could be alias without AS
                    Some(self.expect_identifier()?)
                } else {
                    None
                };
                if let Expr::ColumnRef { table, name } = &expr {
                    projs.push(Projection::Column {
                        table: table.clone(),
                        name: name.clone(),
                        alias,
                    });
                } else {
                    projs.push(Projection::Expr { expr, alias });
                }
            }
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        Ok(projs)
    }

    fn parse_from(&mut self) -> Result<FromClause> {
        let mut left = self.parse_from_atom()?;

        loop {
            if self.peek() == &Token::Inner || self.peek() == &Token::Join {
                if self.peek() == &Token::Inner {
                    self.advance();
                }
                self.expect(&Token::Join)?;
                let right = self.parse_from_atom()?;
                self.expect(&Token::On)?;
                let on = self.parse_or_expr()?;
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

    fn parse_from_atom(&mut self) -> Result<FromClause> {
        let name = self.expect_identifier()?;
        let alias = if self.peek() == &Token::As {
            self.advance();
            Some(self.expect_identifier()?)
        } else if let Token::Identifier(_) = self.peek() {
            Some(self.expect_identifier()?)
        } else {
            None
        };
        Ok(FromClause::Table { name, alias })
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByExpr>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let asc = match self.peek() {
                Token::Asc => {
                    self.advance();
                    true
                }
                Token::Desc => {
                    self.advance();
                    false
                }
                _ => true,
            };
            items.push(OrderByExpr { expr, asc });
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        Ok(items)
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.advance(); // UPDATE
        let table = self.expect_identifier()?;
        self.expect(&Token::Set)?;

        let mut assignments = Vec::new();
        loop {
            let col = self.expect_identifier()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((col, expr));
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_or_expr()?)
        } else {
            None
        };

        Ok(Statement::Update(UpdateStmt {
            table,
            assignments,
            where_clause,
        }))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.advance(); // DELETE
        self.expect(&Token::From)?;
        let table = self.expect_identifier()?;
        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_or_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete(DeleteStmt {
            table,
            where_clause,
        }))
    }

    // Expression parsing (precedence climbing)
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while self.peek() == &Token::Or {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_not_expr()?;
        while self.peek() == &Token::And {
            self.advance();
            let right = self.parse_not_expr()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not_expr(&mut self) -> Result<Expr> {
        if self.peek() == &Token::Not {
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
        let mut left = self.parse_addition()?;

        loop {
            match self.peek() {
                Token::Eq => {
                    self.advance();
                    let right = self.parse_addition()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Eq,
                        right: Box::new(right),
                    };
                }
                Token::Ne => {
                    self.advance();
                    let right = self.parse_addition()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Ne,
                        right: Box::new(right),
                    };
                }
                Token::Lt => {
                    self.advance();
                    let right = self.parse_addition()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Lt,
                        right: Box::new(right),
                    };
                }
                Token::Le => {
                    self.advance();
                    let right = self.parse_addition()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Le,
                        right: Box::new(right),
                    };
                }
                Token::Gt => {
                    self.advance();
                    let right = self.parse_addition()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Gt,
                        right: Box::new(right),
                    };
                }
                Token::Ge => {
                    self.advance();
                    let right = self.parse_addition()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Ge,
                        right: Box::new(right),
                    };
                }
                Token::Is => {
                    self.advance();
                    if self.peek() == &Token::Not {
                        self.advance();
                        self.expect(&Token::Null)?;
                        left = Expr::IsNotNull(Box::new(left));
                    } else {
                        self.expect(&Token::Null)?;
                        left = Expr::IsNull(Box::new(left));
                    }
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplication()?;
        loop {
            match self.peek() {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_multiplication()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Add,
                        right: Box::new(right),
                    };
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_multiplication()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Sub,
                        right: Box::new(right),
                    };
                }
                Token::Concat => {
                    self.advance();
                    let right = self.parse_multiplication()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Concat,
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Token::Star => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Mul,
                        right: Box::new(right),
                    };
                }
                Token::Slash => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOp::Div,
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::IntegerLiteral(i) => {
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
            Token::True => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(false)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }
            Token::LeftParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(expr)
            }
            Token::Identifier(name) => {
                self.advance();
                // Check for function call
                if self.peek() == &Token::LeftParen {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != &Token::RightParen {
                        args.push(self.parse_expr()?);
                        while self.peek() == &Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::Function {
                        name: name.to_uppercase(),
                        args,
                    })
                } else if self.peek() == &Token::Dot {
                    self.advance();
                    let col = self.expect_identifier()?;
                    Ok(Expr::ColumnRef {
                        table: Some(name),
                        name: col,
                    })
                } else {
                    Ok(Expr::ColumnRef { table: None, name })
                }
            }
            tok => Err(MiniDbError::Syntax(format!(
                "unexpected token in expression: {tok:?}"
            ))),
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Vec<Statement>> {
    let mut parser = Parser::new(tokens);
    let mut stmts = Vec::new();

    while parser.peek() != &Token::Eof {
        let stmt = parser.parse_statement()?;
        // Consume optional semicolon
        if parser.peek() == &Token::Semicolon {
            parser.advance();
        }
        stmts.push(stmt);
    }

    Ok(stmts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_sql(sql: &str) -> Result<Vec<Statement>> {
        let tokens = tokenize(sql)?;
        parse(tokens)
    }

    #[test]
    fn test_parse_create_table() {
        let stmts =
            parse_sql("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);").unwrap();
        assert_eq!(stmts.len(), 1);
        if let Statement::CreateTable(ct) = &stmts[0] {
            assert_eq!(ct.name, "users");
            assert_eq!(ct.columns.len(), 2);
            assert!(ct.columns[0].primary_key);
            assert!(ct.columns[1].not_null);
        } else {
            panic!("expected CreateTable");
        }
    }

    #[test]
    fn test_parse_select() {
        let stmts =
            parse_sql("SELECT id, name FROM users WHERE id = 1 ORDER BY name ASC LIMIT 10;")
                .unwrap();
        if let Statement::Select(s) = &stmts[0] {
            assert_eq!(s.projections.len(), 2);
            assert!(s.where_clause.is_some());
            assert!(s.order_by.is_some());
            assert!(s.limit.is_some());
        } else {
            panic!("expected Select");
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmts = parse_sql("INSERT INTO users (id, name) VALUES (1, 'alice');").unwrap();
        if let Statement::Insert(i) = &stmts[0] {
            assert_eq!(i.table, "users");
            assert_eq!(i.columns.as_ref().unwrap().len(), 2);
            assert_eq!(i.values.len(), 1);
        } else {
            panic!("expected Insert");
        }
    }

    #[test]
    fn test_parse_join() {
        let stmts =
            parse_sql("SELECT a.id, b.name FROM users a INNER JOIN orders b ON a.id = b.user_id;")
                .unwrap();
        if let Statement::Select(s) = &stmts[0] {
            assert!(s.from.is_some());
            if let Some(FromClause::Join { .. }) = &s.from {
                // ok
            } else {
                panic!("expected Join");
            }
        } else {
            panic!("expected Select");
        }
    }

    #[test]
    fn test_parse_update() {
        let stmts = parse_sql("UPDATE users SET name = 'bob' WHERE id = 1;").unwrap();
        if let Statement::Update(u) = &stmts[0] {
            assert_eq!(u.table, "users");
            assert_eq!(u.assignments.len(), 1);
        } else {
            panic!("expected Update");
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmts = parse_sql("DELETE FROM users WHERE id = 1;").unwrap();
        if let Statement::Delete(d) = &stmts[0] {
            assert_eq!(d.table, "users");
        } else {
            panic!("expected Delete");
        }
    }

    #[test]
    fn test_parse_expressions() {
        let stmts = parse_sql("SELECT 1 + 2 * 3 FROM t;").unwrap();
        if let Statement::Select(s) = &stmts[0] {
            if let Projection::Expr { expr, .. } = &s.projections[0] {
                if let Expr::BinaryOp { op, .. } = expr {
                    assert_eq!(*op, BinaryOp::Add);
                }
            }
        }
    }

    #[test]
    fn test_parse_is_null() {
        let stmts = parse_sql("SELECT * FROM t WHERE col IS NULL;").unwrap();
        if let Statement::Select(s) = &stmts[0] {
            assert!(s.where_clause.is_some());
        }
    }

    #[test]
    fn test_parse_functions() {
        let stmts = parse_sql("SELECT LENGTH(name), COALESCE(a, b) FROM t;").unwrap();
        if let Statement::Select(s) = &stmts[0] {
            assert_eq!(s.projections.len(), 2);
        }
    }
}
