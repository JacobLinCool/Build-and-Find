//! `DELETE` statement executor.

use crate::catalog::Catalog;
use crate::error::Result;
use crate::exec::is_true;
use crate::parser::ast::DeleteStmt;
use crate::plan::PlanSchema;
use crate::storage::wal::{Wal, WalRecordKind};
use crate::txn::TxnId;

pub fn execute_delete(
    catalog: &mut Catalog,
    wal: &Wal,
    txn_id: TxnId,
    stmt: &DeleteStmt,
) -> Result<usize> {
    let schema = catalog.table(&stmt.table)?.schema.clone();
    let qualified = schema
        .columns
        .iter()
        .map(|c| (Some(stmt.table.clone()), c.name.clone()))
        .collect();
    let ps = PlanSchema::new(schema.clone(), qualified);
    let rows = catalog.table(&stmt.table)?.heap.snapshot();

    let mut removed = 0usize;
    for (rid, row) in rows {
        if let Some(pred) = &stmt.filter {
            if !is_true(pred, &row, &ps)? {
                continue;
            }
        }
        catalog.index_delete_row(&stmt.table, &row, rid.0)?;
        {
            let tbl = catalog.table_mut(&stmt.table)?;
            tbl.heap.delete(rid)?;
        }
        let body = format!("DELETE {} rowid={}", stmt.table, rid.0);
        wal.append(WalRecordKind::Delete, txn_id, body.as_bytes())?;
        removed += 1;
    }
    Ok(removed)
}
