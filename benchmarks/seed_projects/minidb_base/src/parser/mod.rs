//! Hand-written recursive-descent SQL parser.
//!
//! Grammar (informal, Pratt-flavoured for expressions):
//!
//! ```text
//! statement    := select | insert | update | delete | create | drop
//!               | BEGIN | COMMIT | ROLLBACK
//! select       := SELECT proj_list FROM table_ref (join)* (WHERE expr)?
//!                 (ORDER BY order_list)? (LIMIT n)? (OFFSET n)?
//! insert       := INSERT INTO ident ('(' ident_list ')')? VALUES tuple_list
//! update       := UPDATE ident SET assign_list (WHERE expr)?
//! delete       := DELETE FROM ident (WHERE expr)?
//! create_table := CREATE TABLE ident '(' column_def_list ')'
//! create_index := CREATE [UNIQUE] INDEX ident ON ident '(' ident ')'
//! drop_table   := DROP TABLE ident
//! drop_index   := DROP INDEX ident
//! expr         := or_expr
//! ```

pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{tokenize, Token};
use crate::types::{ColumnType, Value};
use ast::*;

/// Parse a single SQL statement. Extra trailing tokens (aside from an
/// optional terminating semicolon and EOF) are an error.
pub fn parse(source: &str) -> Result<Statement> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens);
    let stmt = parser.parse_statement()?;
    parser.expect_terminator()?;
    Ok(stmt)
}

/// Parse potentially many `;`-separated statements.
pub fn parse_many(source: &str) -> Result<Vec<Statement>> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens);
    let mut out = Vec::new();
    loop {
        parser.eat_terminators();
        if parser.peek() == &Token::Eof {
            break;
        }
        out.push(parser.parse_statement()?);
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t, Token::Eof) {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, want: &Token) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, want: Token) -> Result<Token> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(&want) {
            Ok(self.advance())
        } else {
            Err(MiniDbError::Parse(format!(
                "expected {want:?}, got {:?}",
                self.peek()
            )))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            other => Err(MiniDbError::Parse(format!(
                "expected identifier, got {other:?}"
            ))),
        }
    }

    fn expect_terminator(&mut self) -> Result<()> {
        let _ = self.eat(&Token::Semicolon);
        match self.peek() {
            Token::Eof => Ok(()),
            other => Err(MiniDbError::Parse(format!(
                "unexpected trailing token {other:?}"
            ))),
        }
    }

    fn eat_terminators(&mut self) {
        while self.eat(&Token::Semicolon) {}
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek() {
            Token::Select => self.parse_select().map(Statement::Select),
            Token::Insert => self.parse_insert().map(Statement::Insert),
            Token::Update => self.parse_update().map(Statement::Update),
            Token::Delete => self.parse_delete().map(Statement::Delete),
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
            other => Err(MiniDbError::Parse(format!(
                "unrecognised start of statement: {other:?}"
            ))),
        }
    }

    fn parse_select(&mut self) -> Result<SelectStmt> {
        self.expect(Token::Select)?;
        let projections = self.parse_projection_list()?;
        self.expect(Token::From)?;
        let from = self.parse_table_ref()?;
        let mut joins = Vec::new();
        while matches!(self.peek(), Token::Inner | Token::Join) {
            let kind = if matches!(self.peek(), Token::Inner) {
                self.advance();
                self.expect(Token::Join)?;
                JoinKind::Inner
            } else {
                self.advance();
                JoinKind::Inner
            };
            let right = self.parse_table_ref()?;
            self.expect(Token::On)?;
            let on = self.parse_expression()?;
            joins.push(Join { kind, right, on });
        }
        let filter = if self.eat(&Token::Where) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let order_by = if matches!(self.peek(), Token::Order) {
            self.advance();
            self.expect(Token::By)?;
            self.parse_order_list()?
        } else {
            Vec::new()
        };
        let limit = if self.eat(&Token::Limit) {
            Some(self.parse_u64_literal()?)
        } else {
            None
        };
        let offset = if self.eat(&Token::Offset) {
            Some(self.parse_u64_literal()?)
        } else {
            None
        };
        Ok(SelectStmt {
            projections,
            from,
            joins,
            filter,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_u64_literal(&mut self) -> Result<u64> {
        match self.advance() {
            Token::IntLit(i) if i >= 0 => Ok(i as u64),
            other => Err(MiniDbError::Parse(format!(
                "expected non-negative integer, got {other:?}"
            ))),
        }
    }

    fn parse_projection_list(&mut self) -> Result<Vec<Projection>> {
        let mut out = Vec::new();
        loop {
            if matches!(self.peek(), Token::Star) {
                self.advance();
                out.push(Projection::AllColumns);
            } else {
                let expr = self.parse_expression()?;
                let alias = if self.eat(&Token::As) {
                    Some(self.expect_ident()?)
                } else if let Token::Ident(_) = self.peek() {
                    // Implicit alias: `SELECT a b FROM ...`
                    // We only accept when not followed by `FROM` to avoid
                    // swallowing it.
                    let save = self.pos;
                    let id = self.expect_ident().ok();
                    if id.is_some() && !matches!(self.peek(), Token::From | Token::Comma) {
                        self.pos = save;
                        None
                    } else {
                        id
                    }
                } else {
                    None
                };
                out.push(Projection::Expr { expr, alias });
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(out)
    }

    fn parse_table_ref(&mut self) -> Result<TableRef> {
        let name = self.expect_ident()?;
        let alias = if self.eat(&Token::As) {
            Some(self.expect_ident()?)
        } else if let Token::Ident(_) = self.peek() {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(TableRef { name, alias })
    }

    fn parse_order_list(&mut self) -> Result<Vec<OrderTerm>> {
        let mut out = Vec::new();
        loop {
            let expr = self.parse_expression()?;
            let ascending = if self.eat(&Token::Desc) {
                false
            } else {
                let _ = self.eat(&Token::Asc);
                true
            };
            out.push(OrderTerm { expr, ascending });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(out)
    }

    fn parse_insert(&mut self) -> Result<InsertStmt> {
        self.expect(Token::Insert)?;
        self.expect(Token::Into)?;
        let table = self.expect_ident()?;
        let columns = if self.eat(&Token::LParen) {
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RParen)?;
            Some(cols)
        } else {
            None
        };
        self.expect(Token::Values)?;
        let mut rows = Vec::new();
        loop {
            self.expect(Token::LParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expression()?);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RParen)?;
            rows.push(row);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(InsertStmt {
            table,
            columns,
            rows,
        })
    }

    fn parse_update(&mut self) -> Result<UpdateStmt> {
        self.expect(Token::Update)?;
        let table = self.expect_ident()?;
        self.expect(Token::Set)?;
        let mut assignments = Vec::new();
        loop {
            let column = self.expect_ident()?;
            self.expect(Token::Eq)?;
            let expr = self.parse_expression()?;
            assignments.push(Assignment { column, expr });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        let filter = if self.eat(&Token::Where) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        Ok(UpdateStmt {
            table,
            assignments,
            filter,
        })
    }

    fn parse_delete(&mut self) -> Result<DeleteStmt> {
        self.expect(Token::Delete)?;
        self.expect(Token::From)?;
        let table = self.expect_ident()?;
        let filter = if self.eat(&Token::Where) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        Ok(DeleteStmt { table, filter })
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect(Token::Create)?;
        // `CREATE UNIQUE INDEX ...` | `CREATE INDEX ...` | `CREATE TABLE ...`
        let unique = self.eat(&Token::Unique);
        match self.peek() {
            Token::Table => {
                if unique {
                    return Err(MiniDbError::Parse(
                        "UNIQUE keyword is not valid before TABLE".into(),
                    ));
                }
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::LParen)?;
                let mut columns = Vec::new();
                loop {
                    columns.push(self.parse_column_def()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                Ok(Statement::CreateTable(CreateTableStmt { name, columns }))
            }
            Token::Index => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::On)?;
                let table = self.expect_ident()?;
                self.expect(Token::LParen)?;
                let column = self.expect_ident()?;
                self.expect(Token::RParen)?;
                Ok(Statement::CreateIndex(CreateIndexStmt {
                    name,
                    table,
                    column,
                    unique,
                }))
            }
            other => Err(MiniDbError::Parse(format!(
                "expected TABLE or INDEX after CREATE, got {other:?}"
            ))),
        }
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let ty_ident = self.expect_ident()?;
        let ty = ColumnType::parse(&ty_ident)?;
        let mut not_null = false;
        let mut unique = false;
        let mut primary_key = false;
        let mut default = None;
        loop {
            match self.peek() {
                Token::Not => {
                    self.advance();
                    self.expect(Token::Null)?;
                    not_null = true;
                }
                Token::Unique => {
                    self.advance();
                    unique = true;
                }
                Token::Primary => {
                    self.advance();
                    self.expect(Token::Key)?;
                    primary_key = true;
                    not_null = true;
                }
                Token::Default => {
                    self.advance();
                    default = Some(self.parse_expression()?);
                }
                _ => break,
            }
        }
        Ok(ColumnDef {
            name,
            ty,
            not_null,
            unique,
            primary_key,
            default,
        })
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect(Token::Drop)?;
        match self.peek() {
            Token::Table => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(Statement::DropTable(DropTableStmt { name }))
            }
            Token::Index => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(Statement::DropIndex(DropIndexStmt { name }))
            }
            other => Err(MiniDbError::Parse(format!(
                "expected TABLE or INDEX after DROP, got {other:?}"
            ))),
        }
    }

    // -- Expression parser (precedence climbing) -----------------------

    fn parse_expression(&mut self) -> Result<Expression> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expression> {
        let mut left = self.parse_and()?;
        while self.eat(&Token::Or) {
            let right = self.parse_and()?;
            left = Expression::BinaryOp {
                op: BinaryOperator::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expression> {
        let mut left = self.parse_not()?;
        while self.eat(&Token::And) {
            let right = self.parse_not()?;
            left = Expression::BinaryOp {
                op: BinaryOperator::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expression> {
        if self.eat(&Token::Not) {
            let inner = self.parse_not()?;
            return Ok(Expression::UnaryOp {
                op: UnaryOperator::Not,
                expr: Box::new(inner),
            });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expression> {
        let left = self.parse_additive()?;
        if matches!(self.peek(), Token::Is) {
            self.advance();
            let negated = self.eat(&Token::Not);
            self.expect(Token::Null)?;
            return Ok(Expression::IsNull {
                expr: Box::new(left),
                negated,
            });
        }
        let op = match self.peek() {
            Token::Eq => BinaryOperator::Eq,
            Token::Neq => BinaryOperator::Neq,
            Token::Lt => BinaryOperator::Lt,
            Token::Le => BinaryOperator::Le,
            Token::Gt => BinaryOperator::Gt,
            Token::Ge => BinaryOperator::Ge,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_additive()?;
        Ok(Expression::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_additive(&mut self) -> Result<Expression> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOperator::Add,
                Token::Minus => BinaryOperator::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expression::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expression> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOperator::Mul,
                Token::Slash => BinaryOperator::Div,
                Token::Percent => BinaryOperator::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expression::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expression> {
        if self.eat(&Token::Minus) {
            let inner = self.parse_unary()?;
            return Ok(Expression::UnaryOp {
                op: UnaryOperator::Neg,
                expr: Box::new(inner),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expression> {
        match self.advance() {
            Token::IntLit(i) => Ok(Expression::Literal(Value::Int(i))),
            Token::RealLit(r) => Ok(Expression::Literal(Value::Real(r))),
            Token::StringLit(s) => Ok(Expression::Literal(Value::Text(s))),
            Token::True => Ok(Expression::Literal(Value::Bool(true))),
            Token::False => Ok(Expression::Literal(Value::Bool(false))),
            Token::Null => Ok(Expression::Literal(Value::Null)),
            Token::LParen => {
                let inner = self.parse_expression()?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            Token::Ident(first) => {
                if self.eat(&Token::Dot) {
                    let col = self.expect_ident()?;
                    Ok(Expression::ColumnRef {
                        table: Some(first),
                        column: col,
                    })
                } else {
                    Ok(Expression::ColumnRef {
                        table: None,
                        column: first,
                    })
                }
            }
            other => Err(MiniDbError::Parse(format!(
                "unexpected token in expression: {other:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_select() {
        let stmt = parse("SELECT id, name FROM users WHERE id = 3").unwrap();
        match stmt {
            Statement::Select(s) => {
                assert_eq!(s.projections.len(), 2);
                assert_eq!(s.from.name, "users");
                assert!(s.filter.is_some());
            }
            _ => panic!("wrong stmt"),
        }
    }

    #[test]
    fn parses_create_table_with_constraints() {
        let stmt = parse(
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL UNIQUE, age INT DEFAULT 0)",
        )
        .unwrap();
        match stmt {
            Statement::CreateTable(c) => {
                assert_eq!(c.columns.len(), 3);
                assert!(c.columns[0].primary_key);
                assert!(c.columns[1].unique);
                assert!(c.columns[2].default.is_some());
            }
            _ => panic!("wrong stmt"),
        }
    }

    #[test]
    fn parses_join() {
        let stmt = parse(
            "SELECT u.id, p.title FROM users u INNER JOIN posts p ON u.id = p.user_id WHERE u.id = 1",
        )
        .unwrap();
        if let Statement::Select(s) = stmt {
            assert_eq!(s.joins.len(), 1);
        } else {
            panic!();
        }
    }
}
