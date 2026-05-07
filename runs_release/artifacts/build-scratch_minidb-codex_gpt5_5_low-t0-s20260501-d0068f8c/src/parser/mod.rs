pub mod ast;

use crate::{
    error::{MiniDbError, Result},
    lexer::{Token, TokenKind, lex},
    types::{Column, ColumnType, Value},
};
use ast::*;

pub fn parse_sql(sql: &str) -> Result<Vec<Statement>> {
    let tokens = lex(sql)?;
    let mut p = Parser { tokens, pos: 0 };
    let mut out = Vec::new();
    while !p.eof() {
        if p.sym(";") {
            continue;
        }
        out.push(p.statement()?);
        let _ = p.sym(";");
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }
    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }
    fn err<T>(&self, msg: &str) -> Result<T> {
        Err(MiniDbError::parse(
            msg,
            self.peek().line,
            self.peek().column,
        ))
    }
    fn kw(&mut self, k: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Keyword(s) if s == k) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, k: &str) -> Result<()> {
        if self.kw(k) {
            Ok(())
        } else {
            self.err(&format!("expected {k}"))
        }
    }
    fn sym(&mut self, s: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Symbol(x) if x == s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_sym(&mut self, s: &str) -> Result<()> {
        if self.sym(s) {
            Ok(())
        } else {
            self.err(&format!("expected {s}"))
        }
    }
    fn ident(&mut self) -> Result<String> {
        match self.bump().kind {
            TokenKind::Ident(s) => Ok(s),
            TokenKind::Keyword(s) => Ok(s.to_ascii_lowercase()),
            _ => self.err("expected identifier"),
        }
    }
    fn statement(&mut self) -> Result<Statement> {
        if self.kw("CREATE") {
            if self.kw("TABLE") {
                self.create_table()
            } else if self.kw("INDEX") {
                self.create_index()
            } else {
                self.err("expected TABLE or INDEX")
            }
        } else if self.kw("DROP") {
            if self.kw("TABLE") {
                Ok(Statement::DropTable {
                    name: self.ident()?,
                })
            } else if self.kw("INDEX") {
                Ok(Statement::DropIndex {
                    name: self.ident()?,
                })
            } else {
                self.err("expected TABLE or INDEX")
            }
        } else if self.kw("INSERT") {
            self.insert()
        } else if self.kw("SELECT") {
            self.select().map(Statement::Select)
        } else if self.kw("UPDATE") {
            self.update()
        } else if self.kw("DELETE") {
            self.delete()
        } else if self.kw("BEGIN") {
            Ok(Statement::Begin)
        } else if self.kw("COMMIT") {
            Ok(Statement::Commit)
        } else if self.kw("ROLLBACK") {
            Ok(Statement::Rollback)
        } else {
            self.err("unknown statement")
        }
    }
    fn create_table(&mut self) -> Result<Statement> {
        let name = self.ident()?;
        self.expect_sym("(")?;
        let mut columns = Vec::new();
        let mut checks = Vec::new();
        loop {
            if self.kw("CHECK") {
                self.expect_sym("(")?;
                checks.push(self.expr()?);
                self.expect_sym(")")?;
            } else {
                let cname = self.ident()?;
                let ty = if self.kw("INT") || self.kw("BIGINT") {
                    ColumnType::Int
                } else if self.kw("REAL") {
                    ColumnType::Real
                } else if self.kw("TEXT") {
                    ColumnType::Text
                } else if self.kw("BOOL") {
                    ColumnType::Bool
                } else {
                    return self.err("expected column type");
                };
                let mut col = Column {
                    name: cname,
                    ty,
                    primary_key: false,
                    not_null: false,
                    unique: false,
                    default: None,
                };
                loop {
                    if self.kw("PRIMARY") {
                        self.expect_kw("KEY")?;
                        col.primary_key = true;
                        col.unique = true;
                        col.not_null = true;
                    } else if self.kw("NOT") {
                        self.expect_kw("NULL")?;
                        col.not_null = true;
                    } else if self.kw("UNIQUE") {
                        col.unique = true;
                    } else if self.kw("DEFAULT") {
                        col.default = Some(self.literal()?);
                    } else {
                        break;
                    }
                }
                columns.push(col);
            }
            if !self.sym(",") {
                break;
            }
        }
        self.expect_sym(")")?;
        Ok(Statement::CreateTable {
            name,
            columns,
            checks,
        })
    }
    fn create_index(&mut self) -> Result<Statement> {
        let name = self.ident()?;
        self.expect_kw("ON")?;
        let table = self.ident()?;
        self.expect_sym("(")?;
        let column = self.ident()?;
        self.expect_sym(")")?;
        Ok(Statement::CreateIndex {
            name,
            table,
            column,
        })
    }
    fn insert(&mut self) -> Result<Statement> {
        self.expect_kw("INTO")?;
        let table = self.ident()?;
        let columns = if self.sym("(") {
            let xs = self.ident_list()?;
            self.expect_sym(")")?;
            xs
        } else {
            Vec::new()
        };
        self.expect_kw("VALUES")?;
        self.expect_sym("(")?;
        let values = self.expr_list()?;
        self.expect_sym(")")?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }
    fn select(&mut self) -> Result<Select> {
        let projection = if self.sym("*") {
            vec![SelectItem::Star]
        } else {
            self.select_list()?
        };
        self.expect_kw("FROM")?;
        let table = self.ident()?;
        let join = if self.kw("INNER") {
            self.expect_kw("JOIN")?;
            let right = self.ident()?;
            self.expect_kw("ON")?;
            Some(Join {
                right,
                on: self.expr()?,
            })
        } else {
            None
        };
        let selection = if self.kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let order = if self.kw("ORDER") {
            self.expect_kw("BY")?;
            let expr = self.expr()?;
            let asc = !self.kw("DESC");
            let _ = self.kw("ASC");
            Some(Order { expr, asc })
        } else {
            None
        };
        let limit = if self.kw("LIMIT") {
            Some(self.number_usize()?)
        } else {
            None
        };
        let offset = if self.kw("OFFSET") {
            Some(self.number_usize()?)
        } else {
            None
        };
        Ok(Select {
            projection,
            from: table,
            join,
            selection,
            order,
            limit,
            offset,
        })
    }
    fn update(&mut self) -> Result<Statement> {
        let table = self.ident()?;
        self.expect_kw("SET")?;
        let mut assigns = Vec::new();
        loop {
            let c = self.ident()?;
            self.expect_sym("=")?;
            assigns.push((c, self.expr()?));
            if !self.sym(",") {
                break;
            }
        }
        let selection = if self.kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments: assigns,
            selection,
        })
    }
    fn delete(&mut self) -> Result<Statement> {
        self.expect_kw("FROM")?;
        let table = self.ident()?;
        let selection = if self.kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, selection })
    }
    fn select_list(&mut self) -> Result<Vec<SelectItem>> {
        let mut out = Vec::new();
        loop {
            let expr = self.expr()?;
            let alias = if self.kw("AS") {
                Some(self.ident()?)
            } else {
                None
            };
            out.push(SelectItem::Expr { expr, alias });
            if !self.sym(",") {
                break;
            }
        }
        Ok(out)
    }
    fn ident_list(&mut self) -> Result<Vec<String>> {
        let mut xs = Vec::new();
        loop {
            xs.push(self.ident()?);
            if !self.sym(",") {
                break;
            }
        }
        Ok(xs)
    }
    fn expr_list(&mut self) -> Result<Vec<Expr>> {
        let mut xs = Vec::new();
        loop {
            xs.push(self.expr()?);
            if !self.sym(",") {
                break;
            }
        }
        Ok(xs)
    }
    fn literal(&mut self) -> Result<Value> {
        match self.primary()? {
            Expr::Literal(v) => Ok(v),
            _ => self.err("expected literal"),
        }
    }
    fn number_usize(&mut self) -> Result<usize> {
        match self.bump().kind {
            TokenKind::Number(s) => s.parse().map_err(|_| {
                MiniDbError::parse("expected integer", self.peek().line, self.peek().column)
            }),
            _ => self.err("expected integer"),
        }
    }
    fn expr(&mut self) -> Result<Expr> {
        self.or()
    }
    fn or(&mut self) -> Result<Expr> {
        let mut e = self.and()?;
        while self.kw("OR") {
            e = Expr::Binary {
                left: Box::new(e),
                op: BinaryOp::Or,
                right: Box::new(self.and()?),
            };
        }
        Ok(e)
    }
    fn and(&mut self) -> Result<Expr> {
        let mut e = self.not()?;
        while self.kw("AND") {
            e = Expr::Binary {
                left: Box::new(e),
                op: BinaryOp::And,
                right: Box::new(self.not()?),
            };
        }
        Ok(e)
    }
    fn not(&mut self) -> Result<Expr> {
        if self.kw("NOT") {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.not()?),
            })
        } else {
            self.cmp()
        }
    }
    fn cmp(&mut self) -> Result<Expr> {
        let mut e = self.add()?;
        if self.kw("IS") {
            let not = self.kw("NOT");
            self.expect_kw("NULL")?;
            return Ok(Expr::IsNull {
                expr: Box::new(e),
                not,
            });
        }
        for (s, op) in [
            ("=", BinaryOp::Eq),
            ("<>", BinaryOp::Ne),
            ("<", BinaryOp::Lt),
            ("<=", BinaryOp::Le),
            (">", BinaryOp::Gt),
            (">=", BinaryOp::Ge),
        ] {
            if self.sym(s) {
                e = Expr::Binary {
                    left: Box::new(e),
                    op,
                    right: Box::new(self.add()?),
                };
                break;
            }
        }
        Ok(e)
    }
    fn add(&mut self) -> Result<Expr> {
        let mut e = self.mul()?;
        loop {
            let op = if self.sym("+") {
                Some(BinaryOp::Add)
            } else if self.sym("-") {
                Some(BinaryOp::Sub)
            } else if self.sym("||") {
                Some(BinaryOp::Concat)
            } else {
                None
            };
            if let Some(op) = op {
                e = Expr::Binary {
                    left: Box::new(e),
                    op,
                    right: Box::new(self.mul()?),
                };
            } else {
                break;
            }
        }
        Ok(e)
    }
    fn mul(&mut self) -> Result<Expr> {
        let mut e = self.unary()?;
        loop {
            let op = if self.sym("*") {
                Some(BinaryOp::Mul)
            } else if self.sym("/") {
                Some(BinaryOp::Div)
            } else {
                None
            };
            if let Some(op) = op {
                e = Expr::Binary {
                    left: Box::new(e),
                    op,
                    right: Box::new(self.unary()?),
                };
            } else {
                break;
            }
        }
        Ok(e)
    }
    fn unary(&mut self) -> Result<Expr> {
        if self.sym("-") {
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.unary()?),
            })
        } else {
            self.primary()
        }
    }
    fn primary(&mut self) -> Result<Expr> {
        let t = self.bump();
        match t.kind {
            TokenKind::Number(s) => Ok(if s.contains('.') {
                Expr::Literal(Value::Real(
                    s.parse()
                        .map_err(|_| MiniDbError::parse("bad number", t.line, t.column))?,
                ))
            } else {
                Expr::Literal(Value::Int(
                    s.parse()
                        .map_err(|_| MiniDbError::parse("bad number", t.line, t.column))?,
                ))
            }),
            TokenKind::String(s) => Ok(Expr::Literal(Value::Text(s))),
            TokenKind::Keyword(s) if s == "NULL" => Ok(Expr::Literal(Value::Null)),
            TokenKind::Keyword(s) if s == "TRUE" => Ok(Expr::Literal(Value::Bool(true))),
            TokenKind::Keyword(s) if s == "FALSE" => Ok(Expr::Literal(Value::Bool(false))),
            TokenKind::Ident(s) | TokenKind::Keyword(s) => {
                if self.sym("(") {
                    let args = if self.sym(")") {
                        Vec::new()
                    } else {
                        let a = self.expr_list()?;
                        self.expect_sym(")")?;
                        a
                    };
                    Ok(Expr::Func { name: s, args })
                } else {
                    Ok(Expr::Column(s))
                }
            }
            TokenKind::Symbol(s) if s == "(" => {
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            _ => Err(MiniDbError::parse("expected expression", t.line, t.column)),
        }
    }
}
