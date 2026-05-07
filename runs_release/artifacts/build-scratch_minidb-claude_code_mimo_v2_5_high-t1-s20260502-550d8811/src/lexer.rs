use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    SELECT,
    FROM,
    WHERE,
    INSERT,
    INTO,
    VALUES,
    UPDATE,
    SET,
    DELETE,
    CREATE,
    TABLE,
    DROP,
    INDEX,
    ON,
    BEGIN,
    COMMIT,
    ROLLBACK,
    AND,
    OR,
    NOT,
    IS,
    NULL,
    TRUE,
    FALSE,
    PRIMARY,
    KEY,
    DEFAULT,
    CHECK,
    NOTNULL,
    UNIQUE,
    ORDER,
    BY,
    ASC,
    DESC,
    LIMIT,
    OFFSET,
    JOIN,
    INNER,
    AS,

    // Literals
    Integer(i64),
    Float(f64),
    Str(String),
    Ident(String),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Comma,
    Semicolon,
    LParen,
    RParen,
    Dot,

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
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok == Token::Eof;
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

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) -> bool {
        if self.pos + 1 < self.input.len() {
            if self.input[self.pos] == '-' && self.input[self.pos + 1] == '-' {
                while let Some(ch) = self.advance() {
                    if ch == '\n' {
                        return true;
                    }
                }
                return true;
            }
            if self.input[self.pos] == '/' && self.input[self.pos + 1] == '*' {
                self.advance();
                self.advance();
                loop {
                    if let Some(ch) = self.advance() {
                        if ch == '*' && self.peek() == Some('/') {
                            self.advance();
                            return true;
                        }
                    } else {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn read_string(&mut self, quote: char) -> Result<String> {
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(MiniDbError::ParseAt {
                        msg: "unterminated string".into(),
                        line: self.line,
                        col: self.col,
                    })
                }
                Some(c) if c == quote => {
                    if self.peek() == Some(quote) {
                        self.advance();
                        s.push(quote);
                    } else {
                        return Ok(s);
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }

    fn read_number(&mut self) -> Result<Token> {
        let start_col = self.col;
        let start_line = self.line;
        let mut num_str = String::new();
        let mut is_float = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                is_float = true;
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if is_float {
            let val: f64 = num_str.parse().map_err(|_| MiniDbError::ParseAt {
                msg: format!("invalid float: {}", num_str),
                line: start_line,
                col: start_col,
            })?;
            Ok(Token::Float(val))
        } else {
            let val: i64 = num_str.parse().map_err(|_| MiniDbError::ParseAt {
                msg: format!("invalid integer: {}", num_str),
                line: start_line,
                col: start_col,
            })?;
            Ok(Token::Integer(val))
        }
    }

    fn read_identifier(&mut self) -> Token {
        let mut ident = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let upper = ident.to_uppercase();
        match upper.as_str() {
            "SELECT" => Token::SELECT,
            "FROM" => Token::FROM,
            "WHERE" => Token::WHERE,
            "INSERT" => Token::INSERT,
            "INTO" => Token::INTO,
            "VALUES" => Token::VALUES,
            "UPDATE" => Token::UPDATE,
            "SET" => Token::SET,
            "DELETE" => Token::DELETE,
            "CREATE" => Token::CREATE,
            "TABLE" => Token::TABLE,
            "DROP" => Token::DROP,
            "INDEX" => Token::INDEX,
            "ON" => Token::ON,
            "BEGIN" => Token::BEGIN,
            "COMMIT" => Token::COMMIT,
            "ROLLBACK" => Token::ROLLBACK,
            "AND" => Token::AND,
            "OR" => Token::OR,
            "NOT" => Token::NOT,
            "IS" => Token::IS,
            "NULL" => Token::NULL,
            "TRUE" => Token::TRUE,
            "FALSE" => Token::FALSE,
            "PRIMARY" => Token::PRIMARY,
            "KEY" => Token::KEY,
            "DEFAULT" => Token::DEFAULT,
            "CHECK" => Token::CHECK,
            "NOTNULL" => Token::NOTNULL,
            "UNIQUE" => Token::UNIQUE,
            "ORDER" => Token::ORDER,
            "BY" => Token::BY,
            "ASC" => Token::ASC,
            "DESC" => Token::DESC,
            "LIMIT" => Token::LIMIT,
            "OFFSET" => Token::OFFSET,
            "JOIN" => Token::JOIN,
            "INNER" => Token::INNER,
            "AS" => Token::AS,
            "INT" => Token::Ident("INT".into()),
            "BIGINT" => Token::Ident("BIGINT".into()),
            "REAL" => Token::Ident("REAL".into()),
            "TEXT" => Token::Ident("TEXT".into()),
            "BOOL" => Token::Ident("BOOL".into()),
            _ => Token::Ident(ident),
        }
    }

    fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace();
        while self.skip_comment() {
            self.skip_whitespace();
        }

        let line = self.line;
        let col = self.col;

        let ch = match self.peek() {
            None => return Ok(Token::Eof),
            Some(c) => c,
        };

        match ch {
            ';' => {
                self.advance();
                Ok(Token::Semicolon)
            }
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
            '.' => {
                self.advance();
                Ok(Token::Dot)
            }
            '+' => {
                self.advance();
                Ok(Token::Plus)
            }
            '*' => {
                self.advance();
                Ok(Token::Star)
            }
            '%' => {
                self.advance();
                Ok(Token::Percent)
            }
            '/' => {
                self.advance();
                Ok(Token::Slash)
            }
            '|' => {
                self.advance();
                if self.peek() == Some('|') {
                    self.advance();
                    Ok(Token::Concat)
                } else {
                    Err(MiniDbError::ParseAt {
                        msg: "expected '|' after '|'".into(),
                        line,
                        col,
                    })
                }
            }
            '=' => {
                self.advance();
                Ok(Token::Eq)
            }
            '<' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token::Ne)
                } else if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Le)
                } else {
                    Ok(Token::Lt)
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
            '-' => {
                self.advance();
                if self.peek() == Some('-') {
                    // Check if this is a comment or negative sign
                    // Actually -- starts a line comment
                    while let Some(c) = self.advance() {
                        if c == '\n' {
                            break;
                        }
                    }
                    self.next_token()
                } else {
                    Ok(Token::Minus)
                }
            }
            '\'' | '"' => {
                self.advance();
                let s = self.read_string(ch)?;
                Ok(Token::Str(s))
            }
            c if c.is_ascii_digit() => self.read_number(),
            c if c.is_alphabetic() || c == '_' => Ok(self.read_identifier()),
            _ => Err(MiniDbError::ParseAt {
                msg: format!("unexpected character: '{}'", ch),
                line,
                col,
            }),
        }
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn col(&self) -> usize {
        self.col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let mut lexer = Lexer::new("SELECT * FROM users WHERE id = 1;");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::SELECT);
        assert_eq!(tokens[1], Token::Star);
        assert_eq!(tokens[2], Token::FROM);
        assert_eq!(tokens[3], Token::Ident("users".into()));
        assert_eq!(tokens[4], Token::WHERE);
        assert_eq!(tokens[5], Token::Ident("id".into()));
        assert_eq!(tokens[6], Token::Eq);
        assert_eq!(tokens[7], Token::Integer(1));
        assert_eq!(tokens[8], Token::Semicolon);
        assert_eq!(tokens[9], Token::Eof);
    }

    #[test]
    fn test_string_literals() {
        let mut lexer = Lexer::new("'hello world' \"test\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Str("hello world".into()));
        assert_eq!(tokens[1], Token::Str("test".into()));
    }

    #[test]
    fn test_operators() {
        let mut lexer = Lexer::new("<> <= >= ||");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ne);
        assert_eq!(tokens[1], Token::Le);
        assert_eq!(tokens[2], Token::Ge);
        assert_eq!(tokens[3], Token::Concat);
    }

    #[test]
    fn test_comments() {
        let mut lexer = Lexer::new("SELECT -- comment\n1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::SELECT);
        assert_eq!(tokens[1], Token::Integer(1));
    }

    #[test]
    fn test_float() {
        let mut lexer = Lexer::new("3.14");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Float(3.14));
    }

    #[test]
    fn test_keywords_case_insensitive() {
        let mut lexer = Lexer::new("select SELECT Select");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::SELECT);
        assert_eq!(tokens[1], Token::SELECT);
        assert_eq!(tokens[2], Token::SELECT);
    }
}
