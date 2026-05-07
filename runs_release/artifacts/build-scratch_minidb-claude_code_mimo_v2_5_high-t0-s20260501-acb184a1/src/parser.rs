pub mod ast {
    use crate::types::{ColumnType, Value};

    /// Top-level statement
    #[derive(Debug, Clone)]
    pub enum Statement {
        CreateTable(CreateTableStmt),
        DropTable(DropTableStmt),
        CreateIndex(CreateIndexStmt),
        DropIndex(DropIndexStmt),
        Insert(InsertStmt),
        Select(SelectStmt),
        Update(UpdateStmt),
        Delete(DeleteStmt),
        Begin,
        Commit,
        Rollback,
    }

    #[derive(Debug, Clone)]
    pub struct CreateTableStmt {
        pub table_name: String,
        pub columns: Vec<ColumnConstraint>,
        pub table_constraints: Vec<TableConstraint>,
    }

    #[derive(Debug, Clone)]
    pub struct ColumnConstraint {
        pub name: String,
        pub col_type: ColumnType,
        pub primary_key: bool,
        pub not_null: bool,
        pub unique: bool,
        pub default: Option<Value>,
    }

    #[derive(Debug, Clone)]
    pub enum TableConstraint {
        Check(Expr),
    }

    #[derive(Debug, Clone)]
    pub struct DropTableStmt {
        pub table_name: String,
    }

    #[derive(Debug, Clone)]
    pub struct CreateIndexStmt {
        pub index_name: String,
        pub table_name: String,
        pub column_name: String,
    }

    #[derive(Debug, Clone)]
    pub struct DropIndexStmt {
        pub index_name: String,
    }

    #[derive(Debug, Clone)]
    pub struct InsertStmt {
        pub table_name: String,
        pub columns: Option<Vec<String>>,
        pub values: Vec<Vec<Expr>>,
    }

    #[derive(Debug, Clone)]
    pub struct SelectStmt {
        pub columns: Vec<SelectColumn>,
        pub from: FromClause,
        pub where_clause: Option<Expr>,
        pub order_by: Vec<OrderByExpr>,
        pub limit: Option<i64>,
        pub offset: Option<i64>,
    }

    #[derive(Debug, Clone)]
    pub enum SelectColumn {
        Star,
        Expr(Expr, Option<String>),
    }

    #[derive(Debug, Clone)]
    pub enum FromClause {
        Table(String),
        Join {
            left: Box<FromClause>,
            right: String,
            on: Expr,
        },
    }

    #[derive(Debug, Clone)]
    pub struct OrderByExpr {
        pub expr: Expr,
        pub ascending: bool,
    }

    #[derive(Debug, Clone)]
    pub struct UpdateStmt {
        pub table_name: String,
        pub assignments: Vec<(String, Expr)>,
        pub where_clause: Option<Expr>,
    }

    #[derive(Debug, Clone)]
    pub struct DeleteStmt {
        pub table_name: String,
        pub where_clause: Option<Expr>,
    }

    /// Expression AST
    #[derive(Debug, Clone)]
    pub enum Expr {
        Literal(Value),
        Column(String),
        QualifiedColumn {
            table: String,
            column: String,
        },
        BinaryOp {
            left: Box<Expr>,
            op: BinOp,
            right: Box<Expr>,
        },
        UnaryOp {
            op: UnOp,
            expr: Box<Expr>,
        },
        IsNull {
            expr: Box<Expr>,
            negated: bool,
        },
        Function {
            name: String,
            args: Vec<Expr>,
        },
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum BinOp {
        Add,
        Sub,
        Mul,
        Div,
        Concat,
        Eq,
        Ne,
        Lt,
        Le,
        Gt,
        Ge,
        And,
        Or,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum UnOp {
        Neg,
        Not,
    }
}

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
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_ahead(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            Err(self.error(format!("Expected '{}', got '{}'", expected, tok)))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            t => Err(self.error(format!("Expected identifier, got '{}'", t))),
        }
    }

    fn expect_semicolon(&mut self) -> Result<()> {
        self.expect(&Token::Semicolon)
    }

    fn error(&self, msg: String) -> MiniDbError {
        MiniDbError::ParseError {
            message: msg,
            line: 1,
            column: 1,
        }
    }

    fn is_keyword(&self, tok: &Token) -> bool {
        matches!(
            tok,
            Token::Select
                | Token::From
                | Token::Where
                | Token::Insert
                | Token::Into
                | Token::Values
                | Token::Update
                | Token::Set
                | Token::Delete
                | Token::Create
                | Token::Table
                | Token::Drop
                | Token::Index
                | Token::On
                | Token::Primary
                | Token::Key
                | Token::Not
                | Token::Null
                | Token::Is
                | Token::And
                | Token::Or
                | Token::Asc
                | Token::Desc
                | Token::Order
                | Token::By
                | Token::Limit
                | Token::Offset
                | Token::Join
                | Token::Inner
                | Token::Begin
                | Token::Commit
                | Token::Rollback
                | Token::Default
                | Token::Check
                | Token::Unique
                | Token::As
                | Token::True
                | Token::False
        )
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_semicolons();
            match self.peek() {
                Token::Eof => break,
                _ => {
                    let stmt = self.parse_statement()?;
                    stmts.push(stmt);
                }
            }
        }
        Ok(stmts)
    }

    fn skip_semicolons(&mut self) {
        while matches!(self.peek(), Token::Semicolon) {
            self.advance();
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
            t => Err(self.error(format!("Unexpected token: '{}'", t))),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.advance(); // CREATE
        match self.peek() {
            Token::Table => {
                self.advance(); // TABLE
                let table_name = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let mut columns = Vec::new();
                let mut table_constraints = Vec::new();
                loop {
                    match self.peek() {
                        Token::RParen => {
                            self.advance();
                            break;
                        }
                        Token::Check => {
                            self.advance(); // CHECK
                            self.expect(&Token::LParen)?;
                            let expr = self.parse_expr()?;
                            self.expect(&Token::RParen)?;
                            table_constraints.push(TableConstraint::Check(expr));
                        }
                        _ => {
                            let col = self.parse_column_def()?;
                            columns.push(col);
                        }
                    }
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                }
                self.expect_semicolon()?;
                Ok(Statement::CreateTable(CreateTableStmt {
                    table_name,
                    columns,
                    table_constraints,
                }))
            }
            Token::Index => {
                self.advance(); // INDEX
                let index_name = self.expect_ident()?;
                self.expect(&Token::On)?;
                let table_name = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let column_name = self.expect_ident()?;
                self.expect(&Token::RParen)?;
                self.expect_semicolon()?;
                Ok(Statement::CreateIndex(CreateIndexStmt {
                    index_name,
                    table_name,
                    column_name,
                }))
            }
            t => Err(self.error(format!("Expected TABLE or INDEX after CREATE, got '{}'", t))),
        }
    }

    fn parse_column_def(&mut self) -> Result<ColumnConstraint> {
        let name = self.expect_ident()?;
        let type_name = self.expect_ident()?;
        let col_type = ColumnType::from_str(&type_name)
            .ok_or_else(|| self.error(format!("Unknown type: {}", type_name)))?;
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
                    default = Some(self.parse_literal()?);
                }
                _ => break,
            }
        }

        Ok(ColumnConstraint {
            name,
            col_type,
            primary_key,
            not_null: not_null || primary_key,
            unique,
            default,
        })
    }

    fn parse_literal(&mut self) -> Result<Value> {
        match self.advance() {
            Token::Integer(v) => Ok(Value::Int(v)),
            Token::Float(v) => Ok(Value::Real(v)),
            Token::String(s) => Ok(Value::Text(s)),
            Token::True => Ok(Value::Bool(true)),
            Token::False => Ok(Value::Bool(false)),
            Token::Null => Ok(Value::Null),
            Token::Minus => match self.advance() {
                Token::Integer(v) => Ok(Value::Int(-v)),
                Token::Float(v) => Ok(Value::Real(-v)),
                t => Err(self.error(format!("Expected number after '-', got '{}'", t))),
            },
            t => Err(self.error(format!("Expected literal, got '{}'", t))),
        }
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.advance(); // DROP
        match self.peek() {
            Token::Table => {
                self.advance();
                let table_name = self.expect_ident()?;
                self.expect_semicolon()?;
                Ok(Statement::DropTable(DropTableStmt { table_name }))
            }
            Token::Index => {
                self.advance();
                let index_name = self.expect_ident()?;
                self.expect_semicolon()?;
                Ok(Statement::DropIndex(DropIndexStmt { index_name }))
            }
            t => Err(self.error(format!("Expected TABLE or INDEX after DROP, got '{}'", t))),
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.advance(); // INSERT
        self.expect(&Token::Into)?;
        let table_name = self.expect_ident()?;

        let columns = if matches!(self.peek(), Token::LParen)
            && !matches!(self.peek_ahead(1), Token::Select)
        {
            self.advance();
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if !matches!(self.peek(), Token::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;
        let mut values_list = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                if !matches!(self.peek(), Token::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(&Token::RParen)?;
            values_list.push(row);
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.advance();
        }
        self.expect_semicolon()?;
        Ok(Statement::Insert(InsertStmt {
            table_name,
            columns,
            values: values_list,
        }))
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.advance(); // SELECT
        let columns = self.parse_select_columns()?;
        self.expect(&Token::From)?;
        let from = self.parse_from_clause()?;

        let where_clause = if matches!(self.peek(), Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if matches!(self.peek(), Token::Order) {
            self.advance();
            self.expect(&Token::By)?;
            self.parse_order_by()?
        } else {
            Vec::new()
        };

        let limit = if matches!(self.peek(), Token::Limit) {
            self.advance();
            match self.advance() {
                Token::Integer(v) => Some(v),
                t => return Err(self.error(format!("Expected integer after LIMIT, got '{}'", t))),
            }
        } else {
            None
        };

        let offset = if matches!(self.peek(), Token::Offset) {
            self.advance();
            match self.advance() {
                Token::Integer(v) => Some(v),
                t => return Err(self.error(format!("Expected integer after OFFSET, got '{}'", t))),
            }
        } else {
            None
        };

        self.expect_semicolon()?;
        Ok(Statement::Select(SelectStmt {
            columns,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        }))
    }

    fn parse_select_columns(&mut self) -> Result<Vec<SelectColumn>> {
        if matches!(self.peek(), Token::Star) {
            self.advance();
            return Ok(vec![SelectColumn::Star]);
        }
        let mut cols = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let alias = if matches!(self.peek(), Token::As) {
                self.advance();
                Some(self.expect_ident()?)
            } else if !self.is_keyword(self.peek()) && matches!(self.peek(), Token::Ident(_)) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            cols.push(SelectColumn::Expr(expr, alias));
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.advance();
        }
        Ok(cols)
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let mut from = FromClause::Table(self.expect_ident()?);
        loop {
            if matches!(self.peek(), Token::Inner) || matches!(self.peek(), Token::Join) {
                self.advance(); // skip INNER/JOIN
                if matches!(self.peek(), Token::Join) {
                    self.advance();
                }
                let right = self.expect_ident()?;
                self.expect(&Token::On)?;
                let on = self.parse_expr()?;
                from = FromClause::Join {
                    left: Box::new(from),
                    right,
                    on,
                };
            } else {
                break;
            }
        }
        Ok(from)
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByExpr>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let ascending = if matches!(self.peek(), Token::Asc) {
                self.advance();
                true
            } else if matches!(self.peek(), Token::Desc) {
                self.advance();
                false
            } else {
                true
            };
            items.push(OrderByExpr { expr, ascending });
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.advance();
        }
        Ok(items)
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.advance(); // UPDATE
        let table_name = self.expect_ident()?;
        self.expect(&Token::Set)?;
        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((col, expr));
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.advance();
        }
        let where_clause = if matches!(self.peek(), Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect_semicolon()?;
        Ok(Statement::Update(UpdateStmt {
            table_name,
            assignments,
            where_clause,
        }))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.advance(); // DELETE
        self.expect(&Token::From)?;
        let table_name = self.expect_ident()?;
        let where_clause = if matches!(self.peek(), Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect_semicolon()?;
        Ok(Statement::Delete(DeleteStmt {
            table_name,
            where_clause,
        }))
    }

    /// Expression parser with precedence climbing
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_not_expr()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.parse_not_expr()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not_expr(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let expr = self.parse_not_expr()?;
            Ok(Expr::UnaryOp {
                op: UnOp::Not,
                expr: Box::new(expr),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let left = self.parse_concat()?;
        if matches!(self.peek(), Token::Is) {
            self.advance();
            let negated = if matches!(self.peek(), Token::Not) {
                self.advance();
                true
            } else {
                false
            };
            self.expect(&Token::Null)?;
            return Ok(Expr::IsNull {
                expr: Box::new(left),
                negated,
            });
        }
        let op = match self.peek() {
            Token::Eq => Some(BinOp::Eq),
            Token::Ne => Some(BinOp::Ne),
            Token::Lt => Some(BinOp::Lt),
            Token::Le => Some(BinOp::Le),
            Token::Gt => Some(BinOp::Gt),
            Token::Ge => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_concat()?;
            Ok(Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        } else {
            Ok(left)
        }
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while matches!(self.peek(), Token::Pipe) {
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Concat,
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
                        left: Box::new(left),
                        op: BinOp::Add,
                        right: Box::new(right),
                    };
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinOp::Sub,
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
                        left: Box::new(left),
                        op: BinOp::Mul,
                        right: Box::new(right),
                    };
                }
                Token::Slash => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinOp::Div,
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Token::Minus) {
            self.advance();
            let expr = self.parse_primary()?;
            Ok(Expr::UnaryOp {
                op: UnOp::Neg,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::Integer(_)
            | Token::Float(_)
            | Token::String(_)
            | Token::True
            | Token::False
            | Token::Null => Ok(Expr::Literal(self.parse_literal()?)),
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Ident(name) => {
                self.advance();
                // Check for function call: IDENT(
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek(), Token::Comma) {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::Function {
                        name: name.to_uppercase(),
                        args,
                    });
                }
                // Check for qualified name: table.column
                if matches!(self.peek(), Token::Dot) {
                    self.advance();
                    let col = self.expect_ident()?;
                    return Ok(Expr::QualifiedColumn {
                        table: name,
                        column: col,
                    });
                }
                Ok(Expr::Column(name))
            }
            t => Err(self.error(format!("Unexpected token in expression: '{}'", t))),
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Vec<Statement>> {
    let mut parser = Parser::new(tokens);
    parser.parse_statements()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_str(input: &str) -> Vec<Statement> {
        let tokens = tokenize(input).unwrap();
        parse(tokens).unwrap()
    }

    #[test]
    fn test_parse_select() {
        let stmts = parse_str("SELECT * FROM users;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.columns.len(), 1);
                assert!(matches!(s.columns[0], SelectColumn::Star));
                assert!(matches!(s.from, FromClause::Table(ref t) if t == "users"));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let stmts = parse_str("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);");
        match &stmts[0] {
            Statement::CreateTable(s) => {
                assert_eq!(s.table_name, "t");
                assert_eq!(s.columns.len(), 2);
                assert!(s.columns[0].primary_key);
                assert!(s.columns[1].not_null);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmts = parse_str("INSERT INTO t (a, b) VALUES (1, 'hello');");
        match &stmts[0] {
            Statement::Insert(s) => {
                assert_eq!(s.table_name, "t");
                assert_eq!(s.columns.as_ref().unwrap().len(), 2);
                assert_eq!(s.values.len(), 1);
                assert_eq!(s.values[0].len(), 2);
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_where() {
        let stmts = parse_str("SELECT * FROM t WHERE a = 1 AND b <> 'x';");
        match &stmts[0] {
            Statement::Select(s) => {
                assert!(s.where_clause.is_some());
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmts = parse_str("UPDATE t SET a = 1 WHERE b = 2;");
        match &stmts[0] {
            Statement::Update(s) => {
                assert_eq!(s.table_name, "t");
                assert_eq!(s.assignments.len(), 1);
                assert!(s.where_clause.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmts = parse_str("DELETE FROM t WHERE id = 1;");
        match &stmts[0] {
            Statement::Delete(s) => {
                assert_eq!(s.table_name, "t");
                assert!(s.where_clause.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_order_by() {
        let stmts = parse_str("SELECT * FROM t ORDER BY a ASC, b DESC;");
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.order_by.len(), 2);
                assert!(s.order_by[0].ascending);
                assert!(!s.order_by[1].ascending);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_limit_offset() {
        let stmts = parse_str("SELECT * FROM t LIMIT 10 OFFSET 5;");
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.limit, Some(10));
                assert_eq!(s.offset, Some(5));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_create_index() {
        let stmts = parse_str("CREATE INDEX idx ON t (col);");
        match &stmts[0] {
            Statement::CreateIndex(s) => {
                assert_eq!(s.index_name, "idx");
                assert_eq!(s.table_name, "t");
                assert_eq!(s.column_name, "col");
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_begin_commit_rollback() {
        let stmts = parse_str("BEGIN; INSERT INTO t VALUES (1); COMMIT; ROLLBACK;");
        assert_eq!(stmts.len(), 4);
        assert!(matches!(stmts[0], Statement::Begin));
        assert!(matches!(stmts[2], Statement::Commit));
        assert!(matches!(stmts[3], Statement::Rollback));
    }

    #[test]
    fn test_parse_multiple_inserts() {
        let stmts = parse_str("INSERT INTO t VALUES (1, 'a'); INSERT INTO t VALUES (2, 'b');");
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn test_parse_is_null() {
        let stmts = parse_str("SELECT * FROM t WHERE a IS NOT NULL;");
        match &stmts[0] {
            Statement::Select(s) => {
                let where_e = s.where_clause.as_ref().unwrap();
                assert!(matches!(where_e, Expr::IsNull { negated: true, .. }));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_function_call() {
        let stmts = parse_str("SELECT LENGTH(name) FROM t;");
        match &stmts[0] {
            Statement::Select(s) => {
                if let SelectColumn::Expr(Expr::Function { name, args }, _) = &s.columns[0] {
                    assert_eq!(name, "LENGTH");
                    assert_eq!(args.len(), 1);
                } else {
                    panic!("Expected function");
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_join() {
        let stmts = parse_str("SELECT * FROM a INNER JOIN b ON a.id = b.id;");
        match &stmts[0] {
            Statement::Select(s) => {
                assert!(matches!(s.from, FromClause::Join { .. }));
            }
            _ => panic!("Expected SELECT"),
        }
    }
}
