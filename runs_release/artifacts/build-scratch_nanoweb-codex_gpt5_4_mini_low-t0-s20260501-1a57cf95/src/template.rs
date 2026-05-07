use crate::error::NanowebError;
use serde_json::Value;

#[derive(Default)]
pub struct Renderer;

impl Renderer {
    pub fn render(&self, template: &str, context: &Value) -> Result<String, NanowebError> {
        render_inner(template, context)
    }
}

fn render_inner(template: &str, context: &Value) -> Result<String, NanowebError> {
    let mut out = String::new();
    let mut i = 0;
    while let Some(start) = template[i..].find("{{") {
        out.push_str(&template[i..i + start]);
        let rest = &template[i + start + 2..];
        let end = rest
            .find("}}")
            .ok_or_else(|| NanowebError::parse("unclosed tag"))?;
        let tag = rest[..end].trim();
        i = i + start + 2 + end + 2;
        if let Some(name) = tag.strip_prefix('#') {
            let close = format!("{{{{/{}}}}}", name.trim());
            let body_end = template[i..]
                .find(&close)
                .ok_or_else(|| NanowebError::parse("missing close tag"))?;
            let inner = &template[i..i + body_end];
            if let Some(v) = lookup(context, name.trim()) {
                if let Some(arr) = v.as_array() {
                    for item in arr {
                        out.push_str(&render_inner(inner, item)?);
                    }
                } else if truthy(v) {
                    out.push_str(&render_inner(inner, v)?);
                }
            }
            i += body_end + close.len();
        } else if let Some(name) = tag.strip_prefix('^') {
            let close = format!("{{{{/{}}}}}", name.trim());
            let body_end = template[i..]
                .find(&close)
                .ok_or_else(|| NanowebError::parse("missing close tag"))?;
            let inner = &template[i..i + body_end];
            if lookup(context, name.trim()).map_or(true, |v| !truthy(v)) {
                out.push_str(&render_inner(inner, context)?);
            }
            i += body_end + close.len();
        } else if let Some(v) = lookup(context, tag) {
            let rendered = v
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string());
            out.push_str(&rendered);
        }
    }
    out.push_str(&template[i..]);
    Ok(out)
}

fn lookup<'a>(ctx: &'a Value, key: &str) -> Option<&'a Value> {
    ctx.as_object().and_then(|m| m.get(key))
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Array(a) => !a.is_empty(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders() {
        let r = Renderer::default();
        let v = serde_json::json!({"name":"x","list":[{"name":"a"}],"absent":false});
        let rendered = r
            .render(
                "hi {{name}} {{#list}}{{name}}{{/list}}{{^absent}}y{{/absent}}",
                &v,
            )
            .unwrap();
        assert!(rendered.contains("hi x"));
        assert!(rendered.contains('a'));
        assert!(rendered.contains('y'));
    }
}
