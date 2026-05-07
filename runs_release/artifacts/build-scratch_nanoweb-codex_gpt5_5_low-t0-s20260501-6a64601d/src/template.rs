use crate::error::{NanowebError, Result};
use serde_json::Value;

#[derive(Default, Clone)]
pub struct Renderer;

impl Renderer {
    pub fn render(&self, template: &str, context: &Value) -> Result<String> {
        render_section(template, context)
    }
}

fn render_section(template: &str, context: &Value) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("}}") else {
            return Err(NanowebError::Template("unclosed tag".into()));
        };
        let tag = rest[start + 2..start + 2 + end].trim();
        let after = &rest[start + 2 + end + 2..];
        if let Some(name) = tag.strip_prefix('#') {
            let close = format!("{{{{/{}}}}}", name.trim());
            let Some(close_idx) = after.find(&close) else {
                return Err(NanowebError::Template("unclosed section".into()));
            };
            let inner = &after[..close_idx];
            if let Some(Value::Array(items)) = lookup(context, name.trim()) {
                for item in items {
                    out.push_str(&render_section(inner, item)?);
                }
            } else if truthy(lookup(context, name.trim())) {
                out.push_str(&render_section(inner, context)?);
            }
            rest = &after[close_idx + close.len()..];
        } else if let Some(name) = tag.strip_prefix('^') {
            let close = format!("{{{{/{}}}}}", name.trim());
            let Some(close_idx) = after.find(&close) else {
                return Err(NanowebError::Template("unclosed inverted section".into()));
            };
            let inner = &after[..close_idx];
            if !truthy(lookup(context, name.trim())) {
                out.push_str(&render_section(inner, context)?);
            }
            rest = &after[close_idx + close.len()..];
        } else if tag.starts_with('/') {
            return Err(NanowebError::Template("unexpected closing tag".into()));
        } else {
            out.push_str(&escape(&value_to_string(lookup(context, tag))));
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn lookup<'a>(context: &'a Value, key: &str) -> Option<&'a Value> {
    if key == "." {
        return Some(context);
    }
    key.split('.').try_fold(context, |cur, part| cur.get(part))
}

fn value_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(v)) => v.clone(),
        Some(Value::Number(v)) => v.to_string(),
        Some(Value::Bool(v)) => v.to_string(),
        _ => String::new(),
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(v)) => *v,
        Some(Value::Array(v)) => !v.is_empty(),
        Some(Value::Null) | None => false,
        Some(Value::String(v)) => !v.is_empty(),
        _ => true,
    }
}

fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_list_and_inverted() {
        let r = Renderer;
        let out = r
            .render(
                "{{#xs}}{{.}},{{/xs}}{{^missing}}none{{/missing}}",
                &json!({"xs":["a","b"]}),
            )
            .unwrap();
        assert_eq!(out, "a,b,none");
    }
}
