pub mod ast {
    use crate::types::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub enum Statement {
        CreateTable {
            name: String,
            schema: Schema,
        },
        DropTable {
            name: String,
        },
        CreateIndex {
            name: String,
            table: String,
            column: String,
        },
        DropIndex {
            name: String,
        },
        Insert {
            table: String,
            columns: Vec<String>,
            values: Vec<Expr>,
        },
        Select {
            projection: Vec<SelectItem>,
            from: FromClause,
            selection: Option<Expr>,
            order_by: Vec<OrderItem>,
            limit: Option<usize>,
            offset: Option<usize>,
        },
        Update {
            table: String,
            assignments: Vec<(String, Expr)>,
            selection: Option<Expr>,
        },
        Delete {
            table: String,
            selection: Option<Expr>,
        },
        Begin,
        Commit,
        Rollback,
        Meta(MetaCommand),
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub enum MetaCommand {
        Schema,
        Tables,
        Indexes,
        Exit,
        Format(String),
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub enum FromClause {
        Table {
            name: String,
            alias: Option<String>,
        },
        Join {
            left: String,
            right: String,
            on: Expr,
        },
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub enum SelectItem {
        Expr { expr: Expr, alias: Option<String> },
        Star,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct OrderItem {
        pub expr: Expr,
        pub desc: bool,
    }
}

use crate::{
    error::{MiniDbError, Result},
    lexer::{Lexer, Token, TokenKind},
    types::*,
};
use ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(sql: &str) -> Result<Self> {
        Ok(Self {
            tokens: Lexer::new(sql).tokenize()?,
            pos: 0,
        })
    }
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }
    fn cur(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn bump(&mut self) {
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        matches!(self.peek(), TokenKind::Kw(s) if s == kw)
            .then(|| self.bump())
            .is_some()
    }
    fn expect_kw(&mut self, kw: &str) -> Result<()> {
        if self.eat_kw(kw) {
            Ok(())
        } else {
            Err(self.err(format!("expected {kw}")))
        }
    }
    fn expect_ident(&mut self) -> Result<String> {
        match self.peek() {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            TokenKind::Kw(s) => {
                let s = s.to_lowercase();
                self.bump();
                Ok(s)
            }
            _ => Err(self.err("expected identifier".into())),
        }
    }
    fn err(&self, message: String) -> MiniDbError {
        MiniDbError::Parse {
            line: self.cur().line,
            column: self.cur().column,
            message,
        }
    }
    pub fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut out = Vec::new();
        while !matches!(self.peek(), TokenKind::Eof) {
            out.push(self.parse_statement()?);
            if matches!(self.peek(), TokenKind::Semicolon) {
                self.bump();
            }
        }
        Ok(out)
    }
    pub fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek() {
            TokenKind::Kw(k) if k == "CREATE" => {
                self.bump();
                self.parse_create()
            }
            TokenKind::Kw(k) if k == "DROP" => {
                self.bump();
                self.parse_drop()
            }
            TokenKind::Kw(k) if k == "INSERT" => {
                self.bump();
                self.parse_insert()
            }
            TokenKind::Kw(k) if k == "SELECT" => {
                self.bump();
                self.parse_select()
            }
            TokenKind::Kw(k) if k == "UPDATE" => {
                self.bump();
                self.parse_update()
            }
            TokenKind::Kw(k) if k == "DELETE" => {
                self.bump();
                self.parse_delete()
            }
            TokenKind::Kw(k) if k == "BEGIN" => {
                self.bump();
                Ok(Statement::Begin)
            }
            TokenKind::Kw(k) if k == "COMMIT" => {
                self.bump();
                Ok(Statement::Commit)
            }
            TokenKind::Kw(k) if k == "ROLLBACK" => {
                self.bump();
                Ok(Statement::Rollback)
            }
            TokenKind::Dot => self.parse_meta(),
            _ => Err(self.err("unexpected token".into())),
        }
    }
    fn parse_meta(&mut self) -> Result<Statement> {
        self.bump();
        let name = self.expect_ident()?.to_ascii_lowercase();
        Ok(Statement::Meta(match name.as_str() {
            "schema" => MetaCommand::Schema,
            "tables" => MetaCommand::Tables,
            "indexes" => MetaCommand::Indexes,
            "exit" => MetaCommand::Exit,
            "format" => MetaCommand::Format(self.expect_ident()?),
            _ => return Err(self.err("unknown meta-command".into())),
        }))
    }
    fn parse_create(&mut self) -> Result<Statement> {
        if self.eat_kw("TABLE") {
            let name = self.expect_ident()?;
            self.expect_tok(TokenKind::LParen)?;
            let mut cols = Vec::new();
            let mut constraints = Vec::new();
            loop {
                if self.eat_kw("CHECK") {
                    self.expect_tok(TokenKind::LParen)?;
                    let expr = self.parse_expr()?;
                    self.expect_tok(TokenKind::RParen)?;
                    constraints.push(TableConstraint { check: Some(expr) });
                } else {
                    let cname = self.expect_ident()?;
                    let ty = self.parse_type()?;
                    let mut col = ColumnDef {
                        name: cname,
                        ty,
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    };
                    loop {
                        if self.eat_kw("PRIMARY") {
                            self.expect_kw("KEY")?;
                            col.primary_key = true;
                        } else if self.eat_kw("NOT") {
                            self.expect_kw("NULL")?;
                            col.not_null = true;
                        } else if self.eat_kw("UNIQUE") {
                            col.unique = true;
                        } else if self.eat_kw("DEFAULT") {
                            col.default = Some(self.parse_expr()?);
                        } else {
                            break;
                        }
                    }
                    cols.push(col);
                }
                if matches!(self.peek(), TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            self.expect_tok(TokenKind::RParen)?;
            Ok(Statement::CreateTable {
                name,
                schema: Schema {
                    columns: cols,
                    constraints,
                },
            })
        } else if self.eat_kw("INDEX") {
            let name = self.expect_ident()?;
            self.expect_kw("ON")?;
            let table = self.expect_ident()?;
            self.expect_tok(TokenKind::LParen)?;
            let col = self.expect_ident()?;
            self.expect_tok(TokenKind::RParen)?;
            Ok(Statement::CreateIndex {
                name,
                table,
                column: col,
            })
        } else {
            Err(self.err("expected TABLE or INDEX".into()))
        }
    }
    fn parse_drop(&mut self) -> Result<Statement> {
        if self.eat_kw("TABLE") {
            Ok(Statement::DropTable {
                name: self.expect_ident()?,
            })
        } else if self.eat_kw("INDEX") {
            Ok(Statement::DropIndex {
                name: self.expect_ident()?,
            })
        } else {
            Err(self.err("expected TABLE or INDEX".into()))
        }
    }
    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_kw("INTO")?;
        let table = self.expect_ident()?;
        let mut cols = Vec::new();
        if matches!(self.peek(), TokenKind::LParen) {
            self.bump();
            loop {
                cols.push(self.expect_ident()?);
                if matches!(self.peek(), TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                self.expect_tok(TokenKind::RParen)?;
                break;
            }
        }
        self.expect_kw("VALUES")?;
        self.expect_tok(TokenKind::LParen)?;
        let mut vals = Vec::new();
        loop {
            vals.push(self.parse_expr()?);
            if matches!(self.peek(), TokenKind::Comma) {
                self.bump();
                continue;
            }
            self.expect_tok(TokenKind::RParen)?;
            break;
        }
        Ok(Statement::Insert {
            table,
            columns: cols,
            values: vals,
        })
    }
    fn parse_select(&mut self) -> Result<Statement> {
        let mut projection = Vec::new();
        if matches!(self.peek(), TokenKind::Star) {
            self.bump();
            projection.push(SelectItem::Star);
        } else {
            loop {
                let expr = self.parse_expr()?;
                let alias = if self.eat_kw("AS") {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                projection.push(SelectItem::Expr { expr, alias });
                if matches!(self.peek(), TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_kw("FROM")?;
        let from = if self.eat_kw("INNER") {
            self.expect_kw("JOIN")?;
            let left = self.expect_ident()?;
            self.expect_kw("ON")?;
            let on = self.parse_expr()?;
            FromClause::Join {
                left,
                right: String::new(),
                on,
            }
        } else {
            let name = self.expect_ident()?;
            FromClause::Table { name, alias: None }
        };
        let selection = if self.eat_kw("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.eat_kw("ORDER") {
            self.expect_kw("BY")?;
            loop {
                let expr = self.parse_expr()?;
                let desc = self.eat_kw("DESC");
                let _asc = self.eat_kw("ASC");
                order_by.push(OrderItem { expr, desc });
                if matches!(self.peek(), TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        let limit = if self.eat_kw("LIMIT") {
            Some(self.parse_usize()?)
        } else {
            None
        };
        let offset = if self.eat_kw("OFFSET") {
            Some(self.parse_usize()?)
        } else {
            None
        };
        Ok(Statement::Select {
            projection,
            from,
            selection,
            order_by,
            limit,
            offset,
        })
    }
    fn parse_update(&mut self) -> Result<Statement> {
        let table = self.expect_ident()?;
        self.expect_kw("SET")?;
        let mut assignments = Vec::new();
        loop {
            let c = self.expect_ident()?;
            self.expect_tok(TokenKind::Eq)?;
            let e = self.parse_expr()?;
            assignments.push((c, e));
            if matches!(self.peek(), TokenKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        let selection = if self.eat_kw("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments,
            selection,
        })
    }
    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_kw("FROM")?;
        let table = self.expect_ident()?;
        let selection = if self.eat_kw("WHERE") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, selection })
    }
    fn parse_type(&mut self) -> Result<DataType> {
        match self.peek() {
            TokenKind::Kw(k) if k == "INT" => {
                self.bump();
                Ok(DataType::Int)
            }
            TokenKind::Kw(k) if k == "BIGINT" => {
                self.bump();
                Ok(DataType::BigInt)
            }
            TokenKind::Kw(k) if k == "REAL" => {
                self.bump();
                Ok(DataType::Real)
            }
            TokenKind::Kw(k) if k == "TEXT" => {
                self.bump();
                Ok(DataType::Text)
            }
            TokenKind::Kw(k) if k == "BOOL" => {
                self.bump();
                Ok(DataType::Bool)
            }
            _ => Err(self.err("expected type".into())),
        }
    }
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }
    fn parse_or(&mut self) -> Result<Expr> {
        let mut e = self.parse_and()?;
        while self.eat_kw("OR") {
            e = Expr::Binary(Box::new(e), BinaryOp::Or, Box::new(self.parse_and()?));
        }
        Ok(e)
    }
    fn parse_and(&mut self) -> Result<Expr> {
        let mut e = self.parse_not()?;
        while self.eat_kw("AND") {
            e = Expr::Binary(Box::new(e), BinaryOp::And, Box::new(self.parse_not()?));
        }
        Ok(e)
    }
    fn parse_not(&mut self) -> Result<Expr> {
        if self.eat_kw("NOT") {
            Ok(Expr::Unary(UnaryOp::Not, Box::new(self.parse_not()?)))
        } else {
            self.parse_cmp()
        }
    }
    fn parse_cmp(&mut self) -> Result<Expr> {
        let e = self.parse_add()?;
        if self.eat_kw("IS") {
            let neg = self.eat_kw("NOT");
            self.expect_kw("NULL")?;
            return Ok(Expr::IsNull(Box::new(e), neg));
        }
        match self.peek() {
            TokenKind::Eq => {
                self.bump();
                Ok(Expr::Binary(
                    Box::new(e),
                    BinaryOp::Eq,
                    Box::new(self.parse_add()?),
                ))
            }
            TokenKind::Ne => {
                self.bump();
                Ok(Expr::Binary(
                    Box::new(e),
                    BinaryOp::Ne,
                    Box::new(self.parse_add()?),
                ))
            }
            TokenKind::Lt => {
                self.bump();
                Ok(Expr::Binary(
                    Box::new(e),
                    BinaryOp::Lt,
                    Box::new(self.parse_add()?),
                ))
            }
            TokenKind::Lte => {
                self.bump();
                Ok(Expr::Binary(
                    Box::new(e),
                    BinaryOp::Lte,
                    Box::new(self.parse_add()?),
                ))
            }
            TokenKind::Gt => {
                self.bump();
                Ok(Expr::Binary(
                    Box::new(e),
                    BinaryOp::Gt,
                    Box::new(self.parse_add()?),
                ))
            }
            TokenKind::Gte => {
                self.bump();
                Ok(Expr::Binary(
                    Box::new(e),
                    BinaryOp::Gte,
                    Box::new(self.parse_add()?),
                ))
            }
            _ => Ok(e),
        }
    }
    fn parse_add(&mut self) -> Result<Expr> {
        let mut e = self.parse_mul()?;
        loop {
            match self.peek() {
                TokenKind::Plus => {
                    self.bump();
                    e = Expr::Binary(Box::new(e), BinaryOp::Add, Box::new(self.parse_mul()?));
                }
                TokenKind::Minus => {
                    self.bump();
                    e = Expr::Binary(Box::new(e), BinaryOp::Sub, Box::new(self.parse_mul()?));
                }
                TokenKind::PipePipe => {
                    self.bump();
                    e = Expr::Binary(Box::new(e), BinaryOp::Concat, Box::new(self.parse_mul()?));
                }
                _ => break,
            }
        }
        Ok(e)
    }
    fn parse_mul(&mut self) -> Result<Expr> {
        let mut e = self.parse_unary()?;
        loop {
            match self.peek() {
                TokenKind::Star => {
                    self.bump();
                    e = Expr::Binary(Box::new(e), BinaryOp::Mul, Box::new(self.parse_unary()?));
                }
                TokenKind::Slash => {
                    self.bump();
                    e = Expr::Binary(Box::new(e), BinaryOp::Div, Box::new(self.parse_unary()?));
                }
                _ => break,
            }
        }
        Ok(e)
    }
    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.peek(), TokenKind::Minus) {
            self.bump();
            Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.parse_unary()?)))
        } else {
            self.parse_primary()
        }
    }
    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek() {
            TokenKind::Number(n) => {
                let n = n.clone();
                self.bump();
                if n.contains('.') {
                    Ok(Expr::Literal(Value::Real(n.parse().unwrap_or(0.0))))
                } else {
                    Ok(Expr::Literal(Value::Int(n.parse().unwrap_or(0))))
                }
            }
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(Expr::Literal(Value::Text(s)))
            }
            TokenKind::Kw(k) if k == "NULL" => {
                self.bump();
                Ok(Expr::Literal(Value::Null))
            }
            TokenKind::Kw(k) if k == "TRUE" || k == "FALSE" => {
                let v = k == "TRUE";
                self.bump();
                Ok(Expr::Literal(Value::Bool(v)))
            }
            TokenKind::Star => {
                self.bump();
                Ok(Expr::Star)
            }
            TokenKind::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect_tok(TokenKind::RParen)?;
                Ok(e)
            }
            TokenKind::Ident(_) | TokenKind::Kw(_) => {
                let name = self.expect_ident()?;
                if matches!(self.peek(), TokenKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek(), TokenKind::Comma) {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect_tok(TokenKind::RParen)?;
                    Ok(Expr::Func(name.to_ascii_uppercase(), args))
                } else if matches!(self.peek(), TokenKind::Dot) {
                    self.bump();
                    let second = self.expect_ident()?;
                    Ok(Expr::Ident(vec![name, second]))
                } else {
                    Ok(Expr::Ident(vec![name]))
                }
            }
            _ => Err(self.err("expected expression".into())),
        }
    }
    fn parse_usize(&mut self) -> Result<usize> {
        if let TokenKind::Number(n) = self.peek() {
            let v = n.parse().map_err(|_| self.err("bad number".into()))?;
            self.bump();
            Ok(v)
        } else {
            Err(self.err("expected number".into()))
        }
    }
    fn expect_tok(&mut self, kind: TokenKind) -> Result<()> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(&kind) {
            self.bump();
            Ok(())
        } else {
            Err(self.err(format!("expected {:?}", kind)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_select() {
        let mut p = Parser::new("select * from t;").unwrap();
        assert!(matches!(
            p.parse_statement().unwrap(),
            Statement::Select { .. }
        ));
    }
}
