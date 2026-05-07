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
    Table,
    Drop,
    Index,
    On,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Inner,
    Join,
    And,
    Or,
    Not,
    Null,
    Is,
    Primary,
    Key,
    Unique,
    Default,
    Check,
    Begin,
    Commit,
    Rollback,
    True,
    False,
    If,
    Exists,
    Int,
    BigInt,
    Real,
    Text,
    Bool,
    Boolean,
    Varchar,
    Coalesce,
    Length,
    Lower,
    Upper,
    Substr,
    Abs,
    In,
    Like,
    Between,
    Case,
    When,
    Then,
    Else,
    End,
    As,
    Distinct,
    All,
    Group,
    Having,
    Count,
    Sum,
    Avg,
    Min,
    Max,

    // Literals
    IntLit(i64),
    RealLit(f64),
    StringLit(String),

    // Identifiers
    Ident(String),

    // Operators
    Eq,      // =
    Ne,      // <>  !=
    Lt,      // <
    Le,      // <=
    Gt,      // >
    Ge,      // >=
    Plus,    // +
    Minus,   // -
    Star,    // *
    Slash,   // /
    Concat,  // ||
    Percent, // %

    // Punctuation
    LParen,
    RParen,
    Comma,
    Semicolon,
    Dot,

    // Meta commands
    MetaSchema,
    MetaTables,
    MetaIndexes,
    MetaExit,
    MetaFormat,

    // Special
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
        // Post-process: convert Dot+Ident sequences at statement start into meta command tokens
        Ok(postprocess_meta_commands(tokens))
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
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

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while self.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
                self.advance();
            }
            // Single-line comment --
            if self.peek() == Some('-') && self.peek2() == Some('-') {
                while self.peek().map(|c| c != '\n').unwrap_or(false) {
                    self.advance();
                }
                continue;
            }
            // Block comment /* */
            if self.peek() == Some('/') && self.peek2() == Some('*') {
                self.advance();
                self.advance();
                loop {
                    if self.pos >= self.input.len() {
                        break;
                    }
                    if self.peek() == Some('*') && self.peek2() == Some('/') {
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

    fn next_token(&mut self) -> Result<TokenWithPos> {
        self.skip_whitespace_and_comments();

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

        // Dot: could be table.column separator
        if c == '.' {
            self.advance();
            return Ok(TokenWithPos {
                token: Token::Dot,
                line,
                col,
            });
        }

        // String literals
        if c == '\'' {
            return self.read_string(line, col);
        }

        // Numbers
        if c.is_ascii_digit()
            || (c == '-' && self.peek2().map(|c| c.is_ascii_digit()).unwrap_or(false))
        {
            return self.read_number(line, col);
        }

        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' {
            let ident = self.read_ident_chars();
            let token = keyword_or_ident(&ident);
            return Ok(TokenWithPos { token, line, col });
        }

        self.advance();

        let token = match c {
            '=' => Token::Eq,
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Le
                } else if self.peek() == Some('>') {
                    self.advance();
                    Token::Ne
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
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Ne
                } else {
                    return Err(MiniDbError::Parse {
                        line,
                        col,
                        msg: "unexpected '!'".to_string(),
                    });
                }
            }
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    Token::Concat
                } else {
                    return Err(MiniDbError::Parse {
                        line,
                        col,
                        msg: "unexpected '|'".to_string(),
                    });
                }
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            _ => {
                return Err(MiniDbError::Parse {
                    line,
                    col,
                    msg: format!("unexpected character '{}'", c),
                })
            }
        };

        Ok(TokenWithPos { token, line, col })
    }

    fn read_ident_chars(&mut self) -> String {
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

    fn read_string(&mut self, line: usize, col: usize) -> Result<TokenWithPos> {
        self.advance(); // consume opening '
        let mut s = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(MiniDbError::Parse {
                        line,
                        col,
                        msg: "unterminated string literal".to_string(),
                    })
                }
                Some('\'') => {
                    self.advance();
                    if self.peek() == Some('\'') {
                        self.advance();
                        s.push('\'');
                    } else {
                        break;
                    }
                }
                Some(c) => {
                    s.push(c);
                    self.advance();
                }
            }
        }
        Ok(TokenWithPos {
            token: Token::StringLit(s),
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
            } else if (c == 'e' || c == 'E') && !s.contains('e') && !s.contains('E') {
                is_real = true;
                s.push(c);
                self.advance();
                if self.peek() == Some('-') || self.peek() == Some('+') {
                    s.push(self.advance().unwrap());
                }
            } else {
                break;
            }
        }
        if is_real {
            let r: f64 = s.parse().map_err(|_| MiniDbError::Parse {
                line,
                col,
                msg: format!("invalid real literal: {}", s),
            })?;
            Ok(TokenWithPos {
                token: Token::RealLit(r),
                line,
                col,
            })
        } else {
            let n: i64 = s.parse().map_err(|_| MiniDbError::Parse {
                line,
                col,
                msg: format!("invalid integer literal: {}", s),
            })?;
            Ok(TokenWithPos {
                token: Token::IntLit(n),
                line,
                col,
            })
        }
    }
}

fn postprocess_meta_commands(tokens: Vec<TokenWithPos>) -> Vec<TokenWithPos> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // Check for Dot followed by Ident at statement start (first token or after Semicolon)
        let is_stmt_start = result
            .last()
            .map(|t: &TokenWithPos| t.token == Token::Semicolon)
            .unwrap_or(true);
        if is_stmt_start && tokens[i].token == Token::Dot && i + 1 < tokens.len() {
            if let Token::Ident(ref name) = tokens[i + 1].token {
                let line = tokens[i].line;
                let col = tokens[i].col;
                let meta_tok = match name.to_lowercase().as_str() {
                    "schema" => Some(Token::MetaSchema),
                    "tables" => Some(Token::MetaTables),
                    "indexes" => Some(Token::MetaIndexes),
                    "exit" | "quit" => Some(Token::MetaExit),
                    "format" => Some(Token::MetaFormat),
                    _ => None,
                };
                if let Some(tok) = meta_tok {
                    result.push(TokenWithPos {
                        token: tok,
                        line,
                        col,
                    });
                    // If MetaFormat, the next token after the ident is the format name
                    i += 2;
                    continue;
                }
            }
        }
        result.push(tokens[i].clone());
        i += 1;
    }
    result
}

fn keyword_or_ident(s: &str) -> Token {
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
        "TABLE" => Token::Table,
        "DROP" => Token::Drop,
        "INDEX" => Token::Index,
        "ON" => Token::On,
        "ORDER" => Token::Order,
        "BY" => Token::By,
        "ASC" => Token::Asc,
        "DESC" => Token::Desc,
        "LIMIT" => Token::Limit,
        "OFFSET" => Token::Offset,
        "INNER" => Token::Inner,
        "JOIN" => Token::Join,
        "AND" => Token::And,
        "OR" => Token::Or,
        "NOT" => Token::Not,
        "NULL" => Token::Null,
        "IS" => Token::Is,
        "PRIMARY" => Token::Primary,
        "KEY" => Token::Key,
        "UNIQUE" => Token::Unique,
        "DEFAULT" => Token::Default,
        "CHECK" => Token::Check,
        "BEGIN" => Token::Begin,
        "COMMIT" => Token::Commit,
        "ROLLBACK" => Token::Rollback,
        "TRUE" => Token::True,
        "FALSE" => Token::False,
        "IF" => Token::If,
        "EXISTS" => Token::Exists,
        "INT" | "INTEGER" => Token::Int,
        "BIGINT" => Token::BigInt,
        "REAL" | "FLOAT" | "DOUBLE" => Token::Real,
        "TEXT" | "STRING" => Token::Text,
        "BOOL" | "BOOLEAN" => Token::Bool,
        "VARCHAR" | "CHAR" => Token::Varchar,
        "COALESCE" => Token::Coalesce,
        "LENGTH" => Token::Length,
        "LOWER" => Token::Lower,
        "UPPER" => Token::Upper,
        "SUBSTR" | "SUBSTRING" => Token::Substr,
        "ABS" => Token::Abs,
        "IN" => Token::In,
        "LIKE" => Token::Like,
        "BETWEEN" => Token::Between,
        "CASE" => Token::Case,
        "WHEN" => Token::When,
        "THEN" => Token::Then,
        "ELSE" => Token::Else,
        "END" => Token::End,
        "AS" => Token::As,
        "DISTINCT" => Token::Distinct,
        "ALL" => Token::All,
        "GROUP" => Token::Group,
        "HAVING" => Token::Having,
        "COUNT" => Token::Count,
        "SUM" => Token::Sum,
        "AVG" => Token::Avg,
        "MIN" => Token::Min,
        "MAX" => Token::Max,
        _ => Token::Ident(s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(sql: &str) -> Vec<Token> {
        let mut lex = Lexer::new(sql);
        lex.tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .filter(|t| t != &Token::Eof)
            .collect()
    }

    #[test]
    fn test_basic_select() {
        let ts = tokens("SELECT * FROM foo WHERE id = 1;");
        assert_eq!(
            ts,
            vec![
                Token::Select,
                Token::Star,
                Token::From,
                Token::Ident("foo".to_string()),
                Token::Where,
                Token::Ident("id".to_string()),
                Token::Eq,
                Token::IntLit(1),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn test_string_literal() {
        let ts = tokens("'hello''world'");
        assert_eq!(ts, vec![Token::StringLit("hello'world".to_string())]);
    }

    #[test]
    fn test_operators() {
        let ts = tokens("<> <= >= != ||");
        assert_eq!(
            ts,
            vec![Token::Ne, Token::Le, Token::Ge, Token::Ne, Token::Concat]
        );
    }

    #[test]
    fn test_comment() {
        let ts = tokens("-- this is a comment\nSELECT 1");
        assert_eq!(ts, vec![Token::Select, Token::IntLit(1)]);
    }

    #[test]
    fn test_real_literal() {
        let ts = tokens("3.14 1e10");
        assert_eq!(ts, vec![Token::RealLit(3.14), Token::RealLit(1e10)]);
    }

    #[test]
    fn test_meta_commands() {
        let ts = tokens(".tables");
        assert_eq!(ts, vec![Token::MetaTables]);
    }

    #[test]
    fn test_table_dot_col() {
        let ts = tokens("a.id");
        assert_eq!(
            ts,
            vec![
                Token::Ident("a".to_string()),
                Token::Dot,
                Token::Ident("id".to_string()),
            ]
        );
    }
}
