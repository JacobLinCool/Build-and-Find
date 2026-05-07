//! Row insertion. Exposes a plain function rather than the Volcano trait
//! because INSERT is a sink, not a source.

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::eval_expr;
use crate::parser::ast::InsertStmt;
use crate::plan::PlanSchema;
use crate::storage::wal::{Wal, WalRecordKind};
use crate::txn::TxnId;
use crate::types::{Row, Schema, Value};

/// Execute an `INSERT` statement. Returns the number of rows inserted.
pub fn execute_insert(
    catalog: &mut Catalog,
    wal: &Wal,
    txn_id: TxnId,
    stmt: &InsertStmt,
) -> Result<usize> {
    let tbl_schema = catalog.table(&stmt.table)?.schema.clone();
    let column_order = resolve_column_order(&tbl_schema, stmt.columns.as_deref())?;

    let mut inserted = 0usize;
    for row_exprs in &stmt.rows {
        if row_exprs.len() != column_order.len() {
            return Err(MiniDbError::Parse(format!(
                "insert row has {} values, expected {}",
                row_exprs.len(),
                column_order.len()
            )));
        }
        let empty = PlanSchema::new(Schema::default(), vec![]);
        let empty_row: Row = Vec::new();
        let mut full: Row = vec![Value::Null; tbl_schema.len()];
        // Apply DEFAULT values first.
        for (i, col) in tbl_schema.columns.iter().enumerate() {
            full[i] = col.default.clone().unwrap_or(Value::Null);
        }
        for (slot, expr) in column_order.iter().zip(row_exprs.iter()) {
            full[*slot] = eval_expr(expr, &empty_row, &empty)?;
        }
        // Enforce NOT NULL and type coercion.
        for (i, col) in tbl_schema.columns.iter().enumerate() {
            let v = full[i].clone();
            if v.is_null() && col.not_null {
                return Err(MiniDbError::Type(format!(
                    "column `{}` is NOT NULL but value is null",
                    col.name
                )));
            }
            full[i] = v.coerce_to(col.ty)?;
        }
        let tbl = catalog.table_mut(&stmt.table)?;
        let rid = tbl.heap.insert(full.clone());
        catalog.index_insert_row(&stmt.table, &full, rid.0)?;

        let body = format!(
            "INSERT {} rowid={} cols={}",
            stmt.table,
            rid.0,
            full.len()
        );
        wal.append(WalRecordKind::Insert, txn_id, body.as_bytes())?;
        inserted += 1;
    }
    Ok(inserted)
}

fn resolve_column_order(schema: &Schema, columns: Option<&[String]>) -> Result<Vec<usize>> {
    match columns {
        None => Ok((0..schema.len()).collect()),
        Some(cols) => {
            let mut out = Vec::with_capacity(cols.len());
            for c in cols {
                let idx = schema.position(c).ok_or_else(|| {
                    MiniDbError::Catalog(format!("column `{c}` not found in schema"))
                })?;
                out.push(idx);
            }
            Ok(out)
        }
    }
}
