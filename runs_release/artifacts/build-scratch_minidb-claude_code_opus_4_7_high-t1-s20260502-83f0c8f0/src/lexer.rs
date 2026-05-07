//! SQL lexer. Produces a stream of `Token`s with line/column positions.

use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLit(i64),
    RealLit(f64),
    StringLit(String),
    Identifier(String),

    // Keywords
    Select,
    From,
    Where,
    Insert,
    Into,
    Values,
    Update,
    Set,
    Delete,
    Create,
    Drop,
    Table,
    Index,
    On,
    Begin,
    Commit,
    Rollback,
    And,
    Or,
    Not,
    Null,
    True,
    False,
    Primary,
    Key,
    Unique,
    Default,
    Check,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Join,
    Inner,
    As,
    Is,

    // Punctuation / operators
    LParen,
    RParen,
    Comma,
    Semicolon,
    Star,
    Dot,
    Plus,
    Minus,
    Slash,
    ConcatOp,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,

    Eof,
}

impl Token {
    pub fn as_keyword(s: &str) -> Option<Token> {
        let upper = s.to_ascii_uppercase();
        Some(match upper.as_str() {
            "SELECT" => Token::Select,
            "FROM" => Token::From,
            "WHERE" => Token::Where,
            "INSERT" => Token::Insert,
            "INTO" => Token::Into,
            "VALUES" => Token::Values,
            "UPDATE" => Token::Update,
            "SET" => Token::Set,
            "DELETE" => Token::Delete,
            "CREATE" => Token::Create,
            "DROP" => Token::Drop,
            "TABLE" => Token::Table,
            "INDEX" => Token::Index,
            "ON" => Token::On,
            "BEGIN" => Token::Begin,
            "COMMIT" => Token::Commit,
            "ROLLBACK" => Token::Rollback,
            "AND" => Token::And,
            "OR" => Token::Or,
            "NOT" => Token::Not,
            "NULL" => Token::Null,
            "TRUE" => Token::True,
            "FALSE" => Token::False,
            "PRIMARY" => Token::Primary,
            "KEY" => Token::Key,
            "UNIQUE" => Token::Unique,
            "DEFAULT" => Token::Default,
            "CHECK" => Token::Check,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "LIMIT" => Token::Limit,
            "OFFSET" => Token::Offset,
            "JOIN" => Token::Join,
            "INNER" => Token::Inner,
            "AS" => Token::As,
            "IS" => Token::Is,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocatedToken {
    pub token: Token,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input: input.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.input.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(b)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while let Some(b) = self.peek(0) {
                if b.is_ascii_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }
            // -- line comments
            if self.peek(0) == Some(b'-') && self.peek(1) == Some(b'-') {
                while let Some(b) = self.peek(0) {
                    if b == b'\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }
            // /* block comments */
            if self.peek(0) == Some(b'/') && self.peek(1) == Some(b'*') {
                self.advance();
                self.advance();
                while let Some(_) = self.peek(0) {
                    if self.peek(0) == Some(b'*') && self.peek(1) == Some(b'/') {
                        self.advance();
                        self.advance();
                        break;
                    }
                    self.advance();
                }
                continue;
            }
            break;
        }
    }

    fn read_identifier(&mut self) -> Result<LocatedToken> {
        let start = self.pos;
        let line = self.line;
        let column = self.column;
        while let Some(b) = self.peek(0) {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| MiniDbError::lex("non-utf8 identifier", line, column))?
            .to_string();
        let tok = Token::as_keyword(&s).unwrap_or(Token::Identifier(s));
        Ok(LocatedToken {
            token: tok,
            line,
            column,
        })
    }

    fn read_number(&mut self) -> Result<LocatedToken> {
        let start = self.pos;
        let line = self.line;
        let column = self.column;
        while let Some(b) = self.peek(0) {
            if b.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        let mut is_real = false;
        if self.peek(0) == Some(b'.') && self.peek(1).map_or(false, |c| c.is_ascii_digit()) {
            is_real = true;
            self.advance();
            while let Some(b) = self.peek(0) {
                if b.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(0), Some(b'e') | Some(b'E')) {
            is_real = true;
            self.advance();
            if matches!(self.peek(0), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            while let Some(b) = self.peek(0) {
                if b.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| MiniDbError::lex("non-utf8 number", line, column))?;
        let tok = if is_real {
            Token::RealLit(
                s.parse::<f64>()
                    .map_err(|e| MiniDbError::lex(format!("bad real: {}", e), line, column))?,
            )
        } else {
            Token::IntLit(
                s.parse::<i64>()
                    .map_err(|e| MiniDbError::lex(format!("bad int: {}", e), line, column))?,
            )
        };
        Ok(LocatedToken {
            token: tok,
            line,
            column,
        })
    }

    fn read_string(&mut self) -> Result<LocatedToken> {
        let line = self.line;
        let column = self.column;
        self.advance(); // consume opening quote
        let mut s = String::new();
        loop {
            let b = self
                .peek(0)
                .ok_or_else(|| MiniDbError::lex("unterminated string literal", line, column))?;
            if b == b'\'' {
                if self.peek(1) == Some(b'\'') {
                    // escaped quote
                    s.push('\'');
                    self.advance();
                    self.advance();
                } else {
                    self.advance();
                    break;
                }
            } else {
                s.push(b as char);
                self.advance();
            }
        }
        Ok(LocatedToken {
            token: Token::StringLit(s),
            line,
            column,
        })
    }

    pub fn next_token(&mut self) -> Result<LocatedToken> {
        self.skip_whitespace_and_comments();
        let line = self.line;
        let column = self.column;
        let Some(b) = self.peek(0) else {
            return Ok(LocatedToken {
                token: Token::Eof,
                line,
                column,
            });
        };
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => self.read_identifier(),
            b'0'..=b'9' => self.read_number(),
            b'\'' => self.read_string(),
            b'"' => {
                // double-quoted identifier
                self.advance();
                let mut s = String::new();
                loop {
                    let nb = self.peek(0).ok_or_else(|| {
                        MiniDbError::lex("unterminated quoted ident", line, column)
                    })?;
                    if nb == b'"' {
                        self.advance();
                        break;
                    }
                    s.push(nb as char);
                    self.advance();
                }
                Ok(LocatedToken {
                    token: Token::Identifier(s),
                    line,
                    column,
                })
            }
            b'(' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::LParen,
                    line,
                    column,
                })
            }
            b')' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::RParen,
                    line,
                    column,
                })
            }
            b',' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Comma,
                    line,
                    column,
                })
            }
            b';' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Semicolon,
                    line,
                    column,
                })
            }
            b'*' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Star,
                    line,
                    column,
                })
            }
            b'.' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Dot,
                    line,
                    column,
                })
            }
            b'+' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Plus,
                    line,
                    column,
                })
            }
            b'-' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Minus,
                    line,
                    column,
                })
            }
            b'/' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Slash,
                    line,
                    column,
                })
            }
            b'|' => {
                if self.peek(1) == Some(b'|') {
                    self.advance();
                    self.advance();
                    Ok(LocatedToken {
                        token: Token::ConcatOp,
                        line,
                        column,
                    })
                } else {
                    Err(MiniDbError::lex("expected '||'", line, column))
                }
            }
            b'=' => {
                self.advance();
                Ok(LocatedToken {
                    token: Token::Eq,
                    line,
                    column,
                })
            }
            b'<' => {
                self.advance();
                if self.peek(0) == Some(b'=') {
                    self.advance();
                    Ok(LocatedToken {
                        token: Token::LtEq,
                        line,
                        column,
                    })
                } else if self.peek(0) == Some(b'>') {
                    self.advance();
                    Ok(LocatedToken {
                        token: Token::NotEq,
                        line,
                        column,
                    })
                } else {
                    Ok(LocatedToken {
                        token: Token::Lt,
                        line,
                        column,
                    })
                }
            }
            b'>' => {
                self.advance();
                if self.peek(0) == Some(b'=') {
                    self.advance();
                    Ok(LocatedToken {
                        token: Token::GtEq,
                        line,
                        column,
                    })
                } else {
                    Ok(LocatedToken {
                        token: Token::Gt,
                        line,
                        column,
                    })
                }
            }
            b'!' => {
                self.advance();
                if self.peek(0) == Some(b'=') {
                    self.advance();
                    Ok(LocatedToken {
                        token: Token::NotEq,
                        line,
                        column,
                    })
                } else {
                    Err(MiniDbError::lex("unexpected '!'", line, column))
                }
            }
            other => Err(MiniDbError::lex(
                format!("unexpected character: {:?}", other as char),
                line,
                column,
            )),
        }
    }

    pub fn tokenize_all(&mut self) -> Result<Vec<LocatedToken>> {
        let mut out = Vec::new();
        loop {
            let t = self.next_token()?;
            let is_eof = matches!(t.token, Token::Eof);
            out.push(t);
            if is_eof {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Vec<Token> {
        let mut l = Lexer::new(s);
        l.tokenize_all()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn simple_select() {
        let toks = lex("SELECT a, b FROM t WHERE a = 1;");
        assert_eq!(
            toks,
            vec![
                Token::Select,
                Token::Identifier("a".into()),
                Token::Comma,
                Token::Identifier("b".into()),
                Token::From,
                Token::Identifier("t".into()),
                Token::Where,
                Token::Identifier("a".into()),
                Token::Eq,
                Token::IntLit(1),
                Token::Semicolon,
                Token::Eof
            ]
        );
    }

    #[test]
    fn string_with_quotes() {
        let toks = lex("'it''s ok'");
        assert_eq!(toks, vec![Token::StringLit("it's ok".into()), Token::Eof]);
    }

    #[test]
    fn comparison_ops() {
        let toks = lex("<= >= <> != < > =");
        assert_eq!(
            toks,
            vec![
                Token::LtEq,
                Token::GtEq,
                Token::NotEq,
                Token::NotEq,
                Token::Lt,
                Token::Gt,
                Token::Eq,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn real_literal() {
        let toks = lex("3.14 1e2");
        assert_eq!(
            toks,
            vec![Token::RealLit(3.14), Token::RealLit(100.0), Token::Eof]
        );
    }

    #[test]
    fn line_comment_skipped() {
        let toks = lex("SELECT 1 -- comment\n;");
        assert_eq!(
            toks,
            vec![
                Token::Select,
                Token::IntLit(1),
                Token::Semicolon,
                Token::Eof
            ]
        );
    }

    #[test]
    fn position_tracked() {
        let mut l = Lexer::new("\nSELECT");
        let lt = l.next_token().unwrap();
        assert_eq!(lt.line, 2);
        assert_eq!(lt.column, 1);
    }

    #[test]
    fn unterminated_string_errors() {
        let mut l = Lexer::new("'oops");
        assert!(l.tokenize_all().is_err());
    }
}
