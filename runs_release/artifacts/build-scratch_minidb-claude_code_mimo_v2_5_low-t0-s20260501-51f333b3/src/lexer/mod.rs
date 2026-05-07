use crate::error::{MiniDbError, Result};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Select,
    From,
    Where,
    And,
    Or,
    Not,
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
    Begin,
    Commit,
    Rollback,
    Primary,
    Key,
    NotNull,
    Unique,
    Default,
    Check,
    Asc,
    Desc,
    Limit,
    Offset,
    Inner,
    Join,
    Is,
    Order,
    By,
    As,
    Null,
    True,
    False,
    Int,
    Bigint,
    Real,
    Text,
    Bool,

    // Literals
    IntegerLiteral(i64),
    RealLiteral(f64),
    StringLiteral(String),

    // Identifiers
    Identifier(String),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Dot,
    Comma,
    Semicolon,
    LParen,
    RParen,

    // Meta commands
    DotSchema,
    DotTables,
    DotIndexes,
    DotExit,
    DotFormat,

    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Select => write!(f, "SELECT"),
            Token::From => write!(f, "FROM"),
            Token::Where => write!(f, "WHERE"),
            Token::And => write!(f, "AND"),
            Token::Or => write!(f, "OR"),
            Token::Not => write!(f, "NOT"),
            Token::Insert => write!(f, "INSERT"),
            Token::Into => write!(f, "INTO"),
            Token::Values => write!(f, "VALUES"),
            Token::Update => write!(f, "UPDATE"),
            Token::Set => write!(f, "SET"),
            Token::Delete => write!(f, "DELETE"),
            Token::Create => write!(f, "CREATE"),
            Token::Table => write!(f, "TABLE"),
            Token::Drop => write!(f, "DROP"),
            Token::Index => write!(f, "INDEX"),
            Token::On => write!(f, "ON"),
            Token::Begin => write!(f, "BEGIN"),
            Token::Commit => write!(f, "COMMIT"),
            Token::Rollback => write!(f, "ROLLBACK"),
            Token::Primary => write!(f, "PRIMARY"),
            Token::Key => write!(f, "KEY"),
            Token::NotNull => write!(f, "NOT NULL"),
            Token::Unique => write!(f, "UNIQUE"),
            Token::Default => write!(f, "DEFAULT"),
            Token::Check => write!(f, "CHECK"),
            Token::Asc => write!(f, "ASC"),
            Token::Desc => write!(f, "DESC"),
            Token::Limit => write!(f, "LIMIT"),
            Token::Offset => write!(f, "OFFSET"),
            Token::Inner => write!(f, "INNER"),
            Token::Join => write!(f, "JOIN"),
            Token::Is => write!(f, "IS"),
            Token::Order => write!(f, "ORDER"),
            Token::By => write!(f, "BY"),
            Token::As => write!(f, "AS"),
            Token::Null => write!(f, "NULL"),
            Token::True => write!(f, "TRUE"),
            Token::False => write!(f, "FALSE"),
            Token::Int => write!(f, "INT"),
            Token::Bigint => write!(f, "BIGINT"),
            Token::Real => write!(f, "REAL"),
            Token::Text => write!(f, "TEXT"),
            Token::Bool => write!(f, "BOOL"),
            Token::IntegerLiteral(v) => write!(f, "{}", v),
            Token::RealLiteral(v) => write!(f, "{}", v),
            Token::StringLiteral(v) => write!(f, "'{}'", v),
            Token::Identifier(v) => write!(f, "{}", v),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Eq => write!(f, "="),
            Token::Ne => write!(f, "<>"),
            Token::Lt => write!(f, "<"),
            Token::Le => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::Ge => write!(f, ">="),
            Token::Concat => write!(f, "||"),
            Token::Dot => write!(f, "."),
            Token::Comma => write!(f, ","),
            Token::Semicolon => write!(f, ";"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::DotSchema => write!(f, ".schema"),
            Token::DotTables => write!(f, ".tables"),
            Token::DotIndexes => write!(f, ".indexes"),
            Token::DotExit => write!(f, ".exit"),
            Token::DotFormat => write!(f, ".format"),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenWithPos {
    pub token: Token,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else if ch == '-'
                && self.pos + 1 < self.input.len()
                && self.input[self.pos + 1] == '-'
            {
                // Line comment
                while let Some(c) = self.advance() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self, quote: char) -> Result<String> {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(ch) if ch == quote => {
                    // Check for escaped quote
                    if self.peek() == Some(quote) {
                        s.push(quote);
                        self.advance();
                    } else {
                        return Ok(s);
                    }
                }
                Some(ch) => s.push(ch),
                None => {
                    return Err(MiniDbError::ParseError {
                        message: "unterminated string".to_string(),
                        line: self.line,
                        column: self.column,
                    })
                }
            }
        }
    }

    fn read_number(&mut self, first: char) -> Result<Token> {
        let mut s = String::new();
        s.push(first);
        // Advance past the first character since peek() doesn't consume
        self.advance();
        let mut is_real = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !is_real {
                is_real = true;
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if is_real {
            let v: f64 = s.parse().map_err(|_| MiniDbError::ParseError {
                message: format!("invalid real literal: {}", s),
                line: self.line,
                column: self.column,
            })?;
            Ok(Token::RealLiteral(v))
        } else {
            let v: i64 = s.parse().map_err(|_| MiniDbError::ParseError {
                message: format!("invalid integer literal: {}", s),
                line: self.line,
                column: self.column,
            })?;
            Ok(Token::IntegerLiteral(v))
        }
    }

    fn read_identifier(&mut self, first: char) -> String {
        let mut s = String::new();
        s.push(first);
        // Advance past the first character since peek() doesn't consume
        self.advance();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn keyword_or_identifier(s: &str) -> Token {
        match s.to_uppercase().as_str() {
            "SELECT" => Token::Select,
            "FROM" => Token::From,
            "WHERE" => Token::Where,
            "AND" => Token::And,
            "OR" => Token::Or,
            "NOT" => Token::Not,
            "INSERT" => Token::Insert,
            "INTO" => Token::Into,
            "VALUES" => Token::Values,
            "UPDATE" => Token::Update,
            "SET" => Token::Set,
            "DELETE" => Token::Delete,
            "CREATE" => Token::Create,
            "TABLE" => Token::Table,
            "DROP" => Token::Drop,
            "INDEX" => Token::Index,
            "ON" => Token::On,
            "BEGIN" => Token::Begin,
            "COMMIT" => Token::Commit,
            "ROLLBACK" => Token::Rollback,
            "PRIMARY" => Token::Primary,
            "KEY" => Token::Key,
            "NULL" => Token::Null,
            "UNIQUE" => Token::Unique,
            "DEFAULT" => Token::Default,
            "CHECK" => Token::Check,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "LIMIT" => Token::Limit,
            "OFFSET" => Token::Offset,
            "INNER" => Token::Inner,
            "JOIN" => Token::Join,
            "IS" => Token::Is,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "AS" => Token::As,
            "TRUE" => Token::True,
            "FALSE" => Token::False,
            "INT" | "INTEGER" => Token::Int,
            "BIGINT" => Token::Bigint,
            "REAL" | "FLOAT" | "DOUBLE" => Token::Real,
            "TEXT" | "VARCHAR" | "STRING" => Token::Text,
            "BOOL" | "BOOLEAN" => Token::Bool,
            _ => Token::Identifier(s.to_string()),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<TokenWithPos>> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();

            let start_line = self.line;
            let start_col = self.column;

            let token = match self.peek() {
                None => Token::Eof,
                Some(ch) => {
                    if ch.is_alphabetic() || ch == '_' {
                        let word = self.read_identifier(ch);
                        // Check for NOT NULL (two-word keyword)
                        if word.to_uppercase() == "NOT" {
                            self.skip_whitespace();
                            if let Some('N') = self.peek() {
                                // Peek ahead for NULL
                                let saved_pos = self.pos;
                                let saved_line = self.line;
                                let saved_col = self.column;
                                let next_word = self.read_identifier('N');
                                if next_word.to_uppercase() == "NULL" {
                                    Token::NotNull
                                } else {
                                    self.pos = saved_pos;
                                    self.line = saved_line;
                                    self.column = saved_col;
                                    Self::keyword_or_identifier(&word)
                                }
                            } else {
                                Self::keyword_or_identifier(&word)
                            }
                        } else {
                            Self::keyword_or_identifier(&word)
                        }
                    } else if ch.is_ascii_digit() {
                        self.read_number(ch)?
                    } else if ch == '\'' || ch == '"' {
                        self.advance();
                        let s = self.read_string(ch)?;
                        Token::StringLiteral(s)
                    } else if ch == '.' {
                        self.advance();
                        // Check for meta command
                        if let Some(c) = self.peek() {
                            if c.is_alphabetic() {
                                let cmd = self.read_identifier(c);
                                match cmd.to_lowercase().as_str() {
                                    "schema" => Token::DotSchema,
                                    "tables" => Token::DotTables,
                                    "indexes" => Token::DotIndexes,
                                    "exit" => Token::DotExit,
                                    "format" => Token::DotFormat,
                                    _ => {
                                        return Err(MiniDbError::ParseError {
                                            message: format!("unknown meta command: .{}", cmd),
                                            line: start_line,
                                            column: start_col,
                                        });
                                    }
                                }
                            } else {
                                Token::Dot
                            }
                        } else {
                            Token::Dot
                        }
                    } else {
                        self.advance();
                        match ch {
                            '+' => Token::Plus,
                            '-' => Token::Minus,
                            '*' => Token::Star,
                            '/' => Token::Slash,
                            '=' => Token::Eq,
                            '<' => {
                                if self.peek() == Some('>') {
                                    self.advance();
                                    Token::Ne
                                } else if self.peek() == Some('=') {
                                    self.advance();
                                    Token::Le
                                } else {
                                    Token::Lt
                                }
                            }
                            '>' => {
                                if self.peek() == Some('=') {
                                    self.advance();
                                    Token::Ge
                                } else {
                                    Token::Gt
                                }
                            }
                            '|' => {
                                if self.peek() == Some('|') {
                                    self.advance();
                                    Token::Concat
                                } else {
                                    return Err(MiniDbError::ParseError {
                                        message: "unexpected character: |".to_string(),
                                        line: start_line,
                                        column: start_col,
                                    });
                                }
                            }
                            ',' => Token::Comma,
                            ';' => Token::Semicolon,
                            '(' => Token::LParen,
                            ')' => Token::RParen,
                            _ => {
                                return Err(MiniDbError::ParseError {
                                    message: format!("unexpected character: {}", ch),
                                    line: start_line,
                                    column: start_col,
                                });
                            }
                        }
                    }
                }
            };

            tokens.push(TokenWithPos {
                token,
                line: start_line,
                column: start_col,
            });

            if matches!(tokens.last().map(|t| &t.token), Some(Token::Eof)) {
                break;
            }
        }

        Ok(tokens)
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
        let tokens = tokenize("SELECT * FROM users WHERE id = 1;").unwrap();
        let types: Vec<_> = tokens.iter().map(|t| &t.token).collect();
        assert_eq!(
            types,
            vec![
                &Token::Select,
                &Token::Star,
                &Token::From,
                &Token::Identifier("users".to_string()),
                &Token::Where,
                &Token::Identifier("id".to_string()),
                &Token::Eq,
                &Token::IntegerLiteral(1),
                &Token::Semicolon,
                &Token::Eof,
            ]
        );
    }

    #[test]
    fn test_string_literals() {
        let tokens = tokenize("'hello' \"world\"").unwrap();
        assert_eq!(tokens[0].token, Token::StringLiteral("hello".to_string()));
        assert_eq!(tokens[1].token, Token::StringLiteral("world".to_string()));
    }

    #[test]
    fn test_meta_commands() {
        let tokens = tokenize(".schema .tables .exit").unwrap();
        assert_eq!(tokens[0].token, Token::DotSchema);
        assert_eq!(tokens[1].token, Token::DotTables);
        assert_eq!(tokens[2].token, Token::DotExit);
    }

    #[test]
    fn test_not_null() {
        let tokens = tokenize("NOT NULL").unwrap();
        assert_eq!(tokens[0].token, Token::NotNull);
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("<> <= >= ||").unwrap();
        assert_eq!(tokens[0].token, Token::Ne);
        assert_eq!(tokens[1].token, Token::Le);
        assert_eq!(tokens[2].token, Token::Ge);
        assert_eq!(tokens[3].token, Token::Concat);
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("SELECT -- comment\n1").unwrap();
        let types: Vec<_> = tokens.iter().map(|t| &t.token).collect();
        assert_eq!(
            types,
            vec![&Token::Select, &Token::IntegerLiteral(1), &Token::Eof]
        );
    }
}
