use serde::Serialize;
use serde_json::Value;

use crate::error::{NanowebError, Result};

#[derive(Default, Clone)]
pub struct Renderer;

impl Renderer {
    pub fn render<T: Serialize>(&self, template: &str, context: &T) -> Result<String> {
        let value = serde_json::to_value(context)?;
        let stack = vec![value];
        render_template(template, &stack)
    }
}

fn render_template(template: &str, stack: &[Value]) -> Result<String> {
    let mut output = String::new();
    let mut cursor = 0;
    while let Some(start) = template[cursor..].find("{{") {
        let absolute_start = cursor + start;
        output.push_str(&template[cursor..absolute_start]);
        let end = template[absolute_start + 2..]
            .find("}}")
            .ok_or_else(|| NanowebError::Template("unterminated tag".into()))?
            + absolute_start
            + 2;
        let tag = template[absolute_start + 2..end].trim();
        cursor = end + 2;
        if let Some(name) = tag.strip_prefix('#') {
            let (inner, after) = extract_section(template, cursor, name.trim())?;
            if is_truthy(lookup(stack, name.trim())) {
                match lookup(stack, name.trim()) {
                    Some(Value::Array(items)) => {
                        for item in items {
                            let mut next_stack = stack.to_vec();
                            next_stack.push(item.clone());
                            output.push_str(&render_template(inner, &next_stack)?);
                        }
                    }
                    Some(Value::Object(_)) => {
                        let mut next_stack = stack.to_vec();
                        next_stack.push(lookup(stack, name.trim()).unwrap().clone());
                        output.push_str(&render_template(inner, &next_stack)?);
                    }
                    Some(value) => {
                        let mut next_stack = stack.to_vec();
                        next_stack.push(value.clone());
                        output.push_str(&render_template(inner, &next_stack)?);
                    }
                    None => {}
                }
            }
            cursor = after;
        } else if let Some(name) = tag.strip_prefix('^') {
            let (inner, after) = extract_section(template, cursor, name.trim())?;
            if !is_truthy(lookup(stack, name.trim())) {
                output.push_str(&render_template(inner, stack)?);
            }
            cursor = after;
        } else if tag.starts_with('/') {
            return Err(NanowebError::Template("unexpected section close".into()));
        } else {
            if let Some(value) = lookup(stack, tag) {
                output.push_str(&value_to_string(value));
            }
        }
    }
    output.push_str(&template[cursor..]);
    Ok(output)
}

fn extract_section<'a>(template: &'a str, start: usize, name: &str) -> Result<(&'a str, usize)> {
    let mut cursor = start;
    let mut depth = 1;
    while let Some(open) = template[cursor..].find("{{") {
        let absolute_open = cursor + open;
        let close = template[absolute_open + 2..]
            .find("}}")
            .ok_or_else(|| NanowebError::Template("unterminated tag".into()))?
            + absolute_open
            + 2;
        let tag = template[absolute_open + 2..close].trim();
        if tag == format!("/{}", name) {
            depth -= 1;
            if depth == 0 {
                return Ok((&template[start..absolute_open], close + 2));
            }
        } else if tag == format!("#{}", name) || tag == format!("^{}", name) {
            depth += 1;
        }
        cursor = close + 2;
    }
    Err(NanowebError::Template(format!(
        "missing closing tag for {name}"
    )))
}

fn lookup<'a>(stack: &'a [Value], key: &str) -> Option<&'a Value> {
    if key == "." {
        return stack.last();
    }
    for value in stack.iter().rev() {
        if let Some(found) = value.as_object().and_then(|map| map.get(key)) {
            return Some(found);
        }
    }
    None
}

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Null) | None => false,
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Number(value)) => value.as_i64().map_or(true, |number| number != 0),
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_variables_and_sections() {
        let renderer = Renderer;
        let template = "Hello {{name}}{{#items}} {{.}}{{/items}}{{^missing}}!{{/missing}}";
        let output = renderer
            .render(template, &json!({"name":"world","items":["a","b"]}))
            .unwrap();
        assert_eq!(output, "Hello world a b!");
    }
}
