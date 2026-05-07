use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Keyword(Keyword),
    Number(f64, bool), // value, is_int
    String(String),
    Punct(char),
    Op(String),
    Comma,
    Semicolon,
    LParen,
    RParen,
    Star,
    Dot,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
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
    Primary,
    Key,
    Not,
    Null,
    Unique,
    Default,
    Check,
    And,
    Or,
    Is,
    In,
    Like,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Begin,
    Commit,
    Rollback,
    Inner,
    Join,
    As,
    True,
    False,
    If,
    Exists,
}

impl Keyword {
    pub fn from_str(s: &str) -> Option<Keyword> {
        Some(match s.to_ascii_uppercase().as_str() {
            "SELECT" => Keyword::Select,
            "FROM" => Keyword::From,
            "WHERE" => Keyword::Where,
            "INSERT" => Keyword::Insert,
            "INTO" => Keyword::Into,
            "VALUES" => Keyword::Values,
            "UPDATE" => Keyword::Update,
            "SET" => Keyword::Set,
            "DELETE" => Keyword::Delete,
            "CREATE" => Keyword::Create,
            "DROP" => Keyword::Drop,
            "TABLE" => Keyword::Table,
            "INDEX" => Keyword::Index,
            "ON" => Keyword::On,
            "PRIMARY" => Keyword::Primary,
            "KEY" => Keyword::Key,
            "NOT" => Keyword::Not,
            "NULL" => Keyword::Null,
            "UNIQUE" => Keyword::Unique,
            "DEFAULT" => Keyword::Default,
            "CHECK" => Keyword::Check,
            "AND" => Keyword::And,
            "OR" => Keyword::Or,
            "IS" => Keyword::Is,
            "IN" => Keyword::In,
            "LIKE" => Keyword::Like,
            "ORDER" => Keyword::Order,
            "BY" => Keyword::By,
            "ASC" => Keyword::Asc,
            "DESC" => Keyword::Desc,
            "LIMIT" => Keyword::Limit,
            "OFFSET" => Keyword::Offset,
            "BEGIN" => Keyword::Begin,
            "COMMIT" => Keyword::Commit,
            "ROLLBACK" => Keyword::Rollback,
            "INNER" => Keyword::Inner,
            "JOIN" => Keyword::Join,
            "AS" => Keyword::As,
            "TRUE" => Keyword::True,
            "FALSE" => Keyword::False,
            "IF" => Keyword::If,
            "EXISTS" => Keyword::Exists,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    self.bump();
                }
                Some(b'-') if self.peek2() == Some(b'-') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b'/') if self.peek2() == Some(b'*') => {
                    self.bump();
                    self.bump();
                    while let Some(c) = self.peek() {
                        if c == b'*' && self.peek2() == Some(b'/') {
                            self.bump();
                            self.bump();
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> Result<SpannedToken> {
        self.skip_ws_and_comments();
        let line = self.line;
        let col = self.col;
        let c = match self.peek() {
            Some(c) => c,
            None => {
                return Ok(SpannedToken {
                    token: Token::Eof,
                    line,
                    col,
                })
            }
        };

        if c.is_ascii_alphabetic() || c == b'_' {
            let mut s = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_alphanumeric() || c == b'_' {
                    s.push(c as char);
                    self.bump();
                } else {
                    break;
                }
            }
            let token = if let Some(kw) = Keyword::from_str(&s) {
                Token::Keyword(kw)
            } else {
                Token::Ident(s)
            };
            return Ok(SpannedToken { token, line, col });
        }

        if c.is_ascii_digit() || (c == b'.' && self.peek2().map_or(false, |x| x.is_ascii_digit())) {
            let mut s = String::new();
            let mut is_int = true;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c as char);
                    self.bump();
                } else if c == b'.' && is_int {
                    is_int = false;
                    s.push('.');
                    self.bump();
                } else if c == b'e' || c == b'E' {
                    is_int = false;
                    s.push(c as char);
                    self.bump();
                    if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                        s.push(self.peek().unwrap() as char);
                        self.bump();
                    }
                } else {
                    break;
                }
            }
            let v: f64 = s.parse().map_err(|_| MiniDbError::Lex {
                line,
                col,
                msg: format!("invalid number {}", s),
            })?;
            return Ok(SpannedToken {
                token: Token::Number(v, is_int),
                line,
                col,
            });
        }

        if c == b'\'' {
            self.bump();
            let mut s = String::new();
            loop {
                match self.peek() {
                    None => {
                        return Err(MiniDbError::Lex {
                            line,
                            col,
                            msg: "unterminated string literal".to_string(),
                        })
                    }
                    Some(b'\'') => {
                        self.bump();
                        if self.peek() == Some(b'\'') {
                            s.push('\'');
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    Some(c) => {
                        s.push(c as char);
                        self.bump();
                    }
                }
            }
            return Ok(SpannedToken {
                token: Token::String(s),
                line,
                col,
            });
        }

        // Two-char operators
        if c == b'<' && self.peek2() == Some(b'=') {
            self.bump();
            self.bump();
            return Ok(SpannedToken {
                token: Token::Op("<=".into()),
                line,
                col,
            });
        }
        if c == b'>' && self.peek2() == Some(b'=') {
            self.bump();
            self.bump();
            return Ok(SpannedToken {
                token: Token::Op(">=".into()),
                line,
                col,
            });
        }
        if c == b'<' && self.peek2() == Some(b'>') {
            self.bump();
            self.bump();
            return Ok(SpannedToken {
                token: Token::Op("<>".into()),
                line,
                col,
            });
        }
        if c == b'!' && self.peek2() == Some(b'=') {
            self.bump();
            self.bump();
            return Ok(SpannedToken {
                token: Token::Op("<>".into()),
                line,
                col,
            });
        }
        if c == b'|' && self.peek2() == Some(b'|') {
            self.bump();
            self.bump();
            return Ok(SpannedToken {
                token: Token::Op("||".into()),
                line,
                col,
            });
        }

        self.bump();
        let token = match c {
            b',' => Token::Comma,
            b';' => Token::Semicolon,
            b'(' => Token::LParen,
            b')' => Token::RParen,
            b'*' => Token::Star,
            b'.' => Token::Dot,
            b'=' | b'<' | b'>' | b'+' | b'-' | b'/' | b'%' => Token::Op((c as char).to_string()),
            other => {
                return Err(MiniDbError::Lex {
                    line,
                    col,
                    msg: format!("unexpected character '{}'", other as char),
                })
            }
        };
        Ok(SpannedToken { token, line, col })
    }

    pub fn tokenize(mut self) -> Result<Vec<SpannedToken>> {
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

    #[test]
    fn keywords_and_idents() {
        let toks = Lexer::new("SELECT a FROM t").tokenize().unwrap();
        assert!(matches!(toks[0].token, Token::Keyword(Keyword::Select)));
        assert!(matches!(toks[1].token, Token::Ident(ref s) if s == "a"));
    }

    #[test]
    fn numbers_and_strings() {
        let toks = Lexer::new("123 4.5 'he''llo'").tokenize().unwrap();
        assert!(matches!(toks[0].token, Token::Number(_, true)));
        assert!(matches!(toks[1].token, Token::Number(_, false)));
        assert!(matches!(toks[2].token, Token::String(ref s) if s == "he'llo"));
    }

    #[test]
    fn operators() {
        let toks = Lexer::new("<= >= <> != ||").tokenize().unwrap();
        assert!(matches!(toks[0].token, Token::Op(ref s) if s == "<="));
        assert!(matches!(toks[3].token, Token::Op(ref s) if s == "<>"));
        assert!(matches!(toks[4].token, Token::Op(ref s) if s == "||"));
    }

    #[test]
    fn comments_skipped() {
        let toks = Lexer::new("-- comment\nSELECT 1").tokenize().unwrap();
        assert!(matches!(toks[0].token, Token::Keyword(Keyword::Select)));
    }
}
