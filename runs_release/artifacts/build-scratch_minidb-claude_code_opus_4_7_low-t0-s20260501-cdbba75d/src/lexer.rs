use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // identifiers / literals
    Ident(String),
    Number(String),
    String(String),
    // punctuation
    LParen,
    RParen,
    Comma,
    Semicolon,
    Star,
    Plus,
    Minus,
    Slash,
    Dot,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    ConcatOp, // ||
    // keywords (we keep them as Ident with normalized case in matcher,
    // but recognise them here for convenience)
    Keyword(Keyword),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Table,
    Drop,
    Index,
    On,
    PrimaryKey,
    Primary,
    Key,
    NotKw,
    Null,
    Unique,
    Default,
    Check,
    Inner,
    Join,
    OrderBy,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    And,
    Or,
    Is,
    True,
    False,
    Begin,
    Commit,
    Rollback,
    As,
}

#[derive(Debug, Clone)]
pub struct PosToken {
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
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    pub fn tokenize(mut self) -> Result<Vec<PosToken>> {
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            let line = self.line;
            let col = self.col;
            let Some(b) = self.peek() else {
                out.push(PosToken {
                    token: Token::Eof,
                    line,
                    col,
                });
                return Ok(out);
            };
            let tok = match b {
                b'(' => {
                    self.bump();
                    Token::LParen
                }
                b')' => {
                    self.bump();
                    Token::RParen
                }
                b',' => {
                    self.bump();
                    Token::Comma
                }
                b';' => {
                    self.bump();
                    Token::Semicolon
                }
                b'*' => {
                    self.bump();
                    Token::Star
                }
                b'+' => {
                    self.bump();
                    Token::Plus
                }
                b'-' => {
                    self.bump();
                    Token::Minus
                }
                b'/' => {
                    self.bump();
                    Token::Slash
                }
                b'.' => {
                    self.bump();
                    Token::Dot
                }
                b'=' => {
                    self.bump();
                    Token::Eq
                }
                b'<' => {
                    self.bump();
                    match self.peek() {
                        Some(b'=') => {
                            self.bump();
                            Token::LtEq
                        }
                        Some(b'>') => {
                            self.bump();
                            Token::NotEq
                        }
                        _ => Token::Lt,
                    }
                }
                b'>' => {
                    self.bump();
                    match self.peek() {
                        Some(b'=') => {
                            self.bump();
                            Token::GtEq
                        }
                        _ => Token::Gt,
                    }
                }
                b'!' => {
                    self.bump();
                    match self.peek() {
                        Some(b'=') => {
                            self.bump();
                            Token::NotEq
                        }
                        _ => {
                            return Err(MiniDbError::Lex {
                                line,
                                col,
                                msg: "expected '=' after '!'".into(),
                            })
                        }
                    }
                }
                b'|' => {
                    self.bump();
                    match self.peek() {
                        Some(b'|') => {
                            self.bump();
                            Token::ConcatOp
                        }
                        _ => {
                            return Err(MiniDbError::Lex {
                                line,
                                col,
                                msg: "expected second '|'".into(),
                            })
                        }
                    }
                }
                b'\'' => self.read_string(b'\'', line, col)?,
                b'"' => {
                    // double-quoted identifier
                    let s = self.read_quoted_ident(line, col)?;
                    Token::Ident(s)
                }
                b if b.is_ascii_digit() => self.read_number()?,
                b if is_ident_start(b) => self.read_ident_or_keyword(),
                _ => {
                    return Err(MiniDbError::Lex {
                        line,
                        col,
                        msg: format!("unexpected character: {:?}", b as char),
                    })
                }
            };
            out.push(PosToken {
                token: tok,
                line,
                col,
            });
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.bump();
                }
                Some(b'-') if self.peek2() == Some(b'-') => {
                    while let Some(b) = self.peek() {
                        if b == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    fn read_string(&mut self, quote: u8, line: usize, col: usize) -> Result<Token> {
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(MiniDbError::Lex {
                    line,
                    col,
                    msg: "unterminated string".into(),
                });
            };
            if b == quote {
                self.bump();
                if self.peek() == Some(quote) {
                    self.bump();
                    s.push(quote as char);
                } else {
                    return Ok(Token::String(s));
                }
            } else {
                self.bump();
                s.push(b as char);
            }
        }
    }

    fn read_quoted_ident(&mut self, line: usize, col: usize) -> Result<String> {
        self.bump();
        let mut s = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(MiniDbError::Lex {
                    line,
                    col,
                    msg: "unterminated identifier".into(),
                });
            };
            if b == b'"' {
                self.bump();
                return Ok(s);
            }
            self.bump();
            s.push(b as char);
        }
    }

    fn read_number(&mut self) -> Result<Token> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }
        if self.peek() == Some(b'.') && self.peek2().map(|b| b.is_ascii_digit()).unwrap_or(false) {
            self.bump();
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .unwrap()
            .to_string();
        Ok(Token::Number(s))
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_ident_continue(b) {
                self.bump();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        if let Some(k) = match_keyword(s) {
            Token::Keyword(k)
        } else {
            Token::Ident(s.to_string())
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn match_keyword(s: &str) -> Option<Keyword> {
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
        "TABLE" => Keyword::Table,
        "DROP" => Keyword::Drop,
        "INDEX" => Keyword::Index,
        "ON" => Keyword::On,
        "PRIMARY" => Keyword::Primary,
        "KEY" => Keyword::Key,
        "NOT" => Keyword::NotKw,
        "NULL" => Keyword::Null,
        "UNIQUE" => Keyword::Unique,
        "DEFAULT" => Keyword::Default,
        "CHECK" => Keyword::Check,
        "INNER" => Keyword::Inner,
        "JOIN" => Keyword::Join,
        "ORDER" => Keyword::Order,
        "BY" => Keyword::By,
        "ASC" => Keyword::Asc,
        "DESC" => Keyword::Desc,
        "LIMIT" => Keyword::Limit,
        "OFFSET" => Keyword::Offset,
        "AND" => Keyword::And,
        "OR" => Keyword::Or,
        "IS" => Keyword::Is,
        "TRUE" => Keyword::True,
        "FALSE" => Keyword::False,
        "BEGIN" => Keyword::Begin,
        "COMMIT" => Keyword::Commit,
        "ROLLBACK" => Keyword::Rollback,
        "AS" => Keyword::As,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lex_simple() {
        let toks = Lexer::new("SELECT * FROM t WHERE x = 1;")
            .tokenize()
            .unwrap();
        assert!(matches!(toks[0].token, Token::Keyword(Keyword::Select)));
        assert!(matches!(toks[1].token, Token::Star));
        assert!(matches!(toks.last().unwrap().token, Token::Eof));
    }

    #[test]
    fn lex_string_escape() {
        let toks = Lexer::new("'it''s ok'").tokenize().unwrap();
        match &toks[0].token {
            Token::String(s) => assert_eq!(s, "it's ok"),
            _ => panic!(),
        }
    }

    #[test]
    fn lex_operators() {
        let toks = Lexer::new("a <> b || c <= d").tokenize().unwrap();
        let kinds: Vec<_> = toks.into_iter().map(|t| t.token).collect();
        assert!(matches!(kinds[1], Token::NotEq));
        assert!(matches!(kinds[3], Token::ConcatOp));
        assert!(matches!(kinds[5], Token::LtEq));
    }
}
