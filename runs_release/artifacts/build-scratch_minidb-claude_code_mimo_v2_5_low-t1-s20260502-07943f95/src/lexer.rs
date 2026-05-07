use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Select,
    From,
    Where,
    Into,
    Values,
    Insert,
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
    Primary,
    Key,
    Not,
    Null,
    Default,
    Check,
    And,
    Or,
    Is,
    As,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Join,
    Inner,
    Distinct,
    Star,
    Unique,
    // Literals
    Integer(i64),
    Float(f64),
    String(String),
    Ident(String),
    // Operators
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    StarOp,
    Slash,
    Concat,
    Comma,
    Semicolon,
    LParen,
    RParen,
    Dot,
    // Meta
    DotCommand(String),
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Select => write!(f, "SELECT"),
            Token::From => write!(f, "FROM"),
            Token::Where => write!(f, "WHERE"),
            Token::Into => write!(f, "INTO"),
            Token::Values => write!(f, "VALUES"),
            Token::Insert => write!(f, "INSERT"),
            Token::Update => write!(f, "UPDATE"),
            Token::Set => write!(f, "SET"),
            Token::Delete => write!(f, "DELETE"),
            Token::Create => write!(f, "CREATE"),
            Token::Drop => write!(f, "DROP"),
            Token::Table => write!(f, "TABLE"),
            Token::Index => write!(f, "INDEX"),
            Token::On => write!(f, "ON"),
            Token::Begin => write!(f, "BEGIN"),
            Token::Commit => write!(f, "COMMIT"),
            Token::Rollback => write!(f, "ROLLBACK"),
            Token::Primary => write!(f, "PRIMARY"),
            Token::Key => write!(f, "KEY"),
            Token::Not => write!(f, "NOT"),
            Token::Null => write!(f, "NULL"),
            Token::Default => write!(f, "DEFAULT"),
            Token::Check => write!(f, "CHECK"),
            Token::And => write!(f, "AND"),
            Token::Or => write!(f, "OR"),
            Token::Is => write!(f, "IS"),
            Token::As => write!(f, "AS"),
            Token::Order => write!(f, "ORDER"),
            Token::By => write!(f, "BY"),
            Token::Asc => write!(f, "ASC"),
            Token::Desc => write!(f, "DESC"),
            Token::Limit => write!(f, "LIMIT"),
            Token::Offset => write!(f, "OFFSET"),
            Token::Join => write!(f, "JOIN"),
            Token::Inner => write!(f, "INNER"),
            Token::Distinct => write!(f, "DISTINCT"),
            Token::Star => write!(f, "*"),
            Token::Unique => write!(f, "UNIQUE"),
            Token::Integer(v) => write!(f, "{}", v),
            Token::Float(v) => write!(f, "{}", v),
            Token::String(v) => write!(f, "'{}'", v),
            Token::Ident(v) => write!(f, "{}", v),
            Token::Eq => write!(f, "="),
            Token::Ne => write!(f, "<>"),
            Token::Lt => write!(f, "<"),
            Token::Le => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::Ge => write!(f, ">="),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::StarOp => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Concat => write!(f, "||"),
            Token::Comma => write!(f, ","),
            Token::Semicolon => write!(f, ";"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Dot => write!(f, "."),
            Token::DotCommand(v) => write!(f, ".{}", v),
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
    col: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
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

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) -> bool {
        if self.pos + 1 < self.input.len()
            && self.input[self.pos] == '-'
            && self.input[self.pos + 1] == '-'
        {
            while let Some(c) = self.advance() {
                if c == '\n' {
                    return true;
                }
            }
            return true;
        }
        false
    }

    fn read_string(&mut self, quote: char) -> Result<String, String> {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(c) if c == quote => {
                    if self.peek() == Some(quote) {
                        self.advance();
                        s.push(quote);
                    } else {
                        return Ok(s);
                    }
                }
                Some(c) => s.push(c),
                None => return Err("Unterminated string".into()),
            }
        }
    }

    fn read_number(&mut self) -> Token {
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        if self.peek() == Some('.') {
            if self.pos + 1 < self.input.len() && self.input[self.pos + 1].is_ascii_digit() {
                self.advance();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let s: String = self.input[start..self.pos].iter().collect();
                return Token::Float(s.parse().unwrap_or(0.0));
            }
        }
        let s: String = self.input[start..self.pos].iter().collect();
        Token::Integer(s.parse().unwrap_or(0))
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].iter().collect()
    }

    fn keyword_token(word: &str) -> Option<Token> {
        match word.to_uppercase().as_str() {
            "SELECT" => Some(Token::Select),
            "FROM" => Some(Token::From),
            "WHERE" => Some(Token::Where),
            "INTO" => Some(Token::Into),
            "VALUES" => Some(Token::Values),
            "INSERT" => Some(Token::Insert),
            "UPDATE" => Some(Token::Update),
            "SET" => Some(Token::Set),
            "DELETE" => Some(Token::Delete),
            "CREATE" => Some(Token::Create),
            "DROP" => Some(Token::Drop),
            "TABLE" => Some(Token::Table),
            "INDEX" => Some(Token::Index),
            "ON" => Some(Token::On),
            "BEGIN" => Some(Token::Begin),
            "COMMIT" => Some(Token::Commit),
            "ROLLBACK" => Some(Token::Rollback),
            "PRIMARY" => Some(Token::Primary),
            "KEY" => Some(Token::Key),
            "NOT" => Some(Token::Not),
            "NULL" => Some(Token::Null),
            "DEFAULT" => Some(Token::Default),
            "CHECK" => Some(Token::Check),
            "AND" => Some(Token::And),
            "OR" => Some(Token::Or),
            "IS" => Some(Token::Is),
            "AS" => Some(Token::As),
            "ORDER" => Some(Token::Order),
            "BY" => Some(Token::By),
            "ASC" => Some(Token::Asc),
            "DESC" => Some(Token::Desc),
            "LIMIT" => Some(Token::Limit),
            "OFFSET" => Some(Token::Offset),
            "JOIN" => Some(Token::Join),
            "INNER" => Some(Token::Inner),
            "DISTINCT" => Some(Token::Distinct),
            "UNIQUE" => Some(Token::Unique),
            "INT" | "INTEGER" | "BIGINT" | "REAL" | "TEXT" | "BOOL" | "BOOLEAN" => None,
            _ => None,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<TokenWithPos>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            while self.skip_comment() {
                self.skip_whitespace();
            }
            let line = self.line;
            let col = self.col;
            let Some(c) = self.peek() else {
                tokens.push(TokenWithPos {
                    token: Token::Eof,
                    line,
                    column: col,
                });
                break;
            };
            if c == ';' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Semicolon,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '(' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::LParen,
                    line,
                    column: col,
                });
                continue;
            }
            if c == ')' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::RParen,
                    line,
                    column: col,
                });
                continue;
            }
            if c == ',' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Comma,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '.' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Dot,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '\'' || c == '"' {
                self.advance();
                let s = self
                    .read_string(c)
                    .map_err(|e| format!("{} at {}:{}", e, line, col))?;
                tokens.push(TokenWithPos {
                    token: Token::String(s),
                    line,
                    column: col,
                });
                continue;
            }
            if c.is_ascii_digit() {
                let tok = self.read_number();
                tokens.push(TokenWithPos {
                    token: tok,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '-' {
                if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == '-' {
                    continue;
                }
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Minus,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '+' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Plus,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '*' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::StarOp,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '/' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Slash,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '|' && self.pos + 1 < self.input.len() && self.input[self.pos + 1] == '|' {
                self.advance();
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Concat,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '=' {
                self.advance();
                tokens.push(TokenWithPos {
                    token: Token::Eq,
                    line,
                    column: col,
                });
                continue;
            }
            if c == '<' {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    tokens.push(TokenWithPos {
                        token: Token::Ne,
                        line,
                        column: col,
                    });
                } else if self.peek() == Some('=') {
                    self.advance();
                    tokens.push(TokenWithPos {
                        token: Token::Le,
                        line,
                        column: col,
                    });
                } else {
                    tokens.push(TokenWithPos {
                        token: Token::Lt,
                        line,
                        column: col,
                    });
                }
                continue;
            }
            if c == '>' {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    tokens.push(TokenWithPos {
                        token: Token::Ge,
                        line,
                        column: col,
                    });
                } else {
                    tokens.push(TokenWithPos {
                        token: Token::Gt,
                        line,
                        column: col,
                    });
                }
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let ident = self.read_identifier();
                let upper = ident.to_uppercase();
                if let Some(tok) = Self::keyword_token(&upper) {
                    tokens.push(TokenWithPos {
                        token: tok,
                        line,
                        column: col,
                    });
                } else {
                    tokens.push(TokenWithPos {
                        token: Token::Ident(ident),
                        line,
                        column: col,
                    });
                }
                continue;
            }
            return Err(format!("Unexpected character '{}' at {}:{}", c, line, col));
        }
        Ok(tokens)
    }
}

pub fn tokenize(input: &str) -> Result<Vec<TokenWithPos>, String> {
    Lexer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let tokens = tokenize("SELECT * FROM users WHERE id = 1;").unwrap();
        let types: Vec<_> = tokens.iter().map(|t| t.token.clone()).collect();
        assert_eq!(
            types,
            vec![
                Token::Select,
                Token::StarOp,
                Token::From,
                Token::Ident("users".into()),
                Token::Where,
                Token::Ident("id".into()),
                Token::Eq,
                Token::Integer(1),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("INSERT INTO t VALUES ('hello', 42);").unwrap();
        assert_eq!(tokens[5].token, Token::String("hello".into()));
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("a <> b AND c <= d OR e >= f || g").unwrap();
        assert_eq!(tokens[1].token, Token::Ne);
        assert_eq!(tokens[3].token, Token::And);
        assert_eq!(tokens[5].token, Token::Le);
        assert_eq!(tokens[7].token, Token::Or);
        assert_eq!(tokens[9].token, Token::Ge);
        assert_eq!(tokens[11].token, Token::Concat);
    }

    #[test]
    fn test_float_literal() {
        let tokens = tokenize("3.14").unwrap();
        assert_eq!(tokens[0].token, Token::Float(3.14));
    }

    #[test]
    fn test_dot_command() {
        let tokens = tokenize(".tables").unwrap();
        // Dot is now a separate token; dot commands are handled by the REPL before tokenization
        assert_eq!(tokens[0].token, Token::Dot);
        assert_eq!(tokens[1].token, Token::Ident("tables".into()));
    }

    #[test]
    fn test_comment_skipped() {
        let tokens = tokenize("SELECT -- comment\n1;").unwrap();
        let types: Vec<_> = tokens.iter().map(|t| &t.token).collect();
        assert!(types.contains(&&Token::Select));
        assert!(types.contains(&&Token::Integer(1)));
        assert!(!types
            .iter()
            .any(|t| matches!(t, Token::Ident(s) if s == "comment")));
    }
}
