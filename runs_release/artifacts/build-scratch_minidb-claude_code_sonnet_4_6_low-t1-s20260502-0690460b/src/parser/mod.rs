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

    pub fn parse(input: &str) -> Result<Vec<Statement>> {
        let tokens = Lexer::tokenize(input)?;
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
        let tok = self.advance().clone();
        if &tok == expected || self.token_matches(&tok, expected) {
            Ok(())
        } else {
            Err(MiniDbError::Parse {
                msg: format!("expected {:?}, found {:?}", expected, tok),
                line,
                col,
            })
        }
    }

    fn token_matches(&self, a: &Token, b: &Token) -> bool {
        std::mem::discriminant(a) == std::mem::discriminant(b)
    }

    fn expect_ident(&mut self) -> Result<String> {
        let (line, col) = self.peek_pos();
        match self.advance().clone() {
            Token::Ident(s) => Ok(s),
            Token::Text => Ok("text".into()),
            Token::Index => Ok("index".into()),
            tok => Err(MiniDbError::Parse {
                msg: format!("expected identifier, found {:?}", tok),
                line,
                col,
            }),
        }
    }

    fn try_consume(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        loop {
            while self.try_consume(&Token::Semi) {}
            if self.peek() == &Token::Eof {
                break;
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            // consume trailing semicolon(s)
            while self.try_consume(&Token::Semi) {}
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Select => Ok(Statement::Select(self.parse_select()?)),
            Token::Insert => Ok(Statement::Insert(self.parse_insert()?)),
            Token::Update => Ok(Statement::Update(self.parse_update()?)),
            Token::Delete => Ok(Statement::Delete(self.parse_delete()?)),
            Token::Create => self.parse_create(),
            Token::Drop => self.parse_drop(),
            Token::Begin => {
                self.advance();
                self.try_consume(&Token::Ident(_dummy()));
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
                let fmt = self.expect_ident()?;
                Ok(Statement::MetaFormat(fmt))
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("unexpected token {:?}", tok),
                line,
                col,
            }),
        }
    }

    fn parse_select(&mut self) -> Result<SelectStmt> {
        self.expect(&Token::Select)?;
        let distinct = self.try_consume(&Token::Distinct);
        let columns = self.parse_select_list()?;
        let from = if self.try_consume(&Token::From) {
            Some(self.parse_from_clause()?)
        } else {
            None
        };
        let where_clause = if self.try_consume(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let order_by = if self.peek() == &Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            self.parse_order_by()?
        } else {
            Vec::new()
        };
        let limit = if self.try_consume(&Token::Limit) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let offset = if self.try_consume(&Token::Offset) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(SelectStmt {
            distinct,
            columns,
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
            if self.peek() == &Token::Star {
                self.advance();
                items.push(SelectItem::Star);
            } else {
                let expr = self.parse_expr()?;
                // Check for table.* case
                let item = if let Expr::QualifiedColumn(tbl, col) = &expr {
                    if col == "*" {
                        SelectItem::TableStar(tbl.clone())
                    } else {
                        let alias = if self.peek() == &Token::Ident("AS".into())
                            || matches!(self.peek(), Token::Ident(_))
                        {
                            // AS keyword
                            if let Token::Ident(s) = self.peek() {
                                if s.to_uppercase() == "AS" {
                                    self.advance();
                                    Some(self.expect_ident()?)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        SelectItem::Expr { expr, alias }
                    }
                } else {
                    let alias = if let Token::Ident(s) = self.peek() {
                        if s.to_uppercase() == "AS" {
                            self.advance();
                            Some(self.expect_ident()?)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    SelectItem::Expr { expr, alias }
                };
                items.push(item);
            }
            if !self.try_consume(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let name = self.expect_ident()?;
        let alias = if let Token::Ident(s) = self.peek() {
            if s.to_uppercase() != "WHERE"
                && s.to_uppercase() != "ORDER"
                && s.to_uppercase() != "LIMIT"
                && s.to_uppercase() != "INNER"
                && s.to_uppercase() != "JOIN"
                && s.to_uppercase() != "AS"
            {
                let a = s.clone();
                self.advance();
                Some(a)
            } else if s.to_uppercase() == "AS" {
                self.advance();
                Some(self.expect_ident()?)
            } else {
                None
            }
        } else {
            None
        };
        let mut from = FromClause::Table { name, alias };
        // Handle INNER JOIN
        while self.peek() == &Token::Inner || self.peek() == &Token::Join {
            if self.peek() == &Token::Inner {
                self.advance();
            }
            self.expect(&Token::Join)?;
            let right_name = self.expect_ident()?;
            let right_alias = if let Token::Ident(s) = self.peek() {
                if s.to_uppercase() == "AS" {
                    self.advance();
                    Some(self.expect_ident()?)
                } else if s.to_uppercase() != "ON" {
                    let a = s.clone();
                    self.advance();
                    Some(a)
                } else {
                    None
                }
            } else {
                None
            };
            self.expect(&Token::On)?;
            let on = self.parse_expr()?;
            from = FromClause::Join {
                left: Box::new(from),
                right: Box::new(FromClause::Table {
                    name: right_name,
                    alias: right_alias,
                }),
                on,
            };
        }
        Ok(from)
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let asc = if self.try_consume(&Token::Desc) {
                false
            } else {
                self.try_consume(&Token::Asc);
                true
            };
            items.push(OrderByItem { expr, asc });
            if !self.try_consume(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    fn parse_insert(&mut self) -> Result<InsertStmt> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;
        let table = self.expect_ident()?;
        let columns = if self.peek() == &Token::LParen {
            // check if next is ident (column list) or value
            self.advance();
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if !self.try_consume(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };
        self.expect(&Token::Values)?;
        let mut rows = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let mut vals = Vec::new();
            loop {
                vals.push(self.parse_expr()?);
                if !self.try_consume(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            rows.push(vals);
            if !self.try_consume(&Token::Comma) {
                break;
            }
        }
        Ok(InsertStmt {
            table,
            columns,
            values: rows,
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
            if !self.try_consume(&Token::Comma) {
                break;
            }
        }
        let where_clause = if self.try_consume(&Token::Where) {
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
        let where_clause = if self.try_consume(&Token::Where) {
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
                let if_not_exists = if self.peek() == &Token::If {
                    self.advance();
                    self.expect(&Token::Not)?;
                    self.expect(&Token::Exists)?;
                    true
                } else {
                    false
                };
                let name = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let mut columns = Vec::new();
                let mut checks = Vec::new();
                loop {
                    if self.peek() == &Token::Check {
                        self.advance();
                        self.expect(&Token::LParen)?;
                        let expr = self.parse_expr()?;
                        self.expect(&Token::RParen)?;
                        checks.push(expr);
                    } else if self.peek() == &Token::Primary {
                        // table-level PRIMARY KEY (col)
                        self.advance();
                        self.expect(&Token::Key)?;
                        self.expect(&Token::LParen)?;
                        let pk_col = self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                        // Mark the column
                        for col in &mut columns {
                            if let ColumnDef { name, .. } = col {
                                if name == &pk_col {
                                    col.primary_key = true;
                                    col.not_null = true;
                                }
                            }
                        }
                    } else {
                        columns.push(self.parse_column_def()?);
                    }
                    if !self.try_consume(&Token::Comma) {
                        break;
                    }
                    if self.peek() == &Token::RParen {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                Ok(Statement::CreateTable(CreateTableStmt {
                    name,
                    if_not_exists,
                    columns,
                    checks,
                }))
            }
            Token::Index => {
                self.advance();
                let unique = false;
                let if_not_exists = if self.peek() == &Token::If {
                    self.advance();
                    self.expect(&Token::Not)?;
                    self.expect(&Token::Exists)?;
                    true
                } else {
                    false
                };
                let name = self.expect_ident()?;
                self.expect(&Token::On)?;
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
            Token::Unique => {
                self.advance();
                self.expect(&Token::Index)?;
                let if_not_exists = if self.peek() == &Token::If {
                    self.advance();
                    self.expect(&Token::Not)?;
                    self.expect(&Token::Exists)?;
                    true
                } else {
                    false
                };
                let name = self.expect_ident()?;
                self.expect(&Token::On)?;
                let table = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let column = self.expect_ident()?;
                self.expect(&Token::RParen)?;
                Ok(Statement::CreateIndex(CreateIndexStmt {
                    name,
                    table,
                    column,
                    unique: true,
                    if_not_exists,
                }))
            }
            tok => Err(MiniDbError::Parse {
                msg: format!("expected TABLE or INDEX after CREATE, found {:?}", tok),
                line,
                col,
            }),
        }
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let dtype = self.parse_data_type()?;
        let mut not_null = false;
        let mut primary_key = false;
        let mut unique = false;
        let mut default = None;
        loop {
            match self.peek().clone() {
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
                    default = Some(self.parse_expr()?);
                }
                _ => break,
            }
        }
        Ok(ColumnDef {
            name,
            dtype,
            not_null,
            primary_key,
            unique,
            default,
        })
    }

    fn parse_data_type(&mut self) -> Result<DataType> {
        let (line, col) = self.peek_pos();
        let dt = match self.advance().clone() {
            Token::Int => DataType::Int,
            Token::BigInt => DataType::BigInt,
            Token::Real => DataType::Real,
            Token::Text => DataType::Text,
            Token::Bool => DataType::Bool,
            Token::Ident(s) => match s.to_uppercase().as_str() {
                "INT" | "INTEGER" => DataType::Int,
                "BIGINT" => DataType::BigInt,
                "REAL" | "FLOAT" | "DOUBLE" => DataType::Real,
                "TEXT" | "VARCHAR" | "CHAR" | "STRING" => {
                    // consume optional (n) for VARCHAR(n)
                    if self.peek() == &Token::LParen {
                        self.advance();
                        self.parse_expr()?; // discard
                        self.expect(&Token::RParen)?;
                    }
                    DataType::Text
                }
                "BOOL" | "BOOLEAN" => DataType::Bool,
                other => {
                    return Err(MiniDbError::Parse {
                        msg: format!("unknown data type '{}'", other),
                        line,
                        col,
                    })
                }
            },
            tok => {
                return Err(MiniDbError::Parse {
                    msg: format!("expected data type, found {:?}", tok),
                    line,
                    col,
                })
            }
        };
        // consume optional (n)
        if self.peek() == &Token::LParen && matches!(dt, DataType::Text) {
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
                msg: format!("expected TABLE or INDEX after DROP, found {:?}", tok),
                line,
                col,
            }),
        }
    }

    // Expression parsing with precedence climbing
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek() == &Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.peek() == &Token::And {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.peek() == &Token::Not {
            self.advance();
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
        let left = self.parse_addition()?;
        let op = match self.peek() {
            Token::Eq => BinOp::Eq,
            Token::Neq => BinOp::Neq,
            Token::Lt => BinOp::Lt,
            Token::Lte => BinOp::Lte,
            Token::Gt => BinOp::Gt,
            Token::Gte => BinOp::Gte,
            Token::Is => {
                self.advance();
                if self.try_consume(&Token::Not) {
                    self.expect(&Token::Null)?;
                    return Ok(Expr::IsNotNull(Box::new(left)));
                } else {
                    self.expect(&Token::Null)?;
                    return Ok(Expr::IsNull(Box::new(left)));
                }
            }
            Token::Not => {
                self.advance();
                if self.peek() == &Token::In {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let list = self.parse_expr_list()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::InList {
                        expr: Box::new(left),
                        list,
                        negated: true,
                    });
                }
                return Err(MiniDbError::Parse {
                    msg: "expected IN after NOT".into(),
                    line: 0,
                    col: 0,
                });
            }
            Token::In => {
                self.advance();
                self.expect(&Token::LParen)?;
                let list = self.parse_expr_list()?;
                self.expect(&Token::RParen)?;
                return Ok(Expr::InList {
                    expr: Box::new(left),
                    list,
                    negated: false,
                });
            }
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_addition()?;
        Ok(Expr::BinOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_expr_list(&mut self) -> Result<Vec<Expr>> {
        let mut list = Vec::new();
        loop {
            list.push(self.parse_expr()?);
            if !self.try_consume(&Token::Comma) {
                break;
            }
        }
        Ok(list)
    }

    fn parse_addition(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Concat => BinOp::Concat,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek() == &Token::Minus {
            self.advance();
            let expr = self.parse_primary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        if self.peek() == &Token::Not {
            self.advance();
            let expr = self.parse_primary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let (line, col) = self.peek_pos();
        match self.peek().clone() {
            Token::Integer(v) => {
                self.advance();
                Ok(Expr::Literal(Value::Int(v)))
            }
            Token::Float(v) => {
                self.advance();
                Ok(Expr::Literal(Value::Real(v)))
            }
            Token::StringLit(s) => {
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
            Token::Star => {
                // bare * in non-select context is error, but handle gracefully
                self.advance();
                Ok(Expr::Column("*".into()))
            }
            Token::Ident(name) => {
                self.advance();
                // function call
                if self.peek() == &Token::LParen {
                    self.advance();
                    let args = if self.peek() == &Token::RParen {
                        Vec::new()
                    } else if self.peek() == &Token::Star {
                        self.advance();
                        vec![Expr::Column("*".into())]
                    } else {
                        self.parse_expr_list()?
                    };
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::Function {
                        name: name.to_uppercase(),
                        args,
                    });
                }
                // qualified column table.col
                if self.peek() == &Token::Dot {
                    self.advance();
                    if self.peek() == &Token::Star {
                        self.advance();
                        return Ok(Expr::QualifiedColumn(name, "*".into()));
                    }
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

fn _dummy() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let stmts = Parser::parse("SELECT id, name FROM users WHERE id = 1;").unwrap();
        assert_eq!(stmts.len(), 1);
        matches!(stmts[0], Statement::Select(_));
    }

    #[test]
    fn test_parse_create_table() {
        let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL, val REAL DEFAULT 0.0);";
        let stmts = Parser::parse(sql).unwrap();
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
        let stmts = Parser::parse("INSERT INTO t (a, b) VALUES (1, 'x');").unwrap();
        assert_eq!(stmts.len(), 1);
        matches!(stmts[0], Statement::Insert(_));
    }

    #[test]
    fn test_parse_multiple() {
        let stmts = Parser::parse("SELECT 1; SELECT 2;").unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn test_parse_is_null() {
        let stmts = Parser::parse("SELECT * FROM t WHERE x IS NULL;").unwrap();
        assert_eq!(stmts.len(), 1);
    }
}
