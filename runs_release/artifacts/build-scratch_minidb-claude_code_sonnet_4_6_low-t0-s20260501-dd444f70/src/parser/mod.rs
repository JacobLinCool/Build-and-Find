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
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            let (line, col) = self.peek_pos();
            Err(MiniDbError::Parse {
                line,
                col,
                msg: format!("expected {:?}, got {:?}", expected, self.peek()),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            other => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("expected identifier, got {:?}", other),
                })
            }
        }
    }

    fn expect_ident_or_kw(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        match &tok {
            Token::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            Token::Text => {
                self.advance();
                Ok("TEXT".to_string())
            }
            Token::Int => {
                self.advance();
                Ok("INT".to_string())
            }
            Token::BigInt => {
                self.advance();
                Ok("BIGINT".to_string())
            }
            Token::Real => {
                self.advance();
                Ok("REAL".to_string())
            }
            Token::Bool | Token::Boolean => {
                self.advance();
                Ok("BOOL".to_string())
            }
            _ => self.expect_ident(),
        }
    }

    fn consume_if(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn parse_statements(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while self.peek() != &Token::Eof {
            if self.consume_if(&Token::Semicolon) {
                continue;
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            self.consume_if(&Token::Semicolon);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Stmt> {
        match self.peek().clone() {
            Token::Select => self.parse_select().map(Stmt::Select),
            Token::Insert => self.parse_insert().map(Stmt::Insert),
            Token::Update => self.parse_update().map(Stmt::Update),
            Token::Delete => self.parse_delete().map(Stmt::Delete),
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Begin => {
                self.advance();
                // optionally consume TRANSACTION keyword
                if self.peek() == &Token::Ident("TRANSACTION".to_string()) {
                    self.advance();
                }
                Ok(Stmt::Begin)
            }
            Token::Commit => {
                self.advance();
                Ok(Stmt::Commit)
            }
            Token::Rollback => {
                self.advance();
                Ok(Stmt::Rollback)
            }
            Token::MetaSchema => {
                self.advance();
                Ok(Stmt::MetaSchema)
            }
            Token::MetaTables => {
                self.advance();
                Ok(Stmt::MetaTables)
            }
            Token::MetaIndexes => {
                self.advance();
                Ok(Stmt::MetaIndexes)
            }
            Token::MetaExit => {
                self.advance();
                Ok(Stmt::MetaExit)
            }
            Token::MetaFormat => {
                self.advance();
                let fmt = self.expect_ident_or_kw()?;
                Ok(Stmt::MetaFormat(fmt.to_lowercase()))
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("unexpected token: {:?}", tok),
                })
            }
        }
    }

    fn parse_select(&mut self) -> Result<SelectStmt> {
        self.expect(&Token::Select)?;

        let projections = self.parse_select_list()?;

        let from = if self.consume_if(&Token::From) {
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        let where_clause = if self.consume_if(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.peek() == &Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            self.parse_order_by_list()?
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
            projections,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_select_list(&mut self) -> Result<Vec<SelectItem>> {
        let mut items = Vec::new();
        loop {
            let item = self.parse_select_item()?;
            items.push(item);
            if !self.consume_if(&Token::Comma) {
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
        // Check for table.* or table.col
        if let Token::Ident(name) = self.peek().clone() {
            if self.tokens.get(self.pos + 1).map(|t| &t.token) == Some(&Token::Dot) {
                let tbl = name.clone();
                self.advance(); // consume ident
                self.advance(); // consume dot
                if self.peek() == &Token::Star {
                    self.advance();
                    return Ok(SelectItem::TableStar(tbl));
                }
                let col = self.expect_ident()?;
                let expr = Expr::Col(col, Some(tbl));
                let alias = if self.consume_if(&Token::As) {
                    Some(self.expect_ident_or_kw()?)
                } else if let Token::Ident(_) = self.peek() {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                return Ok(SelectItem::Expr(expr, alias));
            }
        }
        let expr = self.parse_expr()?;
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_ident_or_kw()?)
        } else if let Token::Ident(_) = self.peek() {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(SelectItem::Expr(expr, alias))
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let name = self.expect_ident_or_kw()?;
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_ident_or_kw()?)
        } else if let Token::Ident(_) = self.peek() {
            Some(self.expect_ident()?)
        } else {
            None
        };
        let mut from = FromClause::Table(name, alias);

        while self.peek() == &Token::Inner || (self.peek() == &Token::Join) {
            if self.peek() == &Token::Inner {
                self.advance();
            }
            self.expect(&Token::Join)?;
            let right_name = self.expect_ident_or_kw()?;
            let right_alias = if self.consume_if(&Token::As) {
                Some(self.expect_ident_or_kw()?)
            } else if let Token::Ident(_) = self.peek() {
                Some(self.expect_ident()?)
            } else {
                None
            };
            let right = FromClause::Table(right_name, right_alias);
            self.expect(&Token::On)?;
            let condition = self.parse_expr()?;
            from = FromClause::Join {
                left: Box::new(from),
                right: Box::new(right),
                condition,
            };
        }

        Ok(from)
    }

    fn parse_order_by_list(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let asc = !self.consume_if(&Token::Desc);
            if !asc {
                // consumed DESC
            } else {
                self.consume_if(&Token::Asc);
            }
            items.push(OrderByItem { expr, asc });
            if !self.consume_if(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_insert(&mut self) -> Result<InsertStmt> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;
        let table = self.expect_ident_or_kw()?;

        let columns = if self.peek() == &Token::LParen {
            self.advance();
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident_or_kw()?);
                if !self.consume_if(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;

        let mut all_values = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let mut row_vals = Vec::new();
            loop {
                row_vals.push(self.parse_expr()?);
                if !self.consume_if(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            all_values.push(row_vals);
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
        let table = self.expect_ident_or_kw()?;
        self.expect(&Token::Set)?;

        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident_or_kw()?;
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
        let table = self.expect_ident_or_kw()?;

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

    fn parse_create(&mut self) -> Result<Stmt> {
        self.expect(&Token::Create)?;
        match self.peek().clone() {
            Token::Table => {
                self.advance();
                self.parse_create_table().map(Stmt::CreateTable)
            }
            Token::Index => {
                self.advance();
                self.parse_create_index(false).map(Stmt::CreateIndex)
            }
            Token::Unique => {
                self.advance();
                self.expect(&Token::Index)?;
                self.parse_create_index(true).map(Stmt::CreateIndex)
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("expected TABLE or INDEX, got {:?}", tok),
                })
            }
        }
    }

    fn parse_create_table(&mut self) -> Result<CreateTableStmt> {
        let if_not_exists = if self.peek() == &Token::If {
            self.advance();
            self.expect(&Token::Not)?;
            self.expect(&Token::Exists)?;
            true
        } else {
            false
        };

        let table = self.expect_ident_or_kw()?;
        self.expect(&Token::LParen)?;

        let mut columns = Vec::new();
        let mut table_constraints = Vec::new();

        loop {
            // Check if this is a table-level constraint
            match self.peek().clone() {
                Token::Primary => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    self.expect(&Token::LParen)?;
                    let mut cols = Vec::new();
                    loop {
                        cols.push(self.expect_ident_or_kw()?);
                        if !self.consume_if(&Token::Comma) {
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
                        cols.push(self.expect_ident_or_kw()?);
                        if !self.consume_if(&Token::Comma) {
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
                    let col = self.parse_column_spec()?;
                    columns.push(col);
                }
            }

            if !self.consume_if(&Token::Comma) {
                break;
            }
            if self.peek() == &Token::RParen {
                break;
            }
        }

        self.expect(&Token::RParen)?;

        Ok(CreateTableStmt {
            table,
            if_not_exists,
            columns,
            table_constraints,
        })
    }

    fn parse_column_spec(&mut self) -> Result<ColumnSpec> {
        let name = self.expect_ident_or_kw()?;
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
        match self.peek().clone() {
            Token::Int => {
                self.advance();
                Ok(DataType::Int)
            }
            Token::BigInt => {
                self.advance();
                Ok(DataType::BigInt)
            }
            Token::Real => {
                self.advance();
                Ok(DataType::Real)
            }
            Token::Text | Token::Varchar => {
                self.advance();
                // consume optional (N)
                if self.peek() == &Token::LParen {
                    self.advance();
                    self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                }
                Ok(DataType::Text)
            }
            Token::Bool | Token::Boolean => {
                self.advance();
                Ok(DataType::Bool)
            }
            Token::Ident(s) => {
                if let Some(dt) = DataType::from_str(&s) {
                    self.advance();
                    Ok(dt)
                } else {
                    let (line, col) = self.peek_pos();
                    Err(MiniDbError::Parse {
                        line,
                        col,
                        msg: format!("unknown data type: {}", s),
                    })
                }
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("expected data type, got {:?}", tok),
                })
            }
        }
    }

    fn parse_create_index(&mut self, unique: bool) -> Result<CreateIndexStmt> {
        let if_not_exists = if self.peek() == &Token::If {
            self.advance();
            self.expect(&Token::Not)?;
            self.expect(&Token::Exists)?;
            true
        } else {
            false
        };

        let index_name = self.expect_ident_or_kw()?;
        self.expect(&Token::On)?;
        let table = self.expect_ident_or_kw()?;
        self.expect(&Token::LParen)?;
        let column = self.expect_ident_or_kw()?;
        self.expect(&Token::RParen)?;

        Ok(CreateIndexStmt {
            index_name,
            table,
            column,
            unique,
            if_not_exists,
        })
    }

    fn parse_drop(&mut self) -> Result<Stmt> {
        self.expect(&Token::Drop)?;
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
                let table = self.expect_ident_or_kw()?;
                Ok(Stmt::DropTable(DropTableStmt { table, if_exists }))
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
                let index_name = self.expect_ident_or_kw()?;
                Ok(Stmt::DropIndex(DropIndexStmt {
                    index_name,
                    if_exists,
                }))
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("expected TABLE or INDEX after DROP, got {:?}", tok),
                })
            }
        }
    }

    // Expression parsing with precedence climbing
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while self.consume_if(&Token::Or) {
            let right = self.parse_and_expr()?;
            left = Expr::BinOp(Box::new(left), BinOp::Or, Box::new(right));
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_not_expr()?;
        while self.consume_if(&Token::And) {
            let right = self.parse_not_expr()?;
            left = Expr::BinOp(Box::new(left), BinOp::And, Box::new(right));
        }
        Ok(left)
    }

    fn parse_not_expr(&mut self) -> Result<Expr> {
        if self.consume_if(&Token::Not) {
            let e = self.parse_not_expr()?;
            Ok(Expr::UnaryOp(UnaryOp::Not, Box::new(e)))
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;

        loop {
            let op = match self.peek().clone() {
                Token::Eq => BinOp::Eq,
                Token::Ne => BinOp::Ne,
                Token::Lt => BinOp::Lt,
                Token::Le => BinOp::Le,
                Token::Gt => BinOp::Gt,
                Token::Ge => BinOp::Ge,
                Token::Is => {
                    self.advance();
                    if self.consume_if(&Token::Not) {
                        self.expect(&Token::Null)?;
                        left = Expr::IsNotNull(Box::new(left));
                    } else {
                        self.expect(&Token::Null)?;
                        left = Expr::IsNull(Box::new(left));
                    }
                    continue;
                }
                Token::Not => {
                    self.advance();
                    if self.consume_if(&Token::In) {
                        self.expect(&Token::LParen)?;
                        let mut exprs = Vec::new();
                        loop {
                            exprs.push(self.parse_expr()?);
                            if !self.consume_if(&Token::Comma) {
                                break;
                            }
                        }
                        self.expect(&Token::RParen)?;
                        left =
                            Expr::UnaryOp(UnaryOp::Not, Box::new(Expr::In(Box::new(left), exprs)));
                    } else if self.consume_if(&Token::Like) {
                        if let Token::StringLit(pattern) = self.advance().clone() {
                            left = Expr::UnaryOp(
                                UnaryOp::Not,
                                Box::new(Expr::Like(Box::new(left), pattern)),
                            );
                        } else {
                            let (line, col) = self.peek_pos();
                            return Err(MiniDbError::Parse {
                                line,
                                col,
                                msg: "expected string pattern after LIKE".to_string(),
                            });
                        }
                    } else if self.consume_if(&Token::Between) {
                        let low = self.parse_additive()?;
                        self.expect(&Token::And)?;
                        let high = self.parse_additive()?;
                        left = Expr::UnaryOp(
                            UnaryOp::Not,
                            Box::new(Expr::Between(Box::new(left), Box::new(low), Box::new(high))),
                        );
                    } else {
                        let (line, col) = self.peek_pos();
                        return Err(MiniDbError::Parse {
                            line,
                            col,
                            msg: "expected IN, LIKE, or BETWEEN after NOT".to_string(),
                        });
                    }
                    continue;
                }
                Token::In => {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let mut exprs = Vec::new();
                    loop {
                        exprs.push(self.parse_expr()?);
                        if !self.consume_if(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RParen)?;
                    left = Expr::In(Box::new(left), exprs);
                    continue;
                }
                Token::Like => {
                    self.advance();
                    if let Token::StringLit(pattern) = self.advance().clone() {
                        left = Expr::Like(Box::new(left), pattern);
                    } else {
                        let (line, col) = self.peek_pos();
                        return Err(MiniDbError::Parse {
                            line,
                            col,
                            msg: "expected string pattern after LIKE".to_string(),
                        });
                    }
                    continue;
                }
                Token::Between => {
                    self.advance();
                    let low = self.parse_additive()?;
                    self.expect(&Token::And)?;
                    let high = self.parse_additive()?;
                    left = Expr::Between(Box::new(left), Box::new(low), Box::new(high));
                    continue;
                }
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Concat => BinOp::Concat,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let e = self.parse_primary()?;
                Ok(Expr::UnaryOp(UnaryOp::Neg, Box::new(e)))
            }
            Token::Plus => {
                self.advance();
                self.parse_primary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::IntLit(n) => {
                self.advance();
                Ok(Expr::Lit(Value::Int(n)))
            }
            Token::RealLit(r) => {
                self.advance();
                Ok(Expr::Lit(Value::Real(r)))
            }
            Token::StringLit(s) => {
                self.advance();
                Ok(Expr::Lit(Value::Text(s)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Lit(Value::Null))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Lit(Value::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Lit(Value::Bool(false)))
            }
            Token::LParen => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            Token::Coalesce => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut args = Vec::new();
                loop {
                    args.push(self.parse_expr()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::Function("COALESCE".to_string(), args))
            }
            Token::Length => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Function("LENGTH".to_string(), vec![arg]))
            }
            Token::Lower => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Function("LOWER".to_string(), vec![arg]))
            }
            Token::Upper => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Function("UPPER".to_string(), vec![arg]))
            }
            Token::Substr => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut args = Vec::new();
                loop {
                    args.push(self.parse_expr()?);
                    if !self.consume_if(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::Function("SUBSTR".to_string(), args))
            }
            Token::Abs => {
                self.advance();
                self.expect(&Token::LParen)?;
                let arg = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Function("ABS".to_string(), vec![arg]))
            }
            Token::Count | Token::Sum | Token::Avg | Token::Min | Token::Max => {
                let fname = format!("{:?}", self.peek()).to_uppercase();
                self.advance();
                self.expect(&Token::LParen)?;
                let args = if self.consume_if(&Token::Star) {
                    vec![Expr::Lit(Value::Int(1))]
                } else {
                    self.consume_if(&Token::Distinct);
                    vec![self.parse_expr()?]
                };
                self.expect(&Token::RParen)?;
                Ok(Expr::Function(fname, args))
            }
            Token::Case => {
                self.advance();
                self.parse_case_expr()
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                // Check for function call
                if self.peek() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != &Token::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if !self.consume_if(&Token::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::Function(name.to_uppercase(), args));
                }
                // Check for table.col
                if self.peek() == &Token::Dot {
                    self.advance();
                    let col = self.expect_ident_or_kw()?;
                    return Ok(Expr::Col(col, Some(name)));
                }
                Ok(Expr::Col(name, None))
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("unexpected token in expression: {:?}", tok),
                })
            }
        }
    }

    fn parse_case_expr(&mut self) -> Result<Expr> {
        let operand = if self.peek() != &Token::When {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };

        let mut branches = Vec::new();
        while self.consume_if(&Token::When) {
            let cond = self.parse_expr()?;
            self.expect(&Token::Then)?;
            let result = self.parse_expr()?;
            branches.push((cond, result));
        }

        let else_branch = if self.consume_if(&Token::Else) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };

        self.expect(&Token::End)?;

        Ok(Expr::Case {
            operand,
            branches,
            else_branch,
        })
    }
}

pub fn parse(sql: &str) -> Result<Vec<Stmt>> {
    let mut lexer = Lexer::new(sql);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_statements()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let stmts = parse("SELECT id, name FROM users WHERE id = 1;").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::Select(_)));
    }

    #[test]
    fn test_parse_insert() {
        let stmts = parse("INSERT INTO users (id, name) VALUES (1, 'alice');").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::Insert(_)));
    }

    #[test]
    fn test_parse_create_table() {
        let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0);";
        let stmts = parse(sql).unwrap();
        assert_eq!(stmts.len(), 1);
        if let Stmt::CreateTable(ct) = &stmts[0] {
            assert_eq!(ct.table, "t");
            assert_eq!(ct.columns.len(), 3);
        } else {
            panic!("expected CreateTable");
        }
    }

    #[test]
    fn test_parse_join() {
        let sql = "SELECT a.id, b.name FROM a INNER JOIN b ON a.id = b.id;";
        let stmts = parse(sql).unwrap();
        assert!(matches!(stmts[0], Stmt::Select(_)));
    }

    #[test]
    fn test_parse_multiple_stmts() {
        let stmts = parse("SELECT 1; SELECT 2;").unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn test_parse_update() {
        let stmts = parse("UPDATE t SET x = 1, y = 'z' WHERE id = 5;").unwrap();
        assert!(matches!(stmts[0], Stmt::Update(_)));
    }

    #[test]
    fn test_parse_delete() {
        let stmts = parse("DELETE FROM t WHERE id > 10;").unwrap();
        assert!(matches!(stmts[0], Stmt::Delete(_)));
    }

    #[test]
    fn test_parse_begin_commit() {
        let stmts = parse("BEGIN; COMMIT;").unwrap();
        assert!(matches!(stmts[0], Stmt::Begin));
        assert!(matches!(stmts[1], Stmt::Commit));
    }

    #[test]
    fn test_parse_is_null() {
        let stmts = parse("SELECT * FROM t WHERE x IS NULL;").unwrap();
        if let Stmt::Select(s) = &stmts[0] {
            assert!(matches!(s.where_clause, Some(Expr::IsNull(_))));
        }
    }

    #[test]
    fn test_parse_create_index() {
        let stmts = parse("CREATE INDEX idx_age ON users (age);").unwrap();
        assert!(matches!(stmts[0], Stmt::CreateIndex(_)));
    }
}
