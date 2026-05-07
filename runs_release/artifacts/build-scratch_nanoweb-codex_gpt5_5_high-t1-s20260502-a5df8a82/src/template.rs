use serde::Serialize;
use serde_json::Value;

use crate::error::{NanowebError, Result};

#[derive(Clone, Default)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render<T: Serialize>(&self, template: &str, context: &T) -> Result<String> {
        let value = serde_json::to_value(context)?;
        render_inner(template, &value)
    }
}

fn render_inner(template: &str, context: &Value) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let close = after
            .find("}}")
            .ok_or_else(|| NanowebError::Parse("unclosed template tag".into()))?;
        let tag = after[..close].trim();
        rest = &after[close + 2..];
        if let Some(name) = tag.strip_prefix('#') {
            let (body, tail) = section_body(rest, name.trim())?;
            if let Some(value) = lookup(context, name.trim()) {
                match value {
                    Value::Array(items) => {
                        for item in items {
                            out.push_str(&render_inner(body, item)?);
                        }
                    }
                    other if truthy(other) => out.push_str(&render_inner(body, other)?),
                    _ => {}
                }
            }
            rest = tail;
        } else if let Some(name) = tag.strip_prefix('^') {
            let (body, tail) = section_body(rest, name.trim())?;
            if lookup(context, name.trim()).is_none_or(|v| !truthy(v)) {
                out.push_str(&render_inner(body, context)?);
            }
            rest = tail;
        } else if tag.starts_with('/') {
            return Err(NanowebError::Parse(
                "unexpected closing template tag".into(),
            ));
        } else if let Some(value) = lookup(context, tag) {
            out.push_str(&escape(&value_to_string(value)));
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn section_body<'a>(template: &'a str, name: &str) -> Result<(&'a str, &'a str)> {
    let close_tag = format!("{{{{/{name}}}}}");
    let end = template
        .find(&close_tag)
        .ok_or_else(|| NanowebError::Parse(format!("missing closing tag for {name}")))?;
    Ok((&template[..end], &template[end + close_tag.len()..]))
}

fn lookup<'a>(context: &'a Value, key: &str) -> Option<&'a Value> {
    if key == "." {
        return Some(context);
    }
    key.split('.').try_fold(context, |value, part| match value {
        Value::Object(map) => map.get(part),
        _ => None,
    })
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Array(v) => !v.is_empty(),
        Value::String(v) => !v.is_empty(),
        _ => true,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(v) => v.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        other => other.to_string(),
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn renders_variables_and_sections() {
        let output = Renderer::new()
            .render(
                "Hi {{name}}{{#items}} {{.}}{{/items}}{{^missing}} none{{/missing}}",
                &json!({"name":"Ada","items":["a","b"]}),
            )
            .unwrap();
        assert_eq!(output, "Hi Ada a b none");
    }
}
