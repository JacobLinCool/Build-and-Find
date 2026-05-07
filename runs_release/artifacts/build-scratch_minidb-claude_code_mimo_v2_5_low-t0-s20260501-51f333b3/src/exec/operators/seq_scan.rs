use super::PhysicalOperator;
use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::storage::manager::StorageManager;
use crate::types::{Row, Value};

pub struct SeqScan<'a> {
    table: String,
    catalog: &'a mut Catalog,
    storage: &'a mut StorageManager,
    rows: Vec<(u16, Vec<u8>)>,
    pos: usize,
    column_count: usize,
}

impl<'a> SeqScan<'a> {
    pub fn new(
        table: String,
        catalog: &'a mut Catalog,
        storage: &'a mut StorageManager,
    ) -> Result<Self> {
        let schema = catalog.get_table(&table)?;
        let column_count = schema.columns.len();

        Ok(SeqScan {
            table,
            catalog,
            storage,
            rows: Vec::new(),
            pos: 0,
            column_count,
        })
    }
}

impl<'a> PhysicalOperator for SeqScan<'a> {
    fn open(&mut self) -> Result<()> {
        let schema = self.catalog.get_table(&self.table)?;
        let root_page = schema.root_page;
        self.rows = self.storage.scan_page(root_page)?;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }

        let (_, data) = &self.rows[self.pos];
        self.pos += 1;

        let values = deserialize_row(data, self.column_count)?;
        Ok(Some(Row::new(values)))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.catalog
            .get_table(&self.table)
            .map(|s| s.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }
}

pub fn deserialize_row(data: &[u8], column_count: usize) -> Result<Vec<Value>> {
    let mut values = Vec::new();
    let mut offset = 0;

    for _ in 0..column_count {
        if offset >= data.len() {
            values.push(Value::Null);
            continue;
        }

        let type_tag = data[offset];
        offset += 1;

        match type_tag {
            0 => {
                // NULL
                values.push(Value::Null);
            }
            1 => {
                // INT
                if offset + 8 > data.len() {
                    return Err(MiniDbError::StorageError("corrupt row data".to_string()));
                }
                let v = i64::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]);
                values.push(Value::Int(v));
                offset += 8;
            }
            2 => {
                // REAL
                if offset + 8 > data.len() {
                    return Err(MiniDbError::StorageError("corrupt row data".to_string()));
                }
                let v = f64::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]);
                values.push(Value::Real(v));
                offset += 8;
            }
            3 => {
                // TEXT
                if offset + 4 > data.len() {
                    return Err(MiniDbError::StorageError("corrupt row data".to_string()));
                }
                let len = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]) as usize;
                offset += 4;
                if offset + len > data.len() {
                    return Err(MiniDbError::StorageError("corrupt row data".to_string()));
                }
                let s = String::from_utf8_lossy(&data[offset..offset + len]).to_string();
                values.push(Value::Text(s));
                offset += len;
            }
            4 => {
                // BOOL
                if offset >= data.len() {
                    return Err(MiniDbError::StorageError("corrupt row data".to_string()));
                }
                values.push(Value::Bool(data[offset] != 0));
                offset += 1;
            }
            _ => {
                return Err(MiniDbError::StorageError(format!(
                    "unknown type tag: {}",
                    type_tag
                )));
            }
        }
    }

    Ok(values)
}

pub fn serialize_row(values: &[Value]) -> Vec<u8> {
    let mut data = Vec::new();
    for val in values {
        match val {
            Value::Null => data.push(0),
            Value::Int(v) => {
                data.push(1);
                data.extend_from_slice(&v.to_le_bytes());
            }
            Value::Real(v) => {
                data.push(2);
                data.extend_from_slice(&v.to_le_bytes());
            }
            Value::Text(s) => {
                data.push(3);
                let bytes = s.as_bytes();
                data.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                data.extend_from_slice(bytes);
            }
            Value::Bool(v) => {
                data.push(4);
                data.push(*v as u8);
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let values = vec![
            Value::Int(42),
            Value::Text("hello".to_string()),
            Value::Bool(true),
            Value::Null,
            Value::Real(3.14),
        ];
        let data = serialize_row(&values);
        let deserialized = deserialize_row(&data, 5).unwrap();
        assert_eq!(values, deserialized);
    }
}
