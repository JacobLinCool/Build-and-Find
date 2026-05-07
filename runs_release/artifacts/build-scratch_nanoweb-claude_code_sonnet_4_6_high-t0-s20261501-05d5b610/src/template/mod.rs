//! Minimal mustache-style template engine.
//! Supports: {{var}}, {{#list}}...{{/list}}, {{^absent}}...{{/absent}}

use crate::error::NanowebError;
use std::collections::HashMap;

/// A context value for template rendering.
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Bool(bool),
    List(Vec<Context>),
    Null,
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}
impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Self::String(n.to_string())
    }
}
impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Self::String(n.to_string())
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Self::String(n.to_string())
    }
}

/// A template rendering context (key-value map).
#[derive(Debug, Clone, Default)]
pub struct Context {
    values: HashMap<String, Value>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) -> &mut Self {
        self.values.insert(key.into(), value.into());
        self
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.set(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
}

/// Template renderer.
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, ctx: &Context) -> Result<String, NanowebError> {
        render_section(template, ctx)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn render_section(template: &str, ctx: &Context) -> Result<String, NanowebError> {
    let mut output = String::with_capacity(template.len());
    let mut pos = 0;
    let bytes = template.as_bytes();

    while pos < bytes.len() {
        // Look for next `{{`
        if let Some(open) = find_open(&template[pos..]) {
            output.push_str(&template[pos..pos + open]);
            let rest = &template[pos + open + 2..]; // after {{

            // Find closing `}}`
            let close = rest
                .find("}}")
                .ok_or_else(|| NanowebError::Template("Unclosed {{ in template".into()))?;

            let tag = &rest[..close].trim();
            pos = pos + open + 2 + close + 2; // advance past }}

            if let Some(section_name) = tag.strip_prefix('#') {
                // Section: {{#name}}...{{/name}}
                let section_name = section_name.trim();
                let (section_body, after) = find_section(template, pos, section_name)?;

                match ctx.get(section_name) {
                    Some(Value::List(items)) => {
                        for item_ctx in items {
                            output.push_str(&render_section(section_body, item_ctx)?);
                        }
                    }
                    Some(Value::Bool(true)) | Some(Value::String(_)) => {
                        output.push_str(&render_section(section_body, ctx)?);
                    }
                    _ => {} // Empty or false: skip
                }

                pos = after;
            } else if let Some(section_name) = tag.strip_prefix('^') {
                // Inverted section: {{^name}}...{{/name}}
                let section_name = section_name.trim();
                let (section_body, after) = find_section(template, pos, section_name)?;

                let is_absent = match ctx.get(section_name) {
                    None | Some(Value::Bool(false)) | Some(Value::Null) => true,
                    Some(Value::List(items)) => items.is_empty(),
                    _ => false,
                };

                if is_absent {
                    output.push_str(&render_section(section_body, ctx)?);
                }

                pos = after;
            } else if let Some(closing) = tag.strip_prefix('/') {
                // Closing tag encountered out of context - shouldn't happen in well-formed templates
                return Err(NanowebError::Template(format!(
                    "Unexpected closing tag: {}",
                    closing
                )));
            } else {
                // Variable substitution: {{name}}
                match ctx.get(tag) {
                    Some(Value::String(s)) => output.push_str(&html_escape(s)),
                    Some(Value::Bool(b)) => output.push_str(if *b { "true" } else { "false" }),
                    Some(Value::Null) | None => {} // Empty
                    Some(Value::List(_)) => {}     // Lists not interpolated directly
                }
            }
        } else {
            output.push_str(&template[pos..]);
            break;
        }
    }

    Ok(output)
}

fn find_open(s: &str) -> Option<usize> {
    s.find("{{")
}

fn find_section<'a>(
    template: &'a str,
    start: usize,
    name: &str,
) -> Result<(&'a str, usize), NanowebError> {
    let open_tag = format!("{{{{#{}}}}}", name);
    let close_tag = format!("{{{{/{}}}}}", name);

    let rest = &template[start..];

    // Find matching close tag (handling nesting)
    let mut depth = 1;
    let mut search_pos = 0;

    while search_pos < rest.len() {
        if let Some(open) = rest[search_pos..].find(&open_tag) {
            if let Some(close) = rest[search_pos..].find(&close_tag) {
                if open < close {
                    depth += 1;
                    search_pos += open + open_tag.len();
                } else {
                    depth -= 1;
                    if depth == 0 {
                        let body = &rest[..search_pos + close];
                        let after = start + search_pos + close + close_tag.len();
                        return Ok((body, after));
                    }
                    search_pos += close + close_tag.len();
                }
            } else {
                // No close at all
                break;
            }
        } else if let Some(close) = rest[search_pos..].find(&close_tag) {
            depth -= 1;
            if depth == 0 {
                let body = &rest[..search_pos + close];
                let after = start + search_pos + close + close_tag.len();
                return Ok((body, after));
            }
            search_pos += close + close_tag.len();
        } else {
            break;
        }
    }

    Err(NanowebError::Template(format!(
        "Unclosed section: {}",
        name
    )))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable() {
        let r = Renderer::new();
        let ctx = Context::new().with("name", "World");
        let out = r.render("Hello, {{name}}!", &ctx).unwrap();
        assert_eq!(out, "Hello, World!");
    }

    #[test]
    fn test_missing_variable_is_empty() {
        let r = Renderer::new();
        let ctx = Context::new();
        let out = r.render("Hello, {{name}}!", &ctx).unwrap();
        assert_eq!(out, "Hello, !");
    }

    #[test]
    fn test_html_escaping() {
        let r = Renderer::new();
        let ctx = Context::new().with("code", "<script>alert(1)</script>");
        let out = r.render("{{code}}", &ctx).unwrap();
        assert!(!out.contains('<'));
        assert!(out.contains("&lt;"));
    }

    #[test]
    fn test_section_with_list() {
        let r = Renderer::new();
        let items = vec![
            Context::new().with("item", "foo"),
            Context::new().with("item", "bar"),
        ];
        let mut ctx = Context::new();
        ctx.set("items", Value::List(items));
        let out = r
            .render("{{#items}}<li>{{item}}</li>{{/items}}", &ctx)
            .unwrap();
        assert_eq!(out, "<li>foo</li><li>bar</li>");
    }

    #[test]
    fn test_inverted_section_absent() {
        let r = Renderer::new();
        let ctx = Context::new(); // No "items" key
        let out = r.render("{{^items}}empty{{/items}}", &ctx).unwrap();
        assert_eq!(out, "empty");
    }

    #[test]
    fn test_inverted_section_present() {
        let r = Renderer::new();
        let mut ctx = Context::new();
        ctx.set("items", Value::List(vec![Context::new()]));
        let out = r.render("{{^items}}empty{{/items}}", &ctx).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn test_bool_section_true() {
        let r = Renderer::new();
        let ctx = Context::new().with("show", true);
        let out = r.render("{{#show}}visible{{/show}}", &ctx).unwrap();
        assert_eq!(out, "visible");
    }

    #[test]
    fn test_bool_section_false() {
        let r = Renderer::new();
        let ctx = Context::new().with("show", false);
        let out = r.render("{{#show}}visible{{/show}}", &ctx).unwrap();
        assert_eq!(out, "");
    }
}
