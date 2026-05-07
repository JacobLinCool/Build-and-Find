use crate::error::Result;
use crate::exec::{PhysicalOperator, Schema};
use crate::types::Row;

pub struct Limit {
    pub input: Box<dyn PhysicalOperator>,
    pub limit: usize,
    pub offset: usize,
    count: usize,
    skipped: usize,
}

impl Limit {
    pub fn new(input: Box<dyn PhysicalOperator>, limit: usize, offset: usize) -> Self {
        Limit {
            input,
            limit,
            offset,
            count: 0,
            skipped: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn schema(&self) -> &Schema {
        self.input.schema()
    }

    fn open(&mut self) -> Result<()> {
        self.count = 0;
        self.skipped = 0;
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.count >= self.limit {
                return Ok(None);
            }
            let row = match self.input.next()? {
                None => return Ok(None),
                Some(r) => r,
            };
            if self.skipped < self.offset {
                self.skipped += 1;
                continue;
            }
            self.count += 1;
            return Ok(Some(row));
        }
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
}
