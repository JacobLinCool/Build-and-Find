use crate::error::{NanowebError, Result};
use serde_json::Value;

pub struct Renderer;

impl Renderer {
    pub fn render(template: &str, context: &Value) -> Result<String> {
        render_section(template, context)
    }
}

fn render_section(template: &str, ctx: &Value) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| NanowebError::Template("unclosed tag".into()))?;
        let tag = after[..end].trim();
        rest = &after[end + 2..];
        if let Some(name) = tag.strip_prefix('#') {
            let close = format!("{{{{/{}}}}}", name.trim());
            let pos = rest
                .find(&close)
                .ok_or_else(|| NanowebError::Template("unclosed section".into()))?;
            let inner = &rest[..pos];
            rest = &rest[pos + close.len()..];
            if let Some(Value::Array(items)) = lookup(ctx, name.trim()) {
                for item in items {
                    out.push_str(&render_section(inner, item)?);
                }
            }
        } else if let Some(name) = tag.strip_prefix('^') {
            let close = format!("{{{{/{}}}}}", name.trim());
            let pos = rest
                .find(&close)
                .ok_or_else(|| NanowebError::Template("unclosed inverted section".into()))?;
            let inner = &rest[..pos];
            rest = &rest[pos + close.len()..];
            if is_absent(lookup(ctx, name.trim())) {
                out.push_str(&render_section(inner, ctx)?);
            }
        } else if tag.starts_with('/') {
            return Err(NanowebError::Template("unexpected closing tag".into()));
        } else if let Some(v) = lookup(ctx, tag) {
            out.push_str(&escape(&value_to_string(v)));
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn lookup<'a>(ctx: &'a Value, key: &str) -> Option<&'a Value> {
    ctx.as_object()?.get(key)
}

fn is_absent(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn escape(s: &str) -> String {
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
    fn renders_list() {
        assert_eq!(
            Renderer::render("{{#xs}}{{name}}{{/xs}}", &json!({"xs":[{"name":"a"}]})).unwrap(),
            "a"
        );
    }
}
