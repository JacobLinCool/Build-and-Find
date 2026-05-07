use super::PhysicalOperator;
use crate::error::Result;
use crate::types::Row;

pub struct Limit<'a> {
    input: Box<dyn PhysicalOperator + 'a>,
    limit: i64,
    offset: i64,
    rows_seen: i64,
    rows_returned: i64,
}

impl<'a> Limit<'a> {
    pub fn new(input: Box<dyn PhysicalOperator + 'a>, limit: i64, offset: i64) -> Self {
        Limit {
            input,
            limit,
            offset,
            rows_seen: 0,
            rows_returned: 0,
        }
    }
}

impl<'a> PhysicalOperator for Limit<'a> {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        self.rows_seen = 0;
        self.rows_returned = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        while let Some(row) = self.input.next()? {
            self.rows_seen += 1;

            if self.rows_seen <= self.offset {
                continue;
            }

            if self.rows_returned >= self.limit {
                return Ok(None);
            }

            self.rows_returned += 1;
            return Ok(Some(row));
        }
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}
