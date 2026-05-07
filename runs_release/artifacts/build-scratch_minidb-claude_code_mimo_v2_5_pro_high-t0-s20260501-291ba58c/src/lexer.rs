use crate::error::{MiniDbError, Result};

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
    Drop,
    Table,
    Index,
    On,
    Primary,
    Key,
    Null,
    NotNull,
    Unique,
    Default,
    Check,
    Begin,
    Commit,
    Rollback,
    Inner,
    Join,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    As,
    Is,
    Int,
    Bigint,
    Real,
    Text,
    Bool,
    True,
    False,

    // Identifiers and literals
    Identifier(String),
    IntegerLiteral(i64),
    RealLiteral(f64),
    StringLiteral(String),

    // Symbols
    LeftParen,
    RightParen,
    Comma,
    Semicolon,
    Dot,
    Star,
    Plus,
    Minus,
    Slash,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat, // ||

    // Meta-commands
    MetaSchema,
    MetaTables,
    MetaIndexes,
    MetaExit,
    MetaFormat,

    Eof,
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
        let ch = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn skip_block_comment(&mut self) {
        self.advance(); // consume *
        self.advance(); // consume /
        loop {
            match self.peek() {
                None => break,
                Some('*') => {
                    self.advance();
                    if self.peek() == Some('/') {
                        self.advance();
                        break;
                    }
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    fn read_string(&mut self, quote: char) -> Result<Token> {
        let start_line = self.line;
        let start_col = self.col;
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(MiniDbError::Parse {
                        message: "unterminated string".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                }
                Some(ch) if ch == quote => {
                    // Check for escaped quote ('')
                    if self.peek() == Some(quote) {
                        self.advance();
                        s.push(quote);
                    } else {
                        return Ok(Token::StringLiteral(s));
                    }
                }
                Some(ch) => s.push(ch),
            }
        }
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        if self.peek() == Some('-') {
            s.push('-');
            self.advance();
        }
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let mut is_real = false;
        if self.peek() == Some('.') {
            // Check if next char is a digit (to distinguish from table.column)
            let next_pos = self.pos + 1;
            if next_pos < self.input.len() && self.input[next_pos].is_ascii_digit() {
                is_real = true;
                s.push('.');
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        s.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }
        if is_real {
            Token::RealLiteral(s.parse().unwrap_or(0.0))
        } else {
            Token::IntegerLiteral(s.parse().unwrap_or(0))
        }
    }

    fn read_identifier(&mut self) -> Token {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let upper = s.to_uppercase();
        match upper.as_str() {
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
            "DROP" => Token::Drop,
            "TABLE" => Token::Table,
            "INDEX" => Token::Index,
            "ON" => Token::On,
            "PRIMARY" => Token::Primary,
            "KEY" => Token::Key,
            "NULL" => Token::Null,
            "UNIQUE" => Token::Unique,
            "DEFAULT" => Token::Default,
            "CHECK" => Token::Check,
            "BEGIN" => Token::Begin,
            "COMMIT" => Token::Commit,
            "ROLLBACK" => Token::Rollback,
            "INNER" => Token::Inner,
            "JOIN" => Token::Join,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "LIMIT" => Token::Limit,
            "OFFSET" => Token::Offset,
            "AS" => Token::As,
            "IS" => Token::Is,
            "INT" => Token::Int,
            "BIGINT" => Token::Bigint,
            "REAL" => Token::Real,
            "TEXT" => Token::Text,
            "BOOL" => Token::Bool,
            "TRUE" => Token::True,
            "FALSE" => Token::False,
            _ => Token::Identifier(s),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(ch) = self.peek() else {
                tokens.push(Token::Eof);
                break;
            };

            match ch {
                '-' if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == '-' => {
                    self.advance();
                    self.advance();
                    self.skip_line_comment();
                }
                '/' if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == '*' => {
                    self.advance();
                    self.skip_block_comment();
                }
                '\'' | '"' => {
                    self.advance();
                    tokens.push(self.read_string(ch)?);
                }
                '(' => {
                    self.advance();
                    tokens.push(Token::LeftParen);
                }
                ')' => {
                    self.advance();
                    tokens.push(Token::RightParen);
                }
                ',' => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                ';' => {
                    self.advance();
                    tokens.push(Token::Semicolon);
                }
                '.' => {
                    // Check for meta-commands
                    let start = self.pos;
                    self.advance();
                    let mut meta = String::new();
                    while let Some(ch) = self.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            meta.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    match meta.to_lowercase().as_str() {
                        "schema" => tokens.push(Token::MetaSchema),
                        "tables" => tokens.push(Token::MetaTables),
                        "indexes" => tokens.push(Token::MetaIndexes),
                        "exit" => tokens.push(Token::MetaExit),
                        "format" => tokens.push(Token::MetaFormat),
                        _ => {
                            // Not a meta-command, treat as dot
                            self.pos = start;
                            self.col -= 1;
                            self.advance();
                            tokens.push(Token::Dot);
                        }
                    }
                }
                '*' => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                '+' => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                '-' => {
                    // Could be negative number or minus
                    let next_pos = self.pos + 1;
                    if next_pos < self.input.len() && self.input[next_pos].is_ascii_digit() {
                        tokens.push(self.read_number());
                    } else {
                        self.advance();
                        tokens.push(Token::Minus);
                    }
                }
                '/' => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                '=' => {
                    self.advance();
                    tokens.push(Token::Eq);
                }
                '!' if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == '=' => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::Ne);
                }
                '<' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Le);
                    } else if self.peek() == Some('>') {
                        self.advance();
                        tokens.push(Token::Ne);
                    } else {
                        tokens.push(Token::Lt);
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Ge);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                '|' if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == '|' => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::Concat);
                }
                ch if ch.is_ascii_digit() => {
                    tokens.push(self.read_number());
                }
                ch if ch.is_alphabetic() || ch == '_' => {
                    tokens.push(self.read_identifier());
                }
                _ => {
                    return Err(MiniDbError::Parse {
                        message: format!("unexpected character: '{ch}'"),
                        line: self.line,
                        col: self.col,
                    });
                }
            }
        }
        Ok(tokens)
    }
}

pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    Lexer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_select() {
        let tokens = tokenize("SELECT * FROM users WHERE id = 1;").unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Star);
        assert_eq!(tokens[2], Token::From);
        assert_eq!(tokens[3], Token::Identifier("users".to_string()));
        assert_eq!(tokens[4], Token::Where);
        assert_eq!(tokens[5], Token::Identifier("id".to_string()));
        assert_eq!(tokens[6], Token::Eq);
        assert_eq!(tokens[7], Token::IntegerLiteral(1));
        assert_eq!(tokens[8], Token::Semicolon);
    }

    #[test]
    fn test_create_table() {
        let tokens = tokenize("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);").unwrap();
        assert_eq!(tokens[0], Token::Create);
        assert_eq!(tokens[1], Token::Table);
        assert_eq!(tokens[2], Token::Identifier("t".to_string()));
        assert_eq!(tokens[3], Token::LeftParen);
        assert_eq!(tokens[4], Token::Identifier("id".to_string()));
        assert_eq!(tokens[5], Token::Int);
        assert_eq!(tokens[6], Token::Primary);
        assert_eq!(tokens[7], Token::Key);
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("SELECT 'hello world';").unwrap();
        assert_eq!(tokens[1], Token::StringLiteral("hello world".to_string()));
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("a <> b <= c >= d || e").unwrap();
        assert_eq!(tokens[1], Token::Ne);
        assert_eq!(tokens[3], Token::Le);
        assert_eq!(tokens[5], Token::Ge);
        assert_eq!(tokens[7], Token::Concat);
    }

    #[test]
    fn test_meta_commands() {
        let tokens = tokenize(".schema .tables .exit").unwrap();
        assert_eq!(tokens[0], Token::MetaSchema);
        assert_eq!(tokens[1], Token::MetaTables);
        assert_eq!(tokens[2], Token::MetaExit);
    }

    #[test]
    fn test_negative_number() {
        let tokens = tokenize("SELECT -42;").unwrap();
        assert_eq!(tokens[1], Token::IntegerLiteral(-42));
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("SELECT -- comment\n1;").unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::IntegerLiteral(1));
    }
}
