use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Keyword(String),
    Ident(String),
    Number(String),
    String(String),
    Symbol(char),
    Op(String),
    Eof,
}

impl TokenKind {
    pub fn text(&self) -> String {
        match self {
            TokenKind::Keyword(s) | TokenKind::Ident(s) | TokenKind::Number(s) => s.clone(),
            TokenKind::String(s) => format!("'{s}'"),
            TokenKind::Symbol(c) => c.to_string(),
            TokenKind::Op(s) => s.clone(),
            TokenKind::Eof => "<eof>".to_string(),
        }
    }
}

const KEYWORDS: &[&str] = &[
    "CREATE", "TABLE", "DROP", "INDEX", "ON", "INSERT", "INTO", "VALUES", "SELECT", "FROM",
    "INNER", "JOIN", "WHERE", "ORDER", "BY", "ASC", "DESC", "LIMIT", "OFFSET", "UPDATE", "SET",
    "DELETE", "BEGIN", "COMMIT", "ROLLBACK", "PRIMARY", "KEY", "NOT", "NULL", "UNIQUE", "DEFAULT",
    "CHECK", "INT", "BIGINT", "REAL", "TEXT", "BOOL", "TRUE", "FALSE", "AND", "OR", "IS", "AS",
];

pub fn lex(sql: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    let mut line = 1usize;
    let mut col = 1usize;
    while let Some(ch) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            continue;
        }
        if ch == '-' {
            let mut clone = chars.clone();
            clone.next();
            if clone.peek() == Some(&'-') {
                chars.next();
                chars.next();
                col += 2;
                while let Some(c) = chars.peek().copied() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                    col += 1;
                }
                continue;
            }
        }
        let start_line = line;
        let start_col = col;
        if ch == '\'' {
            chars.next();
            col += 1;
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('\'') => {
                        col += 1;
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                            col += 1;
                            s.push('\'');
                        } else {
                            break;
                        }
                    }
                    Some(c) => {
                        if c == '\n' {
                            line += 1;
                            col = 1;
                        } else {
                            col += 1;
                        }
                        s.push(c);
                    }
                    None => {
                        return Err(MiniDbError::parse(
                            "unterminated string literal",
                            start_line,
                            start_col,
                        ));
                    }
                }
            }
            out.push(Token {
                kind: TokenKind::String(s),
                line: start_line,
                column: start_col,
            });
            continue;
        }
        if ch.is_ascii_digit() {
            let mut s = String::new();
            while let Some(c) = chars.peek().copied() {
                if c.is_ascii_digit() || c == '.' {
                    s.push(c);
                    chars.next();
                    col += 1;
                } else {
                    break;
                }
            }
            out.push(Token {
                kind: TokenKind::Number(s),
                line: start_line,
                column: start_col,
            });
            continue;
        }
        if is_ident_start(ch) {
            let mut s = String::new();
            while let Some(c) = chars.peek().copied() {
                if is_ident_continue(c) {
                    s.push(c);
                    chars.next();
                    col += 1;
                } else {
                    break;
                }
            }
            let upper = s.to_ascii_uppercase();
            let kind = if KEYWORDS.contains(&upper.as_str()) {
                TokenKind::Keyword(upper)
            } else {
                TokenKind::Ident(s)
            };
            out.push(Token {
                kind,
                line: start_line,
                column: start_col,
            });
            continue;
        }
        let two = {
            let mut clone = chars.clone();
            let first = clone.next();
            let second = clone.next();
            match (first, second) {
                (Some(a), Some(b)) => Some(format!("{a}{b}")),
                _ => None,
            }
        };
        if let Some(op) = two {
            if matches!(op.as_str(), "<=" | ">=" | "<>" | "||") {
                chars.next();
                chars.next();
                col += 2;
                out.push(Token {
                    kind: TokenKind::Op(op),
                    line: start_line,
                    column: start_col,
                });
                continue;
            }
        }
        chars.next();
        col += 1;
        let kind = match ch {
            '(' | ')' | ',' | ';' | '*' | '.' => TokenKind::Symbol(ch),
            '+' | '-' | '/' | '=' | '<' | '>' => TokenKind::Op(ch.to_string()),
            _ => {
                return Err(MiniDbError::parse(
                    format!("unexpected character {ch:?}"),
                    start_line,
                    start_col,
                ));
            }
        };
        out.push(Token {
            kind,
            line: start_line,
            column: start_col,
        });
    }
    out.push(Token {
        kind: TokenKind::Eof,
        line,
        column: col,
    });
    Ok(out)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_strings_and_operators() {
        let tokens = lex("select 'a''b' || x from t where y <> 1;").unwrap();
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::String("a'b".into()))
        );
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Op("||".into())));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Op("<>".into())));
    }
}
