//! Planner: turns AST `Select`/DML statements into a `PhysicalPlan`.
//!
//! Index selection: when the WHERE clause has the form `col OP literal`
//! (or its commuted variant) and `col` is the only filter on a single base
//! table that has an index, an `IndexScan` is emitted. All other forms fall
//! through to `SeqScan + Filter`.

use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::{
    BinaryOp, Delete as DeleteAst, Expr, FromClause, Insert as InsertAst, OrderItem, Projection,
    Select, Update as UpdateAst,
};
use crate::plan::physical::{IndexBound, PhysicalPlan, WritePlan};
use crate::types::Value;

pub fn plan_select(sel: &Select, catalog: &Catalog) -> Result<PhysicalPlan> {
    // Build the (joined or single-table) source.
    let mut plan = match &sel.from {
        Some(from) => from_clause_to_plan(from, catalog)?,
        None => {
            // SELECT without FROM — synthesise a single-row source. We model
            // this as a projection on top of a degenerate scan; the executor
            // handles the empty-source case via a literal-only scan.
            PhysicalPlan::SeqScan {
                table: "__no_table__".into(),
                alias: None,
            }
        }
    };

    // Try to push a filter down into an IndexScan when applicable.
    if let Some(pred) = &sel.r#where {
        plan = maybe_index_scan(plan, pred, catalog).unwrap_or_else(|| PhysicalPlan::Filter {
            input: Box::new(
                maybe_index_scan_inner(&sel.from, pred, catalog).unwrap_or_else(|| {
                    match &sel.from {
                        Some(f) => {
                            from_clause_to_plan(f, catalog).unwrap_or(PhysicalPlan::SeqScan {
                                table: "__no_table__".into(),
                                alias: None,
                            })
                        }
                        None => PhysicalPlan::SeqScan {
                            table: "__no_table__".into(),
                            alias: None,
                        },
                    }
                }),
            ),
            predicate: pred.clone(),
        });
    }

    if !sel.order_by.is_empty() {
        plan = PhysicalPlan::Sort {
            input: Box::new(plan),
            items: sel.order_by.clone(),
        };
    }

    if sel.limit.is_some() || sel.offset.is_some() {
        plan = PhysicalPlan::Limit {
            input: Box::new(plan),
            limit: sel.limit.unwrap_or(i64::MAX),
            offset: sel.offset.unwrap_or(0),
        };
    }

    plan = PhysicalPlan::Project {
        input: Box::new(plan),
        projections: sel.projections.clone(),
    };

    Ok(plan)
}

/// Convenience entry point that returns just the source plan + filter, used
/// by Update/Delete planning.
pub fn plan_dml_source(
    table: &str,
    where_clause: &Option<Expr>,
    catalog: &Catalog,
) -> Result<PhysicalPlan> {
    let base = PhysicalPlan::SeqScan {
        table: table.to_string(),
        alias: None,
    };
    let plan = match where_clause {
        Some(p) => {
            // If we can use an index, do so.
            if let Some(idx) = single_table_index_scan(table, p, catalog) {
                idx
            } else {
                PhysicalPlan::Filter {
                    input: Box::new(base),
                    predicate: p.clone(),
                }
            }
        }
        None => base,
    };
    Ok(plan)
}

pub fn plan_insert(ins: &InsertAst) -> WritePlan {
    WritePlan::Insert {
        table: ins.table.clone(),
        rows: ins.rows.clone(),
        columns: ins.columns.clone(),
    }
}

pub fn plan_update(upd: &UpdateAst, catalog: &Catalog) -> Result<WritePlan> {
    let scan = plan_dml_source(&upd.table, &upd.r#where, catalog)?;
    Ok(WritePlan::Update {
        table: upd.table.clone(),
        scan,
        assignments: upd.assignments.clone(),
    })
}

pub fn plan_delete(del: &DeleteAst, catalog: &Catalog) -> Result<WritePlan> {
    let scan = plan_dml_source(&del.table, &del.r#where, catalog)?;
    Ok(WritePlan::Delete {
        table: del.table.clone(),
        scan,
    })
}

fn from_clause_to_plan(from: &FromClause, _catalog: &Catalog) -> Result<PhysicalPlan> {
    match from {
        FromClause::Table { name, alias } => Ok(PhysicalPlan::SeqScan {
            table: name.clone(),
            alias: alias.clone(),
        }),
        FromClause::Join { left, right, on } => {
            let l = from_clause_to_plan(left, _catalog)?;
            let r = from_clause_to_plan(right, _catalog)?;
            Ok(PhysicalPlan::NestedLoopJoin {
                left: Box::new(l),
                right: Box::new(r),
                on: on.clone(),
            })
        }
    }
}

fn maybe_index_scan(plan: PhysicalPlan, pred: &Expr, catalog: &Catalog) -> Option<PhysicalPlan> {
    // Only attempt an index scan when the source is a single SeqScan.
    if let PhysicalPlan::SeqScan { table, alias } = &plan {
        if let Some(scan) = single_table_index_scan(table, pred, catalog) {
            return Some(remap_alias(scan, alias.clone()));
        }
    }
    None
}

fn remap_alias(plan: PhysicalPlan, alias: Option<String>) -> PhysicalPlan {
    match plan {
        PhysicalPlan::IndexScan {
            table,
            column,
            index_name,
            bound,
            ..
        } => PhysicalPlan::IndexScan {
            table,
            alias,
            column,
            index_name,
            bound,
        },
        other => other,
    }
}

fn maybe_index_scan_inner(
    from: &Option<FromClause>,
    pred: &Expr,
    catalog: &Catalog,
) -> Option<PhysicalPlan> {
    match from {
        Some(FromClause::Table { name, alias }) => {
            if let Some(scan) = single_table_index_scan(name, pred, catalog) {
                return Some(remap_alias(scan, alias.clone()));
            }
            None
        }
        _ => None,
    }
}

fn single_table_index_scan(table: &str, pred: &Expr, catalog: &Catalog) -> Option<PhysicalPlan> {
    let table_def = catalog.get_table(table).ok()?;
    let (col_qual, col, op, lit) = extract_simple_predicate(pred)?;
    if let Some(q) = col_qual {
        if !q.eq_ignore_ascii_case(table) {
            return None;
        }
    }
    if !table_def
        .columns
        .iter()
        .any(|c| c.name.eq_ignore_ascii_case(&col))
    {
        return None;
    }
    let idx = catalog.find_index_for(table, &col)?;
    let bound = match op {
        BinaryOp::Eq => IndexBound::Equal(lit),
        BinaryOp::Lt => IndexBound::Range {
            lo: None,
            hi: Some(lit),
            lo_inclusive: false,
            hi_inclusive: false,
        },
        BinaryOp::LtEq => IndexBound::Range {
            lo: None,
            hi: Some(lit),
            lo_inclusive: false,
            hi_inclusive: true,
        },
        BinaryOp::Gt => IndexBound::Range {
            lo: Some(lit),
            hi: None,
            lo_inclusive: false,
            hi_inclusive: false,
        },
        BinaryOp::GtEq => IndexBound::Range {
            lo: Some(lit),
            hi: None,
            lo_inclusive: true,
            hi_inclusive: false,
        },
        _ => return None,
    };
    Some(PhysicalPlan::IndexScan {
        table: table.to_string(),
        alias: None,
        column: col,
        index_name: idx.name.clone(),
        bound,
    })
}

/// Recognise a predicate of the form `col OP literal` (or commuted) and
/// return the column name + comparison operator + literal value.
fn extract_simple_predicate(pred: &Expr) -> Option<(Option<String>, String, BinaryOp, Value)> {
    let (op, l, r) = match pred {
        Expr::BinaryOp { op, left, right } => (*op, left, right),
        _ => return None,
    };
    if !matches!(
        op,
        BinaryOp::Eq | BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
    ) {
        return None;
    }
    if let (Expr::Column { qualifier, name }, Expr::Literal(v)) = (l.as_ref(), r.as_ref()) {
        return Some((qualifier.clone(), name.clone(), op, v.clone()));
    }
    if let (Expr::Literal(v), Expr::Column { qualifier, name }) = (l.as_ref(), r.as_ref()) {
        let flipped = match op {
            BinaryOp::Eq => BinaryOp::Eq,
            BinaryOp::Lt => BinaryOp::Gt,
            BinaryOp::LtEq => BinaryOp::GtEq,
            BinaryOp::Gt => BinaryOp::Lt,
            BinaryOp::GtEq => BinaryOp::LtEq,
            _ => return None,
        };
        return Some((qualifier.clone(), name.clone(), flipped, v.clone()));
    }
    None
}

/// Order-by passthrough helper.
pub fn order_items(items: &[OrderItem]) -> Vec<OrderItem> {
    items.to_vec()
}

/// Projection passthrough helper.
pub fn projections(p: &[Projection]) -> Vec<Projection> {
    p.to_vec()
}
