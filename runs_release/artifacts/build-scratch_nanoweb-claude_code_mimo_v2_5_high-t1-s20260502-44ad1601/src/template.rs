use crate::error::Result;
use serde_json::Value;

#[derive(Clone)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Renderer
    }

    pub fn render(&self, template: &str, context: &Value) -> Result<String> {
        let tokens = tokenize(template);
        render_tokens(&tokens, context)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Text(String),
    Variable(String),
    SectionStart(String),
    SectionEnd(String),
    InvertedSectionStart(String),
}

fn tokenize(template: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = template.chars().peekable();
    let mut current_text = String::new();

    while let Some(&ch) = chars.peek() {
        if ch == '{' {
            chars.next();
            if let Some(&'{') = chars.peek() {
                chars.next();
                let mut tag = String::new();
                let mut closed = false;
                while let Some(&ch) = chars.peek() {
                    if ch == '}' {
                        chars.next();
                        if let Some(&'}') = chars.peek() {
                            chars.next();
                            closed = true;
                            break;
                        } else {
                            tag.push('}');
                        }
                    } else {
                        tag.push(ch);
                        chars.next();
                    }
                }

                if !current_text.is_empty() {
                    tokens.push(Token::Text(current_text.clone()));
                    current_text.clear();
                }

                let tag = tag.trim().to_string();
                if tag.starts_with('#') {
                    tokens.push(Token::SectionStart(tag[1..].to_string()));
                } else if tag.starts_with('^') {
                    tokens.push(Token::InvertedSectionStart(tag[1..].to_string()));
                } else if tag.starts_with('/') {
                    tokens.push(Token::SectionEnd(tag[1..].to_string()));
                } else {
                    tokens.push(Token::Variable(tag));
                }

                if !closed {
                    current_text.push_str("{{");
                }
            } else {
                current_text.push('{');
            }
        } else {
            current_text.push(ch);
            chars.next();
        }
    }

    if !current_text.is_empty() {
        tokens.push(Token::Text(current_text));
    }

    tokens
}

fn render_tokens(tokens: &[Token], context: &Value) -> Result<String> {
    let mut output = String::new();
    let mut i = 0;

    while i < tokens.len() {
        match &tokens[i] {
            Token::Text(text) => {
                output.push_str(text);
            }
            Token::Variable(name) => {
                if let Some(val) = resolve_variable(context, name) {
                    output.push_str(&val);
                }
            }
            Token::SectionStart(name) => {
                let mut depth = 1;
                let mut j = i + 1;
                let mut section_tokens = Vec::new();
                while j < tokens.len() && depth > 0 {
                    match &tokens[j] {
                        Token::SectionStart(_) => {
                            depth += 1;
                            section_tokens.push(tokens[j].clone());
                        }
                        Token::SectionEnd(_) => {
                            depth -= 1;
                            if depth > 0 {
                                section_tokens.push(tokens[j].clone());
                            }
                        }
                        other => {
                            section_tokens.push(other.clone());
                        }
                    }
                    j += 1;
                }

                match context.get(name) {
                    Some(Value::Bool(false)) | Some(Value::Null) => {}
                    Some(Value::Array(arr)) => {
                        for item in arr {
                            output.push_str(&render_tokens(&section_tokens, item)?);
                        }
                    }
                    Some(_) => {
                        output.push_str(&render_tokens(&section_tokens, context)?);
                    }
                    None => {}
                }

                i = j;
                continue;
            }
            Token::InvertedSectionStart(name) => {
                let mut depth = 1;
                let mut j = i + 1;
                let mut section_tokens = Vec::new();
                while j < tokens.len() && depth > 0 {
                    match &tokens[j] {
                        Token::InvertedSectionStart(_) | Token::SectionStart(_) => {
                            depth += 1;
                            section_tokens.push(tokens[j].clone());
                        }
                        Token::SectionEnd(_) => {
                            depth -= 1;
                            if depth > 0 {
                                section_tokens.push(tokens[j].clone());
                            }
                        }
                        other => {
                            section_tokens.push(other.clone());
                        }
                    }
                    j += 1;
                }

                match context.get(name) {
                    None | Some(Value::Bool(false)) | Some(Value::Null) => {
                        output.push_str(&render_tokens(&section_tokens, context)?);
                    }
                    _ => {}
                }

                i = j;
                continue;
            }
            Token::SectionEnd(_) => {}
        }
        i += 1;
    }

    Ok(output)
}

fn resolve_variable(context: &Value, name: &str) -> Option<String> {
    // Handle "." as the current context value
    if name == "." {
        return Some(match context {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => return None,
            _ => context.to_string(),
        });
    }

    let parts: Vec<&str> = name.split('.').collect();
    let mut current = context;

    for part in &parts {
        current = current.get(part)?;
    }

    Some(match current {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => return None,
        _ => current.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable() {
        let renderer = Renderer::new();
        let context = serde_json::json!({"name": "World"});
        let result = renderer.render("Hello, {{name}}!", &context).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_section_array() {
        let renderer = Renderer::new();
        let context = serde_json::json!({
            "items": ["a", "b", "c"]
        });
        let result = renderer
            .render("{{#items}}{{.}} {{/items}}", &context)
            .unwrap();
        assert_eq!(result, "a b c ");
    }

    #[test]
    fn test_inverted_section() {
        let renderer = Renderer::new();
        let context = serde_json::json!({});
        let result = renderer
            .render("{{^missing}}default{{/missing}}", &context)
            .unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_nested_variable() {
        let renderer = Renderer::new();
        let context = serde_json::json!({"user": {"name": "Alice"}});
        let result = renderer.render("Hello, {{user.name}}!", &context).unwrap();
        assert_eq!(result, "Hello, Alice!");
    }
}
