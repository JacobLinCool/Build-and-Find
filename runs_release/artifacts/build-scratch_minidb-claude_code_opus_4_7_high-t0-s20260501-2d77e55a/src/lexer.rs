//! SQL lexer: produces a stream of [`Token`]s with line/column positions.

use crate::error::{MiniDbError, Result};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // Punctuation
    LParen,
    RParen,
    Comma,
    Semicolon,
    Dot,
    Star,

    // Operators
    Plus,
    Minus,
    Slash,
    Percent,
    ConcatOp, // ||
    Eq,
    NotEq, // <> or !=
    Lt,
    Le,
    Gt,
    Ge,

    // Literals
    IntLit(i64),
    RealLit(f64),
    StringLit(String),
    Ident(String),

    // Keywords (case-insensitive)
    Keyword(Keyword),

    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    As,
    Join,
    Inner,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    And,
    Or,
    Not,
    Null,
    Is,
    True,
    False,
    Begin,
    Commit,
    Rollback,
    PrimaryKey, // composite handled in parser
    Primary,
    Key,
    Unique,
    Default,
    Check,
    IntKw,
    BigInt,
    RealKw,
    TextKw,
    BoolKw,
    BooleanKw,
}

impl Keyword {
    pub fn from_ident(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "SELECT" => Self::Select,
            "FROM" => Self::From,
            "WHERE" => Self::Where,
            "INSERT" => Self::Insert,
            "INTO" => Self::Into,
            "VALUES" => Self::Values,
            "UPDATE" => Self::Update,
            "SET" => Self::Set,
            "DELETE" => Self::Delete,
            "CREATE" => Self::Create,
            "TABLE" => Self::Table,
            "DROP" => Self::Drop,
            "INDEX" => Self::Index,
            "ON" => Self::On,
            "AS" => Self::As,
            "JOIN" => Self::Join,
            "INNER" => Self::Inner,
            "ORDER" => Self::Order,
            "BY" => Self::By,
            "ASC" => Self::Asc,
            "DESC" => Self::Desc,
            "LIMIT" => Self::Limit,
            "OFFSET" => Self::Offset,
            "AND" => Self::And,
            "OR" => Self::Or,
            "NOT" => Self::Not,
            "NULL" => Self::Null,
            "IS" => Self::Is,
            "TRUE" => Self::True,
            "FALSE" => Self::False,
            "BEGIN" => Self::Begin,
            "COMMIT" => Self::Commit,
            "ROLLBACK" => Self::Rollback,
            "PRIMARY" => Self::Primary,
            "KEY" => Self::Key,
            "UNIQUE" => Self::Unique,
            "DEFAULT" => Self::Default,
            "CHECK" => Self::Check,
            "INT" | "INTEGER" => Self::IntKw,
            "BIGINT" => Self::BigInt,
            "REAL" | "DOUBLE" | "FLOAT" => Self::RealKw,
            "TEXT" | "VARCHAR" | "STRING" => Self::TextKw,
            "BOOL" => Self::BoolKw,
            "BOOLEAN" => Self::BooleanKw,
            _ => return None,
        })
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Self { kind, line, column }
    }
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            src: input.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut out = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            out.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(out)
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
            self.column = 1;
        } else {
            self.column += 1;
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

    fn next_token(&mut self) -> Result<Token> {
        self.skip_ws_and_comments();
        let line = self.line;
        let column = self.column;
        let c = match self.peek() {
            Some(c) => c,
            None => return Ok(Token::new(TokenKind::Eof, line, column)),
        };
        match c {
            b'(' => {
                self.bump();
                Ok(Token::new(TokenKind::LParen, line, column))
            }
            b')' => {
                self.bump();
                Ok(Token::new(TokenKind::RParen, line, column))
            }
            b',' => {
                self.bump();
                Ok(Token::new(TokenKind::Comma, line, column))
            }
            b';' => {
                self.bump();
                Ok(Token::new(TokenKind::Semicolon, line, column))
            }
            b'*' => {
                self.bump();
                Ok(Token::new(TokenKind::Star, line, column))
            }
            b'+' => {
                self.bump();
                Ok(Token::new(TokenKind::Plus, line, column))
            }
            b'-' => {
                self.bump();
                Ok(Token::new(TokenKind::Minus, line, column))
            }
            b'/' => {
                self.bump();
                Ok(Token::new(TokenKind::Slash, line, column))
            }
            b'%' => {
                self.bump();
                Ok(Token::new(TokenKind::Percent, line, column))
            }
            b'=' => {
                self.bump();
                Ok(Token::new(TokenKind::Eq, line, column))
            }
            b'<' => {
                self.bump();
                match self.peek() {
                    Some(b'=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::Le, line, column))
                    }
                    Some(b'>') => {
                        self.bump();
                        Ok(Token::new(TokenKind::NotEq, line, column))
                    }
                    _ => Ok(Token::new(TokenKind::Lt, line, column)),
                }
            }
            b'>' => {
                self.bump();
                match self.peek() {
                    Some(b'=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::Ge, line, column))
                    }
                    _ => Ok(Token::new(TokenKind::Gt, line, column)),
                }
            }
            b'!' => {
                self.bump();
                match self.peek() {
                    Some(b'=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::NotEq, line, column))
                    }
                    _ => Err(MiniDbError::lex("expected '=' after '!'", line, column)),
                }
            }
            b'|' => {
                self.bump();
                match self.peek() {
                    Some(b'|') => {
                        self.bump();
                        Ok(Token::new(TokenKind::ConcatOp, line, column))
                    }
                    _ => Err(MiniDbError::lex("expected '|' after '|'", line, column)),
                }
            }
            b'.' => {
                self.bump();
                if let Some(nc) = self.peek() {
                    if nc.is_ascii_digit() {
                        return self.read_number_starting_with_dot(line, column);
                    }
                }
                Ok(Token::new(TokenKind::Dot, line, column))
            }
            b'\'' | b'"' => self.read_string(line, column),
            c if c.is_ascii_digit() => self.read_number(line, column),
            c if c.is_ascii_alphabetic() || c == b'_' => self.read_ident(line, column),
            other => Err(MiniDbError::lex(
                format!("unexpected character '{}'", other as char),
                line,
                column,
            )),
        }
    }

    fn read_number(&mut self, line: usize, column: usize) -> Result<Token> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }
        let mut is_real = false;
        if self.peek() == Some(b'.') {
            is_real = true;
            self.bump();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_real = true;
            self.bump();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.bump();
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| MiniDbError::lex("invalid utf-8", line, column))?;
        let kind = if is_real {
            let r: f64 = s.parse().map_err(|_| {
                MiniDbError::lex(format!("invalid real literal {}", s), line, column)
            })?;
            TokenKind::RealLit(r)
        } else {
            let i: i64 = s.parse().map_err(|_| {
                MiniDbError::lex(format!("invalid int literal {}", s), line, column)
            })?;
            TokenKind::IntLit(i)
        };
        Ok(Token::new(kind, line, column))
    }

    fn read_number_starting_with_dot(&mut self, line: usize, column: usize) -> Result<Token> {
        let start = self.pos - 1; // include the '.' just consumed
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| MiniDbError::lex("invalid utf-8", line, column))?;
        let r: f64 = s
            .parse()
            .map_err(|_| MiniDbError::lex(format!("invalid real {}", s), line, column))?;
        Ok(Token::new(TokenKind::RealLit(r), line, column))
    }

    fn read_string(&mut self, line: usize, column: usize) -> Result<Token> {
        let quote = self.bump().expect("guard");
        let mut buf = String::new();
        loop {
            match self.peek() {
                None => return Err(MiniDbError::lex("unterminated string", line, column)),
                Some(c) if c == quote => {
                    self.bump();
                    if self.peek() == Some(quote) {
                        // SQL escape: doubled quote
                        self.bump();
                        buf.push(quote as char);
                    } else {
                        break;
                    }
                }
                Some(_) => {
                    let c = self.bump().expect("peek ok");
                    buf.push(c as char);
                }
            }
        }
        let kind = if quote == b'"' {
            // Double-quoted identifiers per SQL standard.
            TokenKind::Ident(buf)
        } else {
            TokenKind::StringLit(buf)
        };
        Ok(Token::new(kind, line, column))
    }

    fn read_ident(&mut self, line: usize, column: usize) -> Result<Token> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.bump();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| MiniDbError::lex("invalid utf-8", line, column))?;
        let kind = if let Some(kw) = Keyword::from_ident(s) {
            TokenKind::Keyword(kw)
        } else {
            TokenKind::Ident(s.to_string())
        };
        Ok(Token::new(kind, line, column))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<TokenKind> {
        Lexer::new(input)
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn keywords_and_idents() {
        let toks = kinds("SELECT a FROM t");
        assert_eq!(toks[0], TokenKind::Keyword(Keyword::Select));
        assert_eq!(toks[1], TokenKind::Ident("a".into()));
        assert_eq!(toks[2], TokenKind::Keyword(Keyword::From));
        assert_eq!(toks[3], TokenKind::Ident("t".into()));
    }

    #[test]
    fn numeric_literals() {
        let toks = kinds("1 2.5 .25 1e3");
        assert_eq!(toks[0], TokenKind::IntLit(1));
        assert!(matches!(toks[1], TokenKind::RealLit(_)));
        assert!(matches!(toks[2], TokenKind::RealLit(_)));
        assert!(matches!(toks[3], TokenKind::RealLit(_)));
    }

    #[test]
    fn string_literal_with_escape() {
        let toks = kinds("'it''s'");
        assert_eq!(toks[0], TokenKind::StringLit("it's".into()));
    }

    #[test]
    fn meta_command_dot_then_ident() {
        let toks = kinds(".tables");
        assert_eq!(toks[0], TokenKind::Dot);
        assert_eq!(toks[1], TokenKind::Ident("tables".into()));
    }

    #[test]
    fn operators() {
        let toks = kinds("<= <> != >= ||");
        assert_eq!(toks[0], TokenKind::Le);
        assert_eq!(toks[1], TokenKind::NotEq);
        assert_eq!(toks[2], TokenKind::NotEq);
        assert_eq!(toks[3], TokenKind::Ge);
        assert_eq!(toks[4], TokenKind::ConcatOp);
    }

    #[test]
    fn comments() {
        let toks = kinds("SELECT -- one\n 1 /* two */ ;");
        assert_eq!(toks[0], TokenKind::Keyword(Keyword::Select));
        assert_eq!(toks[1], TokenKind::IntLit(1));
        assert_eq!(toks[2], TokenKind::Semicolon);
    }
}
