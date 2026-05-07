use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Symbol(String),
    Keyword(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

const KEYWORDS: &[&str] = &[
    "CREATE", "TABLE", "DROP", "INDEX", "ON", "INSERT", "INTO", "VALUES", "SELECT", "FROM",
    "WHERE", "ORDER", "BY", "ASC", "DESC", "LIMIT", "OFFSET", "UPDATE", "SET", "DELETE", "BEGIN",
    "COMMIT", "ROLLBACK", "PRIMARY", "KEY", "NOT", "NULL", "UNIQUE", "DEFAULT", "CHECK", "INT",
    "BIGINT", "REAL", "TEXT", "BOOL", "INNER", "JOIN", "AND", "OR", "IS", "TRUE", "FALSE", "AS",
];

pub fn lex(input: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    let (mut line, mut col) = (1usize, 1usize);
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            continue;
        }
        let start = col;
        if c == '-' {
            chars.next();
            col += 1;
            if chars.peek() == Some(&'-') {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        line += 1;
                        col = 1;
                        break;
                    }
                }
                continue;
            }
            out.push(Token {
                kind: TokenKind::Symbol("-".into()),
                line,
                column: start,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                    s.push(ch);
                    chars.next();
                    col += 1;
                } else {
                    break;
                }
            }
            let up = s.to_ascii_uppercase();
            let kind = if KEYWORDS.contains(&up.as_str()) {
                TokenKind::Keyword(up)
            } else {
                TokenKind::Ident(s)
            };
            out.push(Token {
                kind,
                line,
                column: start,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let mut s = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_digit() || ch == '.' {
                    s.push(ch);
                    chars.next();
                    col += 1;
                } else {
                    break;
                }
            }
            out.push(Token {
                kind: TokenKind::Number(s),
                line,
                column: start,
            });
            continue;
        }
        if c == '\'' {
            chars.next();
            col += 1;
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('\'') => {
                        col += 1;
                        break;
                    }
                    Some(ch) => {
                        s.push(ch);
                        if ch == '\n' {
                            line += 1;
                            col = 1;
                        } else {
                            col += 1;
                        }
                    }
                    None => return Err(MiniDbError::parse("unterminated string", line, col)),
                }
            }
            out.push(Token {
                kind: TokenKind::String(s),
                line,
                column: start,
            });
            continue;
        }
        let mut sym = c.to_string();
        chars.next();
        col += 1;
        if let Some(&n) = chars.peek() {
            let two = format!("{sym}{n}");
            if ["<>", "<=", ">=", "||"].contains(&two.as_str()) {
                sym = two;
                chars.next();
                col += 1;
            }
        }
        out.push(Token {
            kind: TokenKind::Symbol(sym),
            line,
            column: start,
        });
    }
    out.push(Token {
        kind: TokenKind::Eof,
        line,
        column: col,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_select() {
        assert!(lex("select 'x';").unwrap().len() > 2);
    }
}
