use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Integer(i64),
    Float(f64),
    StringLit(String),
    Ident(String),

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
    And,
    Or,
    Is,
    In,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Inner,
    Join,
    Int,
    BigInt,
    Real,
    Text,
    Bool,
    True,
    False,
    If,
    Exists,
    Distinct,

    // Operators
    Eq,     // =
    Neq,    // <>
    Lt,     // <
    Lte,    // <=
    Gt,     // >
    Gte,    // >=
    Plus,   // +
    Minus,  // -
    Star,   // *
    Slash,  // /
    Concat, // ||
    Dot,    // .
    Comma,  // ,
    Semi,   // ;
    LParen, // (
    RParen, // )

    // Meta-commands (dot commands)
    MetaSchema,
    MetaTables,
    MetaIndexes,
    MetaExit,
    MetaFormat,

    Eof,
}

#[derive(Debug, Clone)]
pub struct TokenWithPos {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    at_line_start: bool,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            at_line_start: true,
        }
    }

    pub fn tokenize(input: &str) -> Result<Vec<TokenWithPos>> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            let is_eof = tok.token == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while let Some(c) = self.peek() {
                if c.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }
            // SQL line comment --
            if self.peek() == Some('-') && self.peek2() == Some('-') {
                while let Some(c) = self.advance() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            // Block comment /* ... */
            if self.peek() == Some('/') && self.peek2() == Some('*') {
                self.advance();
                self.advance();
                loop {
                    match self.advance() {
                        None => break,
                        Some('*') if self.peek() == Some('/') => {
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

    fn next_token(&mut self) -> Result<TokenWithPos> {
        let was_at_line_start = self.at_line_start;
        self.skip_whitespace_and_comments();

        let line = self.line;
        let col = self.col;

        // Check for meta-commands only at start of a statement (col == 1 or was_at_line_start)
        if self.peek() == Some('.') && (col == 1 || was_at_line_start) {
            // Peek ahead to see if it looks like a meta-command
            let next = self.input.get(self.pos + 1).copied();
            if next.map(|c| c.is_alphabetic()).unwrap_or(false) {
                self.advance();
                let cmd = self.read_ident_raw();
                let token = match cmd.to_lowercase().as_str() {
                    "schema" => Token::MetaSchema,
                    "tables" => Token::MetaTables,
                    "indexes" | "indices" => Token::MetaIndexes,
                    "exit" | "quit" => Token::MetaExit,
                    "format" => Token::MetaFormat,
                    other => {
                        return Err(MiniDbError::Lex {
                            msg: format!("unknown meta-command '.{}'", other),
                            line,
                            col,
                        })
                    }
                };
                self.at_line_start = false;
                return Ok(TokenWithPos { token, line, col });
            }
        }

        self.at_line_start = false;
        match self.peek() {
            None => Ok(TokenWithPos {
                token: Token::Eof,
                line,
                col,
            }),
            Some(c) => {
                let token = match c {
                    '=' => {
                        self.advance();
                        Token::Eq
                    }
                    '<' => {
                        self.advance();
                        if self.peek() == Some('=') {
                            self.advance();
                            Token::Lte
                        } else if self.peek() == Some('>') {
                            self.advance();
                            Token::Neq
                        } else {
                            Token::Lt
                        }
                    }
                    '>' => {
                        self.advance();
                        if self.peek() == Some('=') {
                            self.advance();
                            Token::Gte
                        } else {
                            Token::Gt
                        }
                    }
                    '!' => {
                        self.advance();
                        if self.peek() == Some('=') {
                            self.advance();
                            Token::Neq
                        } else {
                            return Err(MiniDbError::Lex {
                                msg: "expected '=' after '!'".into(),
                                line,
                                col,
                            });
                        }
                    }
                    '+' => {
                        self.advance();
                        Token::Plus
                    }
                    '-' => {
                        self.advance();
                        Token::Minus
                    }
                    '*' => {
                        self.advance();
                        Token::Star
                    }
                    '/' => {
                        self.advance();
                        Token::Slash
                    }
                    '|' => {
                        self.advance();
                        if self.peek() == Some('|') {
                            self.advance();
                            Token::Concat
                        } else {
                            return Err(MiniDbError::Lex {
                                msg: "expected '||'".into(),
                                line,
                                col,
                            });
                        }
                    }
                    '.' => {
                        self.advance();
                        Token::Dot
                    }
                    ',' => {
                        self.advance();
                        Token::Comma
                    }
                    ';' => {
                        self.advance();
                        self.at_line_start = true;
                        Token::Semi
                    }
                    '(' => {
                        self.advance();
                        Token::LParen
                    }
                    ')' => {
                        self.advance();
                        Token::RParen
                    }
                    '\'' => self.read_string()?,
                    '"' => self.read_quoted_ident()?,
                    '`' => self.read_backtick_ident()?,
                    d if d.is_ascii_digit() => self.read_number()?,
                    a if a.is_alphabetic() || a == '_' => self.read_keyword_or_ident(),
                    other => {
                        return Err(MiniDbError::Lex {
                            msg: format!("unexpected character '{}'", other),
                            line,
                            col,
                        })
                    }
                };
                Ok(TokenWithPos { token, line, col })
            }
        }
    }

    fn read_string(&mut self) -> Result<Token> {
        self.advance(); // consume '
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(MiniDbError::Lex {
                        msg: "unterminated string".into(),
                        line: self.line,
                        col: self.col,
                    })
                }
                Some('\'') => {
                    // escaped '' -> '
                    if self.peek() == Some('\'') {
                        self.advance();
                        s.push('\'');
                    } else {
                        break;
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Ok(Token::StringLit(s))
    }

    fn read_quoted_ident(&mut self) -> Result<Token> {
        self.advance(); // consume "
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(MiniDbError::Lex {
                        msg: "unterminated quoted identifier".into(),
                        line: self.line,
                        col: self.col,
                    })
                }
                Some('"') => break,
                Some(c) => s.push(c),
            }
        }
        Ok(Token::Ident(s))
    }

    fn read_backtick_ident(&mut self) -> Result<Token> {
        self.advance(); // consume `
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(MiniDbError::Lex {
                        msg: "unterminated backtick identifier".into(),
                        line: self.line,
                        col: self.col,
                    })
                }
                Some('`') => break,
                Some(c) => s.push(c),
            }
        }
        Ok(Token::Ident(s))
    }

    fn read_number(&mut self) -> Result<Token> {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if self.peek() == Some('.') && self.peek2().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            s.push('.');
            self.advance();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            let f: f64 = s.parse().map_err(|_| MiniDbError::Lex {
                msg: format!("invalid float '{}'", s),
                line: self.line,
                col: self.col,
            })?;
            Ok(Token::Float(f))
        } else {
            let i: i64 = s.parse().map_err(|_| MiniDbError::Lex {
                msg: format!("invalid integer '{}'", s),
                line: self.line,
                col: self.col,
            })?;
            Ok(Token::Integer(i))
        }
    }

    fn read_ident_raw(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_keyword_or_ident(&mut self) -> Token {
        let s = self.read_ident_raw();
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
            "BEGIN" => Token::Begin,
            "COMMIT" => Token::Commit,
            "ROLLBACK" => Token::Rollback,
            "AND" => Token::And,
            "OR" => Token::Or,
            "IS" => Token::Is,
            "IN" => Token::In,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "LIMIT" => Token::Limit,
            "OFFSET" => Token::Offset,
            "INNER" => Token::Inner,
            "JOIN" => Token::Join,
            "INT" => Token::Int,
            "INTEGER" => Token::Int,
            "BIGINT" => Token::BigInt,
            "REAL" | "FLOAT" | "DOUBLE" => Token::Real,
            "TEXT" | "VARCHAR" | "CHAR" | "STRING" => Token::Text,
            "BOOL" | "BOOLEAN" => Token::Bool,
            "TRUE" => Token::True,
            "FALSE" => Token::False,
            "IF" => Token::If,
            "EXISTS" => Token::Exists,
            "DISTINCT" => Token::Distinct,
            "TRANSACTION" => Token::Begin, // BEGIN TRANSACTION
            _ => Token::Ident(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token> {
        Lexer::tokenize(input)
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn test_basic_select() {
        let t = tokens("SELECT * FROM t WHERE id = 1;");
        assert!(t.contains(&Token::Select));
        assert!(t.contains(&Token::Star));
        assert!(t.contains(&Token::From));
        assert!(t.contains(&Token::Where));
        assert!(t.contains(&Token::Eq));
        assert!(t.contains(&Token::Integer(1)));
        assert!(t.contains(&Token::Semi));
    }

    #[test]
    fn test_string_literal() {
        let t = tokens("'hello world'");
        assert_eq!(t[0], Token::StringLit("hello world".into()));
    }

    #[test]
    fn test_comment_skip() {
        let t = tokens("SELECT -- comment\n 1");
        assert_eq!(t[0], Token::Select);
        assert_eq!(t[1], Token::Integer(1));
    }

    #[test]
    fn test_operators() {
        let t = tokens("<> <= >= != ||");
        assert_eq!(t[0], Token::Neq);
        assert_eq!(t[1], Token::Lte);
        assert_eq!(t[2], Token::Gte);
        assert_eq!(t[3], Token::Neq);
        assert_eq!(t[4], Token::Concat);
    }

    #[test]
    fn test_float() {
        let t = tokens("3.14");
        assert_eq!(t[0], Token::Float(3.14));
    }
}
