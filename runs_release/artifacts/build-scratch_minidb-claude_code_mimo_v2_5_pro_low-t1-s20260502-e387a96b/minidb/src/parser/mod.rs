pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::Token;
use crate::parser::ast::*;
use crate::types::{ColumnType, Value};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        match self.advance() {
            Some(tok) if tok == expected => Ok(()),
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected {expected:?}, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric(format!(
                "expected {expected:?}, got end of input"
            ))),
        }
    }

    fn expect_identifier(&mut self) -> Result<String> {
        match self.advance().cloned() {
            Some(Token::Identifier(s)) => Ok(s),
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected identifier, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric(
                "expected identifier, got end of input".into(),
            )),
        }
    }

    /// Parse an identifier or keyword as an alias (SQL standard allows keywords as aliases)
    fn expect_alias(&mut self) -> Result<String> {
        match self.advance().cloned() {
            Some(Token::Identifier(s)) => Ok(s),
            Some(tok) => {
                // Try to get keyword name from debug representation
                let s = format!("{:?}", tok);
                // Keywords are simple single-word tokens like Sum, Count, etc.
                if s.chars().all(|c| c.is_alphanumeric()) {
                    Ok(s.to_lowercase())
                } else {
                    Err(MiniDbError::ParseGeneric(format!(
                        "expected alias, got {tok:?}"
                    )))
                }
            }
            None => Err(MiniDbError::ParseGeneric(
                "expected alias, got end of input".into(),
            )),
        }
    }

    fn peek_is(&self, tok: &Token) -> bool {
        self.peek() == Some(tok)
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek().cloned() {
            Some(Token::Create) => self.parse_create(),
            Some(Token::Drop) => self.parse_drop(),
            Some(Token::Insert) => self.parse_insert(),
            Some(Token::Select) => self.parse_select().map(Statement::Select),
            Some(Token::Update) => self.parse_update(),
            Some(Token::Delete) => self.parse_delete(),
            Some(Token::Begin) => {
                self.advance();
                Ok(Statement::Begin)
            }
            Some(Token::Commit) => {
                self.advance();
                Ok(Statement::Commit)
            }
            Some(Token::Rollback) => {
                self.advance();
                Ok(Statement::Rollback)
            }
            Some(Token::MetaDot) => self.parse_meta(),
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "unexpected token: {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric("empty statement".into())),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.advance(); // CREATE
        match self.peek().cloned() {
            Some(Token::Table) => self.parse_create_table(),
            Some(Token::Index) => self.parse_create_index(),
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected TABLE or INDEX after CREATE, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric(
                "unexpected end after CREATE".into(),
            )),
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        self.advance(); // TABLE
        let name = self.expect_identifier()?;
        self.expect(&Token::LeftParen)?;
        let mut columns = Vec::new();
        let mut check = None;

        loop {
            match self.peek().cloned() {
                Some(Token::Identifier(_)) => {
                    columns.push(self.parse_column_def()?);
                }
                Some(Token::Check) => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    check = Some(self.parse_expr()?);
                    self.expect(&Token::RightParen)?;
                }
                _ => break,
            }
            if self.peek_is(&Token::Comma) {
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
                Some(Token::Primary) => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    primary_key = true;
                    not_null = true;
                }
                Some(Token::Not) => {
                    self.advance();
                    self.expect(&Token::Null)?;
                    not_null = true;
                }
                Some(Token::Unique) => {
                    self.advance();
                    unique = true;
                }
                Some(Token::Default) => {
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
        match self.advance().cloned() {
            Some(Token::Int) => Ok(ColumnType::Int),
            Some(Token::Bigint) => Ok(ColumnType::Int),
            Some(Token::Real) => Ok(ColumnType::Real),
            Some(Token::Text) => Ok(ColumnType::Text),
            Some(Token::Bool) => Ok(ColumnType::Bool),
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected column type, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric("expected column type".into())),
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.advance(); // INDEX
        let index_name = self.expect_identifier()?;
        self.expect(&Token::On)?;
        let table_name = self.expect_identifier()?;
        self.expect(&Token::LeftParen)?;
        let column_name = self.expect_identifier()?;
        self.expect(&Token::RightParen)?;
        Ok(Statement::CreateIndex(CreateIndexStmt {
            index_name,
            table_name,
            column_name,
        }))
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.advance(); // DROP
        match self.peek().cloned() {
            Some(Token::Table) => {
                self.advance();
                let name = self.expect_identifier()?;
                Ok(Statement::DropTable { name })
            }
            Some(Token::Index) => {
                self.advance();
                let name = self.expect_identifier()?;
                Ok(Statement::DropIndex { name })
            }
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected TABLE or INDEX after DROP, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric(
                "unexpected end after DROP".into(),
            )),
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.advance(); // INSERT
        self.expect(&Token::Into)?;
        let table = self.expect_identifier()?;

        let columns = if self.peek_is(&Token::LeftParen) {
            self.advance();
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_identifier()?);
                if self.peek_is(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::RightParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;
        let mut values = Vec::new();
        loop {
            self.expect(&Token::LeftParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                if self.peek_is(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::RightParen)?;
            values.push(row);
            if self.peek_is(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Statement::Insert(InsertStmt {
            table,
            columns,
            values,
        }))
    }

    pub fn parse_select(&mut self) -> Result<SelectStmt> {
        self.advance(); // SELECT

        let distinct = if self.peek_is(&Token::Distinct) {
            self.advance();
            true
        } else {
            false
        };

        let projection = self.parse_select_list()?;

        let from = if self.peek_is(&Token::From) {
            self.advance();
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        let where_clause = if self.peek_is(&Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.peek_is(&Token::Order) {
            self.advance();
            self.expect(&Token::By)?;
            Some(self.parse_order_by()?)
        } else {
            None
        };

        let limit = if self.peek_is(&Token::Limit) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let offset = if self.peek_is(&Token::Offset) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(SelectStmt {
            distinct,
            projection,
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
            if self.peek_is(&Token::Star) {
                self.advance();
                items.push(SelectItem::Star);
            } else {
                let expr = self.parse_expr()?;
                let alias = if self.peek_is(&Token::As) {
                    self.advance();
                    Some(self.expect_alias()?)
                } else if let Some(Token::Identifier(_)) = self.peek() {
                    // Allow alias without AS
                    Some(self.expect_identifier()?)
                } else {
                    None
                };
                items.push(SelectItem::Expr { expr, alias });
            }
            if self.peek_is(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(items)
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let mut left = self.parse_from_table()?;

        loop {
            let join_type = match self.peek() {
                Some(Token::Join) => {
                    self.advance();
                    JoinType::Inner
                }
                Some(Token::Inner) => {
                    self.advance();
                    self.expect(&Token::Join)?;
                    JoinType::Inner
                }
                Some(Token::Cross) => {
                    self.advance();
                    self.expect(&Token::Join)?;
                    JoinType::Cross
                }
                _ => break,
            };

            let right = self.parse_from_table()?;
            let on = if self.peek_is(&Token::On) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };

            left = FromClause::Join {
                left: Box::new(left),
                right: Box::new(right),
                on,
                join_type,
            };
        }

        Ok(left)
    }

    fn parse_from_table(&mut self) -> Result<FromClause> {
        let name = self.expect_identifier()?;
        let alias = if self.peek_is(&Token::As) {
            self.advance();
            Some(self.expect_alias()?)
        } else if let Some(Token::Identifier(_)) = self.peek() {
            Some(self.expect_identifier()?)
        } else {
            None
        };
        Ok(FromClause::Table { name, alias })
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let asc = match self.peek() {
                Some(Token::Asc) => {
                    self.advance();
                    true
                }
                Some(Token::Desc) => {
                    self.advance();
                    false
                }
                _ => true,
            };
            items.push(OrderByItem { expr, asc });
            if self.peek_is(&Token::Comma) {
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
            let val = self.parse_expr()?;
            assignments.push((col, val));
            if self.peek_is(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let where_clause = if self.peek_is(&Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
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
        let where_clause = if self.peek_is(&Token::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete(DeleteStmt {
            table,
            where_clause,
        }))
    }

    fn parse_meta(&mut self) -> Result<Statement> {
        self.advance(); // .
        match self.advance().cloned() {
            Some(Token::Identifier(s)) => match s.to_lowercase().as_str() {
                "schema" => Ok(Statement::MetaCommand(MetaCommand::Schema)),
                "tables" => Ok(Statement::MetaCommand(MetaCommand::Tables)),
                "indexes" | "indices" => Ok(Statement::MetaCommand(MetaCommand::Indexes)),
                "exit" | "quit" => Ok(Statement::MetaCommand(MetaCommand::Exit)),
                "format" => {
                    let kind = self.expect_identifier()?;
                    Ok(Statement::MetaCommand(MetaCommand::Format(kind)))
                }
                _ => Err(MiniDbError::ParseGeneric(format!(
                    "unknown meta command: .{s}"
                ))),
            },
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected meta command name, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric("unexpected end after '.'".into())),
        }
    }

    // Expression parsing with precedence climbing
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek_is(&Token::Or) {
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
        while self.peek_is(&Token::And) {
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
        if self.peek_is(&Token::Not) {
            self.advance();
            let operand = self.parse_comparison()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_concat()?;
        loop {
            let op = match self.peek() {
                Some(Token::Eq) => {
                    self.advance();
                    BinaryOp::Eq
                }
                Some(Token::Neq) => {
                    self.advance();
                    BinaryOp::Neq
                }
                Some(Token::Lt) => {
                    self.advance();
                    BinaryOp::Lt
                }
                Some(Token::Le) => {
                    self.advance();
                    BinaryOp::Le
                }
                Some(Token::Gt) => {
                    self.advance();
                    BinaryOp::Gt
                }
                Some(Token::Ge) => {
                    self.advance();
                    BinaryOp::Ge
                }
                Some(Token::Is) => {
                    self.advance();
                    if self.peek_is(&Token::Not) {
                        self.advance();
                        self.expect(&Token::Null)?;
                        return Ok(Expr::IsNotNull(Box::new(left)));
                    } else {
                        self.expect(&Token::Null)?;
                        return Ok(Expr::IsNull(Box::new(left)));
                    }
                }
                Some(Token::Not) => {
                    // Check for NOT IN, NOT BETWEEN, NOT LIKE
                    // We need lookahead
                    if self.pos + 1 < self.tokens.len() {
                        match &self.tokens[self.pos + 1] {
                            Token::In => {
                                self.advance(); // NOT
                                self.advance(); // IN
                                self.expect(&Token::LeftParen)?;
                                let mut list = Vec::new();
                                loop {
                                    list.push(self.parse_expr()?);
                                    if self.peek_is(&Token::Comma) {
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                }
                                self.expect(&Token::RightParen)?;
                                left = Expr::UnaryOp {
                                    op: UnaryOp::Not,
                                    operand: Box::new(Expr::InList {
                                        expr: Box::new(left),
                                        list,
                                    }),
                                };
                                continue;
                            }
                            Token::Between => {
                                self.advance(); // NOT
                                self.advance(); // BETWEEN
                                let low = self.parse_additive()?;
                                self.expect(&Token::And)?;
                                let high = self.parse_additive()?;
                                left = Expr::UnaryOp {
                                    op: UnaryOp::Not,
                                    operand: Box::new(Expr::Between {
                                        expr: Box::new(left),
                                        low: Box::new(low),
                                        high: Box::new(high),
                                    }),
                                };
                                continue;
                            }
                            _ => break,
                        }
                    } else {
                        break;
                    }
                }
                Some(Token::In) => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    let mut list = Vec::new();
                    loop {
                        list.push(self.parse_expr()?);
                        if self.peek_is(&Token::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&Token::RightParen)?;
                    left = Expr::InList {
                        expr: Box::new(left),
                        list,
                    };
                    continue;
                }
                Some(Token::Between) => {
                    self.advance();
                    let low = self.parse_additive()?;
                    self.expect(&Token::And)?;
                    let high = self.parse_additive()?;
                    left = Expr::Between {
                        expr: Box::new(left),
                        low: Box::new(low),
                        high: Box::new(high),
                    };
                    continue;
                }
                Some(Token::Like) => {
                    self.advance();
                    let pattern = self.parse_primary()?;
                    // Treat LIKE as a function call
                    left = Expr::FunctionCall {
                        name: "LIKE".into(),
                        args: vec![left, pattern],
                    };
                    continue;
                }
                _ => break,
            };
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
        while self.peek_is(&Token::Concat) {
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
                Some(Token::Plus) => {
                    self.advance();
                    BinaryOp::Add
                }
                Some(Token::Minus) => {
                    self.advance();
                    BinaryOp::Sub
                }
                _ => break,
            };
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
                Some(Token::Star) => {
                    self.advance();
                    BinaryOp::Mul
                }
                Some(Token::Slash) => {
                    self.advance();
                    BinaryOp::Div
                }
                _ => break,
            };
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
        match self.peek() {
            Some(Token::Minus) => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                })
            }
            Some(Token::Not) => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().cloned() {
            Some(Token::IntegerLiteral(n)) => {
                self.advance();
                Ok(Expr::Literal(Value::Int(n)))
            }
            Some(Token::RealLiteral(r)) => {
                self.advance();
                Ok(Expr::Literal(Value::Real(r)))
            }
            Some(Token::StringLiteral(s)) => {
                self.advance();
                Ok(Expr::Literal(Value::Text(s)))
            }
            Some(Token::Null) => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(true)))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(false)))
            }
            Some(Token::LeftParen) => {
                self.advance();
                // Check for subquery
                if self.peek_is(&Token::Select) {
                    let sel = self.parse_select()?;
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::Subquery(Box::new(sel)))
                } else {
                    let expr = self.parse_expr()?;
                    self.expect(&Token::RightParen)?;
                    Ok(expr)
                }
            }
            Some(Token::Identifier(name)) => {
                self.advance();
                if self.peek_is(&Token::LeftParen) {
                    // Function call
                    self.advance();
                    let mut args = Vec::new();
                    if !self.peek_is(&Token::RightParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.peek_is(&Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::FunctionCall { name, args })
                } else if self.peek_is(&Token::Dot) {
                    // Qualified column: table.col
                    self.advance();
                    let col_name = self.expect_identifier()?;
                    Ok(Expr::ColumnRef {
                        table: Some(name),
                        name: col_name,
                    })
                } else {
                    Ok(Expr::ColumnRef { table: None, name })
                }
            }
            // Built-in function keywords used as identifiers
            Some(Token::Count)
            | Some(Token::Sum)
            | Some(Token::Avg)
            | Some(Token::Min)
            | Some(Token::Max)
            | Some(Token::Upper)
            | Some(Token::Lower)
            | Some(Token::Length)
            | Some(Token::Substr)
            | Some(Token::Abs)
            | Some(Token::Coalesce) => {
                let name = match self.peek() {
                    Some(Token::Count) => "COUNT",
                    Some(Token::Sum) => "SUM",
                    Some(Token::Avg) => "AVG",
                    Some(Token::Min) => "MIN",
                    Some(Token::Max) => "MAX",
                    Some(Token::Upper) => "UPPER",
                    Some(Token::Lower) => "LOWER",
                    Some(Token::Length) => "LENGTH",
                    Some(Token::Substr) => "SUBSTR",
                    Some(Token::Abs) => "ABS",
                    Some(Token::Coalesce) => "COALESCE",
                    _ => unreachable!(),
                }
                .to_string();
                self.advance();
                self.expect(&Token::LeftParen)?;
                let mut args = Vec::new();
                if !self.peek_is(&Token::RightParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.peek_is(&Token::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&Token::RightParen)?;
                Ok(Expr::FunctionCall { name, args })
            }
            Some(Token::Case) => {
                self.advance();
                let mut operands = Vec::new();
                while self.peek_is(&Token::When) {
                    self.advance();
                    let condition = self.parse_expr()?;
                    self.expect(&Token::Then)?;
                    let result = self.parse_expr()?;
                    operands.push((condition, result));
                }
                let else_expr = if self.peek_is(&Token::Else) {
                    self.advance();
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };
                self.expect(&Token::End)?;
                Ok(Expr::Case {
                    operands,
                    else_expr,
                })
            }
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "unexpected token in expression: {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric(
                "unexpected end of input in expression".into(),
            )),
        }
    }

    fn parse_literal_value(&mut self) -> Result<Value> {
        match self.advance().cloned() {
            Some(Token::IntegerLiteral(n)) => Ok(Value::Int(n)),
            Some(Token::RealLiteral(r)) => Ok(Value::Real(r)),
            Some(Token::StringLiteral(s)) => Ok(Value::Text(s)),
            Some(Token::Null) => Ok(Value::Null),
            Some(Token::True) => Ok(Value::Bool(true)),
            Some(Token::False) => Ok(Value::Bool(false)),
            Some(Token::Minus) => match self.advance().cloned() {
                Some(Token::IntegerLiteral(n)) => Ok(Value::Int(-n)),
                Some(Token::RealLiteral(r)) => Ok(Value::Real(-r)),
                _ => Err(MiniDbError::ParseGeneric(
                    "expected number after '-'".into(),
                )),
            },
            Some(tok) => Err(MiniDbError::ParseGeneric(format!(
                "expected literal value, got {tok:?}"
            ))),
            None => Err(MiniDbError::ParseGeneric("expected literal value".into())),
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Vec<Statement>> {
    let mut parser = Parser::new(tokens);
    let mut stmts = Vec::new();
    while parser.peek().is_some() {
        stmts.push(parser.parse_statement()?);
        // Consume optional semicolon
        if parser.peek_is(&Token::Semicolon) {
            parser.advance();
        }
    }
    Ok(stmts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse_one(sql: &str) -> Statement {
        let tokens = Lexer::new(sql).tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse_statement().unwrap()
    }

    #[test]
    fn test_parse_create_table() {
        let stmt = parse_one(
            "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0)",
        );
        match stmt {
            Statement::CreateTable(ct) => {
                assert_eq!(ct.name, "users");
                assert_eq!(ct.columns.len(), 3);
                assert!(ct.columns[0].primary_key);
                assert!(ct.columns[1].not_null);
                assert_eq!(ct.columns[2].default, Some(Value::Int(0)));
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmt = parse_one("INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob')");
        match stmt {
            Statement::Insert(ins) => {
                assert_eq!(ins.table, "users");
                assert_eq!(ins.columns.as_ref().unwrap().len(), 2);
                assert_eq!(ins.values.len(), 2);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_parse_select() {
        let stmt = parse_one(
            "SELECT u.name, u.age FROM users u WHERE u.age > 18 ORDER BY u.name ASC LIMIT 10",
        );
        match stmt {
            Statement::Select(sel) => {
                assert_eq!(sel.projection.len(), 2);
                assert!(sel.where_clause.is_some());
                assert!(sel.order_by.is_some());
                assert!(sel.limit.is_some());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_select_join() {
        let stmt = parse_one("SELECT * FROM users u INNER JOIN orders o ON u.id = o.user_id");
        match stmt {
            Statement::Select(sel) => {
                assert!(sel.from.is_some());
                match sel.from.as_ref().unwrap() {
                    FromClause::Join { .. } => {}
                    _ => panic!("expected Join"),
                }
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmt = parse_one("UPDATE users SET name = 'Charlie', age = 30 WHERE id = 1");
        match stmt {
            Statement::Update(upd) => {
                assert_eq!(upd.table, "users");
                assert_eq!(upd.assignments.len(), 2);
                assert!(upd.where_clause.is_some());
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmt = parse_one("DELETE FROM users WHERE id = 1");
        match stmt {
            Statement::Delete(del) => {
                assert_eq!(del.table, "users");
                assert!(del.where_clause.is_some());
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_parse_expressions() {
        let stmt = parse_one("SELECT 1 + 2 * 3, 'a' || 'b', -5 FROM t");
        match stmt {
            Statement::Select(sel) => {
                assert_eq!(sel.projection.len(), 3);
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_is_null() {
        let stmt = parse_one("SELECT * FROM t WHERE x IS NULL AND y IS NOT NULL");
        match stmt {
            Statement::Select(sel) => {
                assert!(sel.where_clause.is_some());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_meta() {
        let stmt = parse_one(".schema");
        assert!(matches!(stmt, Statement::MetaCommand(MetaCommand::Schema)));
    }

    #[test]
    fn test_parse_transaction() {
        assert!(matches!(parse_one("BEGIN"), Statement::Begin));
        assert!(matches!(parse_one("COMMIT"), Statement::Commit));
        assert!(matches!(parse_one("ROLLBACK"), Statement::Rollback));
    }

    #[test]
    fn test_parse_functions() {
        let stmt = parse_one("SELECT LENGTH(name), UPPER(name), ABS(-5), COALESCE(a, b, 0) FROM t");
        match stmt {
            Statement::Select(sel) => {
                assert_eq!(sel.projection.len(), 4);
            }
            _ => panic!("expected Select"),
        }
    }
}
