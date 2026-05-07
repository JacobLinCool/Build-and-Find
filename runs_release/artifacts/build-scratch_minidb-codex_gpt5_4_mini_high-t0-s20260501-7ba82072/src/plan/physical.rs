use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PhysicalPlan {
    Operator,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exists() {
        let _ = PhysicalPlan::Operator;
    }
}
