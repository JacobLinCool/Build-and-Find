//! A mustache-ish template engine.
//!
//! Supported tags:
//! * `{{name}}`          — variable interpolation (HTML-escaped)
//! * `{{{name}}}`        — variable interpolation (unescaped)
//! * `{{#section}}...{{/section}}` — show block if the value is truthy,
//!   or loop if it is a list
//! * `{{^inverted}}...{{/inverted}}` — show block if the value is missing
//!   or falsy
//! * `{{! comment }}`    — comment, rendered as empty

use std::collections::HashMap;

use crate::error::NanowebError;

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Bool(bool),
    Number(f64),
    List(Vec<Value>),
    Map(HashMap<String, Value>),
    Null,
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::String(s) => !s.is_empty(),
            Value::Number(n) => *n != 0.0,
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Bool(true) => "true".into(),
            Value::Bool(false) => "false".into(),
            Value::Number(n) => format!("{}", n),
            Value::Null => String::new(),
            Value::List(_) | Value::Map(_) => String::new(),
        }
    }
}

pub type Context = HashMap<String, Value>;

pub fn render(template: &str, ctx: &Context) -> Result<String, NanowebError> {
    let nodes = parse(template)?;
    let mut out = String::new();
    render_nodes(&nodes, &[ctx.clone()], &mut out)?;
    Ok(out)
}

#[derive(Debug, Clone)]
enum Node {
    Text(String),
    Var { name: String, escape: bool },
    Section { name: String, inverted: bool, body: Vec<Node> },
}

fn parse(input: &str) -> Result<Vec<Node>, NanowebError> {
    let mut pos = 0;
    let bytes = input.as_bytes();
    let (nodes, consumed) = parse_until(input, bytes, &mut pos, None)?;
    if consumed != bytes.len() {
        return Err(NanowebError::Template("unexpected trailing content".into()));
    }
    Ok(nodes)
}

fn parse_until(
    src: &str,
    bytes: &[u8],
    pos: &mut usize,
    terminator: Option<&str>,
) -> Result<(Vec<Node>, usize), NanowebError> {
    let mut nodes = Vec::new();
    while *pos < bytes.len() {
        let rest = &src[*pos..];
        if let Some(end) = terminator {
            if rest.starts_with(end) {
                *pos += end.len();
                return Ok((nodes, *pos));
            }
        }
        if rest.starts_with("{{{") {
            let close = rest
                .find("}}}")
                .ok_or_else(|| NanowebError::Template("malformed triple-mustache".into()))?;
            let name = rest[3..close].trim().to_string();
            *pos += close + 3;
            nodes.push(Node::Var { name, escape: false });
        } else if rest.starts_with("{{") {
            let close = rest
                .find("}}")
                .ok_or_else(|| NanowebError::Template("unclosed tag".into()))?;
            let tag = &rest[2..close];
            *pos += close + 2;
            let tag_trim = tag.trim();
            if let Some(rest) = tag_trim.strip_prefix('!') {
                let _ = rest; // comment
                continue;
            }
            if let Some(rest) = tag_trim.strip_prefix('#') {
                let name = rest.trim().to_string();
                let terminator = format!("{{{{/{}}}}}", name);
                let (body, _) = parse_until(src, bytes, pos, Some(&terminator))?;
                nodes.push(Node::Section {
                    name,
                    inverted: false,
                    body,
                });
                continue;
            }
            if let Some(rest) = tag_trim.strip_prefix('^') {
                let name = rest.trim().to_string();
                let terminator = format!("{{{{/{}}}}}", name);
                let (body, _) = parse_until(src, bytes, pos, Some(&terminator))?;
                nodes.push(Node::Section {
                    name,
                    inverted: true,
                    body,
                });
                continue;
            }
            if tag_trim.starts_with('/') {
                return Err(NanowebError::Template(format!(
                    "unexpected closing tag {}",
                    tag_trim
                )));
            }
            nodes.push(Node::Var {
                name: tag_trim.to_string(),
                escape: true,
            });
        } else {
            let end = rest.find("{{").map(|i| *pos + i).unwrap_or(bytes.len());
            let end = if let Some(t) = terminator {
                match rest.find(t) {
                    Some(i) => std::cmp::min(*pos + i, end),
                    None => end,
                }
            } else {
                end
            };
            nodes.push(Node::Text(src[*pos..end].to_string()));
            *pos = end;
        }
    }
    Ok((nodes, *pos))
}

fn render_nodes(
    nodes: &[Node],
    stack: &[Context],
    out: &mut String,
) -> Result<(), NanowebError> {
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Var { name, escape } => {
                if let Some(v) = lookup(name, stack) {
                    let s = v.as_str();
                    if *escape {
                        out.push_str(&html_escape(&s));
                    } else {
                        out.push_str(&s);
                    }
                }
            }
            Node::Section { name, inverted, body } => {
                let value = lookup(name, stack);
                let truthy = value.as_ref().map(|v| v.is_truthy()).unwrap_or(false);
                if *inverted {
                    if !truthy {
                        render_nodes(body, stack, out)?;
                    }
                } else if truthy {
                    match value {
                        Some(Value::List(items)) => {
                            for item in items {
                                let mut local = HashMap::new();
                                if let Value::Map(m) = &item {
                                    for (k, v) in m {
                                        local.insert(k.clone(), v.clone());
                                    }
                                }
                                local.insert(".".to_string(), item.clone());
                                let mut new_stack = stack.to_vec();
                                new_stack.push(local);
                                render_nodes(body, &new_stack, out)?;
                            }
                        }
                        Some(Value::Map(m)) => {
                            let mut new_stack = stack.to_vec();
                            new_stack.push(m.clone());
                            render_nodes(body, &new_stack, out)?;
                        }
                        _ => {
                            render_nodes(body, stack, out)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn lookup(name: &str, stack: &[Context]) -> Option<Value> {
    for frame in stack.iter().rev() {
        if let Some(v) = frame.get(name) {
            return Some(v.clone());
        }
    }
    None
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

    fn ctx(items: &[(&str, Value)]) -> Context {
        let mut c = Context::new();
        for (k, v) in items {
            c.insert(k.to_string(), v.clone());
        }
        c
    }

    #[test]
    fn interpolates_variables() {
        let out = render("hi {{name}}!", &ctx(&[("name", Value::String("World".into()))]))
            .unwrap();
        assert_eq!(out, "hi World!");
    }

    #[test]
    fn escapes_html_by_default() {
        let out = render("{{x}}", &ctx(&[("x", Value::String("<b>".into()))])).unwrap();
        assert_eq!(out, "&lt;b&gt;");
    }

    #[test]
    fn section_truthy_renders_body() {
        let out = render(
            "{{#ok}}yes{{/ok}}",
            &ctx(&[("ok", Value::Bool(true))]),
        )
        .unwrap();
        assert_eq!(out, "yes");
    }

    #[test]
    fn inverted_section_renders_when_missing() {
        let out = render("{{^x}}none{{/x}}", &Context::new()).unwrap();
        assert_eq!(out, "none");
    }

    #[test]
    fn list_iteration() {
        let list = Value::List(vec![
            Value::Map(HashMap::from([(
                "name".to_string(),
                Value::String("a".into()),
            )])),
            Value::Map(HashMap::from([(
                "name".to_string(),
                Value::String("b".into()),
            )])),
        ]);
        let out = render("{{#items}}{{name}},{{/items}}", &ctx(&[("items", list)])).unwrap();
        assert_eq!(out, "a,b,");
    }

    #[test]
    fn unescaped_triple_mustache() {
        let out = render("{{{x}}}", &ctx(&[("x", Value::String("<b>".into()))])).unwrap();
        assert_eq!(out, "<b>");
    }
}
