//! LIMIT / OFFSET operator.

use super::{ExecContext, PhysicalOperator};
use crate::error::Result;
use crate::types::{Row, Schema};

pub struct Limit {
    limit: Option<u64>,
    offset: Option<u64>,
    child: Box<dyn PhysicalOperator>,
    schema: Schema,
    emitted: u64,
    skipped: u64,
}

impl Limit {
    pub fn new(
        limit: Option<u64>,
        offset: Option<u64>,
        child: Box<dyn PhysicalOperator>,
        schema: Schema,
    ) -> Self {
        Self {
            limit,
            offset,
            child,
            schema,
            emitted: 0,
            skipped: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Row>> {
        while let Some(off) = self.offset {
            if self.skipped >= off {
                break;
            }
            match self.child.next(ctx)? {
                Some(_) => self.skipped += 1,
                None => return Ok(None),
            }
        }
        if let Some(lim) = self.limit {
            if self.emitted >= lim {
                return Ok(None);
            }
        }
        let row = self.child.next(ctx)?;
        if row.is_some() {
            self.emitted += 1;
        }
        Ok(row)
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}
