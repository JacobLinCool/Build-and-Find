use crate::error::{NanowebError, Result};
use serde_json::{Map, Value};

/// A minimal mustache-like template engine. Supports:
/// - `{{name}}` interpolation (HTML escape by default)
/// - `{{{name}}}` raw interpolation (no escape)
/// - `{{#section}}...{{/section}}` (truthy / iteration)
/// - `{{^inverse}}...{{/inverse}}` (rendered when value is falsy)
#[derive(Default, Clone)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> Result<String> {
        let tokens = tokenize(template)?;
        let (rendered, _consumed) = render_tokens(&tokens, 0, &[context], false)?;
        Ok(rendered)
    }
}

#[derive(Debug, Clone)]
enum Token {
    Text(String),
    Var { name: String, escape: bool },
    SectionOpen(String),
    InverseOpen(String),
    SectionClose(String),
}

fn tokenize(s: &str) -> Result<Vec<Token>> {
    let bytes = s.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if last < i {
                tokens.push(Token::Text(s[last..i].to_string()));
            }
            // Determine whether triple-stash
            let triple = i + 2 < bytes.len() && bytes[i + 2] == b'{';
            let open = if triple { i + 3 } else { i + 2 };
            let close_pat: &[u8] = if triple { b"}}}" } else { b"}}" };
            let mut close = None;
            let mut j = open;
            while j + close_pat.len() <= bytes.len() {
                if &bytes[j..j + close_pat.len()] == close_pat {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            let close = close.ok_or_else(|| NanowebError::Template("unterminated tag".into()))?;
            let inner = s[open..close].trim();
            let consumed_to = close + close_pat.len();
            if triple {
                tokens.push(Token::Var {
                    name: inner.to_string(),
                    escape: false,
                });
            } else if let Some(name) = inner.strip_prefix('#') {
                tokens.push(Token::SectionOpen(name.trim().to_string()));
            } else if let Some(name) = inner.strip_prefix('^') {
                tokens.push(Token::InverseOpen(name.trim().to_string()));
            } else if let Some(name) = inner.strip_prefix('/') {
                tokens.push(Token::SectionClose(name.trim().to_string()));
            } else if let Some(name) = inner.strip_prefix('&') {
                tokens.push(Token::Var {
                    name: name.trim().to_string(),
                    escape: false,
                });
            } else if inner.starts_with('!') {
                // comment
            } else {
                tokens.push(Token::Var {
                    name: inner.to_string(),
                    escape: true,
                });
            }
            i = consumed_to;
            last = consumed_to;
        } else {
            i += 1;
        }
    }
    if last < bytes.len() {
        tokens.push(Token::Text(s[last..].to_string()));
    }
    Ok(tokens)
}

fn render_tokens(
    tokens: &[Token],
    start: usize,
    stack: &[&Value],
    inside_inverted: bool,
) -> Result<(String, usize)> {
    let _ = inside_inverted;
    let mut out = String::new();
    let mut i = start;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Text(t) => out.push_str(t),
            Token::Var { name, escape } => {
                let v = lookup(stack, name);
                let s = stringify(v);
                if *escape {
                    out.push_str(&html_escape(&s));
                } else {
                    out.push_str(&s);
                }
            }
            Token::SectionOpen(name) => {
                let block_start = i + 1;
                let block_end = find_close(tokens, block_start, name)?;
                let v = lookup(stack, name);
                if let Some(items) = as_array(v) {
                    let segment_tokens = &tokens[block_start..block_end];
                    for item in items {
                        let mut new_stack: Vec<&Value> = stack.to_vec();
                        new_stack.insert(0, item);
                        let (rendered, _) = render_tokens(segment_tokens, 0, &new_stack, false)?;
                        out.push_str(&rendered);
                    }
                } else if is_truthy(v) {
                    let segment_tokens = &tokens[block_start..block_end];
                    let mut new_stack: Vec<&Value> = stack.to_vec();
                    if let Some(obj) = v {
                        if obj.is_object() {
                            new_stack.insert(0, obj);
                        }
                    }
                    let (rendered, _) = render_tokens(segment_tokens, 0, &new_stack, false)?;
                    out.push_str(&rendered);
                }
                i = block_end;
            }
            Token::InverseOpen(name) => {
                let block_start = i + 1;
                let block_end = find_close(tokens, block_start, name)?;
                let v = lookup(stack, name);
                let truthy = if let Some(arr) = as_array(v) {
                    !arr.is_empty()
                } else {
                    is_truthy(v)
                };
                if !truthy {
                    let segment_tokens = &tokens[block_start..block_end];
                    let (rendered, _) = render_tokens(segment_tokens, 0, stack, true)?;
                    out.push_str(&rendered);
                }
                i = block_end;
            }
            Token::SectionClose(_) => {
                return Ok((out, i));
            }
        }
        i += 1;
    }
    Ok((out, i))
}

fn find_close(tokens: &[Token], start: usize, name: &str) -> Result<usize> {
    let mut depth = 0usize;
    for (idx, t) in tokens.iter().enumerate().skip(start) {
        match t {
            Token::SectionOpen(_) | Token::InverseOpen(_) => depth += 1,
            Token::SectionClose(n) => {
                if depth == 0 {
                    if n == name {
                        return Ok(idx);
                    } else {
                        return Err(NanowebError::Template(format!(
                            "mismatched closing tag: {}",
                            n
                        )));
                    }
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Err(NanowebError::Template(format!(
        "unclosed section: {}",
        name
    )))
}

fn lookup<'a>(stack: &[&'a Value], name: &str) -> Option<&'a Value> {
    if name == "." {
        return stack.first().copied();
    }
    let parts: Vec<&str> = name.split('.').collect();
    for ctx in stack {
        if let Some(v) = lookup_path(ctx, &parts) {
            return Some(v);
        }
    }
    None
}

fn lookup_path<'a>(value: &'a Value, parts: &[&str]) -> Option<&'a Value> {
    let mut cur = value;
    for p in parts {
        match cur {
            Value::Object(obj) => {
                cur = obj.get(*p)?;
            }
            _ => return None,
        }
    }
    Some(cur)
}

fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
    }
}

fn as_array(v: Option<&Value>) -> Option<&Vec<Value>> {
    match v {
        Some(Value::Array(a)) => Some(a),
        _ => None,
    }
}

fn stringify(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
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
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_var() {
        let r = Renderer::new();
        let out = r
            .render("Hello, {{name}}!", &json!({"name":"world"}))
            .unwrap();
        assert_eq!(out, "Hello, world!");
    }

    #[test]
    fn html_escape_default() {
        let r = Renderer::new();
        let out = r.render("{{x}}", &json!({"x":"<script>"})).unwrap();
        assert_eq!(out, "&lt;script&gt;");
    }

    #[test]
    fn iter_section() {
        let r = Renderer::new();
        let out = r
            .render(
                "{{#items}}- {{.}}\n{{/items}}",
                &json!({"items":["a","b","c"]}),
            )
            .unwrap();
        assert_eq!(out, "- a\n- b\n- c\n");
    }

    #[test]
    fn truthy_section_object() {
        let r = Renderer::new();
        let out = r
            .render(
                "{{#user}}name={{name}}{{/user}}",
                &json!({"user":{"name":"alice"}}),
            )
            .unwrap();
        assert_eq!(out, "name=alice");
    }

    #[test]
    fn inverse_section_when_absent() {
        let r = Renderer::new();
        let out = r.render("{{^x}}none{{/x}}", &json!({})).unwrap();
        assert_eq!(out, "none");
    }
}

#[allow(dead_code)]
fn _force_serde_use(v: Map<String, Value>) -> Map<String, Value> {
    v
}
