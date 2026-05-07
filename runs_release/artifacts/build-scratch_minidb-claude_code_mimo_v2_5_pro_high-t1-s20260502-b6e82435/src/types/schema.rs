use crate::types::value::{ColumnType, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub constraints: Vec<ColumnConstraint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnConstraint {
    PrimaryKey,
    NotNull,
    Unique,
    Default(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableDef {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub check_constraints: Vec<String>, // stored as SQL expression text
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    pub name: String,
    pub table_name: String,
    pub column_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<ColumnDef>,
}

impl Schema {
    pub fn new(columns: Vec<ColumnDef>) -> Self {
        Self { columns }
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column_names(&self) -> Vec<&str> {
        self.columns.iter().map(|c| c.name.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn primary_key_index(&self) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.constraints.contains(&ColumnConstraint::PrimaryKey))
    }

    pub fn has_primary_key(&self) -> bool {
        self.primary_key_index().is_some()
    }

    pub fn default_row(&self) -> Vec<Value> {
        self.columns
            .iter()
            .map(|c| {
                c.constraints
                    .iter()
                    .find_map(|constraint| {
                        if let ColumnConstraint::Default(v) = constraint {
                            Some(v.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| Value::default_for(c.col_type))
            })
            .collect()
    }
}

impl TableDef {
    pub fn schema(&self) -> Schema {
        Schema::new(self.columns.clone())
    }
}
