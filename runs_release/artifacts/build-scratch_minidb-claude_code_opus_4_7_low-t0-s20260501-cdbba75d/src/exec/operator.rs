use crate::error::Result;
use crate::types::{DataType, Value};

/// A column in an executor schema. The fully-qualified `qualifier` lets
/// `table.col` and unqualified column references resolve correctly during
/// expression evaluation.
#[derive(Debug, Clone)]
pub struct ExecColumn {
    pub qualifier: Option<String>, // table or alias
    pub name: String,
    pub data_type: Option<DataType>,
}

#[derive(Debug, Clone)]
pub struct ExecSchema {
    pub columns: Vec<ExecColumn>,
}

impl ExecSchema {
    pub fn empty() -> Self {
        ExecSchema {
            columns: Vec::new(),
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }

    pub fn resolve(&self, qualifier: Option<&str>, name: &str) -> Option<usize> {
        // First exact qualifier match.
        if let Some(q) = qualifier {
            for (i, c) in self.columns.iter().enumerate() {
                if c.name.eq_ignore_ascii_case(name)
                    && c.qualifier
                        .as_ref()
                        .map(|cq| cq.eq_ignore_ascii_case(q))
                        .unwrap_or(false)
                {
                    return Some(i);
                }
            }
            None
        } else {
            // Unqualified: any column with that name (first match wins).
            self.columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(name))
        }
    }
}

pub type Tuple = Vec<Value>;

/// Volcano-style iterator. Operators must implement `open`/`next`/`close` and
/// expose their output schema. `next` returns `Ok(None)` when exhausted.
pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Tuple>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> &ExecSchema;
}
