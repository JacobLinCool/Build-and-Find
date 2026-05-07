use crate::error::{MiniDbError, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Symbol(char),
    Op(String),
    DotCommand(String),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
    col: usize,
    at_line_start: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
            line: 1,
            col: 0,
            at_line_start: true,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        if ch == '\n' {
            self.line += 1;
            self.col = 0;
            self.at_line_start = true;
        } else {
            self.col += 1;
            if !ch.is_whitespace() {
                self.at_line_start = false;
            }
        }
        Some(ch)
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut out = Vec::new();
        while let Some(&ch) = self.chars.peek() {
            if ch.is_whitespace() {
                self.bump();
                continue;
            }
            let line = self.line;
            let column = self.col + 1;
            if ch == '-' {
                self.bump();
                if self.chars.peek() == Some(&'-') {
                    while self.bump().is_some_and(|c| c != '\n') {}
                    continue;
                }
                out.push(Token {
                    kind: TokenKind::Op("-".into()),
                    line,
                    column,
                });
                continue;
            }
            if ch == '.' && self.at_line_start {
                let mut s = String::new();
                while let Some(&c) = self.chars.peek() {
                    if c == '\n' || c == ';' {
                        break;
                    }
                    s.push(c);
                    self.bump();
                }
                out.push(Token {
                    kind: TokenKind::DotCommand(s.trim().to_string()),
                    line,
                    column,
                });
                continue;
            }
            if ch.is_ascii_alphabetic() || ch == '_' {
                let mut s = String::new();
                while let Some(&c) = self.chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        s.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    kind: TokenKind::Ident(s),
                    line,
                    column,
                });
                continue;
            }
            if ch.is_ascii_digit() {
                let mut s = String::new();
                while let Some(&c) = self.chars.peek() {
                    if c.is_ascii_digit() || c == '.' {
                        s.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    kind: TokenKind::Number(s),
                    line,
                    column,
                });
                continue;
            }
            if ch == '\'' {
                self.bump();
                let mut s = String::new();
                loop {
                    match self.bump() {
                        Some('\'') => {
                            if self.chars.peek() == Some(&'\'') {
                                self.bump();
                                s.push('\'');
                            } else {
                                break;
                            }
                        }
                        Some(c) => s.push(c),
                        None => {
                            return Err(MiniDbError::parse("unterminated string", line, column));
                        }
                    }
                }
                out.push(Token {
                    kind: TokenKind::String(s),
                    line,
                    column,
                });
                continue;
            }
            let two = {
                let mut it = self.chars.clone();
                let a = it.next();
                let b = it.next();
                a.zip(b).map(|(a, b)| format!("{a}{b}"))
            };
            if matches!(two.as_deref(), Some("<=" | ">=" | "<>" | "||")) {
                let op = match two {
                    Some(op) => op,
                    None => String::new(),
                };
                self.bump();
                self.bump();
                out.push(Token {
                    kind: TokenKind::Op(op),
                    line,
                    column,
                });
                continue;
            }
            self.bump();
            let kind = match ch {
                '(' | ')' | ',' | ';' | '*' | '=' | '<' | '>' | '+' | '/' | '.' => {
                    if "=<>+/".contains(ch) {
                        TokenKind::Op(ch.to_string())
                    } else {
                        TokenKind::Symbol(ch)
                    }
                }
                _ => {
                    return Err(MiniDbError::parse(
                        format!("unexpected character '{ch}'"),
                        line,
                        column,
                    ));
                }
            };
            out.push(Token { kind, line, column });
        }
        out.push(Token {
            kind: TokenKind::Eof,
            line: self.line,
            column: self.col + 1,
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_strings_and_ops() {
        let t = Lexer::new("select 'a''b' <> 3;").tokenize().unwrap();
        assert!(t.iter().any(|x| x.kind == TokenKind::String("a'b".into())));
    }
}
