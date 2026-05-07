use std::fmt;

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
    Table,
    Drop,
    Index,
    On,
    Primary,
    Key,
    Not,
    Null,
    Is,
    And,
    Or,
    Asc,
    Desc,
    Order,
    By,
    Limit,
    Offset,
    Join,
    Inner,
    Begin,
    Commit,
    Rollback,
    Default,
    Check,
    Unique,
    As,
    True,
    False,

    // Literals
    Integer(i64),
    Float(f64),
    String(String),

    // Identifiers
    Ident(String),

    // Symbols
    Semicolon,
    Comma,
    Dot,
    LParen,
    RParen,
    Star,
    Plus,
    Minus,
    Slash,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Pipe,

    // Meta commands
    DotCommand(String),

    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Select => write!(f, "SELECT"),
            Token::From => write!(f, "FROM"),
            Token::Where => write!(f, "WHERE"),
            Token::Insert => write!(f, "INSERT"),
            Token::Into => write!(f, "INTO"),
            Token::Values => write!(f, "VALUES"),
            Token::Update => write!(f, "UPDATE"),
            Token::Set => write!(f, "SET"),
            Token::Delete => write!(f, "DELETE"),
            Token::Create => write!(f, "CREATE"),
            Token::Table => write!(f, "TABLE"),
            Token::Drop => write!(f, "DROP"),
            Token::Index => write!(f, "INDEX"),
            Token::On => write!(f, "ON"),
            Token::Primary => write!(f, "PRIMARY"),
            Token::Key => write!(f, "KEY"),
            Token::Not => write!(f, "NOT"),
            Token::Null => write!(f, "NULL"),
            Token::Is => write!(f, "IS"),
            Token::And => write!(f, "AND"),
            Token::Or => write!(f, "OR"),
            Token::Asc => write!(f, "ASC"),
            Token::Desc => write!(f, "DESC"),
            Token::Order => write!(f, "ORDER"),
            Token::By => write!(f, "BY"),
            Token::Limit => write!(f, "LIMIT"),
            Token::Offset => write!(f, "OFFSET"),
            Token::Join => write!(f, "JOIN"),
            Token::Inner => write!(f, "INNER"),
            Token::Begin => write!(f, "BEGIN"),
            Token::Commit => write!(f, "COMMIT"),
            Token::Rollback => write!(f, "ROLLBACK"),
            Token::Default => write!(f, "DEFAULT"),
            Token::Check => write!(f, "CHECK"),
            Token::Unique => write!(f, "UNIQUE"),
            Token::As => write!(f, "AS"),
            Token::True => write!(f, "TRUE"),
            Token::False => write!(f, "FALSE"),
            Token::Integer(v) => write!(f, "{}", v),
            Token::Float(v) => write!(f, "{}", v),
            Token::String(s) => write!(f, "'{}'", s),
            Token::Ident(s) => write!(f, "{}", s),
            Token::Semicolon => write!(f, ";"),
            Token::Comma => write!(f, ","),
            Token::Dot => write!(f, "."),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Star => write!(f, "*"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Slash => write!(f, "/"),
            Token::Eq => write!(f, "="),
            Token::Ne => write!(f, "<>"),
            Token::Lt => write!(f, "<"),
            Token::Le => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::Ge => write!(f, ">="),
            Token::Pipe => write!(f, "||"),
            Token::DotCommand(s) => write!(f, ".{}", s),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
    at_stmt_start: bool,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
            at_stmt_start: true,
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
            self.column = 1;
        } else {
            self.column += 1;
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

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn read_string(&mut self) -> Result<String, (String, usize, usize)> {
        let start_line = self.line;
        let start_col = self.column;
        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err((
                        format!("Unterminated string literal"),
                        start_line,
                        start_col,
                    ))
                }
                Some('\'') => {
                    // Check for escaped quote ''
                    if self.peek() == Some('\'') {
                        s.push('\'');
                        self.advance();
                    } else {
                        return Ok(s);
                    }
                }
                Some(ch) => s.push(ch),
            }
        }
    }

    fn read_number(&mut self) -> Token {
        let start = self.pos - 1;
        let mut is_float = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else if ch == '.' && !is_float {
                is_float = true;
                self.advance();
            } else {
                break;
            }
        }
        let s: String = self.input[start..self.pos].iter().collect();
        if is_float {
            Token::Float(s.parse().unwrap_or(0.0))
        } else {
            Token::Integer(s.parse().unwrap_or(0))
        }
    }

    fn read_ident(&mut self) -> Token {
        let start = self.pos - 1;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
        let s: String = self.input[start..self.pos].iter().collect();
        let upper = s.to_uppercase();
        match upper.as_str() {
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
            "TABLE" => Token::Table,
            "DROP" => Token::Drop,
            "INDEX" => Token::Index,
            "ON" => Token::On,
            "PRIMARY" => Token::Primary,
            "KEY" => Token::Key,
            "NOT" => Token::Not,
            "NULL" => Token::Null,
            "IS" => Token::Is,
            "AND" => Token::And,
            "OR" => Token::Or,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "LIMIT" => Token::Limit,
            "OFFSET" => Token::Offset,
            "JOIN" | "INNER" => Token::Join,
            "BEGIN" => Token::Begin,
            "COMMIT" => Token::Commit,
            "ROLLBACK" => Token::Rollback,
            "DEFAULT" => Token::Default,
            "CHECK" => Token::Check,
            "UNIQUE" => Token::Unique,
            "AS" => Token::As,
            "TRUE" => Token::True,
            "FALSE" => Token::False,
            _ => Token::Ident(s),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, (String, usize, usize)> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let line = self.line;
            let col = self.column;
            match self.peek() {
                None => {
                    tokens.push(Token::Eof);
                    break;
                }
                Some('-') => {
                    self.advance();
                    if self.peek() == Some('-') {
                        self.advance();
                        self.skip_line_comment();
                    } else {
                        tokens.push(Token::Minus);
                    }
                }
                Some('/') => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                Some('+') => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                Some('*') => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                Some('(') => {
                    self.advance();
                    tokens.push(Token::LParen);
                }
                Some(')') => {
                    self.advance();
                    tokens.push(Token::RParen);
                }
                Some(',') => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                Some(';') => {
                    self.advance();
                    tokens.push(Token::Semicolon);
                    self.at_stmt_start = true;
                }
                Some('.') => {
                    self.advance();
                    if self.at_stmt_start && self.peek().map_or(false, |c| c.is_alphabetic()) {
                        let mut name = String::new();
                        while let Some(ch) = self.peek() {
                            if ch.is_alphanumeric() || ch == '_' {
                                name.push(ch);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        tokens.push(Token::DotCommand(name));
                    } else {
                        tokens.push(Token::Dot);
                    }
                }
                Some('=') => {
                    self.advance();
                    tokens.push(Token::Eq);
                }
                Some('<') => {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        tokens.push(Token::Ne);
                    } else if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Le);
                    } else {
                        tokens.push(Token::Lt);
                    }
                }
                Some('>') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::Ge);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                Some('|') => {
                    self.advance();
                    if self.peek() == Some('|') {
                        self.advance();
                        tokens.push(Token::Pipe);
                    } else {
                        return Err(("Unexpected character: |".into(), line, col));
                    }
                }
                Some('\'') => {
                    self.advance();
                    let s = self.read_string()?;
                    tokens.push(Token::String(s));
                }
                Some(ch) if ch.is_ascii_digit() => {
                    self.advance();
                    tokens.push(self.read_number());
                }
                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    self.advance();
                    tokens.push(self.read_ident());
                }
                Some(ch) => {
                    self.advance();
                    return Err((format!("Unexpected character: '{}'", ch), line, col));
                }
            }
            // After pushing any token other than Semicolon, mark that we're no longer at statement start
            if !matches!(tokens.last(), Some(Token::Semicolon) | Some(Token::Eof)) {
                self.at_stmt_start = false;
            }
        }
        Ok(tokens)
    }
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, (String, usize, usize)> {
    Lexer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_keywords() {
        let tokens = tokenize("SELECT * FROM users WHERE id = 1;").unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Star);
        assert_eq!(tokens[2], Token::From);
        assert_eq!(tokens[3], Token::Ident("users".into()));
        assert_eq!(tokens[4], Token::Where);
        assert_eq!(tokens[5], Token::Ident("id".into()));
        assert_eq!(tokens[6], Token::Eq);
        assert_eq!(tokens[7], Token::Integer(1));
        assert_eq!(tokens[8], Token::Semicolon);
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("INSERT INTO t VALUES ('hello', 'world');").unwrap();
        assert_eq!(tokens[5], Token::String("hello".into()));
        assert_eq!(tokens[7], Token::String("world".into()));
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("a <> b AND c <= d OR e >= f").unwrap();
        assert_eq!(tokens[1], Token::Ne);
        assert_eq!(tokens[3], Token::And);
        assert_eq!(tokens[5], Token::Le);
        assert_eq!(tokens[7], Token::Or);
        assert_eq!(tokens[9], Token::Ge);
    }

    #[test]
    fn test_meta_commands() {
        let tokens = tokenize(".schema; .tables; .exit").unwrap();
        assert_eq!(tokens[0], Token::DotCommand("schema".into()));
        assert_eq!(tokens[2], Token::DotCommand("tables".into()));
        assert_eq!(tokens[4], Token::DotCommand("exit".into()));
    }

    #[test]
    fn test_comment_skip() {
        let tokens = tokenize("SELECT -- comment\n1;").unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Integer(1));
    }

    #[test]
    fn test_create_table() {
        let tokens = tokenize("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);").unwrap();
        assert_eq!(tokens[0], Token::Create);
        assert_eq!(tokens[1], Token::Table);
        assert_eq!(tokens[2], Token::Ident("t".into()));
        assert_eq!(tokens[3], Token::LParen);
        assert_eq!(tokens[4], Token::Ident("id".into()));
        assert_eq!(tokens[5], Token::Ident("INT".into()));
        assert_eq!(tokens[6], Token::Primary);
        assert_eq!(tokens[7], Token::Key);
    }

    #[test]
    fn test_concat_operator() {
        let tokens = tokenize("a || b").unwrap();
        assert_eq!(tokens[1], Token::Pipe);
    }

    #[test]
    fn test_float_literal() {
        let tokens = tokenize("3.14").unwrap();
        assert_eq!(tokens[0], Token::Float(3.14));
    }

    #[test]
    fn test_unterminated_string() {
        let result = tokenize("'hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_input() {
        let tokens = tokenize("").unwrap();
        assert_eq!(tokens, vec![Token::Eof]);
    }
}
