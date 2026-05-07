use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
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
    Primary,
    Key,
    Not,
    Null,
    Unique,
    Default,
    Check,
    Begin,
    Commit,
    Rollback,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    And,
    Or,
    Is,
    In,
    Inner,
    Join,
    If,
    Exists,
    Int,
    BigInt,
    Real,
    Text,
    Bool,
    True,
    False,
    Distinct,
    As,
    Coalesce,
    Length,
    Lower,
    Upper,
    Substr,
    Abs,
    Like,
    // Symbols
    Star,
    Comma,
    Semicolon,
    LParen,
    RParen,
    Dot,
    // Operators
    Eq,     // =
    Ne,     // <>
    Lt,     // <
    Le,     // <=
    Gt,     // >
    Ge,     // >=
    Plus,   // +
    Minus,  // -
    Slash,  // /
    Concat, // ||
    // Literals
    IntLiteral(i64),
    RealLiteral(f64),
    StringLiteral(String),
    // Identifiers
    Ident(String),
    // Meta-commands
    MetaCommand(String),
    // End
    Eof,
}

#[derive(Debug, Clone)]
pub struct TokenWithPos {
    pub token: Token,
    pub line: u32,
    pub col: u32,
}

pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
    /// True if we haven't emitted a significant token since start or last semicolon.
    at_stmt_start: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            at_stmt_start: true,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if ch == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while self
                .peek()
                .map(|c| c.is_ascii_whitespace())
                .unwrap_or(false)
            {
                self.advance();
            }
            // Single-line comment
            if self.peek() == Some(b'-') && self.peek2() == Some(b'-') {
                while self.peek().map(|c| c != b'\n').unwrap_or(false) {
                    self.advance();
                }
                continue;
            }
            // Block comment /* ... */
            if self.peek() == Some(b'/') && self.peek2() == Some(b'*') {
                self.advance();
                self.advance();
                loop {
                    match self.advance() {
                        None => break,
                        Some(b'*') if self.peek() == Some(b'/') => {
                            self.advance();
                            break;
                        }
                        _ => {}
                    }
                }
                continue;
            }
            break;
        }
    }

    fn read_string(&mut self) -> Result<String> {
        // consume opening '
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(MiniDbError::Parse {
                        msg: "unterminated string literal".into(),
                        line: self.line,
                        col: self.col,
                    })
                }
                Some(b'\'') => {
                    // escaped ''?
                    if self.peek() == Some(b'\'') {
                        self.advance();
                        s.push('\'');
                    } else {
                        break;
                    }
                }
                Some(ch) => s.push(ch as char),
            }
        }
        Ok(s)
    }

    fn read_ident_or_keyword(&mut self, first: u8) -> Token {
        let mut s = String::new();
        s.push(first as char);
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                s.push(c as char);
                self.advance();
            } else {
                break;
            }
        }
        keyword_or_ident(s)
    }

    fn read_number(&mut self, first: u8) -> Result<Token> {
        let mut s = String::new();
        s.push(first as char);
        let mut is_real = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c as char);
                self.advance();
            } else if c == b'.' && !is_real {
                is_real = true;
                s.push('.');
                self.advance();
            } else if c == b'e' || c == b'E' {
                is_real = true;
                s.push(c as char);
                self.advance();
                if self.peek() == Some(b'+') || self.peek() == Some(b'-') {
                    s.push(self.advance().unwrap() as char);
                }
            } else {
                break;
            }
        }

        if is_real {
            s.parse::<f64>()
                .map(Token::RealLiteral)
                .map_err(|_| MiniDbError::Parse {
                    msg: format!("invalid number: {}", s),
                    line: self.line,
                    col: self.col,
                })
        } else {
            s.parse::<i64>()
                .map(Token::IntLiteral)
                .map_err(|e| MiniDbError::Parse {
                    msg: format!("invalid integer '{}': {}", s, e),
                    line: self.line,
                    col: self.col,
                })
        }
    }

    fn next_token_inner(&mut self) -> Result<TokenWithPos> {
        self.skip_whitespace_and_comments();

        let line = self.line;
        let col = self.col;

        let ch = match self.advance() {
            None => {
                return Ok(TokenWithPos {
                    token: Token::Eof,
                    line,
                    col,
                })
            }
            Some(c) => c,
        };

        let token = match ch {
            b'.' if self.at_stmt_start
                && self
                    .peek()
                    .map(|c| c.is_ascii_alphabetic())
                    .unwrap_or(false) =>
            {
                // meta-command: only at statement start (.tables, .schema, etc.)
                let mut cmd = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == b'_' {
                        cmd.push(c as char);
                        self.advance();
                    } else {
                        break;
                    }
                }
                Token::MetaCommand(cmd)
            }
            b'.' => Token::Dot,
            b',' => Token::Comma,
            b';' => Token::Semicolon,
            b'(' => Token::LParen,
            b')' => Token::RParen,
            b'*' => Token::Star,
            b'+' => Token::Plus,
            b'-' => Token::Minus,
            b'/' => Token::Slash,
            b'=' => Token::Eq,
            b'<' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    Token::Le
                } else if self.peek() == Some(b'>') {
                    self.advance();
                    Token::Ne
                } else {
                    Token::Lt
                }
            }
            b'>' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    Token::Ge
                } else {
                    Token::Gt
                }
            }
            b'!' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    Token::Ne
                } else {
                    return Err(MiniDbError::Parse {
                        msg: "expected != after !".into(),
                        line,
                        col,
                    });
                }
            }
            b'|' => {
                if self.peek() == Some(b'|') {
                    self.advance();
                    Token::Concat
                } else {
                    return Err(MiniDbError::Parse {
                        msg: "expected || for concat".into(),
                        line,
                        col,
                    });
                }
            }
            b'\'' => {
                let s = self.read_string()?;
                Token::StringLiteral(s)
            }
            b'"' => {
                // quoted identifier
                let mut s = String::new();
                loop {
                    match self.advance() {
                        None => {
                            return Err(MiniDbError::Parse {
                                msg: "unterminated quoted identifier".into(),
                                line,
                                col,
                            })
                        }
                        Some(b'"') => break,
                        Some(c) => s.push(c as char),
                    }
                }
                Token::Ident(s)
            }
            c if c.is_ascii_alphabetic() || c == b'_' => self.read_ident_or_keyword(c),
            c if c.is_ascii_digit() => self.read_number(c)?,
            other => {
                return Err(MiniDbError::Parse {
                    msg: format!("unexpected character: {:?}", other as char),
                    line,
                    col,
                })
            }
        };

        // Track statement start state
        match &token {
            Token::Semicolon => self.at_stmt_start = true,
            Token::MetaCommand(_) => self.at_stmt_start = false,
            _ => {
                // Any other non-whitespace token means we're no longer at stmt start
                self.at_stmt_start = false;
            }
        }

        Ok(TokenWithPos { token, line, col })
    }

    pub fn tokenize(&mut self) -> Result<Vec<TokenWithPos>> {
        let mut tokens = Vec::new();
        loop {
            let t = self.next_token_inner()?;
            let eof = t.token == Token::Eof;
            tokens.push(t);
            if eof {
                break;
            }
        }
        Ok(tokens)
    }
}

fn keyword_or_ident(s: String) -> Token {
    match s.to_uppercase().as_str() {
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
        "PRIMARY" => Token::Primary,
        "KEY" => Token::Key,
        "NOT" => Token::Not,
        "NULL" => Token::Null,
        "UNIQUE" => Token::Unique,
        "DEFAULT" => Token::Default,
        "CHECK" => Token::Check,
        "BEGIN" | "START" => Token::Begin,
        "COMMIT" => Token::Commit,
        "ROLLBACK" => Token::Rollback,
        "ORDER" => Token::Order,
        "BY" => Token::By,
        "ASC" => Token::Asc,
        "DESC" => Token::Desc,
        "LIMIT" => Token::Limit,
        "OFFSET" => Token::Offset,
        "AND" => Token::And,
        "OR" => Token::Or,
        "IS" => Token::Is,
        "IN" => Token::In,
        "INNER" => Token::Inner,
        "JOIN" => Token::Join,
        "IF" => Token::If,
        "EXISTS" => Token::Exists,
        "INT" | "INTEGER" => Token::Int,
        "BIGINT" => Token::BigInt,
        "REAL" | "FLOAT" | "DOUBLE" => Token::Real,
        "TEXT" | "VARCHAR" | "CHAR" | "STRING" => Token::Text,
        "BOOL" | "BOOLEAN" => Token::Bool,
        "TRUE" => Token::True,
        "FALSE" => Token::False,
        "DISTINCT" => Token::Distinct,
        "AS" => Token::As,
        "COALESCE" => Token::Coalesce,
        "LENGTH" => Token::Length,
        "LOWER" => Token::Lower,
        "UPPER" => Token::Upper,
        "SUBSTR" | "SUBSTRING" => Token::Substr,
        "ABS" => Token::Abs,
        "LIKE" => Token::Like,
        "TRANSACTION" => Token::Begin, // BEGIN TRANSACTION
        _ => Token::Ident(s),
    }
}

pub fn tokenize(input: &str) -> Result<Vec<TokenWithPos>> {
    Lexer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let tokens = tokenize("SELECT * FROM t WHERE id = 5;").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.token).collect();
        assert!(kinds.contains(&&Token::Select));
        assert!(kinds.contains(&&Token::Star));
        assert!(kinds.contains(&&Token::From));
        assert!(kinds.contains(&&Token::Where));
        assert!(kinds.contains(&&Token::Eq));
        assert!(kinds.contains(&&Token::Semicolon));
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("'hello world'").unwrap();
        assert_eq!(tokens[0].token, Token::StringLiteral("hello world".into()));
    }

    #[test]
    fn test_number_literals() {
        let tokens = tokenize("42 3.14 -1").unwrap();
        assert_eq!(tokens[0].token, Token::IntLiteral(42));
        if let Token::RealLiteral(r) = tokens[1].token {
            assert!((r - 3.14).abs() < 1e-10);
        } else {
            panic!("expected real");
        }
    }

    #[test]
    fn test_meta_command() {
        let tokens = tokenize(".tables").unwrap();
        assert_eq!(tokens[0].token, Token::MetaCommand("tables".into()));
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("<= >= <> !=").unwrap();
        assert_eq!(tokens[0].token, Token::Le);
        assert_eq!(tokens[1].token, Token::Ge);
        assert_eq!(tokens[2].token, Token::Ne);
        assert_eq!(tokens[3].token, Token::Ne);
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("SELECT -- comment\n 1").unwrap();
        assert_eq!(tokens[0].token, Token::Select);
        assert_eq!(tokens[1].token, Token::IntLiteral(1));
    }
}
