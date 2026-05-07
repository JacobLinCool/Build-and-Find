use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Create,
    Drop,
    Table,
    Index,
    On,
    Insert,
    Into,
    Values,
    Select,
    From,
    Where,
    Update,
    Set,
    Delete,
    Begin,
    Commit,
    Rollback,
    And,
    Or,
    Not,
    Is,
    Null,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Offset,
    Join,
    Inner,
    Primary,
    Key,
    Unique,
    Default,
    Check,
    As,
    In,
    Between,
    Like,
    Exists,
    True,
    False,
    Int,
    Bigint,
    Real,
    Text,
    Bool,
    Cross,
    Left,
    Right,
    Outer,
    Full,
    Group,
    Having,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Distinct,
    All,
    Any,
    Some,
    Union,
    Except,
    Intersect,
    Case,
    When,
    Then,
    Else,
    End,
    Upper,
    Lower,
    Length,
    Substr,
    Abs,
    Coalesce,

    // Literals
    IntegerLiteral(i64),
    RealLiteral(f64),
    StringLiteral(String),

    // Identifiers
    Identifier(String),

    // Punctuation
    Semicolon,
    Comma,
    Dot,
    LeftParen,
    RightParen,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Concat,

    // Comparison
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,

    // Meta commands
    MetaDot, // The initial dot of a meta-command
}

impl Token {
    pub fn keyword(s: &str) -> Option<Token> {
        match s.to_uppercase().as_str() {
            "CREATE" => Some(Token::Create),
            "DROP" => Some(Token::Drop),
            "TABLE" => Some(Token::Table),
            "INDEX" => Some(Token::Index),
            "ON" => Some(Token::On),
            "INSERT" => Some(Token::Insert),
            "INTO" => Some(Token::Into),
            "VALUES" => Some(Token::Values),
            "SELECT" => Some(Token::Select),
            "FROM" => Some(Token::From),
            "WHERE" => Some(Token::Where),
            "UPDATE" => Some(Token::Update),
            "SET" => Some(Token::Set),
            "DELETE" => Some(Token::Delete),
            "BEGIN" => Some(Token::Begin),
            "COMMIT" => Some(Token::Commit),
            "ROLLBACK" => Some(Token::Rollback),
            "AND" => Some(Token::And),
            "OR" => Some(Token::Or),
            "NOT" => Some(Token::Not),
            "IS" => Some(Token::Is),
            "NULL" => Some(Token::Null),
            "ORDER" => Some(Token::Order),
            "BY" => Some(Token::By),
            "ASC" => Some(Token::Asc),
            "DESC" => Some(Token::Desc),
            "LIMIT" => Some(Token::Limit),
            "OFFSET" => Some(Token::Offset),
            "JOIN" => Some(Token::Join),
            "INNER" => Some(Token::Inner),
            "PRIMARY" => Some(Token::Primary),
            "KEY" => Some(Token::Key),
            "UNIQUE" => Some(Token::Unique),
            "DEFAULT" => Some(Token::Default),
            "CHECK" => Some(Token::Check),
            "AS" => Some(Token::As),
            "IN" => Some(Token::In),
            "BETWEEN" => Some(Token::Between),
            "LIKE" => Some(Token::Like),
            "EXISTS" => Some(Token::Exists),
            "TRUE" => Some(Token::True),
            "FALSE" => Some(Token::False),
            "INT" | "INTEGER" => Some(Token::Int),
            "BIGINT" => Some(Token::Bigint),
            "REAL" | "FLOAT" | "DOUBLE" => Some(Token::Real),
            "TEXT" | "VARCHAR" | "CHAR" => Some(Token::Text),
            "BOOL" | "BOOLEAN" => Some(Token::Bool),
            "CROSS" => Some(Token::Cross),
            "LEFT" => Some(Token::Left),
            "RIGHT" => Some(Token::Right),
            "OUTER" => Some(Token::Outer),
            "FULL" => Some(Token::Full),
            "GROUP" => Some(Token::Group),
            "HAVING" => Some(Token::Having),
            "COUNT" => Some(Token::Count),
            "SUM" => Some(Token::Sum),
            "AVG" => Some(Token::Avg),
            "MIN" => Some(Token::Min),
            "MAX" => Some(Token::Max),
            "DISTINCT" => Some(Token::Distinct),
            "ALL" => Some(Token::All),
            "ANY" => Some(Token::Any),
            "SOME" => Some(Token::Some),
            "UNION" => Some(Token::Union),
            "EXCEPT" => Some(Token::Except),
            "INTERSECT" => Some(Token::Intersect),
            "CASE" => Some(Token::Case),
            "WHEN" => Some(Token::When),
            "THEN" => Some(Token::Then),
            "ELSE" => Some(Token::Else),
            "END" => Some(Token::End),
            "UPPER" => Some(Token::Upper),
            "LOWER" => Some(Token::Lower),
            "LENGTH" => Some(Token::Length),
            "SUBSTR" | "SUBSTRING" => Some(Token::Substr),
            "ABS" => Some(Token::Abs),
            "COALESCE" => Some(Token::Coalesce),
            _ => None,
        }
    }
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
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
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) -> bool {
        if self.pos + 1 < self.input.len() {
            if self.input[self.pos] == '-' && self.input[self.pos + 1] == '-' {
                // Line comment
                while let Some(ch) = self.advance() {
                    if ch == '\n' {
                        break;
                    }
                }
                return true;
            }
            if self.input[self.pos] == '/' && self.input[self.pos + 1] == '*' {
                // Block comment
                self.advance(); // /
                self.advance(); // *
                loop {
                    match self.advance() {
                        Some('*') if self.pos < self.input.len() && self.input[self.pos] == '/' => {
                            self.advance();
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                return true;
            }
        }
        false
    }

    fn read_string(&mut self, quote: char) -> Result<String> {
        let start_line = self.line;
        let start_col = self.col;
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(ch) if ch == quote => {
                    // Check for escaped quote ('' inside string)
                    if self.peek() == Some(quote) {
                        self.advance();
                        s.push(quote);
                    } else {
                        return Ok(s);
                    }
                }
                Some(ch) => s.push(ch),
                None => {
                    return Err(MiniDbError::Parse {
                        message: "unterminated string literal".into(),
                        line: start_line,
                        col: start_col,
                    });
                }
            }
        }
    }

    fn read_number(&mut self) -> Result<Token> {
        let start = self.pos;
        let mut is_real = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else if ch == '.' && !is_real {
                is_real = true;
                self.advance();
            } else {
                break;
            }
        }
        let text: String = self.input[start..self.pos].iter().collect();
        if is_real {
            Ok(Token::RealLiteral(text.parse().map_err(|_| {
                MiniDbError::ParseGeneric(format!("invalid real literal: {text}"))
            })?))
        } else {
            Ok(Token::IntegerLiteral(text.parse().map_err(|_| {
                MiniDbError::ParseGeneric(format!("invalid integer literal: {text}"))
            })?))
        }
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].iter().collect()
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            while self.skip_comment() {
                self.skip_whitespace();
            }

            let ch = match self.peek() {
                Some(ch) => ch,
                None => break,
            };

            let start_line = self.line;
            let start_col = self.col;

            match ch {
                ';' => {
                    self.advance();
                    tokens.push(Token::Semicolon);
                }
                ',' => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                '.' => {
                    self.advance();
                    // Only treat as meta-command if at statement start (no tokens or after semicolon)
                    let is_stmt_start =
                        tokens.is_empty() || matches!(tokens.last(), Some(Token::Semicolon));
                    if is_stmt_start {
                        if let Some(next) = self.peek() {
                            if next.is_alphabetic() || next == '_' {
                                let ident = self.read_identifier();
                                tokens.push(Token::MetaDot);
                                tokens.push(Token::Identifier(ident));
                            } else {
                                tokens.push(Token::Dot);
                            }
                        } else {
                            tokens.push(Token::Dot);
                        }
                    } else {
                        tokens.push(Token::Dot);
                    }
                }
                '(' => {
                    self.advance();
                    tokens.push(Token::LeftParen);
                }
                ')' => {
                    self.advance();
                    tokens.push(Token::RightParen);
                }
                '*' => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                '+' => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                '/' => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                '%' => {
                    self.advance();
                    tokens.push(Token::Percent);
                }
                '-' => {
                    self.advance();
                    // Could be negative number or minus operator
                    // If previous token suggests it's a number context, try to read number
                    if let Some(next_ch) = self.peek() {
                        if next_ch.is_ascii_digit() {
                            // Check if this is a negative number (prev token is operator, comma, paren, etc.)
                            let is_neg_num = match tokens.last() {
                                None => true,
                                Some(
                                    Token::Semicolon
                                    | Token::Comma
                                    | Token::LeftParen
                                    | Token::Plus
                                    | Token::Minus
                                    | Token::Star
                                    | Token::Slash
                                    | Token::Eq
                                    | Token::Neq
                                    | Token::Lt
                                    | Token::Le
                                    | Token::Gt
                                    | Token::Ge
                                    | Token::And
                                    | Token::Or
                                    | Token::Not
                                    | Token::Set
                                    | Token::Values
                                    | Token::Where
                                    | Token::On
                                    | Token::Select,
                                ) => true,
                                _ => false,
                            };
                            if is_neg_num {
                                // Read as negative number
                                let num_token = self.read_number()?;
                                match num_token {
                                    Token::IntegerLiteral(n) => {
                                        tokens.push(Token::IntegerLiteral(-n))
                                    }
                                    Token::RealLiteral(r) => tokens.push(Token::RealLiteral(-r)),
                                    _ => unreachable!(),
                                }
                                continue;
                            }
                        }
                    }
                    tokens.push(Token::Minus);
                }
                '|' => {
                    self.advance();
                    if self.peek() == Some('|') {
                        self.advance();
                        tokens.push(Token::Concat);
                    } else {
                        return Err(MiniDbError::Parse {
                            message: "expected '|' for concatenation operator '||'".into(),
                            line: start_line,
                            col: start_col,
                        });
                    }
                }
                '=' => {
                    self.advance();
                    tokens.push(Token::Eq);
                }
                '!' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Neq);
                    } else {
                        return Err(MiniDbError::Parse {
                            message: "expected '=' after '!'".into(),
                            line: start_line,
                            col: start_col,
                        });
                    }
                }
                '<' => {
                    self.advance();
                    match self.peek() {
                        Some('=') => {
                            self.advance();
                            tokens.push(Token::Le);
                        }
                        Some('>') => {
                            self.advance();
                            tokens.push(Token::Neq);
                        }
                        _ => tokens.push(Token::Lt),
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Ge);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                '\'' | '"' => {
                    self.advance();
                    let s = self.read_string(ch)?;
                    tokens.push(Token::StringLiteral(s));
                }
                _ if ch.is_ascii_digit() => {
                    let t = self.read_number()?;
                    tokens.push(t);
                }
                _ if ch.is_alphabetic() || ch == '_' => {
                    let ident = self.read_identifier();
                    if let Some(kw) = Token::keyword(&ident) {
                        tokens.push(kw);
                    } else {
                        tokens.push(Token::Identifier(ident));
                    }
                }
                _ => {
                    return Err(MiniDbError::Parse {
                        message: format!("unexpected character: '{ch}'"),
                        line: start_line,
                        col: start_col,
                    });
                }
            }
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let tokens = Lexer::new("SELECT * FROM users WHERE id = 1;")
            .tokenize()
            .unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Star);
        assert_eq!(tokens[2], Token::From);
        assert_eq!(tokens[3], Token::Identifier("users".into()));
        assert_eq!(tokens[4], Token::Where);
        assert_eq!(tokens[5], Token::Identifier("id".into()));
        assert_eq!(tokens[6], Token::Eq);
        assert_eq!(tokens[7], Token::IntegerLiteral(1));
        assert_eq!(tokens[8], Token::Semicolon);
    }

    #[test]
    fn test_string_literals() {
        let tokens = Lexer::new("SELECT 'hello world' FROM t;")
            .tokenize()
            .unwrap();
        assert_eq!(tokens[1], Token::StringLiteral("hello world".into()));
    }

    #[test]
    fn test_operators() {
        let tokens = Lexer::new("a <= b AND c <> d OR e >= f")
            .tokenize()
            .unwrap();
        assert_eq!(tokens[1], Token::Le);
        assert_eq!(tokens[5], Token::Neq);
        assert_eq!(tokens[9], Token::Ge);
    }

    #[test]
    fn test_negative_numbers() {
        let tokens = Lexer::new("SELECT -42, -3.14").tokenize().unwrap();
        assert_eq!(tokens[1], Token::IntegerLiteral(-42));
        assert_eq!(tokens[3], Token::RealLiteral(-3.14));
    }

    #[test]
    fn test_meta_command() {
        let tokens = Lexer::new(".schema").tokenize().unwrap();
        assert_eq!(tokens[0], Token::MetaDot);
        assert_eq!(tokens[1], Token::Identifier("schema".into()));
    }

    #[test]
    fn test_comments() {
        let tokens = Lexer::new("SELECT -- comment\n1").tokenize().unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::IntegerLiteral(1));
    }

    #[test]
    fn test_concat_operator() {
        let tokens = Lexer::new("a || b").tokenize().unwrap();
        assert_eq!(tokens[1], Token::Concat);
    }
}
