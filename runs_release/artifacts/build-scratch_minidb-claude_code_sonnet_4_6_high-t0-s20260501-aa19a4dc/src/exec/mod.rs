pub mod eval;
pub mod operators;

pub use operators::ExecutionContext;

use crate::error::Result;
use crate::parser::ast::Statement;
use crate::plan::{build_logical_plan, build_physical_plan};
use crate::types::{Row, Schema};
use operators::PhysicalOperator;

/// Result of executing a statement
#[derive(Debug)]
pub enum ExecResult {
    Rows { schema: Schema, rows: Vec<Row> },
    RowsAffected(usize),
    Done,
}

/// Execute a statement using the executor context
pub fn execute(stmt: &Statement, ctx: &mut ExecutionContext) -> Result<ExecResult> {
    use Statement::*;
    match stmt {
        Begin => {
            ctx.txn.begin()?;
            Ok(ExecResult::Done)
        }
        Commit => {
            ctx.txn.commit()?;
            ctx.pager.flush_all()?;
            Ok(ExecResult::Done)
        }
        Rollback => {
            let records = ctx.txn.rollback()?;
            // Undo the changes from the rolled-back transaction
            undo_records(&records, ctx)?;
            Ok(ExecResult::Done)
        }
        MetaSchema | MetaTables | MetaIndexes | MetaExit | MetaFormat(_) => {
            // These are handled at the CLI level
            Ok(ExecResult::Done)
        }
        _ => {
            // All other statements go through the planner
            let auto_commit = !ctx.txn.is_active();
            if auto_commit {
                ctx.txn.begin_auto()?;
            }

            let result = execute_planned(stmt, ctx);

            match result {
                Ok(res) => {
                    if auto_commit {
                        ctx.txn.commit()?;
                        ctx.pager.flush_all()?;
                    }
                    Ok(res)
                }
                Err(e) => {
                    if auto_commit && ctx.txn.is_active() {
                        let _ = ctx.txn.rollback();
                    }
                    Err(e)
                }
            }
        }
    }
}

fn execute_planned(stmt: &Statement, ctx: &mut ExecutionContext) -> Result<ExecResult> {
    let logical = build_logical_plan(stmt);
    let physical = build_physical_plan(&logical, &ctx.catalog)?;
    execute_physical_plan(&physical, ctx)
}

fn execute_physical_plan(
    plan: &crate::plan::PhysicalPlan,
    ctx: &mut ExecutionContext,
) -> Result<ExecResult> {
    use crate::plan::PhysicalPlan::*;

    match plan {
        CreateTable(stmt) => {
            operators::do_create_table(stmt, ctx)?;
            Ok(ExecResult::Done)
        }
        DropTable(stmt) => {
            operators::do_drop_table(stmt, ctx)?;
            Ok(ExecResult::Done)
        }
        CreateIndex(stmt) => {
            operators::do_create_index(stmt, ctx)?;
            Ok(ExecResult::Done)
        }
        DropIndex(stmt) => {
            operators::do_drop_index(stmt, ctx)?;
            Ok(ExecResult::Done)
        }
        Insert {
            table,
            columns,
            values,
        } => {
            let count = operators::do_insert(table, columns.as_deref(), values, ctx)?;
            Ok(ExecResult::RowsAffected(count))
        }
        Update {
            table,
            input,
            assignments,
        } => {
            let count = operators::do_update(table, input, assignments, ctx)?;
            Ok(ExecResult::RowsAffected(count))
        }
        Delete { table, input } => {
            let count = operators::do_delete(table, input, ctx)?;
            Ok(ExecResult::RowsAffected(count))
        }
        _ => {
            // SELECT-style plans: materialize all rows
            let mut op = build_operator(plan, ctx)?;
            let schema = op.schema().clone();
            op.open(ctx)?;
            let mut rows = Vec::new();
            while let Some(row) = op.next(ctx)? {
                rows.push(row);
            }
            op.close(ctx)?;
            Ok(ExecResult::Rows { schema, rows })
        }
    }
}

fn undo_records(records: &[crate::storage::WalRecord], ctx: &mut ExecutionContext) -> Result<()> {
    // Iterate in reverse to undo
    for record in records.iter().rev() {
        match record {
            crate::storage::WalRecord::InsertRow { table, row_id, .. } => {
                // Delete the row that was inserted
                if let Ok(table_meta) = ctx.catalog.get_table(table) {
                    let heap =
                        crate::storage::Heap::open(crate::storage::PageId(table_meta.heap_root));
                    // Find and delete by row_id
                    let mut target = None;
                    heap.scan(&mut ctx.pager, |pid, slot, data| {
                        // Parse row_id from data prefix
                        if data.len() >= 8 {
                            let rid = u64::from_le_bytes(data[..8].try_into().unwrap());
                            if rid == *row_id {
                                target = Some((pid, slot));
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    })?;
                    if let Some((pid, slot)) = target {
                        let _ = heap.delete_row(&mut ctx.pager, pid, slot);
                    }
                }
            }
            crate::storage::WalRecord::DeleteRow {
                table,
                row_id,
                old_data,
                ..
            } => {
                // Re-insert the deleted row
                if let Ok(table_meta) = ctx.catalog.get_table(table) {
                    let heap =
                        crate::storage::Heap::open(crate::storage::PageId(table_meta.heap_root));
                    let _ = heap.insert_row(&mut ctx.pager, old_data);
                }
            }
            crate::storage::WalRecord::UpdateRow {
                table,
                row_id,
                old_data,
                ..
            } => {
                // Restore old data
                if let Ok(table_meta) = ctx.catalog.get_table(table) {
                    let heap =
                        crate::storage::Heap::open(crate::storage::PageId(table_meta.heap_root));
                    let mut target = None;
                    heap.scan(&mut ctx.pager, |pid, slot, data| {
                        if data.len() >= 8 {
                            let rid = u64::from_le_bytes(data[..8].try_into().unwrap());
                            if rid == *row_id {
                                target = Some((pid, slot));
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    })?;
                    if let Some((pid, slot)) = target {
                        let _ = heap.update_row(&mut ctx.pager, pid, slot, old_data);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn build_operator(
    plan: &crate::plan::PhysicalPlan,
    ctx: &mut ExecutionContext,
) -> Result<Box<dyn PhysicalOperator>> {
    use crate::plan::PhysicalPlan as PP;
    match plan {
        PP::SeqScan {
            table,
            alias,
            predicate,
        } => Ok(Box::new(operators::SeqScan::new(
            table.clone(),
            alias.clone(),
            predicate.clone(),
            ctx,
        )?)),
        PP::IndexScan {
            table,
            alias,
            index_name,
            index_col,
            eq_val,
            range_lo,
            range_hi,
            predicate,
        } => Ok(Box::new(operators::IndexScan::new(
            table.clone(),
            alias.clone(),
            index_name.clone(),
            index_col.clone(),
            eq_val.clone(),
            range_lo.clone(),
            range_hi.clone(),
            predicate.clone(),
            ctx,
        )?)),
        PP::Filter { input, predicate } => {
            let inner = build_operator(input, ctx)?;
            Ok(Box::new(operators::Filter::new(inner, predicate.clone())))
        }
        PP::Project { input, items } => {
            let inner = build_operator(input, ctx)?;
            Ok(Box::new(operators::Project::new(
                inner,
                items.clone(),
                ctx,
            )?))
        }
        PP::NestedLoopJoin {
            left,
            right,
            join_type,
            condition,
        } => {
            let left_op = build_operator(left, ctx)?;
            let right_op = build_operator(right, ctx)?;
            Ok(Box::new(operators::NestedLoopJoin::new(
                left_op,
                right_op,
                join_type.clone(),
                condition.clone(),
            )))
        }
        PP::Sort { input, order_by } => {
            let inner = build_operator(input, ctx)?;
            Ok(Box::new(operators::Sort::new(inner, order_by.clone())))
        }
        PP::Limit {
            input,
            limit,
            offset,
        } => {
            let inner = build_operator(input, ctx)?;
            Ok(Box::new(operators::Limit::new(
                inner,
                limit.clone(),
                offset.clone(),
            )))
        }
        PP::Empty => Ok(Box::new(operators::EmptyOperator::new())),
        _ => Ok(Box::new(operators::EmptyOperator::new())),
    }
}
