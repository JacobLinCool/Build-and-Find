//! Top-level statement executor: glues parsing, planning, and execution.

use crate::engine::Engine;
use crate::error::{MiniDbError, Result};
use crate::exec::expr::{bind, eval};
use crate::exec::operators::{
    Delete, ExecCtx, Filter, IndexScan, Insert, Limit, NestedLoopJoin, PhysicalOperator, Project,
    SeqScan, Sort, Update,
};
use crate::parser::ast::Statement;
use crate::plan::physical::{PhysicalPlan, WritePlan};
use crate::plan::planner;
use crate::types::{Schema, Value};

/// Result of a SELECT statement.
#[derive(Debug, Clone)]
pub struct ResultSet {
    pub schema: Schema,
    pub rows: Vec<Vec<Value>>,
}

/// Result of a non-SELECT statement.
#[derive(Debug, Clone)]
pub struct WriteResult {
    /// Affected row count (or 0 for DDL/transaction statements).
    pub affected: i64,
    /// Human-readable summary printed to stderr.
    pub summary: String,
}

#[derive(Debug, Clone)]
pub enum StatementOutcome {
    Select(ResultSet),
    Write(WriteResult),
}

pub fn execute_statement(stmt: &Statement, engine: &mut Engine) -> Result<StatementOutcome> {
    match stmt {
        Statement::Begin => {
            engine.begin()?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: "ok BEGIN".into(),
            }))
        }
        Statement::Commit => {
            engine.commit()?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: "ok COMMIT".into(),
            }))
        }
        Statement::Rollback => {
            engine.rollback()?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: "ok ROLLBACK".into(),
            }))
        }
        Statement::CreateTable(ct) => {
            // Validate CHECK constraints don't reference unknown columns.
            // (We bind them at runtime per-row when checking, so just keep
            // them attached.)
            engine.ddl_create_table(&ct.name, ct.columns.clone())?;
            // Apply post-creation CHECK validation? CHECK at table creation
            // time has no rows; nothing to validate.
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: format!("ok CREATE TABLE {}", ct.name),
            }))
        }
        Statement::DropTable(dt) => {
            if dt.if_exists && engine.catalog().get_table(&dt.name).is_err() {
                return Ok(StatementOutcome::Write(WriteResult {
                    affected: 0,
                    summary: format!("ok DROP TABLE {} (no-op)", dt.name),
                }));
            }
            engine.ddl_drop_table(&dt.name)?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: format!("ok DROP TABLE {}", dt.name),
            }))
        }
        Statement::CreateIndex(ci) => {
            engine.ddl_create_index(&ci.name, &ci.table, &ci.column)?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: format!("ok CREATE INDEX {}", ci.name),
            }))
        }
        Statement::DropIndex(di) => {
            if di.if_exists && engine.catalog().get_index(&di.name).is_err() {
                return Ok(StatementOutcome::Write(WriteResult {
                    affected: 0,
                    summary: format!("ok DROP INDEX {} (no-op)", di.name),
                }));
            }
            engine.ddl_drop_index(&di.name)?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: 0,
                summary: format!("ok DROP INDEX {}", di.name),
            }))
        }
        Statement::Insert(ins) => {
            let plan = planner::plan_insert(ins);
            let WritePlan::Insert {
                table,
                rows,
                columns,
            } = plan
            else {
                unreachable!()
            };
            // Verify table exists, applying table-level CHECKs after insert.
            // (Simplified: CHECKs are pre-validated against each candidate row.)
            let mut op = Insert::new(table.clone(), columns, rows);
            let count = run_dml(&mut op, engine)?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: count,
                summary: format!(
                    "ok {} row{} inserted",
                    count,
                    if count == 1 { "" } else { "s" }
                ),
            }))
        }
        Statement::Update(upd) => {
            let plan = planner::plan_update(upd, engine.catalog())?;
            let WritePlan::Update {
                table,
                scan,
                assignments,
            } = plan
            else {
                unreachable!()
            };
            let child = build_plan(&scan, engine)?;
            let mut op = Update::new(table, assignments, child);
            let count = run_dml(&mut op, engine)?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: count,
                summary: format!(
                    "ok {} row{} affected",
                    count,
                    if count == 1 { "" } else { "s" }
                ),
            }))
        }
        Statement::Delete(del) => {
            let plan = planner::plan_delete(del, engine.catalog())?;
            let WritePlan::Delete { table, scan } = plan else {
                unreachable!()
            };
            let child = build_plan(&scan, engine)?;
            let mut op = Delete::new(table, child);
            let count = run_dml(&mut op, engine)?;
            Ok(StatementOutcome::Write(WriteResult {
                affected: count,
                summary: format!(
                    "ok {} row{} affected",
                    count,
                    if count == 1 { "" } else { "s" }
                ),
            }))
        }
        Statement::Select(sel) => {
            // Special case: SELECT without FROM evaluates a single row of expressions.
            if sel.from.is_none() {
                let mut values = Vec::new();
                let mut names = Vec::new();
                for proj in &sel.projections {
                    match proj {
                        crate::parser::ast::Projection::Expr { expr, alias } => {
                            let bound = bind(expr, &Schema::empty())?;
                            let v = eval(&bound, &[])?;
                            values.push(v);
                            names.push(alias.clone().unwrap_or_else(|| "expr".into()));
                        }
                        _ => {
                            return Err(MiniDbError::ty("* is not allowed in SELECT without FROM"));
                        }
                    }
                }
                let schema = Schema::new(
                    names
                        .into_iter()
                        .map(|n| crate::types::SchemaColumn {
                            name: n,
                            qualifier: None,
                            data_type: None,
                        })
                        .collect(),
                );
                return Ok(StatementOutcome::Select(ResultSet {
                    schema,
                    rows: vec![values],
                }));
            }

            let plan = planner::plan_select(sel, engine.catalog())?;
            let mut op = build_plan(&plan, engine)?;
            let mut ctx = ExecCtx { engine };
            op.open(&mut ctx)?;
            let schema = op.schema().clone();
            let mut rows = Vec::new();
            while let Some(r) = op.next(&mut ctx)? {
                rows.push(r.values);
            }
            op.close(&mut ctx)?;
            Ok(StatementOutcome::Select(ResultSet { schema, rows }))
        }
    }
}

fn run_dml<O: PhysicalOperator + ?Sized>(op: &mut O, engine: &mut Engine) -> Result<i64> {
    let mut ctx = ExecCtx { engine };
    op.open(&mut ctx)?;
    let mut count: i64 = 0;
    while let Some(r) = op.next(&mut ctx)? {
        if let Some(Value::Int(n)) = r.values.first() {
            count = *n;
        }
    }
    op.close(&mut ctx)?;
    Ok(count)
}

pub fn build_plan(plan: &PhysicalPlan, engine: &Engine) -> Result<Box<dyn PhysicalOperator>> {
    Ok(match plan {
        PhysicalPlan::SeqScan { table, alias } => {
            Box::new(SeqScan::new(table.clone(), alias.clone(), engine)?)
        }
        PhysicalPlan::IndexScan {
            table,
            alias,
            column,
            bound,
            ..
        } => Box::new(IndexScan::new(
            table.clone(),
            alias.clone(),
            column.clone(),
            bound.clone(),
            engine,
        )?),
        PhysicalPlan::Filter { input, predicate } => {
            let child = build_plan(input, engine)?;
            Box::new(Filter::new(predicate.clone(), child))
        }
        PhysicalPlan::Project { input, projections } => {
            let child = build_plan(input, engine)?;
            Box::new(Project::new(projections.clone(), child))
        }
        PhysicalPlan::NestedLoopJoin { left, right, on } => {
            let l = build_plan(left, engine)?;
            let r = build_plan(right, engine)?;
            Box::new(NestedLoopJoin::new(l, r, on.clone()))
        }
        PhysicalPlan::Sort { input, items } => {
            let child = build_plan(input, engine)?;
            Box::new(Sort::new(items.clone(), child))
        }
        PhysicalPlan::Limit {
            input,
            limit,
            offset,
        } => {
            let child = build_plan(input, engine)?;
            Box::new(Limit::new(*limit, *offset, child))
        }
    })
}
