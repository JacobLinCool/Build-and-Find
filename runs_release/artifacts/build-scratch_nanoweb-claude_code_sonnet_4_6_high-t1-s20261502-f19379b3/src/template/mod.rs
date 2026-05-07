use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Bool(bool),
    List(Vec<Context>),
}

pub type Context = HashMap<String, Value>;

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Renderer
    }

    pub fn render(&self, template: &str, context: &Context) -> String {
        render_template(template, context)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn render_template(template: &str, context: &Context) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let bytes = template.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        if bytes[pos] == b'{' && bytes.get(pos + 1) == Some(&b'{') {
            // Find closing }}
            let start = pos + 2;
            if let Some(end) = find_close(bytes, start) {
                let tag = template[start..end].trim();
                let after = end + 2;

                if let Some(section_name) = tag.strip_prefix('#') {
                    // Section tag {{#name}}
                    let section_name = section_name.trim();
                    let close_tag = format!("{{{{/{}}}}}", section_name);
                    if let Some(close_pos) = template[after..].find(&close_tag) {
                        let inner = &template[after..after + close_pos];
                        match context.get(section_name) {
                            Some(Value::Bool(true)) => {
                                result.push_str(&render_template(inner, context));
                            }
                            Some(Value::List(items)) if !items.is_empty() => {
                                for item_ctx in items {
                                    result.push_str(&render_template(inner, item_ctx));
                                }
                            }
                            Some(Value::Str(s)) if !s.is_empty() => {
                                result.push_str(&render_template(inner, context));
                            }
                            _ => {}
                        }
                        pos = after + close_pos + close_tag.len();
                        continue;
                    }
                } else if let Some(section_name) = tag.strip_prefix('^') {
                    // Inverted section {{^name}}
                    let section_name = section_name.trim();
                    let close_tag = format!("{{{{/{}}}}}", section_name);
                    if let Some(close_pos) = template[after..].find(&close_tag) {
                        let inner = &template[after..after + close_pos];
                        let is_falsy = match context.get(section_name) {
                            None => true,
                            Some(Value::Bool(false)) => true,
                            Some(Value::List(items)) if items.is_empty() => true,
                            Some(Value::Str(s)) if s.is_empty() => true,
                            _ => false,
                        };
                        if is_falsy {
                            result.push_str(&render_template(inner, context));
                        }
                        pos = after + close_pos + close_tag.len();
                        continue;
                    }
                } else if tag.starts_with('/') {
                    // Closing tag - skip (handled by section parsing)
                    pos = after;
                    continue;
                } else {
                    // Variable substitution {{name}}
                    match context.get(tag) {
                        Some(Value::Str(s)) => result.push_str(&html_escape(s)),
                        Some(Value::Bool(b)) => result.push_str(if *b { "true" } else { "false" }),
                        _ => {}
                    }
                    pos = after;
                    continue;
                }
            }
        }
        // Regular character
        result.push(bytes[pos] as char);
        pos += 1;
    }

    result
}

fn find_close(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(2)
        .position(|w| w == b"}}")
        .map(|p| p + start)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn context_from_pairs(pairs: &[(&str, &str)]) -> Context {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable() {
        let renderer = Renderer::new();
        let mut ctx = Context::new();
        ctx.insert("name".into(), Value::Str("World".into()));
        let result = renderer.render("Hello, {{name}}!", &ctx);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_section_true() {
        let renderer = Renderer::new();
        let mut ctx = Context::new();
        ctx.insert("show".into(), Value::Bool(true));
        let result = renderer.render("{{#show}}visible{{/show}}", &ctx);
        assert_eq!(result, "visible");
    }

    #[test]
    fn test_section_false() {
        let renderer = Renderer::new();
        let mut ctx = Context::new();
        ctx.insert("show".into(), Value::Bool(false));
        let result = renderer.render("{{#show}}visible{{/show}}", &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn test_inverted_section() {
        let renderer = Renderer::new();
        let ctx = Context::new();
        let result = renderer.render("{{^absent}}shown{{/absent}}", &ctx);
        assert_eq!(result, "shown");
    }

    #[test]
    fn test_list_section() {
        let renderer = Renderer::new();
        let mut ctx = Context::new();
        let items = vec![
            {
                let mut c = Context::new();
                c.insert("item".into(), Value::Str("a".into()));
                c
            },
            {
                let mut c = Context::new();
                c.insert("item".into(), Value::Str("b".into()));
                c
            },
        ];
        ctx.insert("list".into(), Value::List(items));
        let result = renderer.render("{{#list}}{{item}},{{/list}}", &ctx);
        assert_eq!(result, "a,b,");
    }

    #[test]
    fn test_html_escaping() {
        let renderer = Renderer::new();
        let mut ctx = Context::new();
        ctx.insert(
            "html".into(),
            Value::Str("<script>alert('xss')</script>".into()),
        );
        let result = renderer.render("{{html}}", &ctx);
        assert!(!result.contains('<'));
        assert!(result.contains("&lt;"));
    }
}
