use crate::error::{MiniDbError, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Comma,
    Dot,
    LParen,
    RParen,
    Semicolon,
    Star,
    Plus,
    Minus,
    Slash,
    PipePipe,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Kw(String),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    peeked: Option<char>,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars(),
            peeked: None,
            line: 1,
            column: 0,
        }
    }
    fn bump(&mut self) -> Option<char> {
        let ch = self.peeked.take().or_else(|| self.chars.next())?;
        if ch == '\n' {
            self.line += 1;
            self.column = 0;
        } else {
            self.column += 1;
        }
        Some(ch)
    }
    fn peek(&mut self) -> Option<char> {
        if self.peeked.is_none() {
            self.peeked = self.chars.next();
        }
        self.peeked
    }
    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.bump();
                continue;
            }
            let line = self.line;
            let column = self.column + 1;
            let kind = match c {
                ',' => {
                    self.bump();
                    TokenKind::Comma
                }
                '.' => {
                    self.bump();
                    TokenKind::Dot
                }
                '(' => {
                    self.bump();
                    TokenKind::LParen
                }
                ')' => {
                    self.bump();
                    TokenKind::RParen
                }
                ';' => {
                    self.bump();
                    TokenKind::Semicolon
                }
                '*' => {
                    self.bump();
                    TokenKind::Star
                }
                '+' => {
                    self.bump();
                    TokenKind::Plus
                }
                '-' => {
                    self.bump();
                    TokenKind::Minus
                }
                '/' => {
                    self.bump();
                    TokenKind::Slash
                }
                '|' => {
                    self.bump();
                    if self.peek() == Some('|') {
                        self.bump();
                        TokenKind::PipePipe
                    } else {
                        return Err(MiniDbError::Parse {
                            line,
                            column,
                            message: "unexpected '|'".into(),
                        });
                    }
                }
                '=' => {
                    self.bump();
                    TokenKind::Eq
                }
                '<' => {
                    self.bump();
                    match self.peek() {
                        Some('=') => {
                            self.bump();
                            TokenKind::Lte
                        }
                        Some('>') => {
                            self.bump();
                            TokenKind::Ne
                        }
                        _ => TokenKind::Lt,
                    }
                }
                '>' => {
                    self.bump();
                    match self.peek() {
                        Some('=') => {
                            self.bump();
                            TokenKind::Gte
                        }
                        _ => TokenKind::Gt,
                    }
                }
                '\'' => {
                    self.bump();
                    let mut s = String::new();
                    while let Some(ch) = self.bump() {
                        if ch == '\'' {
                            break;
                        } else {
                            s.push(ch);
                        }
                    }
                    TokenKind::String(s)
                }
                d if d.is_ascii_digit() => {
                    let mut s = String::new();
                    while let Some(ch) = self.peek() {
                        if ch.is_ascii_digit() || ch == '.' {
                            s.push(ch);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    TokenKind::Number(s)
                }
                a if a.is_ascii_alphabetic() || a == '_' => {
                    let mut s = String::new();
                    while let Some(ch) = self.peek() {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            s.push(ch);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let upper = s.to_ascii_uppercase();
                    match upper.as_str() {
                        "SELECT" | "FROM" | "WHERE" | "CREATE" | "TABLE" | "DROP" | "INDEX"
                        | "INSERT" | "INTO" | "VALUES" | "UPDATE" | "SET" | "DELETE" | "BEGIN"
                        | "COMMIT" | "ROLLBACK" | "AS" | "ON" | "INNER" | "JOIN" | "ORDER"
                        | "BY" | "ASC" | "DESC" | "LIMIT" | "OFFSET" | "AND" | "OR" | "NOT"
                        | "IS" | "NULL" | "PRIMARY" | "KEY" | "UNIQUE" | "DEFAULT" | "CHECK"
                        | "INT" | "BIGINT" | "REAL" | "TEXT" | "BOOL" | "TRUE" | "FALSE"
                        | "LENGTH" | "LOWER" | "UPPER" | "SUBSTR" | "ABS" | "COALESCE" => {
                            TokenKind::Kw(upper)
                        }
                        _ => TokenKind::Ident(s),
                    }
                }
                _ => {
                    return Err(MiniDbError::Parse {
                        line,
                        column,
                        message: format!("unexpected char {c}"),
                    });
                }
            };
            out.push(Token { kind, line, column });
        }
        out.push(Token {
            kind: TokenKind::Eof,
            line: self.line,
            column: self.column + 1,
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_keywords() {
        assert!(!Lexer::new("select 1").tokenize().unwrap().is_empty());
    }
}
