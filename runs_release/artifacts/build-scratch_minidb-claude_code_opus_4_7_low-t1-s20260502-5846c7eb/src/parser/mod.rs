pub mod ast;

use crate::error::{MiniDbError, Result};
use crate::lexer::{Keyword, Lexer, SpannedToken, Token};
use crate::types::{DataType, Value};
use ast::*;

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn parse_sql(src: &str) -> Result<Vec<Statement>> {
        let tokens = Lexer::new(src).tokenize()?;
        let mut p = Parser { tokens, pos: 0 };
        let mut stmts = Vec::new();
        loop {
            // skip leading semicolons
            while matches!(p.peek().token, Token::Semicolon) {
                p.bump();
            }
            if matches!(p.peek().token, Token::Eof) {
                break;
            }
            let s = p.parse_statement()?;
            stmts.push(s);
            match &p.peek().token {
                Token::Semicolon => {
                    p.bump();
                }
                Token::Eof => break,
                _ => {
                    let t = p.peek().clone();
                    return Err(MiniDbError::Parse {
                        line: t.line,
                        col: t.col,
                        msg: format!("expected ';' or EOF, got {:?}", t.token),
                    });
                }
            }
        }
        Ok(stmts)
    }

    fn peek(&self) -> &SpannedToken {
        &self.tokens[self.pos]
    }
    fn peek_at(&self, off: usize) -> &SpannedToken {
        let i = (self.pos + off).min(self.tokens.len() - 1);
        &self.tokens[i]
    }
    fn bump(&mut self) -> SpannedToken {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.token, Token::Eof) {
            self.pos += 1;
        }
        t
    }
    fn err<T>(&self, t: &SpannedToken, msg: String) -> Result<T> {
        Err(MiniDbError::Parse {
            line: t.line,
            col: t.col,
            msg,
        })
    }
    fn expect_kw(&mut self, kw: Keyword) -> Result<()> {
        let t = self.bump();
        if matches!(&t.token, Token::Keyword(k) if *k == kw) {
            Ok(())
        } else {
            self.err(&t, format!("expected keyword {:?}", kw))
        }
    }
    fn match_kw(&mut self, kw: Keyword) -> bool {
        if matches!(&self.peek().token, Token::Keyword(k) if *k == kw) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_ident(&mut self) -> Result<String> {
        let t = self.bump();
        match &t.token {
            Token::Ident(s) => Ok(s.clone()),
            _ => self.err(&t, "expected identifier".to_string()),
        }
    }
    fn expect_token(&mut self, want: &Token) -> Result<()> {
        let t = self.bump();
        if std::mem::discriminant(&t.token) == std::mem::discriminant(want) {
            Ok(())
        } else {
            self.err(&t, format!("expected {:?}", want))
        }
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        let t = self.peek().clone();
        match &t.token {
            Token::Keyword(Keyword::Create) => self.parse_create(),
            Token::Keyword(Keyword::Drop) => self.parse_drop(),
            Token::Keyword(Keyword::Insert) => self.parse_insert(),
            Token::Keyword(Keyword::Select) => {
                Ok(Statement::Select(Box::new(self.parse_select()?)))
            }
            Token::Keyword(Keyword::Update) => self.parse_update(),
            Token::Keyword(Keyword::Delete) => self.parse_delete(),
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
            _ => self.err(&t, format!("unexpected token {:?}", t.token)),
        }
    }

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Create)?;
        if self.match_kw(Keyword::Table) {
            let if_not_exists = if self.match_kw(Keyword::If) {
                self.expect_kw(Keyword::Not)?;
                self.expect_kw(Keyword::Exists)?;
                true
            } else {
                false
            };
            let name = self.expect_ident()?;
            self.expect_token(&Token::LParen)?;
            let mut columns = Vec::new();
            let mut checks = Vec::new();
            loop {
                if matches!(&self.peek().token, Token::Keyword(Keyword::Check)) {
                    self.bump();
                    self.expect_token(&Token::LParen)?;
                    let e = self.parse_expr()?;
                    self.expect_token(&Token::RParen)?;
                    checks.push(e);
                } else {
                    columns.push(self.parse_column_def()?);
                }
                match &self.peek().token {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let t = self.peek().clone();
                        return self.err(&t, "expected ',' or ')'".into());
                    }
                }
            }
            Ok(Statement::CreateTable(CreateTable {
                name,
                columns,
                checks,
                if_not_exists,
            }))
        } else if self.match_kw(Keyword::Index) {
            let name = self.expect_ident()?;
            self.expect_kw(Keyword::On)?;
            let table = self.expect_ident()?;
            self.expect_token(&Token::LParen)?;
            let column = self.expect_ident()?;
            self.expect_token(&Token::RParen)?;
            Ok(Statement::CreateIndex {
                name,
                table,
                column,
            })
        } else {
            let t = self.peek().clone();
            self.err(&t, "expected TABLE or INDEX".into())
        }
    }

    fn parse_column_def(&mut self) -> Result<ColumnDef> {
        let name = self.expect_ident()?;
        let t = self.bump();
        let type_name = match &t.token {
            Token::Ident(s) => s.clone(),
            Token::Keyword(_) => {
                // Some keywords could double as types but we keep it simple
                return self.err(&t, "expected type name".into());
            }
            _ => return self.err(&t, "expected type name".into()),
        };
        let data_type = DataType::parse(&type_name).map_err(|e| MiniDbError::Parse {
            line: t.line,
            col: t.col,
            msg: format!("{}", e),
        })?;
        let mut primary_key = false;
        let mut not_null = false;
        let mut unique = false;
        let mut default = None;
        loop {
            match &self.peek().token {
                Token::Keyword(Keyword::Primary) => {
                    self.bump();
                    self.expect_kw(Keyword::Key)?;
                    primary_key = true;
                    not_null = true;
                    unique = true;
                }
                Token::Keyword(Keyword::Not) => {
                    self.bump();
                    self.expect_kw(Keyword::Null)?;
                    not_null = true;
                }
                Token::Keyword(Keyword::Unique) => {
                    self.bump();
                    unique = true;
                }
                Token::Keyword(Keyword::Default) => {
                    self.bump();
                    default = Some(self.parse_expr()?);
                }
                _ => break,
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
            let if_exists = if self.match_kw(Keyword::If) {
                self.expect_kw(Keyword::Exists)?;
                true
            } else {
                false
            };
            let name = self.expect_ident()?;
            Ok(Statement::DropTable { name, if_exists })
        } else if self.match_kw(Keyword::Index) {
            let if_exists = if self.match_kw(Keyword::If) {
                self.expect_kw(Keyword::Exists)?;
                true
            } else {
                false
            };
            let name = self.expect_ident()?;
            Ok(Statement::DropIndex { name, if_exists })
        } else {
            let t = self.peek().clone();
            self.err(&t, "expected TABLE or INDEX".into())
        }
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Insert)?;
        self.expect_kw(Keyword::Into)?;
        let table = self.expect_ident()?;
        let columns = if matches!(self.peek().token, Token::LParen) {
            self.bump();
            let mut cols = Vec::new();
            loop {
                cols.push(self.expect_ident()?);
                match &self.peek().token {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let t = self.peek().clone();
                        return self.err(&t, "expected ',' or ')'".into());
                    }
                }
            }
            Some(cols)
        } else {
            None
        };
        self.expect_kw(Keyword::Values)?;
        let mut rows = Vec::new();
        loop {
            self.expect_token(&Token::LParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                match &self.peek().token {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let t = self.peek().clone();
                        return self.err(&t, "expected ',' or ')'".into());
                    }
                }
            }
            rows.push(row);
            if matches!(self.peek().token, Token::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(Statement::Insert(Insert {
            table,
            columns,
            rows,
        }))
    }

    fn parse_select(&mut self) -> Result<Select> {
        self.expect_kw(Keyword::Select)?;
        let mut projections = Vec::new();
        loop {
            if matches!(self.peek().token, Token::Star) {
                self.bump();
                projections.push(Projection::Star);
            } else if let Token::Ident(name) = &self.peek().token.clone() {
                if matches!(self.peek_at(1).token, Token::Dot)
                    && matches!(self.peek_at(2).token, Token::Star)
                {
                    self.bump();
                    self.bump();
                    self.bump();
                    projections.push(Projection::QualifiedStar(name.clone()));
                } else {
                    let e = self.parse_expr()?;
                    let alias = self.parse_optional_alias()?;
                    projections.push(Projection::Expr(e, alias));
                }
            } else {
                let e = self.parse_expr()?;
                let alias = self.parse_optional_alias()?;
                projections.push(Projection::Expr(e, alias));
            }
            if matches!(self.peek().token, Token::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect_kw(Keyword::From)?;
        let from = self.parse_table_ref()?;
        let join = if self.match_kw(Keyword::Inner) {
            self.expect_kw(Keyword::Join)?;
            let right = self.parse_table_ref()?;
            self.expect_kw(Keyword::On)?;
            let on = self.parse_expr()?;
            Some(Join { right, on })
        } else if matches!(self.peek().token, Token::Keyword(Keyword::Join)) {
            self.bump();
            let right = self.parse_table_ref()?;
            self.expect_kw(Keyword::On)?;
            let on = self.parse_expr()?;
            Some(Join { right, on })
        } else {
            None
        };
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.match_kw(Keyword::Order) {
            self.expect_kw(Keyword::By)?;
            loop {
                let e = self.parse_expr()?;
                let desc = if self.match_kw(Keyword::Desc) {
                    true
                } else {
                    self.match_kw(Keyword::Asc);
                    false
                };
                order_by.push(OrderBy { expr: e, desc });
                if matches!(self.peek().token, Token::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        let limit = if self.match_kw(Keyword::Limit) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let offset = if self.match_kw(Keyword::Offset) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Select {
            projections,
            from,
            join,
            where_clause,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_table_ref(&mut self) -> Result<TableRef> {
        let name = self.expect_ident()?;
        let alias = self.parse_optional_alias()?;
        Ok(TableRef { name, alias })
    }

    fn parse_optional_alias(&mut self) -> Result<Option<String>> {
        if self.match_kw(Keyword::As) {
            let n = self.expect_ident()?;
            return Ok(Some(n));
        }
        if let Token::Ident(_) = &self.peek().token {
            // Disambiguate: only treat as alias if next is comma, FROM, etc. Be conservative.
            // We look at the position after to ensure it's not interpreted as something else.
            // Simpler: never auto-alias without AS.
        }
        Ok(None)
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Update)?;
        let table = self.expect_ident()?;
        self.expect_kw(Keyword::Set)?;
        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            let t = self.bump();
            if !matches!(&t.token, Token::Op(o) if o == "=") {
                return self.err(&t, "expected '='".into());
            }
            let e = self.parse_expr()?;
            assignments.push((col, e));
            if matches!(self.peek().token, Token::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        let where_clause = if self.match_kw(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update(Update {
            table,
            assignments,
            where_clause,
        }))
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
        Ok(Statement::Delete(Delete {
            table,
            where_clause,
        }))
    }

    /// Pratt-ish expression parser. Precedence: OR < AND < NOT < comparison < + - < * / < unary < primary
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.match_kw(Keyword::Or) {
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                op: BinaryOp::Or,
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
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.match_kw(Keyword::Not) {
            let e = self.parse_not()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(e),
            });
        }
        self.parse_compare()
    }

    fn parse_compare(&mut self) -> Result<Expr> {
        let left = self.parse_add()?;
        // IS NULL / IS NOT NULL
        if matches!(self.peek().token, Token::Keyword(Keyword::Is)) {
            self.bump();
            let neg = self.match_kw(Keyword::Not);
            self.expect_kw(Keyword::Null)?;
            return Ok(if neg {
                Expr::IsNotNull(Box::new(left))
            } else {
                Expr::IsNull(Box::new(left))
            });
        }
        let op = match &self.peek().token {
            Token::Op(o) if o == "=" => Some(BinaryOp::Eq),
            Token::Op(o) if o == "<>" => Some(BinaryOp::Neq),
            Token::Op(o) if o == "<" => Some(BinaryOp::Lt),
            Token::Op(o) if o == "<=" => Some(BinaryOp::Lte),
            Token::Op(o) if o == ">" => Some(BinaryOp::Gt),
            Token::Op(o) if o == ">=" => Some(BinaryOp::Gte),
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
            let op = match &self.peek().token {
                Token::Op(o) if o == "+" => Some(BinaryOp::Add),
                Token::Op(o) if o == "-" => Some(BinaryOp::Sub),
                Token::Op(o) if o == "||" => Some(BinaryOp::Concat),
                _ => None,
            };
            match op {
                Some(op) => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::BinaryOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                None => break,
            }
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match &self.peek().token {
                Token::Star => Some(BinaryOp::Mul),
                Token::Op(o) if o == "/" => Some(BinaryOp::Div),
                Token::Op(o) if o == "%" => Some(BinaryOp::Mod),
                _ => None,
            };
            match op {
                Some(op) => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::BinaryOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                None => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if let Token::Op(o) = &self.peek().token {
            if o == "-" {
                self.bump();
                let e = self.parse_unary()?;
                return Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(e),
                });
            } else if o == "+" {
                self.bump();
                return self.parse_unary();
            }
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let t = self.bump();
        match &t.token {
            Token::Number(v, is_int) => Ok(Expr::Literal(if *is_int {
                Value::Int(*v as i64)
            } else {
                Value::Real(*v)
            })),
            Token::String(s) => Ok(Expr::Literal(Value::Text(s.clone()))),
            Token::Keyword(Keyword::Null) => Ok(Expr::Literal(Value::Null)),
            Token::Keyword(Keyword::True) => Ok(Expr::Literal(Value::Bool(true))),
            Token::Keyword(Keyword::False) => Ok(Expr::Literal(Value::Bool(false))),
            Token::LParen => {
                let e = self.parse_expr()?;
                self.expect_token(&Token::RParen)?;
                Ok(e)
            }
            Token::Ident(name) => {
                // function call?
                if matches!(self.peek().token, Token::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek().token, Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek().token, Token::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_token(&Token::RParen)?;
                    return Ok(Expr::Function {
                        name: name.clone(),
                        args,
                    });
                }
                if matches!(self.peek().token, Token::Dot) {
                    self.bump();
                    let col = self.expect_ident()?;
                    return Ok(Expr::Column {
                        table: Some(name.clone()),
                        name: col,
                    });
                }
                Ok(Expr::Column {
                    table: None,
                    name: name.clone(),
                })
            }
            _ => self.err(&t, format!("unexpected token in expr: {:?}", t.token)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_create_table() {
        let s = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);";
        let stmts = Parser::parse_sql(s).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::CreateTable(c) => {
                assert_eq!(c.name, "t");
                assert_eq!(c.columns.len(), 2);
                assert!(c.columns[0].primary_key);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_select_join() {
        let s = "SELECT a.x, b.y FROM a INNER JOIN b ON a.id = b.id WHERE a.x > 1 ORDER BY a.x DESC LIMIT 10;";
        let stmts = Parser::parse_sql(s).unwrap();
        assert_eq!(stmts.len(), 1);
        if let Statement::Select(sel) = &stmts[0] {
            assert!(sel.join.is_some());
            assert!(sel.where_clause.is_some());
            assert_eq!(sel.order_by.len(), 1);
            assert!(sel.order_by[0].desc);
            assert!(sel.limit.is_some());
        } else {
            panic!();
        }
    }

    #[test]
    fn parse_insert_multi() {
        let s = "INSERT INTO t (a,b) VALUES (1,'x'),(2,'y');";
        let stmts = Parser::parse_sql(s).unwrap();
        if let Statement::Insert(i) = &stmts[0] {
            assert_eq!(i.rows.len(), 2);
            assert_eq!(i.columns.as_ref().unwrap().len(), 2);
        } else {
            panic!();
        }
    }

    #[test]
    fn parse_expr_precedence() {
        let stmts = Parser::parse_sql("SELECT 1+2*3 FROM t;").unwrap();
        if let Statement::Select(s) = &stmts[0] {
            if let Projection::Expr(Expr::BinaryOp { op, .. }, _) = &s.projections[0] {
                assert_eq!(*op, BinaryOp::Add);
            } else {
                panic!();
            }
        }
    }
}
