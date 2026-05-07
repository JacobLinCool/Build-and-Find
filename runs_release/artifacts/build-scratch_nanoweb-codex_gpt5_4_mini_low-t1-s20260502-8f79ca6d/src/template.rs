use std::collections::HashMap;

pub struct Renderer;

impl Renderer {
    pub fn render(template: &str, context: &HashMap<String, String>) -> String {
        let mut out = template.to_string();
        for (k, v) in context {
            out = out.replace(&format!("{{{{{}}}}}", k), v);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replace() {
        let mut ctx = HashMap::new();
        ctx.insert("name".into(), "world".into());
        assert_eq!(Renderer::render("hi {{name}}", &ctx), "hi world");
    }
}
