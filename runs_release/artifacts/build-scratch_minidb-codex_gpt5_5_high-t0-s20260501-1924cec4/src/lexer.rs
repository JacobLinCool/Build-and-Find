use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Keyword(String),
    Ident(String),
    Number(String),
    String(String),
    Symbol(char),
    Operator(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

impl Token {
    fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Self { kind, line, column }
    }
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\n' => {
                    self.bump();
                }
                '-' if self.peek_next() == Some('-') => self.skip_line_comment(),
                '/' if self.peek_next() == Some('*') => self.skip_block_comment()?,
                '\'' => tokens.push(self.read_string()?),
                '0'..='9' => tokens.push(self.read_number()),
                'a'..='z' | 'A'..='Z' | '_' => tokens.push(self.read_word()),
                '(' | ')' | ',' | ';' | '.' => {
                    let line = self.line;
                    let column = self.column;
                    let value = self.bump().unwrap_or(ch);
                    tokens.push(Token::new(TokenKind::Symbol(value), line, column));
                }
                '*' | '+' | '-' | '/' => {
                    let line = self.line;
                    let column = self.column;
                    let value = self.bump().unwrap_or(ch);
                    tokens.push(Token::new(
                        TokenKind::Operator(value.to_string()),
                        line,
                        column,
                    ));
                }
                '|' if self.peek_next() == Some('|') => {
                    let line = self.line;
                    let column = self.column;
                    self.bump();
                    self.bump();
                    tokens.push(Token::new(TokenKind::Operator("||".into()), line, column));
                }
                '<' | '>' | '=' | '!' => tokens.push(self.read_operator()?),
                _ => {
                    return Err(MiniDbError::parse(
                        format!("unexpected character {ch:?}"),
                        self.line,
                        self.column,
                    ));
                }
            }
        }
        tokens.push(Token::new(TokenKind::Eof, self.line, self.column));
        Ok(tokens)
    }

    pub fn input(&self) -> &'a str {
        self.input
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.peek() {
            self.bump();
            if ch == '\n' {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) -> Result<()> {
        let start_line = self.line;
        let start_col = self.column;
        self.bump();
        self.bump();
        while let Some(ch) = self.peek() {
            if ch == '*' && self.peek_next() == Some('/') {
                self.bump();
                self.bump();
                return Ok(());
            }
            self.bump();
        }
        Err(MiniDbError::parse(
            "unterminated block comment",
            start_line,
            start_col,
        ))
    }

    fn read_string(&mut self) -> Result<Token> {
        let line = self.line;
        let column = self.column;
        self.bump();
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            self.bump();
            if ch == '\'' {
                if self.peek() == Some('\'') {
                    self.bump();
                    value.push('\'');
                } else {
                    return Ok(Token::new(TokenKind::String(value), line, column));
                }
            } else {
                value.push(ch);
            }
        }
        Err(MiniDbError::parse("unterminated string", line, column))
    }

    fn read_number(&mut self) -> Token {
        let line = self.line;
        let column = self.column;
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '.' {
                value.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        Token::new(TokenKind::Number(value), line, column)
    }

    fn read_word(&mut self) -> Token {
        let line = self.line;
        let column = self.column;
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                value.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        let upper = value.to_ascii_uppercase();
        if is_keyword(&upper) {
            Token::new(TokenKind::Keyword(upper), line, column)
        } else {
            Token::new(TokenKind::Ident(value), line, column)
        }
    }

    fn read_operator(&mut self) -> Result<Token> {
        let line = self.line;
        let column = self.column;
        let first = self.bump().unwrap_or('=');
        let op = match (first, self.peek()) {
            ('<', Some('=')) | ('>', Some('=')) | ('<', Some('>')) | ('!', Some('=')) => {
                let second = self.bump().unwrap_or('=');
                format!("{first}{second}")
            }
            ('<', _) | ('>', _) | ('=', _) => first.to_string(),
            _ => {
                return Err(MiniDbError::parse(
                    format!("unexpected operator {first}"),
                    line,
                    column,
                ))
            }
        };
        Ok(Token::new(TokenKind::Operator(op), line, column))
    }
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "CREATE"
            | "TABLE"
            | "DROP"
            | "INDEX"
            | "ON"
            | "INSERT"
            | "INTO"
            | "VALUES"
            | "SELECT"
            | "FROM"
            | "WHERE"
            | "ORDER"
            | "BY"
            | "ASC"
            | "DESC"
            | "LIMIT"
            | "OFFSET"
            | "UPDATE"
            | "SET"
            | "DELETE"
            | "BEGIN"
            | "COMMIT"
            | "ROLLBACK"
            | "PRIMARY"
            | "KEY"
            | "NOT"
            | "NULL"
            | "UNIQUE"
            | "DEFAULT"
            | "CHECK"
            | "INT"
            | "BIGINT"
            | "REAL"
            | "TEXT"
            | "BOOL"
            | "TRUE"
            | "FALSE"
            | "AND"
            | "OR"
            | "IS"
            | "INNER"
            | "JOIN"
            | "AS"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_keywords_identifiers_and_strings() {
        let tokens = Lexer::new("select 'a''b', x from t;").tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Keyword(k) if k == "SELECT"));
        assert!(matches!(&tokens[1].kind, TokenKind::String(s) if s == "a'b"));
    }

    #[test]
    fn reports_unterminated_string_location() {
        let err = Lexer::new("'x").tokenize().unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }
}
