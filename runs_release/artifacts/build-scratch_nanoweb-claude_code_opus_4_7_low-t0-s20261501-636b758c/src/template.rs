//! A tiny mustache-flavored template engine.
//!
//! Supports `{{name}}`, `{{#section}}...{{/section}}`, `{{^inverted}}...{{/inverted}}`.

use crate::NanowebError;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum Node {
    Text(String),
    Var(String),
    Section {
        key: String,
        inverted: bool,
        body: Vec<Node>,
    },
}

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> Result<String, NanowebError> {
        let nodes = parse(template)?;
        let mut out = String::new();
        render_nodes(&nodes, &[context], &mut out);
        Ok(out)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn parse(input: &str) -> Result<Vec<Node>, NanowebError> {
    let (nodes, rest) = parse_until(input, None)?;
    if !rest.is_empty() {
        return Err(NanowebError::Template("unexpected closing tag".into()));
    }
    Ok(nodes)
}

fn parse_until<'a>(
    mut input: &'a str,
    closing: Option<&str>,
) -> Result<(Vec<Node>, &'a str), NanowebError> {
    let mut nodes = Vec::new();
    while !input.is_empty() {
        if let Some(idx) = input.find("{{") {
            if idx > 0 {
                nodes.push(Node::Text(input[..idx].to_string()));
            }
            let after = &input[idx + 2..];
            let end = after
                .find("}}")
                .ok_or_else(|| NanowebError::Template("unclosed tag".into()))?;
            let tag = after[..end].trim();
            let rest = &after[end + 2..];
            if let Some(name) = tag.strip_prefix('#') {
                let (body, after_body) = parse_until(rest, Some(name.trim()))?;
                nodes.push(Node::Section {
                    key: name.trim().to_string(),
                    inverted: false,
                    body,
                });
                input = after_body;
            } else if let Some(name) = tag.strip_prefix('^') {
                let (body, after_body) = parse_until(rest, Some(name.trim()))?;
                nodes.push(Node::Section {
                    key: name.trim().to_string(),
                    inverted: true,
                    body,
                });
                input = after_body;
            } else if let Some(name) = tag.strip_prefix('/') {
                if Some(name.trim()) != closing {
                    return Err(NanowebError::Template(format!(
                        "mismatched close: {}",
                        name
                    )));
                }
                return Ok((nodes, rest));
            } else {
                nodes.push(Node::Var(tag.to_string()));
                input = rest;
            }
        } else {
            nodes.push(Node::Text(input.to_string()));
            input = "";
        }
    }
    if closing.is_some() {
        return Err(NanowebError::Template("unterminated section".into()));
    }
    Ok((nodes, ""))
}

fn lookup<'a>(stack: &'a [&'a Value], key: &str) -> Option<&'a Value> {
    if key == "." {
        return stack.last().copied();
    }
    for ctx in stack.iter().rev() {
        if let Some(v) = ctx.get(key) {
            return Some(v);
        }
    }
    None
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(_) => true,
    }
}

fn render_nodes(nodes: &[Node], stack: &[&Value], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(s) => out.push_str(s),
            Node::Var(k) => {
                if let Some(v) = lookup(stack, k) {
                    match v {
                        Value::String(s) => out.push_str(&html_escape(s)),
                        _ => out.push_str(&html_escape(&v.to_string())),
                    }
                }
            }
            Node::Section {
                key,
                inverted,
                body,
            } => {
                let v = lookup(stack, key);
                let t = v.map(truthy).unwrap_or(false);
                if *inverted {
                    if !t {
                        render_nodes(body, stack, out);
                    }
                } else if t {
                    if let Some(Value::Array(items)) = v {
                        for item in items {
                            let mut new_stack = stack.to_vec();
                            new_stack.push(item);
                            render_nodes(body, &new_stack, out);
                        }
                    } else if let Some(val) = v {
                        let mut new_stack = stack.to_vec();
                        new_stack.push(val);
                        render_nodes(body, &new_stack, out);
                    }
                }
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_var() {
        let r = Renderer::new();
        let s = r
            .render("Hello {{name}}!", &json!({"name": "world"}))
            .unwrap();
        assert_eq!(s, "Hello world!");
    }

    #[test]
    fn section_array() {
        let r = Renderer::new();
        let s = r
            .render(
                "{{#items}}-{{name}};{{/items}}",
                &json!({"items": [{"name":"a"},{"name":"b"}]}),
            )
            .unwrap();
        assert_eq!(s, "-a;-b;");
    }

    #[test]
    fn inverted_section() {
        let r = Renderer::new();
        let s = r
            .render("{{^empty}}none{{/empty}}", &json!({"empty": false}))
            .unwrap();
        assert_eq!(s, "none");
    }

    #[test]
    fn html_escapes() {
        let r = Renderer::new();
        let s = r.render("{{x}}", &json!({"x": "<b>"})).unwrap();
        assert_eq!(s, "&lt;b&gt;");
    }
}
