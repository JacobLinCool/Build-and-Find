use crate::error::{MiniDbError, Result};
use crate::lexer::{Lexer, SpannedToken, Token};
use crate::parser::ast::*;
use crate::types::{ColumnType, Value};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn new(input: &str) -> Result<Self> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        Ok(Self { tokens, pos: 0 })
    }

    fn peek(&self) -> &Token {
        &self
            .tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof)
    }

    fn current_span(&self) -> (usize, usize) {
        self.tokens
            .get(self.pos)
            .map(|t| (t.line, t.col))
            .unwrap_or((0, 0))
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(SpannedToken {
            token: Token::Eof,
            line: 0,
            col: 0,
        });
        self.pos += 1;
        tok.token
    }

    fn expect(&mut self, expected: &Token) -> Result<Token> {
        let tok = self.advance();
        if &tok == expected {
            Ok(tok)
        } else {
            let (line, col) = self.current_span();
            Err(MiniDbError::Parse {
                line,
                col,
                message: format!("expected '{}', got '{}'", expected, tok),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            // Allow SQL keywords to be used as identifiers (e.g., aliases)
            Token::Sum => Ok("sum".to_string()),
            Token::Count => Ok("count".to_string()),
            Token::Avg => Ok("avg".to_string()),
            Token::Min => Ok("min".to_string()),
            Token::Max => Ok("max".to_string()),
            Token::Order => Ok("order".to_string()),
            Token::Group => Ok("group".to_string()),
            Token::By => Ok("by".to_string()),
            Token::Having => Ok("having".to_string()),
            Token::Limit => Ok("limit".to_string()),
            Token::Offset => Ok("offset".to_string()),
            Token::Set => Ok("set".to_string()),
            Token::Key => Ok("key".to_string()),
            Token::On => Ok("on".to_string()),
            Token::As => Ok("as".to_string()),
            Token::Is => Ok("is".to_string()),
            Token::In => Ok("in".to_string()),
            Token::Or => Ok("or".to_string()),
            Token::And => Ok("and".to_string()),
            Token::Not => Ok("not".to_string()),
            Token::All => Ok("all".to_string()),
            Token::Any => Ok("any".to_string()),
            Token::End => Ok("end".to_string()),
            Token::Else => Ok("else".to_string()),
            Token::Then => Ok("then".to_string()),
            Token::When => Ok("when".to_string()),
            Token::Case => Ok("case".to_string()),
            tok => {
                let (line, col) = self.current_span();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    message: format!("expected identifier, got '{}'", tok),
                })
            }
        }
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof | Token::Semicolon)
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek().clone() {
            Token::Create => {
                self.advance();
                match self.peek().clone() {
                    Token::Table => self.parse_create_table(),
                    Token::Index => self.parse_create_index(),
                    _ => {
                        let (line, col) = self.current_span();
                        Err(MiniDbError::Parse {
                            line,
                            col,
                            message: "expected TABLE or INDEX after CREATE".into(),
                        })
                    }
                }
            }
            Token::Drop => {
                self.advance();
                match self.peek().clone() {
                    Token::Table => {
                        self.advance();
                        let name = self.expect_ident()?;
                        Ok(Statement::DropTable { name })
                    }
                    Token::Index => {
                        self.advance();
                        let name = self.expect_ident()?;
                        Ok(Statement::DropIndex { name })
                    }
                    _ => {
                        let (line, col) = self.current_span();
                        Err(MiniDbError::Parse {
                            line,
                            col,
                            message: "expected TABLE or INDEX after DROP".into(),
                        })
                    }
                }
            }
            Token::Insert => self.parse_insert(),
            Token::Select => self.parse_select(),
            Token::Update => self.parse_update(),
            Token::Delete => self.parse_delete(),
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
            Token::Dot => self.parse_meta(),
            _ => {
                let (line, col) = self.current_span();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    message: format!("unexpected token '{}'", self.peek()),
                })
            }
        }
    }

    fn parse_meta(&mut self) -> Result<Statement> {
        self.advance(); // consume '.'
        let name = self.expect_ident()?;
        match name.to_lowercase().as_str() {
            "schema" => Ok(Statement::MetaSchema),
            "tables" => Ok(Statement::MetaTables),
            "indexes" => Ok(Statement::MetaIndexes),
            "exit" | "quit" => Ok(Statement::MetaExit),
            "format" => {
                let kind = self.expect_ident()?;
                Ok(Statement::MetaFormat(kind))
            }
            _ => {
                let (line, col) = self.current_span();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    message: format!("unknown meta-command '.{}'", name),
                })
            }
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        self.advance(); // consume TABLE
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;

        let mut columns = Vec::new();
        let mut check_constraints = Vec::new();

        loop {
            if self.peek() == &Token::RParen {
                break;
            }

            // Check for table-level constraint
            if self.peek() == &Token::Check {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.parse_or()?;
                self.expect(&Token::RParen)?;
                check_constraints.push(expr);
            } else {
                columns.push(self.parse_column_def()?);
            }

            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(&Token::RParen)?;

        Ok(Statement::CreateTable(CreateTableStmt {
            name,
            columns,
            check_constraints,
        }))
    }

    fn parse_column_def(&mut self) -> Result<ColumnDefAst> {
        let name = self.expect_ident()?;
        let col_type = self.parse_column_type()?;
        let mut constraints = Vec::new();

        loop {
            match self.peek() {
                Token::Primary => {
                    self.advance();
                    self.expect(&Token::Key)?;
                    constraints.push(ColumnConstraintAst::PrimaryKey);
                }
                Token::Not => {
                    self.advance();
                    self.expect(&Token::Null)?;
                    constraints.push(ColumnConstraintAst::NotNull);
                }
                Token::Unique => {
                    self.advance();
                    constraints.push(ColumnConstraintAst::Unique);
                }
                Token::Default => {
                    self.advance();
                    let expr = self.parse_primary()?;
                    constraints.push(ColumnConstraintAst::Default(expr));
                }
                _ => break,
            }
        }

        Ok(ColumnDefAst {
            name,
            col_type,
            constraints,
        })
    }

    fn parse_column_type(&mut self) -> Result<ColumnType> {
        match self.advance() {
            Token::IntType | Token::IntegerType => Ok(ColumnType::Int),
            Token::BigIntType => Ok(ColumnType::BigInt),
            Token::RealType | Token::FloatType | Token::DoubleType => Ok(ColumnType::Real),
            Token::TextType | Token::VarcharType | Token::CharType | Token::StringType => {
                Ok(ColumnType::Text)
            }
            Token::BoolType | Token::BooleanType => Ok(ColumnType::Bool),
            tok => {
                let (line, col) = self.current_span();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    message: format!("expected column type, got '{}'", tok),
                })
            }
        }
    }

    fn parse_create_index(&mut self) -> Result<Statement> {
        self.advance(); // consume INDEX
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
        }))
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.advance(); // consume INSERT
        self.expect(&Token::Into)?;
        let table = self.expect_ident()?;

        let columns = if self.peek() == &Token::LParen {
            self.advance();
            let cols = self.parse_ident_list()?;
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;

        let mut values = Vec::new();
        loop {
            self.expect(&Token::LParen)?;
            let row = self.parse_expr_list()?;
            self.expect(&Token::RParen)?;
            values.push(row);

            if self.peek() == &Token::Comma {
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

    fn parse_ident_list(&mut self) -> Result<Vec<String>> {
        let mut idents = vec![self.expect_ident()?];
        while self.peek() == &Token::Comma {
            self.advance();
            idents.push(self.expect_ident()?);
        }
        Ok(idents)
    }

    fn parse_expr_list(&mut self) -> Result<Vec<Expr>> {
        let mut exprs = vec![self.parse_or()?];
        while self.peek() == &Token::Comma {
            self.advance();
            exprs.push(self.parse_or()?);
        }
        Ok(exprs)
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.advance(); // consume SELECT

        let distinct = if self.peek() == &Token::Distinct {
            self.advance();
            true
        } else {
            false
        };

        let projection = self.parse_projection()?;

        let from = if self.peek() == &Token::From {
            self.advance();
            Some(self.parse_from()?)
        } else {
            None
        };

        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_or()?)
        } else {
            None
        };

        let group_by = if self.peek() == &Token::Group {
            self.advance();
            self.expect(&Token::By)?;
            Some(self.parse_expr_list()?)
        } else {
            None
        };

        let having = if self.peek() == &Token::Having {
            self.advance();
            Some(self.parse_or()?)
        } else {
            None
        };

        let order_by = if self.peek() == &Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            Some(self.parse_order_by()?)
        } else {
            None
        };

        let limit = if self.peek() == &Token::Limit {
            self.advance();
            Some(self.parse_primary()?)
        } else {
            None
        };

        let offset = if self.peek() == &Token::Offset {
            self.advance();
            Some(self.parse_primary()?)
        } else {
            None
        };

        Ok(Statement::Select(SelectStmt {
            distinct,
            projection,
            from,
            where_clause,
            group_by,
            having,
            order_by,
            limit,
            offset,
        }))
    }

    fn parse_projection(&mut self) -> Result<Vec<SelectItem>> {
        let mut items = vec![self.parse_select_item()?];
        while self.peek() == &Token::Comma {
            self.advance();
            items.push(self.parse_select_item()?);
        }
        Ok(items)
    }

    fn parse_select_item(&mut self) -> Result<SelectItem> {
        if self.peek() == &Token::Star {
            self.advance();
            return Ok(SelectItem::Wildcard);
        }

        // Check for table.*
        if let Token::Ident(name) = self.peek().clone() {
            if self.pos + 1 < self.tokens.len() && self.tokens[self.pos + 1].token == Token::Dot {
                if self.pos + 2 < self.tokens.len()
                    && self.tokens[self.pos + 2].token == Token::Star
                {
                    self.advance(); // ident
                    self.advance(); // dot
                    self.advance(); // star
                    return Ok(SelectItem::QualifiedWildcard(name));
                }
            }
        }

        let expr = self.parse_or()?;
        let alias = if self.peek() == &Token::As {
            self.advance();
            Some(self.expect_ident()?)
        } else if let Token::Ident(_) = self.peek() {
            // Allow alias without AS
            Some(self.expect_ident()?)
        } else {
            None
        };

        Ok(SelectItem::Expr { expr, alias })
    }

    fn parse_from(&mut self) -> Result<FromClause> {
        let mut left = self.parse_from_table()?;

        loop {
            let join_type = match self.peek() {
                Token::Inner | Token::Join => JoinType::Inner,
                Token::Left => JoinType::Left,
                Token::Right => JoinType::Right,
                Token::Cross => JoinType::Cross,
                _ => break,
            };

            if matches!(self.peek(), Token::Left | Token::Right | Token::Cross) {
                self.advance();
                if self.peek() == &Token::Outer {
                    self.advance();
                }
                self.expect(&Token::Join)?;
            } else if self.peek() == &Token::Inner {
                self.advance();
                self.expect(&Token::Join)?;
            } else {
                self.advance(); // JOIN
            }

            let right = self.parse_from_table()?;

            let condition = if join_type == JoinType::Cross {
                // CROSS JOIN has no ON clause
                crate::parser::ast::Expr::Literal(Value::Bool(true))
            } else {
                self.expect(&Token::On)?;
                self.parse_or()?
            };

            left = FromClause::Join {
                left: Box::new(left),
                right: Box::new(right),
                join_type,
                condition,
            };
        }

        Ok(left)
    }

    fn parse_from_table(&mut self) -> Result<FromClause> {
        let name = self.expect_ident()?;
        let alias = if self.peek() == &Token::As {
            self.advance();
            Some(self.expect_ident()?)
        } else if let Token::Ident(_) = self.peek() {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(FromClause::Table { name, alias })
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderByItem>> {
        let mut items = vec![self.parse_order_by_item()?];
        while self.peek() == &Token::Comma {
            self.advance();
            items.push(self.parse_order_by_item()?);
        }
        Ok(items)
    }

    fn parse_order_by_item(&mut self) -> Result<OrderByItem> {
        let expr = self.parse_or()?;
        let ascending = match self.peek() {
            Token::Asc => {
                self.advance();
                true
            }
            Token::Desc => {
                self.advance();
                false
            }
            _ => true,
        };
        Ok(OrderByItem { expr, ascending })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        self.advance(); // consume UPDATE
        let table = self.expect_ident()?;
        self.expect(&Token::Set)?;

        let mut assignments = vec![self.parse_assignment()?];
        while self.peek() == &Token::Comma {
            self.advance();
            assignments.push(self.parse_assignment()?);
        }

        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_or()?)
        } else {
            None
        };

        Ok(Statement::Update(UpdateStmt {
            table,
            assignments,
            where_clause,
        }))
    }

    fn parse_assignment(&mut self) -> Result<(String, Expr)> {
        let col = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let expr = self.parse_or()?;
        Ok((col, expr))
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.advance(); // consume DELETE
        self.expect(&Token::From)?;
        let table = self.expect_ident()?;

        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_or()?)
        } else {
            None
        };

        Ok(Statement::Delete(DeleteStmt {
            table,
            where_clause,
        }))
    }

    // Expression parsing with precedence climbing

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek() == &Token::Or {
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
        while self.peek() == &Token::And {
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
        if self.peek() == &Token::Not {
            self.advance();
            let expr = self.parse_comparison()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_concat()?;

        loop {
            let op = match self.peek() {
                Token::Eq => BinaryOp::Eq,
                Token::Ne => BinaryOp::Ne,
                Token::Lt => BinaryOp::Lt,
                Token::Le => BinaryOp::Le,
                Token::Gt => BinaryOp::Gt,
                Token::Ge => BinaryOp::Ge,
                Token::Like => BinaryOp::Like,
                Token::Is => {
                    self.advance();
                    if self.peek() == &Token::Not {
                        self.advance();
                        self.expect(&Token::Null)?;
                        left = Expr::IsNotNull(Box::new(left));
                    } else {
                        self.expect(&Token::Null)?;
                        left = Expr::IsNull(Box::new(left));
                    }
                    continue;
                }
                Token::In => {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    if self.peek() == &Token::Select {
                        let subquery = self.parse_select()?;
                        if let Statement::Select(sel) = subquery {
                            self.expect(&Token::RParen)?;
                            left = Expr::InList {
                                expr: Box::new(left),
                                list: vec![Expr::Subquery(Box::new(sel))],
                            };
                        }
                    } else {
                        let list = self.parse_expr_list()?;
                        self.expect(&Token::RParen)?;
                        left = Expr::InList {
                            expr: Box::new(left),
                            list,
                        };
                    }
                    continue;
                }
                Token::Between => {
                    self.advance();
                    let low = self.parse_concat()?;
                    self.expect(&Token::And)?;
                    let high = self.parse_concat()?;
                    // x BETWEEN low AND high => x >= low AND x <= high
                    left = Expr::BinaryOp {
                        left: Box::new(Expr::BinaryOp {
                            left: Box::new(left.clone()),
                            op: BinaryOp::Ge,
                            right: Box::new(low),
                        }),
                        op: BinaryOp::And,
                        right: Box::new(Expr::BinaryOp {
                            left: Box::new(left),
                            op: BinaryOp::Le,
                            right: Box::new(high),
                        }),
                    };
                    continue;
                }
                _ => break,
            };
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

    fn parse_concat(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while self.peek() == &Token::Concat {
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
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
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
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
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
            Token::Minus => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::Integer(i) => {
                self.advance();
                Ok(Expr::Literal(Value::Int(i)))
            }
            Token::Real(r) => {
                self.advance();
                Ok(Expr::Literal(Value::Real(r)))
            }
            Token::StringLit(s) => {
                self.advance();
                Ok(Expr::Literal(Value::Text(s)))
            }
            Token::BoolLit(b) => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(b)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_or()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Case => self.parse_case(),
            Token::Ident(name) => {
                self.advance();

                // Check for builtin functions
                let upper = name.to_uppercase();
                match upper.as_str() {
                    "LENGTH" | "LOWER" | "UPPER" | "SUBSTR" | "ABS" | "COALESCE" => {
                        self.expect(&Token::LParen)?;
                        let args = if self.peek() == &Token::RParen {
                            Vec::new()
                        } else {
                            self.parse_expr_list()?
                        };
                        self.expect(&Token::RParen)?;
                        return Ok(Expr::FunctionCall { name: upper, args });
                    }
                    _ => {}
                }

                // Check for table.column
                if self.peek() == &Token::Dot {
                    self.advance();
                    let col = self.expect_ident()?;
                    Ok(Expr::ColumnRef {
                        table: Some(name),
                        name: col,
                    })
                } else {
                    Ok(Expr::ColumnRef { table: None, name })
                }
            }
            Token::Count | Token::Sum | Token::Avg | Token::Min | Token::Max => {
                let name = format!("{}", self.peek());
                self.advance();
                self.expect(&Token::LParen)?;
                let args = if self.peek() == &Token::RParen {
                    Vec::new()
                } else if self.peek() == &Token::Star {
                    // COUNT(*) - treat * as a special column ref
                    self.advance();
                    vec![Expr::ColumnRef {
                        table: None,
                        name: "*".to_string(),
                    }]
                } else {
                    self.parse_expr_list()?
                };
                self.expect(&Token::RParen)?;
                Ok(Expr::FunctionCall { name, args })
            }
            _ => {
                let (line, col) = self.current_span();
                Err(MiniDbError::Parse {
                    line,
                    col,
                    message: format!("unexpected token '{}' in expression", self.peek()),
                })
            }
        }
    }

    fn parse_case(&mut self) -> Result<Expr> {
        self.advance(); // consume CASE

        let mut when_clauses = Vec::new();

        while self.peek() == &Token::When {
            self.advance();
            let condition = self.parse_or()?;
            self.expect(&Token::Then)?;
            let result = self.parse_or()?;
            when_clauses.push((condition, result));
        }

        let else_clause = if self.peek() == &Token::Else {
            self.advance();
            Some(Box::new(self.parse_or()?))
        } else {
            None
        };

        self.expect(&Token::End)?;

        Ok(Expr::Case {
            when_clauses,
            else_clause,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_create_table() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0)";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
        match stmt {
            Statement::CreateTable(ct) => {
                assert_eq!(ct.name, "users");
                assert_eq!(ct.columns.len(), 3);
                assert_eq!(ct.columns[0].name, "id");
                assert_eq!(ct.columns[0].col_type, ColumnType::Int);
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn test_parse_select() {
        let sql = "SELECT id, name FROM users WHERE age > 18 ORDER BY name ASC LIMIT 10";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
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
    fn test_parse_insert() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob')";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
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
    fn test_parse_update() {
        let sql = "UPDATE users SET name = 'Charlie' WHERE id = 1";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
        match stmt {
            Statement::Update(upd) => {
                assert_eq!(upd.table, "users");
                assert_eq!(upd.assignments.len(), 1);
                assert!(upd.where_clause.is_some());
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let sql = "DELETE FROM users WHERE id = 1";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
        match stmt {
            Statement::Delete(del) => {
                assert_eq!(del.table, "users");
                assert!(del.where_clause.is_some());
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_parse_join() {
        let sql = "SELECT a.id, b.name FROM users a INNER JOIN orders b ON a.id = b.user_id";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
        match stmt {
            Statement::Select(sel) => {
                assert!(sel.from.is_some());
                match sel.from.unwrap() {
                    FromClause::Join { join_type, .. } => {
                        assert_eq!(join_type, JoinType::Inner);
                    }
                    _ => panic!("expected Join"),
                }
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_create_index() {
        let sql = "CREATE INDEX idx_name ON users (name)";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
        match stmt {
            Statement::CreateIndex(ci) => {
                assert_eq!(ci.name, "idx_name");
                assert_eq!(ci.table, "users");
                assert_eq!(ci.column, "name");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_parse_expressions() {
        let sql = "SELECT 1 + 2 * 3, 'hello' || ' world', -5, NOT true";
        let mut parser = Parser::new(sql).unwrap();
        let stmt = parser.parse_statement().unwrap();
        match stmt {
            Statement::Select(sel) => {
                assert_eq!(sel.projection.len(), 4);
            }
            _ => panic!("expected Select"),
        }
    }
}
