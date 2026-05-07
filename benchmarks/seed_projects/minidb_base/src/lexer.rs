//! Hand-written SQL lexer.
//!
//! Produces a flat [`Token`] stream for the parser. Keywords are matched
//! case-insensitively. String literals use single quotes with `''` as the
//! escape for an embedded quote. Numeric literals recognise both integers
//! and real numbers.

use crate::error::{MiniDbError, Result};

/// A single lexical unit.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Select,
    From,
    Where,
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
    Inner,
    Join,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    And,
    Or,
    Not,
    Null,
    True,
    False,
    Begin,
    Commit,
    Rollback,
    Primary,
    Key,
    Unique,
    Default,
    As,
    Is,

    // Literals
    IntLit(i64),
    RealLit(f64),
    StringLit(String),

    // Identifier
    Ident(String),

    // Punctuation
    Comma,
    Semicolon,
    LParen,
    RParen,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    Dot,

    // Sentinel
    Eof,
}

impl Token {
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Token::Select
                | Token::From
                | Token::Where
                | Token::Insert
                | Token::Into
                | Token::Values
                | Token::Update
                | Token::Set
                | Token::Delete
                | Token::Create
                | Token::Drop
                | Token::Table
                | Token::Index
                | Token::On
                | Token::Inner
                | Token::Join
                | Token::Order
                | Token::By
                | Token::Asc
                | Token::Desc
                | Token::Limit
                | Token::Offset
                | Token::And
                | Token::Or
                | Token::Not
                | Token::Null
                | Token::True
                | Token::False
                | Token::Begin
                | Token::Commit
                | Token::Rollback
                | Token::Primary
                | Token::Key
                | Token::Unique
                | Token::Default
                | Token::As
                | Token::Is
        )
    }
}

fn keyword_for(word: &str) -> Option<Token> {
    let up = word.to_ascii_uppercase();
    Some(match up.as_str() {
        "SELECT" => Token::Select,
        "FROM" => Token::From,
        "WHERE" => Token::Where,
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
        "INNER" => Token::Inner,
        "JOIN" => Token::Join,
        "ORDER" => Token::Order,
        "BY" => Token::By,
        "ASC" => Token::Asc,
        "DESC" => Token::Desc,
        "LIMIT" => Token::Limit,
        "OFFSET" => Token::Offset,
        "AND" => Token::And,
        "OR" => Token::Or,
        "NOT" => Token::Not,
        "NULL" => Token::Null,
        "TRUE" => Token::True,
        "FALSE" => Token::False,
        "BEGIN" => Token::Begin,
        "COMMIT" => Token::Commit,
        "ROLLBACK" => Token::Rollback,
        "PRIMARY" => Token::Primary,
        "KEY" => Token::Key,
        "UNIQUE" => Token::Unique,
        "DEFAULT" => Token::Default,
        "AS" => Token::As,
        "IS" => Token::Is,
        _ => return None,
    })
}

/// Lex `source` into a `Vec<Token>` terminated with [`Token::Eof`].
pub fn tokenize(source: &str) -> Result<Vec<Token>> {
    let bytes: Vec<char> = source.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '-' && i + 1 < bytes.len() && bytes[i + 1] == '-' {
            // line comment
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            let mut seen_dot = false;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                if bytes[i] == '.' {
                    if seen_dot {
                        break;
                    }
                    seen_dot = true;
                }
                i += 1;
            }
            let lit: String = bytes[start..i].iter().collect();
            if seen_dot {
                out.push(Token::RealLit(lit.parse::<f64>()?));
            } else {
                out.push(Token::IntLit(lit.parse::<i64>()?));
            }
            continue;
        }
        if c == '\'' {
            i += 1;
            let mut s = String::new();
            while i < bytes.len() {
                if bytes[i] == '\'' {
                    if i + 1 < bytes.len() && bytes[i + 1] == '\'' {
                        s.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    out.push(Token::StringLit(s));
                    break;
                }
                s.push(bytes[i]);
                i += 1;
                if i == bytes.len() {
                    return Err(MiniDbError::Parse("unterminated string literal".into()));
                }
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let word: String = bytes[start..i].iter().collect();
            out.push(match keyword_for(&word) {
                Some(k) => k,
                None => Token::Ident(word),
            });
            continue;
        }
        // operators and punctuation
        let next = if i + 1 < bytes.len() {
            Some(bytes[i + 1])
        } else {
            None
        };
        let tok = match (c, next) {
            ('<', Some('=')) => {
                i += 2;
                Token::Le
            }
            ('>', Some('=')) => {
                i += 2;
                Token::Ge
            }
            ('!', Some('=')) => {
                i += 2;
                Token::Neq
            }
            ('<', Some('>')) => {
                i += 2;
                Token::Neq
            }
            _ => {
                i += 1;
                match c {
                    ',' => Token::Comma,
                    ';' => Token::Semicolon,
                    '(' => Token::LParen,
                    ')' => Token::RParen,
                    '*' => Token::Star,
                    '+' => Token::Plus,
                    '-' => Token::Minus,
                    '/' => Token::Slash,
                    '%' => Token::Percent,
                    '=' => Token::Eq,
                    '<' => Token::Lt,
                    '>' => Token::Gt,
                    '.' => Token::Dot,
                    other => {
                        return Err(MiniDbError::Parse(format!(
                            "unexpected character `{other}`"
                        )))
                    }
                }
            }
        };
        out.push(tok);
    }
    out.push(Token::Eof);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_basic_select() {
        let toks = tokenize("SELECT id FROM users WHERE id = 1;").unwrap();
        assert_eq!(toks[0], Token::Select);
        assert!(matches!(toks[1], Token::Ident(ref s) if s == "id"));
        assert_eq!(toks[2], Token::From);
    }

    #[test]
    fn lexes_string_with_escape() {
        let toks = tokenize("'it''s'").unwrap();
        assert_eq!(toks[0], Token::StringLit("it's".into()));
    }
}
