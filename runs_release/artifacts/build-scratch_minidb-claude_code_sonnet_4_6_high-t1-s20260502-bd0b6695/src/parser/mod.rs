pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{tokenize, Token, TokenWithPos};
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

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof)
    }

    fn peek_pos(&self) -> (u32, u32) {
        self.tokens
            .get(self.pos)
            .map(|t| (t.line, t.col))
            .unwrap_or((0, 0))
    }

    fn advance(&mut self) -> &Token {
        let t = self
            .tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        let (line, col) = self.peek_pos();
        let tok = self.peek().clone();
        if &tok == expected {
            self.advance();
            Ok(())
        } else {
            Err(MiniDbError::Parse {
                msg: format!("expected {:?}, got {:?}", expected, tok),
                line,
                col,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            // Allow keywords as identifiers in some contexts
            Token::Index => {
                self.advance();
                Ok("index".to_string())
            }
            Token::Key => {
                self.advance();
                Ok("key".to_string())
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("expected identifier, got {:?}", tok),
                line,
                col,
            }),
        }
    }

    fn eat_if(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while self.peek() != &Token::Eof {
            if self.eat_if(&Token::Semicolon) {
                continue;
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            // optional trailing semicolon
            self.eat_if(&Token::Semicolon);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Select => self.parse_select().map(Statement::Select),
            Token::Insert => self.parse_insert().map(Statement::Insert),
            Token::Update => self.parse_update().map(Statement::Update),
            Token::Delete => self.parse_delete().map(Statement::Delete),
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Begin => {
                self.advance();
                // optional TRANSACTION keyword
                if self.peek() == &Token::Ident("TRANSACTION".to_string())
                    || matches!(self.peek(), Token::Ident(s) if s.eq_ignore_ascii_case("transaction"))
                {
                    self.advance();
                }
                Ok(Statement::Begin)
            }
            Token::Commit => {
                self.advance();
                if matches!(self.peek(), Token::Ident(s) if s.eq_ignore_ascii_case("transaction")) {
                    self.advance();
                }
                Ok(Statement::Commit)
            }
            Token::Rollback => {
                self.advance();
                if matches!(self.peek(), Token::Ident(s) if s.eq_ignore_ascii_case("transaction")) {
                    self.advance();
                }
                Ok(Statement::Rollback)
            }
            Token::MetaCommand(cmd) => {
                self.advance();
                self.parse_meta_command(cmd)
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("unexpected token {:?}", tok),
                line,
                col,
            }),
        }
    }

    fn parse_meta_command(&mut self, cmd: String) -> Result<Statement> {
        let mc = match cmd.to_lowercase().as_str() {
            "schema" => MetaCmd::Schema,
            "tables" => MetaCmd::Tables,
            "indexes" | "indices" => MetaCmd::Indexes,
            "exit" | "quit" => MetaCmd::Exit,
            "format" => {
                let fmt = self.expect_ident()?;
                MetaCmd::Format(fmt)
            }
            _ => {
                let (line, col) = self.peek_pos();
                return Err(MiniDbError::Parse {
                    msg: format!("unknown meta-command: .{}", cmd),
                    line,
                    col,
                });
            }
        };
        Ok(Statement::MetaCommand(mc))
    }

    fn parse_select(&mut self) -> Result<SelectStmt> {
        self.expect(&Token::Select)?;
        let distinct = self.eat_if(&Token::Distinct);

        let projections = self.parse_select_items()?;

        let from = if self.eat_if(&Token::From) {
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        let where_clause = if self.eat_if(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.peek() == &Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            self.parse_order_by()?
        } else {
            vec![]
        };

        let limit = if self.eat_if(&Token::Limit) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let offset = if self.eat_if(&Token::Offset) {
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
            items.push(self.parse_select_item()?);
            if !self.eat_if(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_select_item(&mut self) -> Result<SelectItem> {
        if self.peek() == &Token::Star {
            self.advance();
            return Ok(SelectItem::Star);
        }
        // Check for table.*
        if let Token::Ident(name) = self.peek().clone() {
            if self.tokens.get(self.pos + 1).map(|t| &t.token) == Some(&Token::Dot) {
                if self.tokens.get(self.pos + 2).map(|t| &t.token) == Some(&Token::Star) {
                    self.advance(); // name
                    self.advance(); // .
                    self.advance(); // *
                    return Ok(SelectItem::TableStar(name));
                }
            }
        }
        let expr = self.parse_expr()?;
        let alias = if self.eat_if(&Token::As) {
            Some(self.expect_ident()?)
        } else if let Token::Ident(_) = self.peek() {
            // implicit alias
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(SelectItem::Expr(expr, alias))
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let left = self.parse_table_ref()?;

        // Check for JOIN
        let join_type = if self.eat_if(&Token::Inner) {
            self.expect(&Token::Join)?;
            Some(JoinType::Inner)
        } else if self.eat_if(&Token::Join) {
            Some(JoinType::Inner)
        } else {
            None
        };

        if let Some(jt) = join_type {
            let right = self.parse_table_ref()?;
            let condition = if matches!(self.peek(), Token::On)
                || matches!(self.peek(), Token::Ident(s) if s.eq_ignore_ascii_case("on"))
            {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            Ok(FromClause::Join {
                left,
                right,
                condition,
                join_type: jt,
            })
        } else {
            Ok(FromClause::Table(left))
        }
    }

    fn parse_table_ref(&mut self) -> Result<TableRef> {
        let name = self.expect_ident()?;
        let alias = if self.eat_if(&Token::As) {
            Some(self.expect_ident()?)
        } else if let Token::Ident(_) = self.peek() {
            // check it's not a keyword
            match self.peek() {
                Token::Where
                | Token::Order
                | Token::Limit
                | Token::Inner
                | Token::Join
                | Token::On
                | Token::Offset
                | Token::Eof
                | Token::Semicolon => None,
                Token::Ident(_) => Some(self.expect_ident()?),
                _ => None,
            }
        } else {
            None
        };
        Ok(TableRef { name, alias })
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let asc = if self.eat_if(&Token::Desc) {
                false
            } else {
                self.eat_if(&Token::Asc);
                true
            };
            items.push(OrderByItem { expr, asc });
            if !self.eat_if(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_insert(&mut self) -> Result<InsertStmt> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;
        let table = self.expect_ident()?;

        let columns = if self.eat_if(&Token::LParen) {
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if !self.eat_if(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;

        let mut value_rows = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let mut vals = Vec::new();
            loop {
                vals.push(self.parse_expr()?);
                if !self.eat_if(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            value_rows.push(vals);
            if !self.eat_if(&Token::Comma) {
                break;
            }
        }

        Ok(InsertStmt {
            table,
            columns,
            values: value_rows,
        })
    }

    fn parse_update(&mut self) -> Result<UpdateStmt> {
        self.expect(&Token::Update)?;
        let table = self.expect_ident()?;
        self.expect(&Token::Set)?;

        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((col, expr));
            if !self.eat_if(&Token::Comma) {
                break;
            }
        }

        let where_clause = if self.eat_if(&Token::Where) {
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
        let table = self.expect_ident()?;
        let where_clause = if self.eat_if(&Token::Where) {
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
                let if_not_exists = self.parse_if_not_exists()?;
                let name = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let (columns, table_constraints) = self.parse_column_defs()?;
                self.expect(&Token::RParen)?;
                Ok(Statement::CreateTable(CreateTableStmt {
                    name,
                    if_not_exists,
                    columns,
                    table_constraints,
                }))
            }
            Token::Index | Token::Unique => {
                let unique = self.eat_if(&Token::Unique);
                self.expect(&Token::Index)?;
                let if_not_exists = self.parse_if_not_exists()?;
                let name = self.expect_ident()?;
                let on_kw = matches!(self.peek(), Token::On);
                if on_kw {
                    self.advance();
                } else {
                    return Err(MiniDbError::Parse {
                        msg: "expected ON".into(),
                        line,
                        col,
                    });
                }
                let table = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let column = self.expect_ident()?;
                self.expect(&Token::RParen)?;
                Ok(Statement::CreateIndex(CreateIndexStmt {
                    name,
                    table,
                    column,
                    unique,
                    if_not_exists,
                }))
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("expected TABLE or INDEX after CREATE, got {:?}", tok),
                line,
                col,
            }),
        }
    }

    fn parse_if_not_exists(&mut self) -> Result<bool> {
        if self.peek() == &Token::If {
            self.advance();
            self.expect(&Token::Not)?;
            self.expect(&Token::Exists)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn parse_column_defs(&mut self) -> Result<(Vec<ColumnSpec>, Vec<TableConstraint>)> {
        let mut columns = Vec::new();
        let mut table_constraints = Vec::new();

        loop {
            // Check for table-level constraints
            match self.peek().clone() {
                Token::Primary => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    self.expect(&Token::LParen)?;
                    let mut cols = Vec::new();
                    loop {
                        cols.push(self.expect_ident()?);
                        if !self.eat_if(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RParen)?;
                    table_constraints.push(TableConstraint::PrimaryKey(cols));
                }
                Token::Unique => {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let mut cols = Vec::new();
                    loop {
                        cols.push(self.expect_ident()?);
                        if !self.eat_if(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RParen)?;
                    table_constraints.push(TableConstraint::Unique(cols));
                }
                Token::Check => {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let expr = self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                    table_constraints.push(TableConstraint::Check(expr));
                }
                _ => {
                    // column definition
                    let col = self.parse_column_spec()?;
                    columns.push(col);
                }
            }

            if !self.eat_if(&Token::Comma) {
                break;
            }
            // Check if next is a closing paren (trailing comma case for some parsers)
            if self.peek() == &Token::RParen {
                break;
            }
        }

        Ok((columns, table_constraints))
    }

    fn parse_column_spec(&mut self) -> Result<ColumnSpec> {
        let name = self.expect_ident()?;
        let data_type = self.parse_data_type()?;
        let mut constraints = Vec::new();

        loop {
            match self.peek().clone() {
                Token::Primary => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    constraints.push(ColumnConstraint::PrimaryKey);
                }
                Token::Not => {
                    self.advance();
                    self.expect(&Token::Null)?;
                    constraints.push(ColumnConstraint::NotNull);
                }
                Token::Unique => {
                    self.advance();
                    constraints.push(ColumnConstraint::Unique);
                }
                Token::Default => {
                    self.advance();
                    let expr = self.parse_primary()?;
                    constraints.push(ColumnConstraint::Default(expr));
                }
                Token::Check => {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let expr = self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                    constraints.push(ColumnConstraint::Check(expr));
                }
                _ => break,
            }
        }

        Ok(ColumnSpec {
            name,
            data_type,
            constraints,
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
            Token::Ident(s) => {
                let dt = DataType::from_str(&s).ok_or_else(|| MiniDbError::Parse {
                    msg: format!("unknown data type: {}", s),
                    line,
                    col,
                })?;
                self.advance();
                dt
            }
            tok => {
                return Err(MiniDbError::Parse {
                    msg: format!("expected data type, got {:?}", tok),
                    line,
                    col,
                })
            }
        };
        // Optional length specifier (e.g., VARCHAR(255)) - just ignore the length
        if self.peek() == &Token::LParen {
            self.advance();
            self.parse_expr()?;
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
                let if_exists = if self.peek() == &Token::If {
                    self.advance();
                    self.expect(&Token::Exists)?;
                    true
                } else {
                    false
                };
                let name = self.expect_ident()?;
                Ok(Statement::DropTable(DropTableStmt { name, if_exists }))
            }
            Token::Index => {
                self.advance();
                let if_exists = if self.peek() == &Token::If {
                    self.advance();
                    self.expect(&Token::Exists)?;
                    true
                } else {
                    false
                };
                let name = self.expect_ident()?;
                Ok(Statement::DropIndex(DropIndexStmt { name, if_exists }))
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("expected TABLE or INDEX after DROP, got {:?}", tok),
                line,
                col,
            }),
        }
    }

    // Expression parsing using precedence climbing
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.eat_if(&Token::Or) {
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
        while self.eat_if(&Token::And) {
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
        if self.eat_if(&Token::Not) {
            let expr = self.parse_not()?;
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

        let op = match self.peek() {
            Token::Eq => Some(BinaryOp::Eq),
            Token::Ne => Some(BinaryOp::Ne),
            Token::Lt => Some(BinaryOp::Lt),
            Token::Le => Some(BinaryOp::Le),
            Token::Gt => Some(BinaryOp::Gt),
            Token::Ge => Some(BinaryOp::Ge),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_concat()?;
            return Ok(Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });
        }

        // IS NULL / IS NOT NULL
        if self.eat_if(&Token::Is) {
            if self.eat_if(&Token::Not) {
                self.expect(&Token::Null)?;
                return Ok(Expr::IsNotNull(Box::new(left)));
            } else {
                self.expect(&Token::Null)?;
                return Ok(Expr::IsNull(Box::new(left)));
            }
        }

        // NOT LIKE / LIKE
        if self.peek() == &Token::Not {
            if self.tokens.get(self.pos + 1).map(|t| &t.token) == Some(&Token::Like) {
                self.advance(); // NOT
                self.advance(); // LIKE
                let pattern = self.parse_concat()?;
                return Ok(Expr::Like {
                    expr: Box::new(left),
                    pattern: Box::new(pattern),
                    negated: true,
                });
            }
        }
        if self.eat_if(&Token::Like) {
            let pattern = self.parse_concat()?;
            return Ok(Expr::Like {
                expr: Box::new(left),
                pattern: Box::new(pattern),
                negated: false,
            });
        }

        // IN (...)
        let negated_in = if self.peek() == &Token::Not {
            if self.tokens.get(self.pos + 1).map(|t| &t.token) == Some(&Token::In) {
                self.advance();
                true
            } else {
                false
            }
        } else {
            false
        };
        if negated_in || self.eat_if(&Token::In) {
            if !negated_in {
                // already consumed In
            }
            if negated_in {
                self.expect(&Token::In)?;
            }
            self.expect(&Token::LParen)?;
            let mut list = Vec::new();
            loop {
                list.push(self.parse_expr()?);
                if !self.eat_if(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            return Ok(Expr::InList {
                expr: Box::new(left),
                list,
                negated: negated_in,
            });
        }

        Ok(left)
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while self.eat_if(&Token::Concat) {
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
                Token::Plus => Some(BinaryOp::Add),
                Token::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_multiplicative()?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => Some(BinaryOp::Mul),
                Token::Slash => Some(BinaryOp::Div),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_unary()?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.eat_if(&Token::Minus) {
            let expr = self.parse_primary()?;
            // If it's a literal, fold the negation
            if let Expr::Literal(Value::Int(i)) = expr {
                return Ok(Expr::Literal(Value::Int(-i)));
            }
            if let Expr::Literal(Value::Real(r)) = expr {
                return Ok(Expr::Literal(Value::Real(-r)));
            }
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        if self.eat_if(&Token::Plus) {
            return self.parse_primary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
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
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            // Builtin functions
            Token::Coalesce => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut args = Vec::new();
                loop {
                    args.push(self.parse_expr()?);
                    if !self.eat_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall {
                    name: "COALESCE".into(),
                    args,
                })
            }
            Token::Length => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall {
                    name: "LENGTH".into(),
                    args: vec![arg],
                })
            }
            Token::Lower => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall {
                    name: "LOWER".into(),
                    args: vec![arg],
                })
            }
            Token::Upper => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall {
                    name: "UPPER".into(),
                    args: vec![arg],
                })
            }
            Token::Substr => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut args = Vec::new();
                loop {
                    args.push(self.parse_expr()?);
                    if !self.eat_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall {
                    name: "SUBSTR".into(),
                    args,
                })
            }
            Token::Abs => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall {
                    name: "ABS".into(),
                    args: vec![arg],
                })
            }
            Token::Ident(name) => {
                self.advance();
                // Function call?
                if self.peek() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != &Token::RParen {
                        // Handle COUNT(*) - treat * as a special literal
                        if self.peek() == &Token::Star {
                            self.advance();
                            args.push(Expr::Literal(crate::types::Value::Text("*".into())));
                        } else {
                            loop {
                                args.push(self.parse_expr()?);
                                if !self.eat_if(&Token::Comma) {
                                    break;
                                }
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::FunctionCall {
                        name: name.to_uppercase(),
                        args,
                    });
                }
                // Qualified column: table.col?
                if self.peek() == &Token::Dot {
                    self.advance();
                    let col = self.expect_ident()?;
                    return Ok(Expr::QualifiedColumn(name, col));
                }
                Ok(Expr::Column(name))
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("unexpected token in expression: {:?}", tok),
                line,
                col,
            }),
        }
    }
}

pub fn parse(sql: &str) -> Result<Vec<Statement>> {
    let tokens = tokenize(sql)?;
    Parser::new(tokens).parse_statements()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let stmts =
            parse("SELECT id, name FROM users WHERE age > 18 ORDER BY name ASC LIMIT 10;").unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Select(s) => {
                assert_eq!(s.projections.len(), 2);
                assert!(s.where_clause.is_some());
                assert_eq!(s.order_by.len(), 1);
                assert!(s.limit.is_some());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmts = parse("INSERT INTO t (a, b) VALUES (1, 'hello');").unwrap();
        match &stmts[0] {
            Statement::Insert(ins) => {
                assert_eq!(ins.table, "t");
                assert_eq!(ins.columns.as_ref().unwrap(), &["a", "b"]);
                assert_eq!(ins.values.len(), 1);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0);";
        let stmts = parse(sql).unwrap();
        match &stmts[0] {
            Statement::CreateTable(ct) => {
                assert_eq!(ct.name, "users");
                assert_eq!(ct.columns.len(), 3);
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn test_parse_multiple_statements() {
        let sql = "BEGIN; INSERT INTO t VALUES (1); COMMIT;";
        let stmts = parse(sql).unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn test_parse_meta_commands() {
        let stmts = parse(".tables").unwrap();
        assert!(matches!(&stmts[0], Statement::MetaCommand(MetaCmd::Tables)));
    }
}
