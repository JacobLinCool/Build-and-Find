use crate::error::{MiniDbError, Result};
use crate::lexer::Token;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub col: usize,
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
            } else if ch == '-'
                && self.pos + 1 < self.input.len()
                && self.input[self.pos + 1] == '-'
            {
                // Line comment
                self.advance();
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch == '\n' {
                        break;
                    }
                    self.advance();
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
                    // Check for escaped quote ('' or "")
                    if self.peek() == Some(ch) {
                        self.advance();
                        s.push(ch);
                    } else {
                        return Ok(s);
                    }
                }
                Some(ch) => s.push(ch),
                None => {
                    return Err(MiniDbError::Parse {
                        line: self.line,
                        col: self.col,
                        message: "unterminated string literal".into(),
                    })
                }
            }
        }
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        let mut is_real = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !is_real {
                // Check if next char is a digit to distinguish from qualified name
                if self.pos + 1 < self.input.len() && self.input[self.pos + 1].is_ascii_digit() {
                    is_real = true;
                    s.push(ch);
                    self.advance();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if is_real {
            Token::Real(s.parse().unwrap_or(0.0))
        } else {
            Token::Integer(s.parse().unwrap_or(0))
        }
    }

    fn read_identifier(&mut self) -> String {
        let mut s = String::new();
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

    pub fn tokenize(&mut self) -> Result<Vec<SpannedToken>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let line = self.line;
            let col = self.col;

            let ch = match self.peek() {
                Some(ch) => ch,
                None => {
                    tokens.push(SpannedToken {
                        token: Token::Eof,
                        line,
                        col,
                    });
                    return Ok(tokens);
                }
            };

            let token = match ch {
                '(' => {
                    self.advance();
                    Token::LParen
                }
                ')' => {
                    self.advance();
                    Token::RParen
                }
                ',' => {
                    self.advance();
                    Token::Comma
                }
                ';' => {
                    self.advance();
                    Token::Semicolon
                }
                '.' => {
                    self.advance();
                    Token::Dot
                }
                '*' => {
                    self.advance();
                    Token::Star
                }
                '+' => {
                    self.advance();
                    Token::Plus
                }
                '-' => {
                    self.advance();
                    Token::Minus
                }
                '/' => {
                    self.advance();
                    Token::Slash
                }
                '%' => {
                    self.advance();
                    Token::Percent
                }
                '=' => {
                    self.advance();
                    Token::Eq
                }
                '!' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::Ne
                    } else {
                        return Err(MiniDbError::Parse {
                            line,
                            col,
                            message: format!("unexpected character '!'"),
                        });
                    }
                }
                '<' => {
                    self.advance();
                    match self.peek() {
                        Some('=') => {
                            self.advance();
                            Token::Le
                        }
                        Some('>') => {
                            self.advance();
                            Token::Ne
                        }
                        _ => Token::Lt,
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::Ge
                    } else {
                        Token::Gt
                    }
                }
                '|' => {
                    self.advance();
                    if self.peek() == Some('|') {
                        self.advance();
                        Token::Concat
                    } else {
                        return Err(MiniDbError::Parse {
                            line,
                            col,
                            message: "unexpected character '|' (did you mean '||'?)".into(),
                        });
                    }
                }
                '\'' | '"' => {
                    self.advance();
                    let s = self.read_string(ch)?;
                    Token::StringLit(s)
                }
                _ if ch.is_ascii_digit() => self.read_number(),
                _ if ch.is_alphabetic() || ch == '_' => {
                    let ident = self.read_identifier();
                    Token::keyword(&ident).unwrap_or(Token::Ident(ident))
                }
                '.' if self.pos + 1 < self.input.len()
                    && self.input[self.pos + 1].is_ascii_digit() =>
                {
                    self.read_number()
                }
                _ => {
                    return Err(MiniDbError::Parse {
                        line,
                        col,
                        message: format!("unexpected character '{}'", ch),
                    });
                }
            };

            tokens.push(SpannedToken { token, line, col });
        }
    }
}

/// Split SQL text into individual statements on semicolons.
pub fn split_statements(input: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    let chars: Vec<char> = input.chars().collect();
    let mut in_string = false;
    let mut string_char = '\0';

    for (i, &ch) in chars.iter().enumerate() {
        if in_string {
            if ch == string_char {
                // Check for escaped quote
                if i + 1 < chars.len() && chars[i + 1] == string_char {
                    continue; // skip escaped quote
                }
                in_string = false;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                in_string = true;
                string_char = ch;
            }
            ';' => {
                let stmt = input[start..i].trim();
                if !stmt.is_empty() {
                    statements.push(stmt);
                }
                start = i + 1;
            }
            _ => {}
        }
    }

    let remaining = input[start..].trim();
    if !remaining.is_empty() {
        statements.push(remaining);
    }

    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let mut lexer = Lexer::new("SELECT * FROM users WHERE id = 1;");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Select);
        assert_eq!(tokens[1].token, Token::Star);
        assert_eq!(tokens[2].token, Token::From);
        assert_eq!(tokens[3].token, Token::Ident("users".into()));
        assert_eq!(tokens[4].token, Token::Where);
        assert_eq!(tokens[5].token, Token::Ident("id".into()));
        assert_eq!(tokens[6].token, Token::Eq);
        assert_eq!(tokens[7].token, Token::Integer(1));
        assert_eq!(tokens[8].token, Token::Semicolon);
        assert_eq!(tokens[9].token, Token::Eof);
    }

    #[test]
    fn test_string_literal() {
        let mut lexer = Lexer::new("'hello world'");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("hello world".into()));
    }

    #[test]
    fn test_keywords() {
        let mut lexer = Lexer::new("CREATE TABLE IF NOT EXISTS");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Create);
        assert_eq!(tokens[1].token, Token::Table);
        assert_eq!(tokens[2].token, Token::If);
        assert_eq!(tokens[3].token, Token::Not);
        assert_eq!(tokens[4].token, Token::Exists);
    }

    #[test]
    fn test_operators() {
        let mut lexer = Lexer::new("<= >= <> || != < > =");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Le);
        assert_eq!(tokens[1].token, Token::Ge);
        assert_eq!(tokens[2].token, Token::Ne);
        assert_eq!(tokens[3].token, Token::Concat);
        assert_eq!(tokens[4].token, Token::Ne);
        assert_eq!(tokens[5].token, Token::Lt);
        assert_eq!(tokens[6].token, Token::Gt);
        assert_eq!(tokens[7].token, Token::Eq);
    }

    #[test]
    fn test_real_numbers() {
        let mut lexer = Lexer::new("3.14 42 0.5");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Real(3.14));
        assert_eq!(tokens[1].token, Token::Integer(42));
        assert_eq!(tokens[2].token, Token::Real(0.5));
    }

    #[test]
    fn test_split_statements() {
        let stmts = split_statements("SELECT 1; SELECT 2; SELECT 3");
        assert_eq!(stmts.len(), 3);
        assert_eq!(stmts[0], "SELECT 1");
        assert_eq!(stmts[1], "SELECT 2");
        assert_eq!(stmts[2], "SELECT 3");
    }

    #[test]
    fn test_split_statements_with_strings() {
        let stmts = split_statements("INSERT INTO t VALUES ('a;b'); SELECT 1");
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "INSERT INTO t VALUES ('a;b')");
        assert_eq!(stmts[1], "SELECT 1");
    }
}
