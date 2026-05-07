pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Token, TokenWithPos};
use crate::types::{ColumnRef, ColumnType, SqlLiteral};
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
            .map(|t| (t.line, t.column))
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
        let tok = self.advance().clone();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            let (line, col) = self.peek_pos();
            Err(MiniDbError::ParseError {
                message: format!("expected {}, found {}", expected, tok),
                line,
                column: col,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance().clone() {
            Token::Identifier(name) => Ok(name),
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::ParseError {
                    message: format!("expected identifier, found {}", tok),
                    line,
                    column: col,
                })
            }
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

    pub fn parse_program(&mut self) -> Result<Program> {
        match self.peek() {
            Token::DotSchema => {
                self.advance();
                Ok(Program::Meta(MetaCommand::Schema))
            }
            Token::DotTables => {
                self.advance();
                Ok(Program::Meta(MetaCommand::Tables))
            }
            Token::DotIndexes => {
                self.advance();
                Ok(Program::Meta(MetaCommand::Indexes))
            }
            Token::DotExit => {
                self.advance();
                Ok(Program::Meta(MetaCommand::Exit))
            }
            Token::DotFormat => {
                self.advance();
                let fmt = self.expect_ident()?;
                Ok(Program::Meta(MetaCommand::Format(fmt)))
            }
            _ => {
                let stmt = self.parse_statement()?;
                Ok(Program::Statement(stmt))
            }
        }
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek() {
            Token::Select => self.parse_select(),
            Token::Insert => self.parse_insert(),
            Token::Update => self.parse_update(),
            Token::Delete => self.parse_delete(),
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
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::ParseError {
                    message: format!("unexpected token: {}", tok),
                    line,
                    column: col,
                })
            }
        }
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.advance(); // SELECT

        let columns = self.parse_select_columns()?;
        self.expect(&Token::From)?;
        let from = self.parse_from_clause()?;

        let where_clause = if self.match_token(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.match_token(&Token::Order) {
            self.expect(&Token::By)?;
            self.parse_order_by()?
        } else {
            vec![]
        };

        let limit = if self.match_token(&Token::Limit) {
            Some(self.parse_i64()?)
        } else {
            None
        };

        let offset = if self.match_token(&Token::Offset) {
            Some(self.parse_i64()?)
        } else {
            None
        };

        Ok(Statement::Select {
            columns,
            from,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_select_columns(&mut self) -> Result<Vec<SelectColumn>> {
        if self.match_token(&Token::Star) {
            return Ok(vec![SelectColumn::Star]);
        }

        let mut cols = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let alias =
                if self.match_token(&Token::As) || matches!(self.peek(), Token::Identifier(_)) {
                    if matches!(self.peek(), Token::Identifier(_)) {
                        Some(self.expect_ident()?)
                    } else {
                        Some(self.expect_ident()?)
                    }
                } else {
                    None
                };
            cols.push(SelectColumn::Expr(expr, alias));

            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(cols)
    }

    fn parse_from_clause(&mut self) -> Result<FromClause> {
        let table = self.expect_ident()?;
        let mut from = FromClause::Table(table);

        loop {
            if self.match_token(&Token::Inner) {
                self.expect(&Token::Join)?;
                let right = self.expect_ident()?;
                let alias = if self.match_token(&Token::As)
                    || matches!(self.peek(), Token::Identifier(_))
                {
                    if matches!(self.peek(), Token::Identifier(_)) {
                        Some(self.expect_ident()?)
                    } else {
                        Some(self.expect_ident()?)
                    }
                } else {
                    None
                };
                from = FromClause::Join {
                    left: Box::new(from),
                    right,
                    alias,
                };
            } else {
                break;
            }
        }

        Ok(from)
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByExpr>> {
        let mut exprs = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let ascending = if self.match_token(&Token::Desc) {
                false
            } else {
                self.match_token(&Token::Asc);
                true
            };
            exprs.push(OrderByExpr { expr, ascending });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(exprs)
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.advance(); // INSERT
        self.expect(&Token::Into)?;
        let table = self.expect_ident()?;

        let columns = if self.match_token(&Token::LParen) {
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if !self.match_token(&Token::Comma) {
                    break;
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
            loop {
                row.push(self.parse_expr()?);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
            values.push(row);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }

        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.advance(); // UPDATE
        let table = self.expect_ident()?;
        self.expect(&Token::Set)?;

        let mut set_clauses = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            set_clauses.push((col, expr));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }

        let where_clause = if self.match_token(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Update {
            table,
            set_clauses,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.advance(); // DELETE
        self.expect(&Token::From)?;
        let table = self.expect_ident()?;

        let where_clause = if self.match_token(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Delete {
            table,
            where_clause,
        })
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.advance(); // CREATE

        if self.match_token(&Token::Table) {
            self.parse_create_table()
        } else if self.match_token(&Token::Index) {
            self.parse_create_index()
        } else {
            let (line, col) = self.peek_pos();
            Err(MiniDbError::ParseError {
                message: "expected TABLE or INDEX after CREATE".to_string(),
                line,
                column: col,
            })
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;

        let mut columns = Vec::new();
        loop {
            let col = self.parse_column_def()?;
            columns.push(col);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }

        self.expect(&Token::RParen)?;
        Ok(Statement::CreateTable { name, columns })
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let col_type = self.parse_column_type()?;

        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;
        let mut check = None;

        loop {
            match self.peek() {
                Token::Primary => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    primary_key = true;
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
                    default = Some(self.parse_literal()?);
                }
                Token::Check => {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    // Parse check expression as a string
                    let mut depth = 1;
                    let mut expr_str = String::new();
                    while depth > 0 {
                        match self.peek() {
                            Token::LParen => {
                                expr_str.push('(');
                                self.advance();
                                depth += 1;
                            }
                            Token::RParen => {
                                depth -= 1;
                                if depth > 0 {
                                    expr_str.push(')');
                                }
                                self.advance();
                            }
                            Token::Eof => {
                                let (line, col) = self.peek_pos();
                                return Err(MiniDbError::ParseError {
                                    message: "unterminated CHECK constraint".to_string(),
                                    line,
                                    column: col,
                                });
                            }
                            tok => {
                                expr_str.push_str(&format!("{} ", tok));
                                self.advance();
                            }
                        }
                    }
                    check = Some(expr_str.trim().to_string());
                }
                _ => break,
            }
        }

        Ok(ColumnDef {
            name,
            col_type,
            primary_key,
            not_null,
            unique,
            default,
            check,
        })
    }

    fn parse_column_type(&mut self) -> Result<ColumnType> {
        match self.advance().clone() {
            Token::Int | Token::Bigint => Ok(ColumnType::Int),
            Token::Real => Ok(ColumnType::Real),
            Token::Text => Ok(ColumnType::Text),
            Token::Bool => Ok(ColumnType::Bool),
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::ParseError {
                    message: format!("expected column type, found {}", tok),
                    line,
                    column: col,
                })
            }
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect(&Token::On)?;
        let table = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let column = self.expect_ident()?;
        self.expect(&Token::RParen)?;

        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.advance(); // DROP

        if self.match_token(&Token::Table) {
            let name = self.expect_ident()?;
            Ok(Statement::DropTable { name })
        } else if self.match_token(&Token::Index) {
            let name = self.expect_ident()?;
            Ok(Statement::DropIndex { name })
        } else {
            let (line, col) = self.peek_pos();
            Err(MiniDbError::ParseError {
                message: "expected TABLE or INDEX after DROP".to_string(),
                line,
                column: col,
            })
        }
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while self.match_token(&Token::Or) {
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
        while self.match_token(&Token::And) {
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
        if self.match_token(&Token::Not) {
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
        let mut left = self.parse_concat()?;
        // Check for IS [NOT] NULL
        if matches!(self.peek(), Token::Is) {
            self.advance();
            let negated = self.match_token(&Token::Not);
            self.expect(&Token::Null)?;
            return Ok(Expr::IsNull {
                expr: Box::new(left),
                negated,
            });
        }

        if let Some(op) = self.peek_binop() {
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

    fn peek_binop(&self) -> Option<BinOp> {
        match self.peek() {
            Token::Eq => Some(BinOp::Eq),
            Token::Ne => Some(BinOp::Ne),
            Token::Lt => Some(BinOp::Lt),
            Token::Le => Some(BinOp::Le),
            Token::Gt => Some(BinOp::Gt),
            Token::Ge => Some(BinOp::Ge),
            _ => None,
        }
    }

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while self.match_token(&Token::Concat) {
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
            if self.match_token(&Token::Plus) {
                let right = self.parse_multiplicative()?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Add,
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Minus) {
                let right = self.parse_multiplicative()?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Sub,
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
            if self.match_token(&Token::Star) {
                let right = self.parse_unary()?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Mul,
                    right: Box::new(right),
                };
            } else if self.match_token(&Token::Slash) {
                let right = self.parse_unary()?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Div,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.match_token(&Token::Minus) {
            let expr = self.parse_primary()?;
            Ok(Expr::UnaryOp {
                op: UnOp::Neg,
                expr: Box::new(expr),
            })
        } else if self.match_token(&Token::Plus) {
            self.parse_primary()
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::IntegerLiteral(v) => {
                self.advance();
                Ok(Expr::Literal(SqlLiteral::Int(v)))
            }
            Token::RealLiteral(v) => {
                self.advance();
                Ok(Expr::Literal(SqlLiteral::Real(v)))
            }
            Token::StringLiteral(v) => {
                self.advance();
                Ok(Expr::Literal(SqlLiteral::Text(v)))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Literal(SqlLiteral::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Literal(SqlLiteral::Bool(false)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(SqlLiteral::Null))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Identifier(name) => {
                self.advance();
                // Check if it's a function call
                if self.match_token(&Token::LParen) {
                    let mut args = Vec::new();
                    if !self.match_token(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if !self.match_token(&Token::Comma) {
                                break;
                            }
                        }
                        self.expect(&Token::RParen)?;
                    }
                    Ok(Expr::Function {
                        name: name.to_uppercase(),
                        args,
                    })
                } else if self.match_token(&Token::Dot) {
                    let col = self.expect_ident()?;
                    Ok(Expr::Column(ColumnRef::qualified(name, col)))
                } else {
                    Ok(Expr::Column(ColumnRef::new(name)))
                }
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::ParseError {
                    message: format!("unexpected token in expression: {}", tok),
                    line,
                    column: col,
                })
            }
        }
    }

    fn parse_literal(&mut self) -> Result<SqlLiteral> {
        match self.peek().clone() {
            Token::IntegerLiteral(v) => {
                self.advance();
                Ok(SqlLiteral::Int(v))
            }
            Token::RealLiteral(v) => {
                self.advance();
                Ok(SqlLiteral::Real(v))
            }
            Token::StringLiteral(v) => {
                self.advance();
                Ok(SqlLiteral::Text(v))
            }
            Token::True => {
                self.advance();
                Ok(SqlLiteral::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(SqlLiteral::Bool(false))
            }
            Token::Null => {
                self.advance();
                Ok(SqlLiteral::Null)
            }
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::IntegerLiteral(v) => {
                        self.advance();
                        Ok(SqlLiteral::Int(-v))
                    }
                    Token::RealLiteral(v) => {
                        self.advance();
                        Ok(SqlLiteral::Real(-v))
                    }
                    tok => {
                        let (line, col) = self.peek_pos();
                        Err(MiniDbError::ParseError {
                            message: format!("expected number after minus, found {}", tok),
                            line,
                            column: col,
                        })
                    }
                }
            }
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::ParseError {
                    message: format!("expected literal, found {}", tok),
                    line,
                    column: col,
                })
            }
        }
    }

    fn parse_i64(&mut self) -> Result<i64> {
        match self.advance().clone() {
            Token::IntegerLiteral(v) => Ok(v),
            tok => {
                let (line, col) = self.peek_pos();
                Err(MiniDbError::ParseError {
                    message: format!("expected integer, found {}", tok),
                    line,
                    column: col,
                })
            }
        }
    }
}

pub fn parse(tokens: Vec<TokenWithPos>) -> Result<Program> {
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_sql(sql: &str) -> Result<Program> {
        let tokens = tokenize(sql)?;
        parse(tokens)
    }

    #[test]
    fn test_parse_select_star() {
        let prog = parse_sql("SELECT * FROM users").unwrap();
        match prog {
            Program::Statement(Statement::Select { columns, .. }) => {
                assert_eq!(columns.len(), 1);
                assert!(matches!(columns[0], SelectColumn::Star));
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let prog = parse_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL)").unwrap();
        match prog {
            Program::Statement(Statement::CreateTable { name, columns }) => {
                assert_eq!(name, "t");
                assert_eq!(columns.len(), 2);
                assert!(columns[0].primary_key);
                assert!(columns[1].not_null);
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let prog = parse_sql("INSERT INTO t (id, name) VALUES (1, 'hello')").unwrap();
        match prog {
            Program::Statement(Statement::Insert {
                table,
                columns,
                values,
            }) => {
                assert_eq!(table, "t");
                assert_eq!(columns, Some(vec!["id".to_string(), "name".to_string()]));
                assert_eq!(values.len(), 1);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_parse_where_clause() {
        let prog = parse_sql("SELECT * FROM t WHERE a = 1 AND b > 2").unwrap();
        match prog {
            Program::Statement(Statement::Select { where_clause, .. }) => {
                assert!(where_clause.is_some());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_meta_command() {
        let prog = parse_sql(".schema").unwrap();
        assert!(matches!(prog, Program::Meta(MetaCommand::Schema)));
    }
}
