use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLiteral(i64),
    RealLiteral(f64),
    StringLiteral(String),
    BoolLiteral(bool),
    Null,

    // Identifiers
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
    Table,
    Drop,
    Index,
    On,
    Primary,
    Key,
    Not,
    Null_kw, // NULL keyword (distinct from Null token for context)
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
    Like,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Join,
    Inner,
    Outer,
    Left,
    Right,
    Cross,
    As,
    Distinct,
    All,
    Int,
    BigInt,
    Real,
    Text,
    Bool,

    // Operators
    Eq,     // =
    Neq,    // <>
    Lt,     // <
    Le,     // <=
    Gt,     // >
    Ge,     // >=
    Plus,   // +
    Minus,  // -
    Star,   // *
    Slash,  // /
    Concat, // ||
    Dot,    // .

    // Punctuation
    LParen,    // (
    RParen,    // )
    Comma,     // ,
    Semicolon, // ;

    // Meta commands
    MetaSchema,  // .schema
    MetaTables,  // .tables
    MetaIndexes, // .indexes
    MetaExit,    // .exit
    MetaFormat,  // .format

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
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<TokenWithPos>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
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

    fn peek_ahead(&self, offset: usize) -> Option<char> {
        self.input.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.input.get(self.pos).copied();
        if let Some(ch) = c {
            self.pos += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else if c == '-' && self.peek_ahead(1) == Some('-') {
                // Line comment
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.advance();
                }
            } else if c == '/' && self.peek_ahead(1) == Some('*') {
                // Block comment
                self.advance(); // /
                self.advance(); // *
                loop {
                    match self.peek() {
                        None => break,
                        Some('*') if self.peek_ahead(1) == Some('/') => {
                            self.advance();
                            self.advance();
                            break;
                        }
                        _ => {
                            self.advance();
                        }
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<TokenWithPos> {
        self.skip_whitespace();

        let line = self.line;
        let col = self.col;

        let c = match self.peek() {
            None => {
                return Ok(TokenWithPos {
                    token: Token::Eof,
                    line,
                    col,
                })
            }
            Some(c) => c,
        };

        // Meta commands
        if c == '.' {
            self.advance();
            let mut name = String::new();
            while let Some(c) = self.peek() {
                if c.is_alphanumeric() || c == '_' {
                    name.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            let token = match name.to_lowercase().as_str() {
                "schema" => Token::MetaSchema,
                "tables" => Token::MetaTables,
                "indexes" => Token::MetaIndexes,
                "exit" => Token::MetaExit,
                "format" => Token::MetaFormat,
                _ => Token::Dot,
            };
            // For plain dot (not a meta command), we need to put the name back
            if token == Token::Dot {
                // We consumed the name as an identifier after dot
                // This is table.col syntax - return Dot and let the caller handle the rest
                // Actually, we need to un-advance the name chars
                // Let's just set pos back
                self.pos -= name.len();
                // Also fix col (approximate)
                self.col -= name.len();
            }
            return Ok(TokenWithPos { token, line, col });
        }

        if c == '\'' || c == '"' {
            return self.read_string(line, col);
        }

        if c.is_ascii_digit()
            || (c == '-' && self.peek_ahead(1).map_or(false, |x| x.is_ascii_digit()))
        {
            return self.read_number(line, col);
        }

        if c.is_alphabetic() || c == '_' {
            return self.read_identifier_or_keyword(line, col);
        }

        // Operators and punctuation
        self.advance();
        let token = match c {
            '=' => Token::Eq,
            '<' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Neq
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
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    Token::Concat
                } else {
                    return Err(MiniDbError::lex(
                        format!("unexpected char '{}'", c),
                        line,
                        col,
                    ));
                }
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            _ => {
                return Err(MiniDbError::lex(
                    format!("unexpected character '{}'", c),
                    line,
                    col,
                ))
            }
        };

        Ok(TokenWithPos { token, line, col })
    }

    fn read_string(&mut self, line: usize, col: usize) -> Result<TokenWithPos> {
        let quote = self.advance().unwrap();
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err(MiniDbError::lex("unterminated string literal", line, col)),
                Some(c) if c == quote => {
                    // Check for escaped quote (doubled)
                    if self.peek() == Some(quote) {
                        self.advance();
                        s.push(quote);
                    } else {
                        break;
                    }
                }
                Some('\\') if quote == '"' => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some('\'') => s.push('\''),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => {
                        return Err(MiniDbError::lex("unterminated escape in string", line, col))
                    }
                },
                Some(c) => s.push(c),
            }
        }
        Ok(TokenWithPos {
            token: Token::StringLiteral(s),
            line,
            col,
        })
    }

    fn read_number(&mut self, line: usize, col: usize) -> Result<TokenWithPos> {
        let mut s = String::new();
        let mut is_real = false;

        if self.peek() == Some('-') {
            s.push('-');
            self.advance();
        }

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else if c == '.' && !is_real {
                is_real = true;
                s.push(c);
                self.advance();
            } else if (c == 'e' || c == 'E') && !s.is_empty() {
                is_real = true;
                s.push(c);
                self.advance();
                if let Some('+') | Some('-') = self.peek() {
                    s.push(self.advance().unwrap());
                }
            } else {
                break;
            }
        }

        if is_real {
            let r: f64 = s
                .parse()
                .map_err(|_| MiniDbError::lex(format!("invalid real: {}", s), line, col))?;
            Ok(TokenWithPos {
                token: Token::RealLiteral(r),
                line,
                col,
            })
        } else {
            let i: i64 = s
                .parse()
                .map_err(|_| MiniDbError::lex(format!("invalid integer: {}", s), line, col))?;
            Ok(TokenWithPos {
                token: Token::IntLiteral(i),
                line,
                col,
            })
        }
    }

    fn read_identifier_or_keyword(&mut self, line: usize, col: usize) -> Result<TokenWithPos> {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        let token = match s.to_uppercase().as_str() {
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
            "TABLE" => Token::Table,
            "DROP" => Token::Drop,
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
            "LIKE" => Token::Like,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "LIMIT" => Token::Limit,
            "OFFSET" => Token::Offset,
            "JOIN" => Token::Join,
            "INNER" => Token::Inner,
            "OUTER" => Token::Outer,
            "LEFT" => Token::Left,
            "RIGHT" => Token::Right,
            "CROSS" => Token::Cross,
            "AS" => Token::As,
            "DISTINCT" => Token::Distinct,
            "ALL" => Token::All,
            "INT" => Token::Int,
            "INTEGER" => Token::Int,
            "BIGINT" => Token::BigInt,
            "REAL" => Token::Real,
            "FLOAT" => Token::Real,
            "DOUBLE" => Token::Real,
            "TEXT" => Token::Text,
            "VARCHAR" => Token::Text,
            "CHAR" => Token::Text,
            "STRING" => Token::Text,
            "BOOL" => Token::Bool,
            "BOOLEAN" => Token::Bool,
            "TRUE" => Token::BoolLiteral(true),
            "FALSE" => Token::BoolLiteral(false),
            _ => Token::Identifier(s),
        };

        Ok(TokenWithPos { token, line, col })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(s: &str) -> Vec<Token> {
        Lexer::new(s)
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn test_basic_tokens() {
        let tokens = tokenize("SELECT * FROM t WHERE id = 1;");
        assert!(matches!(tokens[0], Token::Select));
        assert!(matches!(tokens[1], Token::Star));
        assert!(matches!(tokens[2], Token::From));
        assert!(matches!(tokens[3], Token::Identifier(_)));
        assert!(matches!(tokens[4], Token::Where));
        assert!(matches!(tokens[5], Token::Identifier(_)));
        assert!(matches!(tokens[6], Token::Eq));
        assert!(matches!(tokens[7], Token::IntLiteral(1)));
        assert!(matches!(tokens[8], Token::Semicolon));
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("'hello world'");
        assert!(matches!(&tokens[0], Token::StringLiteral(s) if s == "hello world"));
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("<> <= >= < >");
        assert!(matches!(tokens[0], Token::Neq));
        assert!(matches!(tokens[1], Token::Le));
        assert!(matches!(tokens[2], Token::Ge));
        assert!(matches!(tokens[3], Token::Lt));
        assert!(matches!(tokens[4], Token::Gt));
    }

    #[test]
    fn test_concat() {
        let tokens = tokenize("a || b");
        assert!(matches!(tokens[1], Token::Concat));
    }

    #[test]
    fn test_meta_commands() {
        let tokens = tokenize(".schema");
        assert!(matches!(tokens[0], Token::MetaSchema));
        let tokens = tokenize(".tables");
        assert!(matches!(tokens[0], Token::MetaTables));
        let tokens = tokenize(".exit");
        assert!(matches!(tokens[0], Token::MetaExit));
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("SELECT -- this is a comment\n1");
        assert!(matches!(tokens[0], Token::Select));
        assert!(matches!(tokens[1], Token::IntLiteral(1)));
    }

    #[test]
    fn test_real_literal() {
        let tokens = tokenize("3.14 2.0e10");
        assert!(matches!(tokens[0], Token::RealLiteral(_)));
        assert!(matches!(tokens[1], Token::RealLiteral(_)));
    }
}
