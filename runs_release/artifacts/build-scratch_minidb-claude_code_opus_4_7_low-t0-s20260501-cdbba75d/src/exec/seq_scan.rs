use super::operator::{ExecColumn, ExecSchema, PhysicalOperator, Tuple};
use crate::catalog::TableMeta;
use crate::error::Result;

/// Volcano `SeqScan`: emits rows of one table in row-id order.
pub struct SeqScan {
    schema: ExecSchema,
    rows: Vec<Tuple>,
    cursor: usize,
}

impl SeqScan {
    pub fn new(meta: &TableMeta, alias: Option<&str>) -> Self {
        let qualifier = alias.unwrap_or(&meta.name).to_string();
        let columns = meta
            .schema
            .columns
            .iter()
            .map(|c| ExecColumn {
                qualifier: Some(qualifier.clone()),
                name: c.name.clone(),
                data_type: Some(c.data_type),
            })
            .collect();
        let rows = meta.rows.values().cloned().collect();
        SeqScan {
            schema: ExecSchema { columns },
            rows,
            cursor: 0,
        }
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self) -> Result<()> {
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        if self.cursor < self.rows.len() {
            let row = self.rows[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &ExecSchema {
        &self.schema
    }
}
