pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Keyword, Lexer, PosToken, Token};
use crate::types::{DataType, Value};
use ast::*;

pub fn parse_sql(input: &str) -> Result<Vec<Statement>> {
    let toks = Lexer::new(input).tokenize()?;
    let mut p = Parser::new(toks);
    let mut stmts = Vec::new();
    while !p.is_eof() {
        // tolerate leading semicolons
        while p.match_token(&Token::Semicolon) {}
        if p.is_eof() {
            break;
        }
        let s = p.parse_statement()?;
        // expect a `;` or EOF
        if !p.match_token(&Token::Semicolon) && !p.is_eof() {
            let pt = p.peek().clone();
            return Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: format!("expected ';' or end of input, got {:?}", pt.token),
            });
        }
        stmts.push(s);
    }
    Ok(stmts)
}

pub struct Parser {
    toks: Vec<PosToken>,
    pos: usize,
}

impl Parser {
    pub fn new(toks: Vec<PosToken>) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &PosToken {
        &self.toks[self.pos]
    }

    fn peek_at(&self, n: usize) -> &PosToken {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i]
    }

    fn bump(&mut self) -> PosToken {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek().token, Token::Eof)
    }

    fn match_token(&mut self, t: &Token) -> bool {
        if std::mem::discriminant(&self.peek().token) == std::mem::discriminant(t)
            && self.peek().token == *t
        {
            self.bump();
            true
        } else {
            false
        }
    }

    fn match_kw(&mut self, k: Keyword) -> bool {
        if matches!(&self.peek().token, Token::Keyword(kk) if *kk == k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, k: Keyword) -> Result<()> {
        if self.match_kw(k) {
            Ok(())
        } else {
            let pt = self.peek().clone();
            Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: format!("expected keyword {:?}, got {:?}", k, pt.token),
            })
        }
    }

    fn expect_token(&mut self, t: Token) -> Result<()> {
        if self.match_token(&t) {
            Ok(())
        } else {
            let pt = self.peek().clone();
            Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: format!("expected {:?}, got {:?}", t, pt.token),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        let pt = self.peek().clone();
        match pt.token {
            Token::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: format!("expected identifier, got {:?}", pt.token),
            }),
        }
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        let pt = self.peek().clone();
        match &pt.token {
            Token::Keyword(Keyword::Select) => self.parse_select().map(Statement::Select),
            Token::Keyword(Keyword::Insert) => self.parse_insert(),
            Token::Keyword(Keyword::Update) => self.parse_update(),
            Token::Keyword(Keyword::Delete) => self.parse_delete(),
            Token::Keyword(Keyword::Create) => self.parse_create(),
            Token::Keyword(Keyword::Drop) => self.parse_drop(),
            Token::Keyword(Keyword::Begin) => {
                self.bump();
                Ok(Statement::Begin)
            }
            Token::Keyword(Keyword::Commit) => {
                self.bump();
                Ok(Statement::Commit)
            }
            Token::Keyword(Keyword::Rollback) => {
                self.bump();
                Ok(Statement::Rollback)
            }
            _ => Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: format!("unexpected token at start of statement: {:?}", pt.token),
            }),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Create)?;
        if self.match_kw(Keyword::Table) {
            let name = self.expect_ident()?;
            self.expect_token(Token::LParen)?;
            let mut columns = Vec::new();
            let mut checks = Vec::new();
            loop {
                if self.match_kw(Keyword::Check) {
                    self.expect_token(Token::LParen)?;
                    let e = self.parse_expr()?;
                    self.expect_token(Token::RParen)?;
                    checks.push(e);
                } else {
                    let col = self.parse_column_def()?;
                    columns.push(col);
                }
                if self.match_token(&Token::Comma) {
                    continue;
                }
                break;
            }
            self.expect_token(Token::RParen)?;
            Ok(Statement::CreateTable {
                name,
                columns,
                checks,
            })
        } else if self.match_kw(Keyword::Index) {
            let name = self.expect_ident()?;
            self.expect_kw(Keyword::On)?;
            let table = self.expect_ident()?;
            self.expect_token(Token::LParen)?;
            let col = self.expect_ident()?;
            self.expect_token(Token::RParen)?;
            Ok(Statement::CreateIndex {
                name,
                table,
                column: col,
            })
        } else {
            let pt = self.peek().clone();
            Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: "expected TABLE or INDEX after CREATE".into(),
            })
        }
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let ty_pt = self.peek().clone();
        let ty_name = match ty_pt.token {
            Token::Ident(ref s) => s.clone(),
            Token::Keyword(_) => format!("{:?}", ty_pt.token),
            _ => {
                return Err(MiniDbError::Parse {
                    line: ty_pt.line,
                    col: ty_pt.col,
                    msg: "expected type name".into(),
                });
            }
        };
        self.bump();
        let data_type = DataType::parse(&ty_name).ok_or_else(|| MiniDbError::Parse {
            line: ty_pt.line,
            col: ty_pt.col,
            msg: format!("unknown type: {}", ty_name),
        })?;

        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;
        loop {
            if self.match_kw(Keyword::Primary) {
                self.expect_kw(Keyword::Key)?;
                primary_key = true;
                not_null = true;
            } else if self.match_kw(Keyword::NotKw) {
                self.expect_kw(Keyword::Null)?;
                not_null = true;
            } else if self.match_kw(Keyword::Unique) {
                unique = true;
            } else if self.match_kw(Keyword::Default) {
                let e = self.parse_primary_expr()?;
                default = Some(e);
            } else {
                break;
            }
        }
        Ok(ColumnDef {
            name,
            data_type,
            primary_key,
            not_null,
            unique,
            default,
        })
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Drop)?;
        if self.match_kw(Keyword::Table) {
            let name = self.expect_ident()?;
            Ok(Statement::DropTable { name })
        } else if self.match_kw(Keyword::Index) {
            let name = self.expect_ident()?;
            Ok(Statement::DropIndex { name })
        } else {
            let pt = self.peek().clone();
            Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: "expected TABLE or INDEX after DROP".into(),
            })
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Insert)?;
        self.expect_kw(Keyword::Into)?;
        let table = self.expect_ident()?;
        let mut columns: Option<Vec<String>> = None;
        if self.match_token(&Token::LParen) {
            let mut cs = Vec::new();
            loop {
                cs.push(self.expect_ident()?);
                if self.match_token(&Token::Comma) {
                    continue;
                }
                break;
            }
            self.expect_token(Token::RParen)?;
            columns = Some(cs);
        }
        self.expect_kw(Keyword::Values)?;
        let mut rows = Vec::new();
        loop {
            self.expect_token(Token::LParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                if self.match_token(&Token::Comma) {
                    continue;
                }
                break;
            }
            self.expect_token(Token::RParen)?;
            rows.push(row);
            if self.match_token(&Token::Comma) {
                continue;
            }
            break;
        }
        Ok(Statement::Insert {
            table,
            columns,
            rows,
        })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Update)?;
        let table = self.expect_ident()?;
        self.expect_kw(Keyword::Set)?;
        let mut sets = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect_token(Token::Eq)?;
            let val = self.parse_expr()?;
            sets.push((col, val));
            if self.match_token(&Token::Comma) {
                continue;
            }
            break;
        }
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            sets,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Delete)?;
        self.expect_kw(Keyword::From)?;
        let table = self.expect_ident()?;
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete {
            table,
            where_clause,
        })
    }

    fn parse_select(&mut self) -> Result<Select> {
        self.expect_kw(Keyword::Select)?;
        let mut projections = Vec::new();
        loop {
            if self.match_token(&Token::Star) {
                projections.push(Projection::Star);
            } else {
                // detect table.* form
                if matches!(&self.peek().token, Token::Ident(_))
                    && matches!(self.peek_at(1).token, Token::Dot)
                    && matches!(self.peek_at(2).token, Token::Star)
                {
                    let t = if let Token::Ident(s) = self.bump().token {
                        s
                    } else {
                        unreachable!()
                    };
                    self.bump(); // dot
                    self.bump(); // star
                    projections.push(Projection::QualifiedStar(t));
                } else {
                    let e = self.parse_expr()?;
                    let alias = if self.match_kw(Keyword::As) {
                        Some(self.expect_ident()?)
                    } else if matches!(&self.peek().token, Token::Ident(_)) {
                        // simple alias without AS
                        if let Token::Ident(s) = self.bump().token {
                            Some(s)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    projections.push(Projection::Expr(e, alias));
                }
            }
            if self.match_token(&Token::Comma) {
                continue;
            }
            break;
        }
        self.expect_kw(Keyword::From)?;
        let table = self.expect_ident()?;
        let table_alias = if matches!(&self.peek().token, Token::Ident(_)) {
            if let Token::Ident(s) = self.bump().token {
                Some(s)
            } else {
                None
            }
        } else if self.match_kw(Keyword::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        let mut join: Option<Join> = None;
        if self.match_kw(Keyword::Inner) || self.match_kw(Keyword::Join) {
            // already consumed JOIN if first; if INNER, expect JOIN next
            let last_was_inner = matches!(
                self.toks[self.pos - 1].token,
                Token::Keyword(Keyword::Inner)
            );
            if last_was_inner {
                self.expect_kw(Keyword::Join)?;
            }
            let rhs_table = self.expect_ident()?;
            let rhs_alias = if matches!(&self.peek().token, Token::Ident(_)) {
                if let Token::Ident(s) = self.bump().token {
                    Some(s)
                } else {
                    None
                }
            } else if self.match_kw(Keyword::As) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            self.expect_kw(Keyword::On)?;
            let cond = self.parse_expr()?;
            join = Some(Join {
                table: rhs_table,
                alias: rhs_alias,
                on: cond,
            });
        }
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by: Vec<(Expr, bool)> = Vec::new();
        if self.match_kw(Keyword::Order) {
            self.expect_kw(Keyword::By)?;
            loop {
                let e = self.parse_expr()?;
                let asc = if self.match_kw(Keyword::Desc) {
                    false
                } else {
                    self.match_kw(Keyword::Asc);
                    true
                };
                order_by.push((e, asc));
                if self.match_token(&Token::Comma) {
                    continue;
                }
                break;
            }
        }
        let mut limit: Option<i64> = None;
        let mut offset: Option<i64> = None;
        if self.match_kw(Keyword::Limit) {
            limit = Some(self.parse_int_literal()?);
            if self.match_kw(Keyword::Offset) {
                offset = Some(self.parse_int_literal()?);
            }
        }
        Ok(Select {
            projections,
            table,
            table_alias,
            join,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_int_literal(&mut self) -> Result<i64> {
        let pt = self.peek().clone();
        match pt.token {
            Token::Number(s) => {
                self.bump();
                s.parse::<i64>().map_err(|_| MiniDbError::Parse {
                    line: pt.line,
                    col: pt.col,
                    msg: format!("invalid integer literal: {}", s),
                })
            }
            _ => Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: "expected integer literal".into(),
            }),
        }
    }

    // --- expressions: precedence climbing ---
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.match_kw(Keyword::Or) {
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.match_kw(Keyword::And) {
            let right = self.parse_not()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.match_kw(Keyword::NotKw) {
            let inner = self.parse_not()?;
            return Ok(Expr::UnaryOp {
                op: UnOp::Not,
                expr: Box::new(inner),
            });
        }
        self.parse_compare()
    }

    fn parse_compare(&mut self) -> Result<Expr> {
        let left = self.parse_add()?;
        // IS NULL / IS NOT NULL
        if self.match_kw(Keyword::Is) {
            let negate = self.match_kw(Keyword::NotKw);
            self.expect_kw(Keyword::Null)?;
            let mut e = Expr::IsNull(Box::new(left));
            if negate {
                e = Expr::UnaryOp {
                    op: UnOp::Not,
                    expr: Box::new(e),
                };
            }
            return Ok(e);
        }
        let op = match self.peek().token {
            Token::Eq => Some(BinOp::Eq),
            Token::NotEq => Some(BinOp::NotEq),
            Token::Lt => Some(BinOp::Lt),
            Token::LtEq => Some(BinOp::LtEq),
            Token::Gt => Some(BinOp::Gt),
            Token::GtEq => Some(BinOp::GtEq),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let right = self.parse_add()?;
            return Ok(Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek().token {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::ConcatOp => BinOp::Concat,
                _ => break,
            };
            self.bump();
            let right = self.parse_mul()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().token {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.bump();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.match_token(&Token::Minus) {
            let inner = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnOp::Neg,
                expr: Box::new(inner),
            });
        }
        self.parse_primary_expr()
    }

    fn parse_primary_expr(&mut self) -> Result<Expr> {
        let pt = self.peek().clone();
        match pt.token {
            Token::Number(s) => {
                self.bump();
                if s.contains('.') {
                    Ok(Expr::Literal(Value::Real(s.parse::<f64>().map_err(
                        |_| MiniDbError::Parse {
                            line: pt.line,
                            col: pt.col,
                            msg: format!("bad real: {}", s),
                        },
                    )?)))
                } else {
                    Ok(Expr::Literal(Value::Int(s.parse::<i64>().map_err(
                        |_| MiniDbError::Parse {
                            line: pt.line,
                            col: pt.col,
                            msg: format!("bad int: {}", s),
                        },
                    )?)))
                }
            }
            Token::String(s) => {
                self.bump();
                Ok(Expr::Literal(Value::Text(s)))
            }
            Token::Keyword(Keyword::Null) => {
                self.bump();
                Ok(Expr::Literal(Value::Null))
            }
            Token::Keyword(Keyword::True) => {
                self.bump();
                Ok(Expr::Literal(Value::Bool(true)))
            }
            Token::Keyword(Keyword::False) => {
                self.bump();
                Ok(Expr::Literal(Value::Bool(false)))
            }
            Token::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect_token(Token::RParen)?;
                Ok(e)
            }
            Token::Ident(name) => {
                self.bump();
                if self.match_token(&Token::Dot) {
                    let col = self.expect_ident()?;
                    Ok(Expr::Column {
                        table: Some(name),
                        name: col,
                    })
                } else if self.match_token(&Token::LParen) {
                    let mut args = Vec::new();
                    if !matches!(self.peek().token, Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.match_token(&Token::Comma) {
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect_token(Token::RParen)?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    Ok(Expr::Column { table: None, name })
                }
            }
            _ => Err(MiniDbError::Parse {
                line: pt.line,
                col: pt.col,
                msg: format!("unexpected token in expression: {:?}", pt.token),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_select() {
        let s = parse_sql("SELECT a, b FROM t WHERE a = 1 ORDER BY a DESC LIMIT 10;").unwrap();
        assert_eq!(s.len(), 1);
        match &s[0] {
            Statement::Select(sel) => {
                assert_eq!(sel.projections.len(), 2);
                assert_eq!(sel.table, "t");
                assert_eq!(sel.limit, Some(10));
                assert_eq!(sel.order_by.len(), 1);
                assert!(!sel.order_by[0].1);
            }
            _ => panic!("expected select"),
        }
    }

    #[test]
    fn parse_create_table() {
        let s =
            parse_sql("CREATE TABLE t (a INT PRIMARY KEY, b TEXT NOT NULL, c REAL DEFAULT 0.0);")
                .unwrap();
        match &s[0] {
            Statement::CreateTable { columns, .. } => {
                assert_eq!(columns.len(), 3);
                assert!(columns[0].primary_key);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_insert_named() {
        let s = parse_sql("INSERT INTO t (a, b) VALUES (1, 'x');").unwrap();
        match &s[0] {
            Statement::Insert { columns, rows, .. } => {
                assert!(columns.is_some());
                assert_eq!(rows.len(), 1);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_join() {
        let s = parse_sql("SELECT a.x, b.y FROM a INNER JOIN b ON a.id = b.id;").unwrap();
        match &s[0] {
            Statement::Select(sel) => assert!(sel.join.is_some()),
            _ => panic!(),
        }
    }
}
