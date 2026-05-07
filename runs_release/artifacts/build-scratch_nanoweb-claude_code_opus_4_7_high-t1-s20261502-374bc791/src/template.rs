//! Tiny mustache-style template engine.
//!
//! Supported tags: `{{name}}` for interpolation (HTML-escaped),
//! `{{#section}} ... {{/section}}` for sections, `{{^section}} ... {{/section}}`
//! for inverted sections, and `{{! comment }}` for comments. `{{{name}}}`
//! emits the value without HTML escaping.

use serde_json::Value;

use crate::error::{NanowebError, Result};

#[derive(Debug, Clone)]
enum Token {
    Text(String),
    Var { name: String, escaped: bool },
    SectionStart { name: String, inverted: bool },
    SectionEnd { name: String },
    Comment,
}

fn tokenize(template: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if i > last {
                tokens.push(Token::Text(template[last..i].to_string()));
            }
            let triple = i + 2 < bytes.len() && bytes[i + 2] == b'{';
            let open_len = if triple { 3 } else { 2 };
            let close_pat: &[u8] = if triple { b"}}}" } else { b"}}" };
            let mut j = i + open_len;
            let mut found = None;
            while j + close_pat.len() <= bytes.len() {
                if &bytes[j..j + close_pat.len()] == close_pat {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            let close =
                found.ok_or_else(|| NanowebError::Template("unterminated mustache tag".into()))?;
            let inner = template[i + open_len..close].trim();
            if let Some(name) = inner.strip_prefix('#') {
                tokens.push(Token::SectionStart {
                    name: name.trim().to_string(),
                    inverted: false,
                });
            } else if let Some(name) = inner.strip_prefix('^') {
                tokens.push(Token::SectionStart {
                    name: name.trim().to_string(),
                    inverted: true,
                });
            } else if let Some(name) = inner.strip_prefix('/') {
                tokens.push(Token::SectionEnd {
                    name: name.trim().to_string(),
                });
            } else if inner.starts_with('!') {
                tokens.push(Token::Comment);
            } else {
                tokens.push(Token::Var {
                    name: inner.to_string(),
                    escaped: !triple,
                });
            }
            i = close + close_pat.len();
            last = i;
        } else {
            i += 1;
        }
    }
    if last < template.len() {
        tokens.push(Token::Text(template[last..].to_string()));
    }
    Ok(tokens)
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn lookup<'a>(stack: &[&'a Value], name: &str) -> Option<&'a Value> {
    if name == "." {
        return stack.last().copied();
    }
    for ctx in stack.iter().rev() {
        let mut current: Option<&Value> = Some(*ctx);
        for part in name.split('.') {
            current = match current {
                Some(Value::Object(o)) => o.get(part),
                _ => None,
            };
            if current.is_none() {
                break;
            }
        }
        if current.is_some() {
            return current;
        }
    }
    None
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn render_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => v.to_string(),
    }
}

fn render_tokens<'a>(tokens: &[Token], stack: &mut Vec<&'a Value>, out: &mut String) -> Result<()> {
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Text(t) => {
                out.push_str(t);
                i += 1;
            }
            Token::Comment => {
                i += 1;
            }
            Token::Var { name, escaped } => {
                let s = lookup(stack, name).map(render_value).unwrap_or_default();
                if *escaped {
                    out.push_str(&html_escape(&s));
                } else {
                    out.push_str(&s);
                }
                i += 1;
            }
            Token::SectionStart { name, inverted } => {
                let end = find_section_end(tokens, i, name)?;
                let inner = &tokens[i + 1..end];
                let v = lookup(stack, name);
                let is_truthy = v.map(truthy).unwrap_or(false);
                if *inverted {
                    if !is_truthy {
                        render_tokens(inner, stack, out)?;
                    }
                } else if let Some(v) = v {
                    if is_truthy {
                        match v {
                            Value::Array(items) => {
                                for item in items {
                                    stack.push(item);
                                    let r = render_tokens(inner, stack, out);
                                    stack.pop();
                                    r?;
                                }
                            }
                            Value::Object(_) => {
                                stack.push(v);
                                let r = render_tokens(inner, stack, out);
                                stack.pop();
                                r?;
                            }
                            _ => {
                                render_tokens(inner, stack, out)?;
                            }
                        }
                    }
                }
                i = end + 1;
            }
            Token::SectionEnd { name } => {
                return Err(NanowebError::Template(format!(
                    "unexpected section end: {}",
                    name
                )));
            }
        }
    }
    Ok(())
}

fn find_section_end(tokens: &[Token], start: usize, name: &str) -> Result<usize> {
    let mut depth = 0usize;
    for (j, tok) in tokens.iter().enumerate().skip(start + 1) {
        match tok {
            Token::SectionStart { name: n, .. } if n == name => depth += 1,
            Token::SectionEnd { name: n } if n == name => {
                if depth == 0 {
                    return Ok(j);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Err(NanowebError::Template(format!(
        "section {} not closed",
        name
    )))
}

/// Renderer entry point.
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> Result<String> {
        let tokens = tokenize(template)?;
        let mut stack: Vec<&Value> = vec![context];
        let mut out = String::new();
        render_tokens(&tokens, &mut stack, &mut out)?;
        Ok(out)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn interpolates_var() {
        let r = Renderer::new();
        let ctx = json!({ "name": "world" });
        assert_eq!(r.render("Hello, {{name}}!", &ctx).unwrap(), "Hello, world!");
    }

    #[test]
    fn html_escapes_by_default() {
        let r = Renderer::new();
        let ctx = json!({ "msg": "<b>" });
        assert_eq!(r.render("{{msg}}", &ctx).unwrap(), "&lt;b&gt;");
        assert_eq!(r.render("{{{msg}}}", &ctx).unwrap(), "<b>");
    }

    #[test]
    fn section_iterates_array() {
        let r = Renderer::new();
        let ctx = json!({ "items": [{"n": 1}, {"n": 2}, {"n": 3}] });
        let out = r.render("{{#items}}[{{n}}]{{/items}}", &ctx).unwrap();
        assert_eq!(out, "[1][2][3]");
    }

    #[test]
    fn inverted_section_when_absent() {
        let r = Renderer::new();
        let ctx = json!({});
        let out = r.render("{{^missing}}none{{/missing}}", &ctx).unwrap();
        assert_eq!(out, "none");
    }

    #[test]
    fn empty_array_is_falsy() {
        let r = Renderer::new();
        let ctx = json!({ "items": [] });
        assert_eq!(
            r.render("{{#items}}x{{/items}}{{^items}}empty{{/items}}", &ctx)
                .unwrap(),
            "empty"
        );
    }
}
