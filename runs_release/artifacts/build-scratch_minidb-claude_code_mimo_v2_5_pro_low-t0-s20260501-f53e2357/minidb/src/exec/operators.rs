use crate::error::Result;
use crate::types::{Row, Value};

/// Trait for physical operators in the Volcano-style execution model
pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<String>;
}

/// Sequential scan operator
pub struct SeqScanOp {
    pub table: String,
    pub rows: Vec<Row>,
    pub column_names: Vec<String>,
    pos: usize,
}

impl SeqScanOp {
    pub fn new(table: String, rows: Vec<Row>, column_names: Vec<String>) -> Self {
        Self {
            table,
            rows,
            column_names,
            pos: 0,
        }
    }
}

impl PhysicalOperator for SeqScanOp {
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let row = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.column_names.clone()
    }
}

/// Index scan operator
pub struct IndexScanOp {
    pub rows: Vec<Row>,
    pub column_names: Vec<String>,
    pos: usize,
}

impl IndexScanOp {
    pub fn new(rows: Vec<Row>, column_names: Vec<String>) -> Self {
        Self {
            rows,
            column_names,
            pos: 0,
        }
    }
}

impl PhysicalOperator for IndexScanOp {
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let row = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.column_names.clone()
    }
}

/// Filter operator
pub struct FilterOp {
    pub child: Box<dyn PhysicalOperator>,
    pub predicate: Box<dyn Fn(&Row) -> bool>,
}

impl PhysicalOperator for FilterOp {
    fn open(&mut self) -> Result<()> {
        self.child.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            match self.child.next()? {
                Some(row) => {
                    if (self.predicate)(&row) {
                        return Ok(Some(row));
                    }
                }
                None => return Ok(None),
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.child.schema()
    }
}

/// Project operator
pub struct ProjectOp {
    pub child: Box<dyn PhysicalOperator>,
    pub column_names: Vec<String>,
    pub indices: Vec<usize>,
}

impl PhysicalOperator for ProjectOp {
    fn open(&mut self) -> Result<()> {
        self.child.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        match self.child.next()? {
            Some(row) => {
                let projected: Row = self
                    .indices
                    .iter()
                    .map(|&i| row.get(i).cloned().unwrap_or(Value::Null))
                    .collect();
                Ok(Some(projected))
            }
            None => Ok(None),
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.column_names.clone()
    }
}

/// Nested loop join operator
pub struct NestedLoopJoinOp {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    pub left_schema_len: usize,
    pub right_schema_len: usize,
    current_left: Option<Row>,
    right_exhausted: bool,
}

impl NestedLoopJoinOp {
    pub fn new(left: Box<dyn PhysicalOperator>, right: Box<dyn PhysicalOperator>) -> Self {
        let left_schema_len = left.schema().len();
        let right_schema_len = right.schema().len();
        Self {
            left,
            right,
            left_schema_len,
            right_schema_len,
            current_left: None,
            right_exhausted: false,
        }
    }
}

impl PhysicalOperator for NestedLoopJoinOp {
    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        self.right.open()?;
        self.current_left = self.left.next()?;
        self.right_exhausted = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.current_left.is_none() {
                return Ok(None);
            }

            if !self.right_exhausted {
                match self.right.next()? {
                    Some(right_row) => {
                        let mut joined = self.current_left.clone().unwrap();
                        joined.extend(right_row);
                        return Ok(Some(joined));
                    }
                    None => {
                        self.right_exhausted = true;
                    }
                }
            }

            // Move to next left row and reset right
            self.current_left = self.left.next()?;
            if self.current_left.is_some() {
                self.right.close()?;
                self.right.open()?;
                self.right_exhausted = false;
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.left.close()?;
        self.right.close()?;
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        let mut s = self.left.schema();
        s.extend(self.right.schema());
        s
    }
}

/// Sort operator
pub struct SortOp {
    pub child: Box<dyn PhysicalOperator>,
    pub column_names: Vec<String>,
    pub sort_indices: Vec<usize>,
    pub ascending: Vec<bool>,
    sorted_rows: Vec<Row>,
    pos: usize,
}

impl SortOp {
    pub fn new(
        child: Box<dyn PhysicalOperator>,
        column_names: Vec<String>,
        sort_indices: Vec<usize>,
        ascending: Vec<bool>,
    ) -> Self {
        Self {
            child,
            column_names,
            sort_indices,
            ascending,
            sorted_rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for SortOp {
    fn open(&mut self) -> Result<()> {
        self.child.open()?;
        self.sorted_rows.clear();
        loop {
            match self.child.next()? {
                Some(row) => self.sorted_rows.push(row),
                None => break,
            }
        }

        let sort_indices = self.sort_indices.clone();
        let ascending = self.ascending.clone();
        self.sorted_rows.sort_by(|a, b| {
            for (idx_i, &col_idx) in sort_indices.iter().enumerate() {
                let va = a.get(col_idx).unwrap_or(&Value::Null);
                let vb = b.get(col_idx).unwrap_or(&Value::Null);
                let ord = va.partial_cmp(vb).unwrap_or(std::cmp::Ordering::Equal);
                if ord != std::cmp::Ordering::Equal {
                    return if ascending.get(idx_i).copied().unwrap_or(true) {
                        ord
                    } else {
                        ord.reverse()
                    };
                }
            }
            std::cmp::Ordering::Equal
        });

        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.sorted_rows.len() {
            let row = self.sorted_rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.column_names.clone()
    }
}

/// Limit operator
pub struct LimitOp {
    pub child: Box<dyn PhysicalOperator>,
    pub count: usize,
    pub offset: usize,
    emitted: usize,
}

impl LimitOp {
    pub fn new(child: Box<dyn PhysicalOperator>, count: usize, offset: usize) -> Self {
        Self {
            child,
            count,
            offset,
            emitted: 0,
        }
    }
}

impl PhysicalOperator for LimitOp {
    fn open(&mut self) -> Result<()> {
        self.child.open()?;
        self.emitted = 0;
        // Skip offset rows
        for _ in 0..self.offset {
            if self.child.next()?.is_none() {
                break;
            }
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.emitted >= self.count {
            return Ok(None);
        }
        match self.child.next()? {
            Some(row) => {
                self.emitted += 1;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.child.schema()
    }
}

/// Insert operator
pub struct InsertOp {
    pub rows_affected: u64,
}

impl InsertOp {
    pub fn new() -> Self {
        Self { rows_affected: 0 }
    }
}

impl PhysicalOperator for InsertOp {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["rows_affected".into()]
    }
}

/// Update operator
pub struct UpdateOp {
    pub rows_affected: u64,
}

impl UpdateOp {
    pub fn new() -> Self {
        Self { rows_affected: 0 }
    }
}

impl PhysicalOperator for UpdateOp {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["rows_affected".into()]
    }
}

/// Delete operator
pub struct DeleteOp {
    pub rows_affected: u64,
}

impl DeleteOp {
    pub fn new() -> Self {
        Self { rows_affected: 0 }
    }
}

impl PhysicalOperator for DeleteOp {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["rows_affected".into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seq_scan() {
        let rows = vec![
            vec![Value::Int(1), Value::Text("a".into())],
            vec![Value::Int(2), Value::Text("b".into())],
        ];
        let mut op = SeqScanOp::new("t".into(), rows, vec!["id".into(), "name".into()]);
        op.open().unwrap();
        assert_eq!(op.next().unwrap().unwrap().len(), 2);
        assert_eq!(op.next().unwrap().unwrap().len(), 2);
        assert!(op.next().unwrap().is_none());
        op.close().unwrap();
    }

    #[test]
    fn test_limit() {
        let rows = vec![
            vec![Value::Int(1)],
            vec![Value::Int(2)],
            vec![Value::Int(3)],
            vec![Value::Int(4)],
            vec![Value::Int(5)],
        ];
        let scan = SeqScanOp::new("t".into(), rows, vec!["id".into()]);
        let mut limit = LimitOp::new(Box::new(scan), 2, 1);
        limit.open().unwrap();
        assert_eq!(limit.next().unwrap().unwrap(), vec![Value::Int(2)]);
        assert_eq!(limit.next().unwrap().unwrap(), vec![Value::Int(3)]);
        assert!(limit.next().unwrap().is_none());
        limit.close().unwrap();
    }

    #[test]
    fn test_nested_loop_join() {
        let left_rows = vec![vec![Value::Int(1)], vec![Value::Int(2)]];
        let right_rows = vec![vec![Value::Text("a".into())], vec![Value::Text("b".into())]];
        let left = SeqScanOp::new("l".into(), left_rows, vec!["id".into()]);
        let right = SeqScanOp::new("r".into(), right_rows, vec!["name".into()]);
        let mut join = NestedLoopJoinOp::new(Box::new(left), Box::new(right));
        join.open().unwrap();
        let mut count = 0;
        while join.next().unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 4); // 2x2 cross product
        join.close().unwrap();
    }
}
