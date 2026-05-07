use super::expr::eval;
use super::operator::{ExecSchema, PhysicalOperator, Tuple};
use crate::error::Result;
use crate::parser::ast::Expr;
use crate::types::Value;
use std::cmp::Ordering;

pub struct Sort {
    child: Box<dyn PhysicalOperator>,
    keys: Vec<(Expr, bool)>,
    sorted: Vec<Tuple>,
    cursor: usize,
}

impl Sort {
    pub fn new(child: Box<dyn PhysicalOperator>, keys: Vec<(Expr, bool)>) -> Self {
        Sort {
            child,
            keys,
            sorted: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self) -> Result<()> {
        self.child.open()?;
        let schema = self.child.schema().clone();
        self.sorted.clear();
        while let Some(r) = self.child.next()? {
            self.sorted.push(r);
        }
        // Compute key vectors once for stable performance.
        let mut decorated: Vec<(Vec<Value>, Tuple)> = Vec::with_capacity(self.sorted.len());
        for row in self.sorted.drain(..) {
            let mut kvs = Vec::with_capacity(self.keys.len());
            for (e, _) in &self.keys {
                kvs.push(eval(e, &schema, &row)?);
            }
            decorated.push((kvs, row));
        }
        let asc_flags: Vec<bool> = self.keys.iter().map(|(_, a)| *a).collect();
        decorated.sort_by(|a, b| compare_keys(&a.0, &b.0, &asc_flags));
        self.sorted = decorated.into_iter().map(|(_, r)| r).collect();
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        if self.cursor < self.sorted.len() {
            let r = self.sorted[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> &ExecSchema {
        self.child.schema()
    }
}

fn compare_keys(a: &[Value], b: &[Value], asc: &[bool]) -> Ordering {
    for ((va, vb), is_asc) in a.iter().zip(b.iter()).zip(asc.iter()) {
        let ord = match (va.is_null(), vb.is_null()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => va.cmp_sql(vb).unwrap_or(Ordering::Equal),
        };
        if ord != Ordering::Equal {
            return if *is_asc { ord } else { ord.reverse() };
        }
    }
    Ordering::Equal
}
