use crate::error::MiniDbError;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Comma,
    Dot,
    LParen,
    RParen,
    Star,
    Plus,
    Minus,
    Slash,
    PipePipe,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    Semi,
    Keyword(String),
    EOF,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    cur: Option<char>,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        let mut chars = src.chars();
        let cur = chars.next();
        Self {
            chars,
            cur,
            line: 1,
            col: 1,
        }
    }
    fn bump(&mut self) {
        if let Some(c) = self.cur {
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        self.cur = self.chars.next();
    }
    fn take_while<F: Fn(char) -> bool>(&mut self, f: F) -> String {
        let mut s = String::new();
        while let Some(c) = self.cur {
            if f(c) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        s
    }
    pub fn tokenize(mut self) -> Result<Vec<Token>, MiniDbError> {
        let mut toks = Vec::new();
        while let Some(c) = self.cur {
            let line = self.line;
            let column = self.col;
            if c.is_whitespace() {
                self.bump();
                continue;
            }
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
                ';' => {
                    self.bump();
                    TokenKind::Semi
                }
                '=' => {
                    self.bump();
                    TokenKind::Eq
                }
                '<' => {
                    self.bump();
                    match self.cur {
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
                    match self.cur {
                        Some('=') => {
                            self.bump();
                            TokenKind::Gte
                        }
                        _ => TokenKind::Gt,
                    }
                }
                '|' => {
                    self.bump();
                    if self.cur == Some('|') {
                        self.bump();
                        TokenKind::PipePipe
                    } else {
                        return Err(MiniDbError::sql(format!("unexpected | at {line}:{column}")));
                    }
                }
                '\'' => {
                    self.bump();
                    let mut s = String::new();
                    while let Some(ch) = self.cur {
                        if ch == '\'' {
                            self.bump();
                            break;
                        }
                        s.push(ch);
                        self.bump();
                    }
                    TokenKind::String(s)
                }
                c if c.is_ascii_digit() => {
                    TokenKind::Number(self.take_while(|ch| ch.is_ascii_digit() || ch == '.'))
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let ident = self.take_while(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                    let upper = ident.to_ascii_uppercase();
                    match upper.as_str() {
                        "SELECT" | "FROM" | "WHERE" | "CREATE" | "TABLE" | "DROP" | "INDEX"
                        | "INSERT" | "INTO" | "VALUES" | "UPDATE" | "SET" | "DELETE" | "BEGIN"
                        | "COMMIT" | "ROLLBACK" | "PRIMARY" | "KEY" | "NOT" | "NULL" | "UNIQUE"
                        | "DEFAULT" | "CHECK" | "AND" | "OR" | "AS" | "ORDER" | "BY" | "ASC"
                        | "DESC" | "LIMIT" | "OFFSET" | "JOIN" | "INNER" | "ON" | "IS" | "INT"
                        | "BIGINT" | "REAL" | "TEXT" | "BOOL" => TokenKind::Keyword(upper),
                        _ => TokenKind::Ident(ident),
                    }
                }
                _ => {
                    return Err(MiniDbError::sql(format!(
                        "unexpected character {c} at {line}:{column}"
                    )))
                }
            };
            toks.push(Token { kind, line, column });
        }
        toks.push(Token {
            kind: TokenKind::EOF,
            line: self.line,
            column: self.col,
        });
        Ok(toks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_keywords() {
        let toks = Lexer::new("select * from t;").tokenize().unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(_)));
    }
}
