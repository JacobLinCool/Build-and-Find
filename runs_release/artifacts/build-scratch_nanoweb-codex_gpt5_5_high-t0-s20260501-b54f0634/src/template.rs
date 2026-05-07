use serde_json::Value;

#[derive(Default, Clone)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> String {
        render_section(template, context)
    }
}

fn render_section(template: &str, context: &Value) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let tag = rest[start + 2..start + 2 + end].trim();
        let after = &rest[start + 2 + end + 2..];
        if let Some(name) = tag.strip_prefix('#') {
            if let Some((inner, tail)) = take_block(after, name.trim()) {
                if let Some(value) = lookup(context, name.trim()) {
                    match value {
                        Value::Array(items) => {
                            for item in items {
                                out.push_str(&render_section(inner, item));
                            }
                        }
                        Value::Bool(true) => out.push_str(&render_section(inner, context)),
                        Value::Object(_) => out.push_str(&render_section(inner, value)),
                        _ => {}
                    }
                }
                rest = tail;
            } else {
                rest = after;
            }
        } else if let Some(name) = tag.strip_prefix('^') {
            if let Some((inner, tail)) = take_block(after, name.trim()) {
                let render = match lookup(context, name.trim()) {
                    None | Some(Value::Null) => true,
                    Some(Value::Bool(false)) => true,
                    Some(Value::Array(v)) if v.is_empty() => true,
                    _ => false,
                };
                if render {
                    out.push_str(&render_section(inner, context));
                }
                rest = tail;
            } else {
                rest = after;
            }
        } else if tag.starts_with('/') {
            rest = after;
        } else {
            if let Some(value) = lookup(context, tag) {
                out.push_str(&html_escape(&scalar_to_string(value)));
            }
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn take_block<'a>(s: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let close = format!("{{{{/{name}}}}}");
    let idx = s.find(&close)?;
    Some((&s[..idx], &s[idx + close.len()..]))
}

fn lookup<'a>(context: &'a Value, path: &str) -> Option<&'a Value> {
    if path == "." {
        return Some(context);
    }
    let mut value = context;
    for part in path.split('.') {
        value = value.get(part)?;
    }
    Some(value)
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
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
    fn renders_variables_sections_and_inverted() {
        let ctx = json!({"name":"Ada","items":[{"name":"a"},{"name":"b"}]});
        let out = Renderer::new().render(
            "{{name}} {{#items}}{{name}}{{/items}}{{^absent}}!{{/absent}}",
            &ctx,
        );
        assert_eq!(out, "Ada ab!");
    }
}
