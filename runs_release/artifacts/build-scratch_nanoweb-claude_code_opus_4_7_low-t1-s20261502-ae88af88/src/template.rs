//! Tiny mustache-style template engine: {{name}}, {{#section}}..{{/section}}, {{^inverse}}..{{/inverse}}.

use serde_json::Value;

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> String {
        render_inner(template, context)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn render_inner(tpl: &str, ctx: &Value) -> String {
    let tokens = tokenize(tpl);
    let mut out = String::new();
    render_tokens(&tokens, &mut 0, ctx, &mut out, None);
    out
}

#[derive(Debug)]
enum Token {
    Text(String),
    Var(String),
    SectionOpen(String),
    InverseOpen(String),
    SectionClose(String),
}

fn tokenize(tpl: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut rest = tpl;
    while !rest.is_empty() {
        if let Some(idx) = rest.find("{{") {
            if idx > 0 {
                tokens.push(Token::Text(rest[..idx].to_string()));
            }
            let after = &rest[idx + 2..];
            let end = match after.find("}}") {
                Some(e) => e,
                None => {
                    tokens.push(Token::Text(rest.to_string()));
                    return tokens;
                }
            };
            let tag = after[..end].trim();
            if let Some(name) = tag.strip_prefix('#') {
                tokens.push(Token::SectionOpen(name.trim().to_string()));
            } else if let Some(name) = tag.strip_prefix('^') {
                tokens.push(Token::InverseOpen(name.trim().to_string()));
            } else if let Some(name) = tag.strip_prefix('/') {
                tokens.push(Token::SectionClose(name.trim().to_string()));
            } else {
                tokens.push(Token::Var(tag.to_string()));
            }
            rest = &after[end + 2..];
        } else {
            tokens.push(Token::Text(rest.to_string()));
            break;
        }
    }
    tokens
}

fn render_tokens(
    tokens: &[Token],
    i: &mut usize,
    ctx: &Value,
    out: &mut String,
    stop: Option<&str>,
) {
    while *i < tokens.len() {
        match &tokens[*i] {
            Token::Text(t) => {
                out.push_str(t);
                *i += 1;
            }
            Token::Var(name) => {
                let v = lookup(ctx, name);
                out.push_str(&value_to_string(&v));
                *i += 1;
            }
            Token::SectionOpen(name) => {
                *i += 1;
                // capture inner tokens until matching close
                let start = *i;
                let inner = collect_until(tokens, i, name);
                let v = lookup(ctx, name);
                if is_truthy(&v) {
                    match v {
                        Value::Array(items) => {
                            for item in items {
                                let mut j = 0usize;
                                render_tokens(&inner, &mut j, &item, out, None);
                            }
                        }
                        _ => {
                            let mut j = 0usize;
                            render_tokens(&inner, &mut j, &v, out, None);
                        }
                    }
                }
                let _ = start;
            }
            Token::InverseOpen(name) => {
                *i += 1;
                let inner = collect_until(tokens, i, name);
                let v = lookup(ctx, name);
                if !is_truthy(&v) {
                    let mut j = 0usize;
                    render_tokens(&inner, &mut j, ctx, out, None);
                }
            }
            Token::SectionClose(n) => {
                if Some(n.as_str()) == stop {
                    return;
                }
                *i += 1;
            }
        }
    }
}

fn collect_until(tokens: &[Token], i: &mut usize, name: &str) -> Vec<Token> {
    let mut depth = 1;
    let mut out = Vec::new();
    while *i < tokens.len() {
        match &tokens[*i] {
            Token::SectionOpen(n) if n == name => {
                depth += 1;
                out.push(Token::SectionOpen(n.clone()));
            }
            Token::InverseOpen(n) if n == name => {
                depth += 1;
                out.push(Token::InverseOpen(n.clone()));
            }
            Token::SectionClose(n) if n == name => {
                depth -= 1;
                if depth == 0 {
                    *i += 1;
                    return out;
                }
                out.push(Token::SectionClose(n.clone()));
            }
            other => out.push(clone_token(other)),
        }
        *i += 1;
    }
    out
}

fn clone_token(t: &Token) -> Token {
    match t {
        Token::Text(s) => Token::Text(s.clone()),
        Token::Var(s) => Token::Var(s.clone()),
        Token::SectionOpen(s) => Token::SectionOpen(s.clone()),
        Token::InverseOpen(s) => Token::InverseOpen(s.clone()),
        Token::SectionClose(s) => Token::SectionClose(s.clone()),
    }
}

fn lookup(ctx: &Value, name: &str) -> Value {
    if name == "." {
        return ctx.clone();
    }
    let mut cur = ctx.clone();
    for part in name.split('.') {
        cur = match cur {
            Value::Object(map) => map.get(part).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        };
    }
    cur
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => html_escape(s),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
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
            _ => out.push(c),
        }
    }
    out
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn var() {
        let r = Renderer::new();
        assert_eq!(r.render("hi {{name}}", &json!({"name": "bob"})), "hi bob");
    }

    #[test]
    fn section() {
        let r = Renderer::new();
        let out = r.render("{{#xs}}<{{.}}>{{/xs}}", &json!({"xs": ["a", "b", "c"]}));
        assert_eq!(out, "<a><b><c>");
    }

    #[test]
    fn inverse() {
        let r = Renderer::new();
        let out = r.render("{{^missing}}none{{/missing}}", &json!({}));
        assert_eq!(out, "none");
    }

    #[test]
    fn escape() {
        let r = Renderer::new();
        assert_eq!(r.render("{{x}}", &json!({"x": "<b>"})), "&lt;b&gt;");
    }
}
