use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenKind {
    Identifier(String),
    Number(String),
    String(String),
    Comma,
    Dot,
    Star,
    LParen,
    RParen,
    Semicolon,
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
    Bang,
    Keyword(Keyword),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Keyword {
    Select,
    From,
    Where,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
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
    Primary,
    Key,
    Not,
    Null,
    Unique,
    Default,
    Check,
    Begin,
    Commit,
    Rollback,
    And,
    Or,
    Is,
    Join,
    Inner,
    On,
    As,
    True,
    False,
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            column: 1,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peeked.take().or_else(|| self.chars.next())?;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
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

    fn current_position(&self) -> (usize, usize) {
        (self.line, self.column)
    }

    fn consume_while<F: Fn(char) -> bool>(&mut self, pred: F) -> String {
        let mut buf = String::new();
        while let Some(ch) = self.peek() {
            if pred(ch) {
                buf.push(self.bump().unwrap_or(ch));
            } else {
                break;
            }
        }
        buf
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
                self.bump();
            }
            let next = self.chars.clone().next();
            if self.peek() == Some('-') && next == Some('-') {
                self.bump();
                self.bump();
                while let Some(ch) = self.peek() {
                    self.bump();
                    if ch == '\n' {
                        break;
                    }
                }
                continue;
            }
            if self.peek() == Some('/') && next == Some('*') {
                self.bump();
                self.bump();
                let mut prev = '\0';
                while let Some(ch) = self.bump() {
                    if prev == '*' && ch == '/' {
                        break;
                    }
                    prev = ch;
                }
                continue;
            }
            break;
        }
    }

    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_ws_and_comments();
        let (line, column) = self.current_position();
        let token = match self.peek() {
            None => TokenKind::Eof,
            Some('(') => {
                self.bump();
                TokenKind::LParen
            }
            Some(')') => {
                self.bump();
                TokenKind::RParen
            }
            Some(',') => {
                self.bump();
                TokenKind::Comma
            }
            Some('.') => {
                self.bump();
                TokenKind::Dot
            }
            Some('*') => {
                self.bump();
                TokenKind::Star
            }
            Some(';') => {
                self.bump();
                TokenKind::Semicolon
            }
            Some('+') => {
                self.bump();
                TokenKind::Plus
            }
            Some('-') => {
                self.bump();
                TokenKind::Minus
            }
            Some('/') => {
                self.bump();
                TokenKind::Slash
            }
            Some('%') => {
                self.bump();
                TokenKind::Percent
            }
            Some('|') => {
                self.bump();
                if self.peek() == Some('|') {
                    self.bump();
                    TokenKind::PipePipe
                } else {
                    return Err(MiniDbError::parse(
                        "unexpected '|'",
                        Some(line),
                        Some(column),
                    ));
                }
            }
            Some('=') => {
                self.bump();
                TokenKind::Eq
            }
            Some('!') => {
                self.bump();
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::Neq
                } else {
                    TokenKind::Bang
                }
            }
            Some('<') => {
                self.bump();
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::Lte
                } else if self.peek() == Some('>') {
                    self.bump();
                    TokenKind::Neq
                } else {
                    TokenKind::Lt
                }
            }
            Some('>') => {
                self.bump();
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::Gte
                } else {
                    TokenKind::Gt
                }
            }
            Some('\'') => {
                self.bump();
                let mut buf = String::new();
                let mut closed = false;
                while let Some(ch) = self.bump() {
                    match ch {
                        '\'' => {
                            if self.peek() == Some('\'') {
                                self.bump();
                                buf.push('\'');
                            } else {
                                closed = true;
                                break;
                            }
                        }
                        _ => buf.push(ch),
                    }
                }
                if !closed {
                    return Err(MiniDbError::parse(
                        "unterminated string literal",
                        Some(line),
                        Some(column),
                    ));
                }
                TokenKind::String(buf)
            }
            Some(ch) if ch.is_ascii_digit() => {
                let mut num = self.consume_while(|c| c.is_ascii_digit());
                if self.peek() == Some('.') {
                    let mut clone = self.chars.clone();
                    if clone.next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                        num.push(self.bump().unwrap_or('.'));
                        num.push_str(&self.consume_while(|c| c.is_ascii_digit()));
                    }
                }
                TokenKind::Number(num)
            }
            Some(ch) if is_ident_start(ch) => {
                let ident = self.consume_while(is_ident_continue);
                let upper = ident.to_ascii_uppercase();
                if let Some(keyword) = keyword_of(&upper) {
                    TokenKind::Keyword(keyword)
                } else {
                    TokenKind::Identifier(ident)
                }
            }
            Some(other) => {
                return Err(MiniDbError::parse(
                    format!("unexpected character '{other}'"),
                    Some(line),
                    Some(column),
                ));
            }
        };
        Ok(Token {
            kind: token,
            line,
            column,
        })
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn keyword_of(upper: &str) -> Option<Keyword> {
    use Keyword::*;
    Some(match upper {
        "SELECT" => Select,
        "FROM" => From,
        "WHERE" => Where,
        "ORDER" => Order,
        "BY" => By,
        "ASC" => Asc,
        "DESC" => Desc,
        "LIMIT" => Limit,
        "OFFSET" => Offset,
        "INSERT" => Insert,
        "INTO" => Into,
        "VALUES" => Values,
        "UPDATE" => Update,
        "SET" => Set,
        "DELETE" => Delete,
        "CREATE" => Create,
        "TABLE" => Table,
        "DROP" => Drop,
        "INDEX" => Index,
        "PRIMARY" => Primary,
        "KEY" => Key,
        "NOT" => Not,
        "NULL" => Null,
        "UNIQUE" => Unique,
        "DEFAULT" => Default,
        "CHECK" => Check,
        "BEGIN" => Begin,
        "COMMIT" => Commit,
        "ROLLBACK" => Rollback,
        "AND" => And,
        "OR" => Or,
        "IS" => Is,
        "JOIN" => Join,
        "INNER" => Inner,
        "ON" => On,
        "AS" => As,
        "TRUE" => True,
        "FALSE" => False,
        "INT" => Int,
        "BIGINT" => BigInt,
        "REAL" => Real,
        "TEXT" => Text,
        "BOOL" => Bool,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_keywords_and_symbols() {
        let mut lexer = Lexer::new("SELECT * FROM t;");
        assert!(matches!(
            lexer.next_token().unwrap().kind,
            TokenKind::Keyword(Keyword::Select)
        ));
        assert!(matches!(lexer.next_token().unwrap().kind, TokenKind::Star));
    }
}
