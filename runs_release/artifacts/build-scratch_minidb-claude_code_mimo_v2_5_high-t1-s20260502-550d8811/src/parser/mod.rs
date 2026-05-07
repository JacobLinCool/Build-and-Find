pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::Token;
use crate::types::{ColumnDef, ColumnType};
use ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_semicolons();
            if self.peek() == Token::Eof {
                break;
            }
            stmts.push(self.parse_statement()?);
        }
        Ok(stmts)
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn _peek_ahead(&self, offset: usize) -> Token {
        self.tokens
            .get(self.pos + offset)
            .cloned()
            .unwrap_or(Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek();
        if tok != Token::Eof {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        let tok = self.advance();
        if &tok == expected {
            Ok(())
        } else {
            Err(MiniDbError::Parse(format!(
                "expected {:?}, got {:?}",
                expected, tok
            )))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            t => Err(MiniDbError::Parse(format!(
                "expected identifier, got {:?}",
                t
            ))),
        }
    }

    fn skip_semicolons(&mut self) {
        while self.peek() == Token::Semicolon {
            self.advance();
        }
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek() {
            Token::SELECT => self.parse_select(),
            Token::INSERT => self.parse_insert(),
            Token::UPDATE => self.parse_update(),
            Token::DELETE => self.parse_delete(),
            Token::CREATE => self.parse_create(),
            Token::DROP => self.parse_drop(),
            Token::BEGIN => {
                self.advance();
                Ok(Statement::Begin)
            }
            Token::COMMIT => {
                self.advance();
                Ok(Statement::Commit)
            }
            Token::ROLLBACK => {
                self.advance();
                Ok(Statement::Rollback)
            }
            _ => Err(MiniDbError::Parse(format!(
                "unexpected token: {:?}",
                self.peek()
            ))),
        }
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.advance(); // SELECT
        let projection = self.parse_select_list()?;

        let from = if self.peek() == Token::FROM {
            self.advance();
            self.parse_from_clause()?
        } else {
            vec![]
        };

        let where_clause = if self.peek() == Token::WHERE {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.peek() == Token::ORDER {
            self.advance();
            self.expect(&Token::BY)?;
            self.parse_order_by()?
        } else {
            vec![]
        };

        let limit = if self.peek() == Token::LIMIT {
            self.advance();
            Some(self.parse_limit()?)
        } else {
            None
        };

        Ok(Statement::Select(SelectStatement {
            projection,
            from,
            where_clause,
            order_by,
            limit,
        }))
    }

    fn parse_select_list(&mut self) -> Result<Vec<SelectItem>> {
        let mut items = Vec::new();
        if self.peek() == Token::Star {
            self.advance();
            items.push(SelectItem::Star);
        } else {
            items.push(self.parse_select_item()?);
            while self.peek() == Token::Comma {
                self.advance();
                items.push(self.parse_select_item()?);
            }
        }
        Ok(items)
    }

    fn parse_select_item(&mut self) -> Result<SelectItem> {
        let expr = self.parse_expr()?;
        let alias = if self.peek() == Token::AS {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(SelectItem::Expr { expr, alias })
    }

    fn parse_from_clause(&mut self) -> Result<Vec<FromItem>> {
        let mut items = Vec::new();
        items.push(self.parse_from_item()?);

        while self.peek() == Token::INNER || self.peek() == Token::JOIN {
            if self.peek() == Token::INNER {
                self.advance();
            }
            self.expect(&Token::JOIN)?;
            let right = self.parse_from_item()?;
            self.expect(&Token::ON)?;
            let on = self.parse_expr()?;
            items.push(FromItem::Join {
                right: Box::new(right),
                on,
            });
        }

        Ok(items)
    }

    fn parse_from_item(&mut self) -> Result<FromItem> {
        let name = self.expect_ident()?;
        let alias = if self.peek() == Token::AS {
            self.advance();
            Some(self.expect_ident()?)
        } else if matches!(self.peek(), Token::Ident(_)) {
            // Implicit alias
            let alias = self.expect_ident()?;
            // Make sure it's not a keyword
            Some(alias)
        } else {
            None
        };
        Ok(FromItem::Table { name, alias })
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = Vec::new();
        items.push(self.parse_order_by_item()?);
        while self.peek() == Token::Comma {
            self.advance();
            items.push(self.parse_order_by_item()?);
        }
        Ok(items)
    }

    fn parse_order_by_item(&mut self) -> Result<OrderByItem> {
        let expr = self.parse_expr()?;
        let direction = if self.peek() == Token::DESC {
            self.advance();
            OrderDirection::Desc
        } else {
            if self.peek() == Token::ASC {
                self.advance();
            }
            OrderDirection::Asc
        };
        Ok(OrderByItem { expr, direction })
    }

    fn parse_limit(&mut self) -> Result<LimitClause> {
        let count = self.parse_expr()?;
        let offset = if self.peek() == Token::OFFSET {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(LimitClause { count, offset })
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.advance(); // INSERT
        self.expect(&Token::INTO)?;
        let table = self.expect_ident()?;

        let columns = if self.peek() == Token::LParen {
            self.advance();
            let mut cols = Vec::new();
            cols.push(self.expect_ident()?);
            while self.peek() == Token::Comma {
                self.advance();
                cols.push(self.expect_ident()?);
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::VALUES)?;
        let mut rows = Vec::new();
        rows.push(self.parse_value_list()?);
        while self.peek() == Token::Comma {
            self.advance();
            rows.push(self.parse_value_list()?);
        }

        Ok(Statement::Insert(InsertStatement {
            table,
            columns,
            rows,
        }))
    }

    fn parse_value_list(&mut self) -> Result<Vec<Expr>> {
        self.expect(&Token::LParen)?;
        let mut values = Vec::new();
        values.push(self.parse_expr()?);
        while self.peek() == Token::Comma {
            self.advance();
            values.push(self.parse_expr()?);
        }
        self.expect(&Token::RParen)?;
        Ok(values)
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.advance(); // UPDATE
        let table = self.expect_ident()?;
        self.expect(&Token::SET)?;

        let mut assignments = Vec::new();
        loop {
            let column = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let value = self.parse_expr()?;
            assignments.push(Assignment { column, value });
            if self.peek() != Token::Comma {
                break;
            }
            self.advance();
        }

        let where_clause = if self.peek() == Token::WHERE {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Update(UpdateStatement {
            table,
            assignments,
            where_clause,
        }))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.advance(); // DELETE
        self.expect(&Token::FROM)?;
        let table = self.expect_ident()?;

        let where_clause = if self.peek() == Token::WHERE {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Delete(DeleteStatement {
            table,
            where_clause,
        }))
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.advance(); // CREATE
        match self.peek() {
            Token::TABLE => {
                self.advance();
                self.parse_create_table()
            }
            Token::INDEX => {
                self.advance();
                self.parse_create_index()
            }
            _ => Err(MiniDbError::Parse(format!(
                "expected TABLE or INDEX after CREATE, got {:?}",
                self.peek()
            ))),
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;

        let mut columns = Vec::new();
        let mut table_constraints: Vec<TableConstraint> = Vec::new();

        loop {
            if self.peek() == Token::RParen {
                break;
            }

            // Check if this is a table constraint
            if matches!(self.peek(), Token::PRIMARY)
                || matches!(self.peek(), Token::CHECK)
                || matches!(self.peek(), Token::UNIQUE)
            {
                match self.peek() {
                    Token::PRIMARY => {
                        self.advance();
                        self.expect(&Token::KEY)?;
                        self.expect(&Token::LParen)?;
                        let col = self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                        table_constraints.push(TableConstraint::PrimaryKey(col));
                    }
                    Token::CHECK => {
                        self.advance();
                        self.expect(&Token::LParen)?;
                        let expr = self.parse_expr()?;
                        self.expect(&Token::RParen)?;
                        table_constraints.push(TableConstraint::Check(expr));
                    }
                    Token::UNIQUE => {
                        self.advance();
                        self.expect(&Token::LParen)?;
                        let col = self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                        table_constraints.push(TableConstraint::Unique(col));
                    }
                    _ => unreachable!(),
                }
            } else {
                let col_name = self.expect_ident()?;
                let col_type = self.parse_column_type()?;

                let mut primary_key = false;
                let mut not_null = false;
                let mut unique = false;
                let mut default = None;
                let mut check = None;

                loop {
                    match self.peek() {
                        Token::PRIMARY => {
                            self.advance();
                            self.expect(&Token::KEY)?;
                            primary_key = true;
                        }
                        Token::NOT => {
                            self.advance();
                            self.expect(&Token::NULL)?;
                            not_null = true;
                        }
                        Token::UNIQUE => {
                            self.advance();
                            unique = true;
                        }
                        Token::DEFAULT => {
                            self.advance();
                            default = Some(self.parse_literal()?);
                        }
                        Token::CHECK => {
                            self.advance();
                            self.expect(&Token::LParen)?;
                            check = Some(self.parse_expr()?);
                            self.expect(&Token::RParen)?;
                        }
                        _ => break,
                    }
                }

                let mut col = ColumnDef::new(col_name, col_type);
                col.primary_key = primary_key;
                col.not_null = not_null || primary_key;
                col.unique = unique || primary_key;
                col.default = default;
                if let Some(check_expr) = check {
                    col.check = Some(format!("{:?}", check_expr));
                }
                columns.push(col);
            }

            if self.peek() == Token::Comma {
                self.advance();
            }
        }

        self.expect(&Token::RParen)?;

        // Apply table constraints
        for tc in table_constraints {
            match tc {
                TableConstraint::PrimaryKey(col) => {
                    if let Some(c) = columns.iter_mut().find(|c| c.name == col) {
                        c.primary_key = true;
                        c.not_null = true;
                        c.unique = true;
                    }
                }
                TableConstraint::Unique(_col) => {
                    // For now, we'll track uniqueness at the column level
                }
                TableConstraint::Check(_expr) => {
                    // TODO: store check constraint
                }
            }
        }

        Ok(Statement::CreateTable(CreateTableStatement {
            name,
            columns,
        }))
    }

    fn parse_column_type(&mut self) -> Result<ColumnType> {
        match self.advance() {
            Token::Ident(s) => match s.to_uppercase().as_str() {
                "INT" | "INTEGER" => Ok(ColumnType::Int),
                "BIGINT" => Ok(ColumnType::Bigint),
                "REAL" | "FLOAT" | "DOUBLE" => Ok(ColumnType::Real),
                "TEXT" | "VARCHAR" | "STRING" => Ok(ColumnType::Text),
                "BOOL" | "BOOLEAN" => Ok(ColumnType::Bool),
                _ => Err(MiniDbError::Parse(format!("unknown type: {}", s))),
            },
            t => Err(MiniDbError::Parse(format!("expected type, got {:?}", t))),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect(&Token::ON)?;
        let table = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let column = self.expect_ident()?;
        self.expect(&Token::RParen)?;

        Ok(Statement::CreateIndex(CreateIndexStatement {
            name,
            table,
            column,
        }))
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.advance(); // DROP
        match self.peek() {
            Token::TABLE => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(Statement::DropTable(DropTableStatement { name }))
            }
            Token::INDEX => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(Statement::DropIndex(DropIndexStatement { name }))
            }
            _ => Err(MiniDbError::Parse(format!(
                "expected TABLE or INDEX after DROP"
            ))),
        }
    }

    // Expression parser with precedence climbing
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while self.peek() == Token::OR {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_not_expr()?;
        while self.peek() == Token::AND {
            self.advance();
            let right = self.parse_not_expr()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not_expr(&mut self) -> Result<Expr> {
        if self.peek() == Token::NOT {
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
        let left = self.parse_concat()?;

        // IS [NOT] NULL
        if self.peek() == Token::IS {
            self.advance();
            if self.peek() == Token::NOT {
                self.advance();
                self.expect(&Token::NULL)?;
                return Ok(Expr::BinaryOp {
                    op: BinOp::IsNotNull,
                    left: Box::new(left),
                    right: Box::new(Expr::Literal(Literal::Null)),
                });
            }
            self.expect(&Token::NULL)?;
            return Ok(Expr::BinaryOp {
                op: BinOp::IsNull,
                left: Box::new(left),
                right: Box::new(Expr::Literal(Literal::Null)),
            });
        }

        let op = match self.peek() {
            Token::Eq => BinOp::Eq,
            Token::Ne => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::Le => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::Ge => BinOp::Ge,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_concat()?;
        Ok(Expr::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while self.peek() == Token::Concat {
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                op: BinOp::Concat,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.peek() {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = Expr::BinaryOp {
                        op: BinOp::Add,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = Expr::BinaryOp {
                        op: BinOp::Sub,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Token::Star => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        op: BinOp::Mul,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Slash => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        op: BinOp::Div,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Percent => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        op: BinOp::Mod,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek() == Token::Minus {
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
            Token::Integer(v) => {
                self.advance();
                Ok(Expr::Literal(Literal::Integer(v)))
            }
            Token::Float(v) => {
                self.advance();
                Ok(Expr::Literal(Literal::Float(v)))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Literal(Literal::String(s)))
            }
            Token::TRUE => {
                self.advance();
                Ok(Expr::Literal(Literal::Bool(true)))
            }
            Token::FALSE => {
                self.advance();
                Ok(Expr::Literal(Literal::Bool(false)))
            }
            Token::NULL => {
                self.advance();
                Ok(Expr::Literal(Literal::Null))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Ident(name) => {
                self.advance();
                // Check for function call
                if self.peek() == Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Function { name, args })
                }
                // Check for table.column
                else if self.peek() == Token::Dot {
                    self.advance();
                    let col = self.expect_ident()?;
                    Ok(Expr::ColumnRef {
                        table: Some(name),
                        column: col,
                    })
                } else {
                    Ok(Expr::ColumnRef {
                        table: None,
                        column: name,
                    })
                }
            }
            Token::Star => {
                self.advance();
                Ok(Expr::Star)
            }
            t => Err(MiniDbError::Parse(format!(
                "unexpected token in expression: {:?}",
                t
            ))),
        }
    }

    fn parse_literal(&mut self) -> Result<crate::types::Value> {
        match self.peek().clone() {
            Token::Integer(v) => {
                self.advance();
                Ok(crate::types::Value::Int(v))
            }
            Token::Float(v) => {
                self.advance();
                Ok(crate::types::Value::Real(crate::types::RealOrd(v)))
            }
            Token::Str(s) => {
                self.advance();
                Ok(crate::types::Value::Text(s))
            }
            Token::TRUE => {
                self.advance();
                Ok(crate::types::Value::Bool(true))
            }
            Token::FALSE => {
                self.advance();
                Ok(crate::types::Value::Bool(false))
            }
            Token::NULL => {
                self.advance();
                Ok(crate::types::Value::Null)
            }
            Token::Minus => {
                self.advance();
                match self.peek() {
                    Token::Integer(v) => {
                        let val = v;
                        self.advance();
                        Ok(crate::types::Value::Int(-val))
                    }
                    Token::Float(v) => {
                        let val = v;
                        self.advance();
                        Ok(crate::types::Value::Real(crate::types::RealOrd(-val)))
                    }
                    _ => Err(MiniDbError::Parse("expected number after '-'".into())),
                }
            }
            _ => Err(MiniDbError::Parse(format!(
                "expected literal, got {:?}",
                self.peek()
            ))),
        }
    }
}

#[derive(Debug)]
enum TableConstraint {
    PrimaryKey(String),
    Unique(String),
    Check(Expr),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(sql: &str) -> Vec<Statement> {
        let mut lexer = Lexer::new(sql);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse_statements().unwrap()
    }

    #[test]
    fn test_parse_select() {
        let stmts = parse("SELECT id, name FROM users WHERE id = 1;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.from.len(), 1);
                assert!(s.where_clause.is_some());
            }
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmts = parse("INSERT INTO users (id, name) VALUES (1, 'Alice');");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Insert(s) => {
                assert_eq!(s.table, "users");
                assert_eq!(s.columns.as_ref().unwrap().len(), 2);
            }
            _ => panic!("expected INSERT"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let stmts = parse("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::CreateTable(s) => {
                assert_eq!(s.name, "users");
                assert_eq!(s.columns.len(), 2);
                assert!(s.columns[0].primary_key);
                assert!(s.columns[1].not_null);
            }
            _ => panic!("expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmts = parse("UPDATE users SET name = 'Bob' WHERE id = 1;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Update(s) => {
                assert_eq!(s.table, "users");
                assert_eq!(s.assignments.len(), 1);
                assert!(s.where_clause.is_some());
            }
            _ => panic!("expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmts = parse("DELETE FROM users WHERE id = 1;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Delete(s) => {
                assert_eq!(s.table, "users");
                assert!(s.where_clause.is_some());
            }
            _ => panic!("expected DELETE"),
        }
    }

    #[test]
    fn test_parse_expression_precedence() {
        let stmts = parse("SELECT 1 + 2 * 3;");
        match &stmts[0] {
            Statement::Select(s) => {
                // Should be 1 + (2 * 3)
                if let SelectItem::Expr { expr, .. } = &s.projection[0] {
                    if let Expr::BinaryOp { op: BinOp::Add, .. } = expr {
                        // good
                    } else {
                        panic!("expected Add, got {:?}", expr);
                    }
                }
            }
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_create_index() {
        let stmts = parse("CREATE INDEX idx_name ON users (name);");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::CreateIndex(s) => {
                assert_eq!(s.name, "idx_name");
                assert_eq!(s.table, "users");
                assert_eq!(s.column, "name");
            }
            _ => panic!("expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_join() {
        let stmts =
            parse("SELECT a.id, b.name FROM users a INNER JOIN orders b ON a.id = b.user_id;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.from.len(), 2);
            }
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_is_null() {
        let stmts = parse("SELECT * FROM t WHERE x IS NULL;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                if let Some(Expr::BinaryOp {
                    op: BinOp::IsNull, ..
                }) = &s.where_clause
                {
                    // good
                } else {
                    panic!("expected IsNull");
                }
            }
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_order_by_limit() {
        let stmts = parse("SELECT * FROM t ORDER BY id DESC LIMIT 10 OFFSET 5;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.order_by.len(), 1);
                assert!(s.limit.is_some());
            }
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_begin_commit() {
        let stmts = parse("BEGIN; INSERT INTO t VALUES (1); COMMIT;");
        assert_eq!(stmts.len(), 3);
        assert!(matches!(stmts[0], Statement::Begin));
        assert!(matches!(stmts[2], Statement::Commit));
    }
}
