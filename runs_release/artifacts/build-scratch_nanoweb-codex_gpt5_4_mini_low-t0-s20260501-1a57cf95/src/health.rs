pub fn healthz() -> &'static str {
    "ok"
}
pub fn readyz(ready: bool) -> &'static str {
    if ready {
        "ready"
    } else {
        "not ready"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn works() {
        assert_eq!(healthz(), "ok");
    }
}
