use super::operator::{ExecSchema, PhysicalOperator, Tuple};
use crate::error::Result;

pub struct Limit {
    child: Box<dyn PhysicalOperator>,
    limit: Option<i64>,
    offset: i64,
    skipped: i64,
    emitted: i64,
}

impl Limit {
    pub fn new(child: Box<dyn PhysicalOperator>, limit: Option<i64>, offset: Option<i64>) -> Self {
        Limit {
            child,
            limit,
            offset: offset.unwrap_or(0),
            skipped: 0,
            emitted: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self) -> Result<()> {
        self.child.open()?;
        self.skipped = 0;
        self.emitted = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        while self.skipped < self.offset {
            if self.child.next()?.is_none() {
                return Ok(None);
            }
            self.skipped += 1;
        }
        if let Some(l) = self.limit {
            if self.emitted >= l {
                return Ok(None);
            }
        }
        match self.child.next()? {
            Some(t) => {
                self.emitted += 1;
                Ok(Some(t))
            }
            None => Ok(None),
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> &ExecSchema {
        self.child.schema()
    }
}
