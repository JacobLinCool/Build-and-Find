#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Create,
    Drop,
    Table,
    Index,
    On,
    Insert,
    Into,
    Values,
    Select,
    From,
    Where,
    And,
    Or,
    Not,
    Join,
    Inner,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Update,
    Set,
    Delete,
    Begin,
    Commit,
    Rollback,
    Primary,
    Key,
    Null,
    NotNull,
    Unique,
    Default,
    Check,
    Is,
    As,
    Cross,
    Left,
    Right,
    Outer,

    // Types
    Int,
    Bigint,
    Real,
    Text,
    Bool,

    // Literals
    IntegerLit(i64),
    RealLit(f64),
    StringLit(String),
    BoolLit(bool),

    // Identifiers
    Ident(String),

    // Symbols
    LParen,
    RParen,
    Comma,
    Semicolon,
    Dot,
    Star,
    Plus,
    Minus,
    Slash,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,

    // Meta commands
    DotSchema,
    DotTables,
    DotIndexes,
    DotExit,
    DotFormat,
    DotHelp,

    Eof,
}

impl Token {
    pub fn description(&self) -> String {
        match self {
            Self::Ident(s) => format!("identifier '{}'", s),
            Self::IntegerLit(n) => format!("integer {}", n),
            Self::RealLit(n) => format!("real {}", n),
            Self::StringLit(s) => format!("string '{}'", s),
            Self::BoolLit(b) => format!("bool {}", b),
            Self::Eof => "end of input".to_string(),
            _ => format!("{:?}", self),
        }
    }
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

    pub fn tokenize(&mut self) -> crate::error::Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() {
                tokens.push(Token::Eof);
                break;
            }
            let (line, col) = (self.line, self.col);
            let tok = self.next_token()?;
            if tok == Token::Eof {
                tokens.push(tok);
                break;
            }
            tokens.push(tok);
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }
            // Skip -- line comments
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == '-'
                && self.input[self.pos + 1] == '-'
            {
                while let Some(ch) = self.peek() {
                    if ch == '\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }
            break;
        }
    }

    fn next_token(&mut self) -> crate::error::Result<Token> {
        let ch = match self.peek() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match ch {
            '(' => {
                self.advance();
                Ok(Token::LParen)
            }
            ')' => {
                self.advance();
                Ok(Token::RParen)
            }
            ',' => {
                self.advance();
                Ok(Token::Comma)
            }
            ';' => {
                self.advance();
                Ok(Token::Semicolon)
            }
            '.' => {
                let save_pos = self.pos;
                let save_line = self.line;
                let save_col = self.col;
                self.advance();
                // Check for meta-commands
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c.is_alphabetic() || c == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let word: String = self.input[start..self.pos].iter().collect();
                if word.is_empty() {
                    return Ok(Token::Dot);
                }
                let lower = word.to_lowercase();
                match lower.as_str() {
                    "schema" => Ok(Token::DotSchema),
                    "tables" => Ok(Token::DotTables),
                    "indexes" => Ok(Token::DotIndexes),
                    "exit" => Ok(Token::DotExit),
                    "format" => Ok(Token::DotFormat),
                    "help" => Ok(Token::DotHelp),
                    _ => {
                        // Not a meta-command - backtrack to just after the dot
                        self.pos = save_pos + 1; // just after '.'
                        self.line = save_line;
                        self.col = save_col + 1;
                        Ok(Token::Dot)
                    }
                }
            }
            '*' => {
                self.advance();
                Ok(Token::Star)
            }
            '+' => {
                self.advance();
                Ok(Token::Plus)
            }
            '/' => {
                self.advance();
                Ok(Token::Slash)
            }
            '(' if false => unreachable!(),
            '-' => {
                self.advance();
                Ok(Token::Minus)
            }
            '|' => {
                self.advance();
                if self.peek() == Some('|') {
                    self.advance();
                    Ok(Token::Concat)
                } else {
                    Err(crate::error::MiniDbError::Parse {
                        message: "expected '|' for concatenation operator".into(),
                        line: Some(self.line),
                        column: Some(self.col),
                    })
                }
            }
            '=' => {
                self.advance();
                Ok(Token::Eq)
            }
            '<' => {
                self.advance();
                match self.peek() {
                    Some('>') => {
                        self.advance();
                        Ok(Token::Neq)
                    }
                    Some('=') => {
                        self.advance();
                        Ok(Token::Le)
                    }
                    _ => Ok(Token::Lt),
                }
            }
            '>' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Ge)
                } else {
                    Ok(Token::Gt)
                }
            }
            '\'' => self.read_string(),
            '"' => self.read_quoted_ident(),
            _ if ch.is_ascii_digit() => self.read_number(),
            _ if ch.is_alphabetic() || ch == '_' => self.read_word(),
            _ => Err(crate::error::MiniDbError::Parse {
                message: format!("unexpected character '{}'", ch),
                line: Some(self.line),
                column: Some(self.col),
            }),
        }
    }

    fn read_string(&mut self) -> crate::error::Result<Token> {
        self.advance(); // skip opening quote
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\'') => {
                    // Check for escaped quote ''
                    if self.peek() == Some('\'') {
                        self.advance();
                        s.push('\'');
                    } else {
                        break;
                    }
                }
                Some(c) => s.push(c),
                None => {
                    return Err(crate::error::MiniDbError::Parse {
                        message: "unterminated string literal".into(),
                        line: Some(self.line),
                        column: Some(self.col),
                    });
                }
            }
        }
        Ok(Token::StringLit(s))
    }

    fn read_quoted_ident(&mut self) -> crate::error::Result<Token> {
        self.advance(); // skip opening quote
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => {
                    if self.peek() == Some('"') {
                        self.advance();
                        s.push('"');
                    } else {
                        break;
                    }
                }
                Some(c) => s.push(c),
                None => {
                    return Err(crate::error::MiniDbError::Parse {
                        message: "unterminated quoted identifier".into(),
                        line: Some(self.line),
                        column: Some(self.col),
                    });
                }
            }
        }
        Ok(Token::Ident(s))
    }

    fn read_number(&mut self) -> crate::error::Result<Token> {
        let start = self.pos;
        let mut has_dot = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else if ch == '.' && !has_dot {
                has_dot = true;
                self.advance();
            } else {
                break;
            }
        }
        let text: String = self.input[start..self.pos].iter().collect();
        if has_dot {
            text.parse::<f64>()
                .map(Token::RealLit)
                .map_err(|_| crate::error::MiniDbError::Parse {
                    message: format!("invalid real number '{}'", text),
                    line: Some(self.line),
                    column: Some(self.col),
                })
        } else {
            text.parse::<i64>().map(Token::IntegerLit).map_err(|_| {
                crate::error::MiniDbError::Parse {
                    message: format!("invalid integer '{}'", text),
                    line: Some(self.line),
                    column: Some(self.col),
                }
            })
        }
    }

    fn read_word(&mut self) -> crate::error::Result<Token> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
        let word: String = self.input[start..self.pos].iter().collect();
        let lower = word.to_lowercase();
        let tok = match lower.as_str() {
            "create" => Token::Create,
            "drop" => Token::Drop,
            "table" => Token::Table,
            "index" => Token::Index,
            "on" => Token::On,
            "insert" => Token::Insert,
            "into" => Token::Into,
            "values" => Token::Values,
            "select" => Token::Select,
            "from" => Token::From,
            "where" => Token::Where,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "join" => Token::Join,
            "inner" => Token::Inner,
            "cross" => Token::Cross,
            "left" => Token::Left,
            "right" => Token::Right,
            "outer" => Token::Outer,
            "order" => Token::Order,
            "by" => Token::By,
            "asc" => Token::Asc,
            "desc" => Token::Desc,
            "limit" => Token::Limit,
            "offset" => Token::Offset,
            "update" => Token::Update,
            "set" => Token::Set,
            "delete" => Token::Delete,
            "begin" => Token::Begin,
            "commit" => Token::Commit,
            "rollback" => Token::Rollback,
            "primary" => Token::Primary,
            "key" => Token::Key,
            "null" => Token::Null,
            "unique" => Token::Unique,
            "default" => Token::Default,
            "check" => Token::Check,
            "is" => Token::Is,
            "as" => Token::As,
            "int" | "integer" => Token::Int,
            "bigint" => Token::Bigint,
            "real" | "double" | "float" => Token::Real,
            "text" | "varchar" | "char" | "string" => Token::Text,
            "boolean" | "bool" => Token::Bool,
            "true" => Token::BoolLit(true),
            "false" => Token::BoolLit(false),
            _ => Token::Ident(word),
        };
        Ok(tok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let tokens = Lexer::new("SELECT * FROM t;").tokenize().unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Star);
        assert_eq!(tokens[2], Token::From);
        assert_eq!(tokens[3], Token::Ident("t".into()));
        assert_eq!(tokens[4], Token::Semicolon);
        assert_eq!(tokens[5], Token::Eof);
    }

    #[test]
    fn test_string_literal() {
        let tokens = Lexer::new("'hello world'").tokenize().unwrap();
        assert_eq!(tokens[0], Token::StringLit("hello world".into()));
    }

    #[test]
    fn test_operators() {
        let tokens = Lexer::new("<= >= <> ||").tokenize().unwrap();
        assert_eq!(tokens[0], Token::Le);
        assert_eq!(tokens[1], Token::Ge);
        assert_eq!(tokens[2], Token::Neq);
        assert_eq!(tokens[3], Token::Concat);
    }

    #[test]
    fn test_keywords_case_insensitive() {
        let tokens = Lexer::new("SELECT select Select").tokenize().unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Select);
        assert_eq!(tokens[2], Token::Select);
    }

    #[test]
    fn test_create_table() {
        let tokens = Lexer::new("CREATE TABLE t (id INT PRIMARY KEY);")
            .tokenize()
            .unwrap();
        assert_eq!(tokens[0], Token::Create);
        assert_eq!(tokens[1], Token::Table);
        assert_eq!(tokens[3], Token::LParen);
        assert_eq!(tokens[5], Token::Int);
        assert_eq!(tokens[6], Token::Primary);
        assert_eq!(tokens[7], Token::Key);
    }

    #[test]
    fn test_meta_command() {
        let tokens = Lexer::new(".schema").tokenize().unwrap();
        assert_eq!(tokens[0], Token::DotSchema);
    }

    #[test]
    fn test_line_comment() {
        let tokens = Lexer::new("SELECT -- comment\n*").tokenize().unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Star);
    }
}
