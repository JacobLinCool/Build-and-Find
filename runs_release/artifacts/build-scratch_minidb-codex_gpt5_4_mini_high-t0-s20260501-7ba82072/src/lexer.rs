use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Comma,
    Dot,
    Star,
    LParen,
    RParen,
    Plus,
    Minus,
    Slash,
    Percent,
    PipePipe,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    Semicolon,
    Keyword(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    lookahead: Option<char>,
    line: usize,
    column: usize,
    finished: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut chars = input.chars();
        let lookahead = chars.next();
        Self {
            chars,
            lookahead,
            line: 1,
            column: 1,
            finished: false,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let current = self.lookahead;
        if let Some(ch) = current {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        self.lookahead = self.chars.next();
        current
    }

    fn peek(&self) -> Option<char> {
        self.lookahead
    }

    fn token(&self, kind: TokenKind, line: usize, column: usize) -> Token {
        Token { kind, line, column }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
                self.bump();
            }
            let mut consumed = false;
            if self.peek() == Some('-') {
                let line = self.line;
                let col = self.column;
                self.bump();
                if self.peek() == Some('-') {
                    consumed = true;
                    while let Some(ch) = self.peek() {
                        self.bump();
                        if ch == '\n' {
                            break;
                        }
                    }
                } else {
                    self.lookahead = Some('-');
                    self.line = line;
                    self.column = col;
                    break;
                }
            }
            if self.peek() == Some('/') {
                let line = self.line;
                let col = self.column;
                self.bump();
                if self.peek() == Some('*') {
                    consumed = true;
                    self.bump();
                    while let Some(ch) = self.peek() {
                        self.bump();
                        if ch == '*' && self.peek() == Some('/') {
                            self.bump();
                            break;
                        }
                    }
                } else {
                    self.lookahead = Some('/');
                    self.line = line;
                    self.column = col;
                    break;
                }
            }
            if !consumed {
                break;
            }
        }
    }

    fn lex_ident_or_keyword(&mut self) -> TokenKind {
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
            "SELECT" | "FROM" | "WHERE" | "CREATE" | "TABLE" | "DROP" | "INDEX" | "INSERT"
            | "INTO" | "VALUES" | "UPDATE" | "SET" | "DELETE" | "BEGIN" | "COMMIT" | "ROLLBACK"
            | "PRIMARY" | "KEY" | "NOT" | "NULL" | "UNIQUE" | "DEFAULT" | "CHECK" | "INNER"
            | "JOIN" | "ON" | "AND" | "OR" | "AS" | "ORDER" | "BY" | "ASC" | "DESC" | "LIMIT"
            | "OFFSET" | "INT" | "BIGINT" | "REAL" | "TEXT" | "BOOL" | "IS" | "TRUE" | "FALSE"
            | "LENGTH" | "LOWER" | "UPPER" | "SUBSTR" | "ABS" | "COALESCE" => {
                TokenKind::Keyword(upper)
            }
            _ => TokenKind::Ident(s),
        }
    }

    fn lex_number(&mut self) -> TokenKind {
        let mut s = String::new();
        let mut seen_dot = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.bump();
            } else if ch == '.' && !seen_dot {
                seen_dot = true;
                s.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        TokenKind::Number(s)
    }

    fn lex_string(&mut self) -> Result<TokenKind> {
        self.bump();
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            self.bump();
            match ch {
                '\'' => {
                    if self.peek() == Some('\'') {
                        s.push('\'');
                        self.bump();
                    } else {
                        return Ok(TokenKind::String(s));
                    }
                }
                '\\' => {
                    if let Some(next) = self.peek() {
                        self.bump();
                        s.push(match next {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            '\\' => '\\',
                            '\'' => '\'',
                            other => other,
                        });
                    }
                }
                other => s.push(other),
            }
        }
        Err(MiniDbError::Lex {
            line: self.line,
            column: self.column,
            message: "unterminated string literal".to_string(),
        })
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Result<Token>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        self.skip_ws_and_comments();
        let line = self.line;
        let column = self.column;
        let ch = match self.peek() {
            Some(ch) => ch,
            None => {
                self.finished = true;
                return Some(Ok(self.token(TokenKind::Eof, line, column)));
            }
        };
        let kind = match ch {
            ',' => {
                self.bump();
                TokenKind::Comma
            }
            '.' => {
                self.bump();
                TokenKind::Dot
            }
            '*' => {
                self.bump();
                TokenKind::Star
            }
            '(' => {
                self.bump();
                TokenKind::LParen
            }
            ')' => {
                self.bump();
                TokenKind::RParen
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
            '%' => {
                self.bump();
                TokenKind::Percent
            }
            ';' => {
                self.bump();
                TokenKind::Semicolon
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
                        TokenKind::Neq
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
            '|' => {
                self.bump();
                if self.peek() == Some('|') {
                    self.bump();
                    TokenKind::PipePipe
                } else {
                    return Some(Err(MiniDbError::Lex {
                        line,
                        column,
                        message: "unexpected '|'".to_string(),
                    }));
                }
            }
            '\'' => match self.lex_string() {
                Ok(kind) => kind,
                Err(err) => return Some(Err(err)),
            },
            ch if ch.is_ascii_digit() => self.lex_number(),
            ch if ch.is_ascii_alphabetic() || ch == '_' => self.lex_ident_or_keyword(),
            _ => {
                self.bump();
                return Some(Err(MiniDbError::Lex {
                    line,
                    column,
                    message: format!("unexpected character '{ch}'"),
                }));
            }
        };
        Some(Ok(self.token(kind, line, column)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_basic_sql() {
        let tokens = Lexer::new("SELECT a, b FROM t;")
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Keyword(ref k) if k == "SELECT"));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "a"));
    }
}
