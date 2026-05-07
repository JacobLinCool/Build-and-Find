//! `UPDATE` statement executor.

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::{eval_expr, is_true};
use crate::parser::ast::UpdateStmt;
use crate::plan::PlanSchema;
use crate::storage::wal::{Wal, WalRecordKind};
use crate::txn::TxnId;

/// Execute an `UPDATE`, returning the number of rows modified.
pub fn execute_update(
    catalog: &mut Catalog,
    wal: &Wal,
    txn_id: TxnId,
    stmt: &UpdateStmt,
) -> Result<usize> {
    let schema = catalog.table(&stmt.table)?.schema.clone();
    let qualified = schema
        .columns
        .iter()
        .map(|c| (Some(stmt.table.clone()), c.name.clone()))
        .collect();
    let ps = PlanSchema::new(schema.clone(), qualified);

    let rows: Vec<(crate::storage::RowId, Vec<crate::types::Value>)> = {
        let tbl = catalog.table(&stmt.table)?;
        tbl.heap.snapshot()
    };

    let mut touched = 0usize;
    for (rid, row) in rows {
        if let Some(pred) = &stmt.filter {
            if !is_true(pred, &row, &ps)? {
                continue;
            }
        }
        let mut new_row = row.clone();
        for a in &stmt.assignments {
            let idx = schema
                .position(&a.column)
                .ok_or_else(|| MiniDbError::Catalog(format!("unknown column `{}`", a.column)))?;
            let val = eval_expr(&a.expr, &row, &ps)?;
            let coerced = val.coerce_to(schema.columns[idx].ty)?;
            if coerced.is_null() && schema.columns[idx].not_null {
                return Err(MiniDbError::Type(format!(
                    "column `{}` is NOT NULL",
                    a.column
                )));
            }
            new_row[idx] = coerced;
        }
        catalog.index_delete_row(&stmt.table, &row, rid.0)?;
        {
            let tbl = catalog.table_mut(&stmt.table)?;
            tbl.heap.update(rid, new_row.clone())?;
        }
        catalog.index_insert_row(&stmt.table, &new_row, rid.0)?;

        let body = format!("UPDATE {} rowid={}", stmt.table, rid.0);
        wal.append(WalRecordKind::Update, txn_id, body.as_bytes())?;
        touched += 1;
    }
    Ok(touched)
}
