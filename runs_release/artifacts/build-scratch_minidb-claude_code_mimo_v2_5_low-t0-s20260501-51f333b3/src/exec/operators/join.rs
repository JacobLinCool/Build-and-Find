use super::PhysicalOperator;
use crate::error::Result;
use crate::types::{Row, Value};

pub struct NestedLoopJoin<'a> {
    left: Box<dyn PhysicalOperator + 'a>,
    right: Box<dyn PhysicalOperator + 'a>,
    left_schema: Vec<String>,
    right_schema: Vec<String>,
    outer_row: Option<Row>,
    inner_opened: bool,
}

impl<'a> NestedLoopJoin<'a> {
    pub fn new(
        left: Box<dyn PhysicalOperator + 'a>,
        right: Box<dyn PhysicalOperator + 'a>,
    ) -> Self {
        let left_schema = left.schema();
        let right_schema = right.schema();

        NestedLoopJoin {
            left,
            right,
            left_schema,
            right_schema,
            outer_row: None,
            inner_opened: false,
        }
    }
}

impl<'a> PhysicalOperator for NestedLoopJoin<'a> {
    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        self.outer_row = None;
        self.inner_opened = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.outer_row.is_none() {
                self.outer_row = self.left.next()?;
                if self.outer_row.is_none() {
                    return Ok(None);
                }
                self.right.open()?;
                self.inner_opened = true;
            }

            if let Some(inner_row) = self.right.next()? {
                let outer = self.outer_row.as_ref().unwrap();
                let mut combined_values = outer.values.clone();
                combined_values.extend(inner_row.values);
                return Ok(Some(Row::new(combined_values)));
            }

            // Inner exhausted, move to next outer
            self.right.close()?;
            self.inner_opened = false;
            self.outer_row = None;
        }
    }

    fn close(&mut self) -> Result<()> {
        if self.inner_opened {
            self.right.close()?;
        }
        self.left.close()
    }

    fn schema(&self) -> Vec<String> {
        let mut schema = self.left_schema.clone();
        schema.extend(self.right_schema.clone());
        schema
    }
}
