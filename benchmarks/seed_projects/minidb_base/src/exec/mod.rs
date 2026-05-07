//! Volcano-style physical operators.
//!
//! All operators implement [`PhysicalOperator`], which exposes the
//! classic `open` / `next` / `close` surface together with the output
//! [`Schema`]. A query is executed by calling `open` once, draining
//! `next` until it returns `Ok(None)`, then calling `close`.

pub mod delete;
pub mod filter;
pub mod index_scan;
pub mod insert;
pub mod limit;
pub mod nested_loop_join;
pub mod project;
pub mod seq_scan;
pub mod sort;
pub mod update;

use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::*;
use crate::plan::{IndexPredicate, PhysicalPlan, PlanSchema};
use crate::types::{Row, Schema, Value};

/// Execution context threaded through every operator. Holds the catalog
/// reference and any runtime knobs.
pub struct ExecContext<'a> {
    pub catalog: &'a mut Catalog,
}

impl<'a> ExecContext<'a> {
    pub fn new(catalog: &'a mut Catalog) -> Self {
        Self { catalog }
    }
}

/// The Volcano operator trait.
pub trait PhysicalOperator {
    /// Prepare for iteration. Must be called exactly once before `next`.
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()>;
    /// Pull the next output row, or `Ok(None)` on end-of-stream.
    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Row>>;
    /// Release resources. Called even on error paths.
    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()>;
    /// The output schema of this operator.
    fn schema(&self) -> &Schema;
}

/// Materialise a physical plan into an operator tree.
pub fn build_operator(
    plan: PhysicalPlan,
    schema: PlanSchema,
    ctx: &ExecContext<'_>,
) -> Result<Box<dyn PhysicalOperator>> {
    match plan {
        PhysicalPlan::SeqScan { table, alias } => {
            let tbl = ctx.catalog.table(&table)?;
            let out = seq_scan::SeqScan::new(table.clone(), alias, tbl.schema.clone());
            Ok(Box::new(out))
        }
        PhysicalPlan::IndexScan {
            table,
            alias,
            index,
            predicate,
        } => {
            let tbl = ctx.catalog.table(&table)?;
            let op = index_scan::IndexScan::new(
                table.clone(),
                alias,
                index,
                predicate,
                tbl.schema.clone(),
            );
            Ok(Box::new(op))
        }
        PhysicalPlan::Filter { predicate, input } => {
            let child_schema = child_plan_schema(&input, ctx)?;
            let child = build_operator(*input, child_schema.clone(), ctx)?;
            Ok(Box::new(filter::Filter::new(
                predicate,
                child,
                child_schema,
                schema,
            )))
        }
        PhysicalPlan::NestedLoopJoin { left, right, on } => {
            let ls = child_plan_schema(&left, ctx)?;
            let rs = child_plan_schema(&right, ctx)?;
            let combined = ls.concat(&rs);
            let lplan = build_operator(*left, ls, ctx)?;
            let rplan = build_operator(*right, rs, ctx)?;
            Ok(Box::new(nested_loop_join::NestedLoopJoin::new(
                lplan, rplan, on, schema, combined,
            )))
        }
        PhysicalPlan::Project { projections, input } => {
            let child_schema = child_plan_schema(&input, ctx)?;
            let child = build_operator(*input, child_schema.clone(), ctx)?;
            Ok(Box::new(project::Project::new(
                projections,
                child,
                child_schema,
                schema,
            )))
        }
        PhysicalPlan::Sort { order_by, input } => {
            let child_schema = child_plan_schema(&input, ctx)?;
            let child = build_operator(*input, child_schema.clone(), ctx)?;
            Ok(Box::new(sort::Sort::new(order_by, child, child_schema)))
        }
        PhysicalPlan::Limit {
            limit,
            offset,
            input,
        } => {
            let child = build_operator(*input, schema.clone(), ctx)?;
            Ok(Box::new(limit::Limit::new(limit, offset, child, schema.schema)))
        }
    }
}

/// Produce the full [`PlanSchema`] (including qualified names) of a
/// subplan. This mirrors the planner's lowering logic so operators can
/// resolve column references against the correct context.
pub fn child_plan_schema(plan: &PhysicalPlan, ctx: &ExecContext<'_>) -> Result<PlanSchema> {
    match plan {
        PhysicalPlan::SeqScan { table, alias } | PhysicalPlan::IndexScan { table, alias, .. } => {
            let tbl = ctx.catalog.table(table)?;
            let alias_name = alias.clone().unwrap_or_else(|| table.clone());
            let qualified = tbl
                .schema
                .columns
                .iter()
                .map(|c| (Some(alias_name.clone()), c.name.clone()))
                .collect();
            Ok(PlanSchema::new(tbl.schema.clone(), qualified))
        }
        PhysicalPlan::Filter { input, .. } => child_plan_schema(input, ctx),
        PhysicalPlan::NestedLoopJoin { left, right, .. } => {
            let l = child_plan_schema(left, ctx)?;
            let r = child_plan_schema(right, ctx)?;
            Ok(l.concat(&r))
        }
        PhysicalPlan::Project { projections, input } => {
            let input_schema = child_plan_schema(input, ctx)?;
            project_schema(projections, &input_schema)
        }
        PhysicalPlan::Sort { input, .. } | PhysicalPlan::Limit { input, .. } => {
            child_plan_schema(input, ctx)
        }
    }
}

fn project_schema(projections: &[Projection], input: &PlanSchema) -> Result<PlanSchema> {
    let mut columns = Vec::new();
    let mut qualified = Vec::new();
    for p in projections {
        match p {
            Projection::AllColumns => {
                columns.extend(input.schema.columns.iter().cloned());
                qualified.extend(input.qualified.iter().cloned());
            }
            Projection::Expr { expr, alias } => {
                let name = alias.clone().unwrap_or_else(|| expr_label(expr));
                columns.push(crate::types::Column::new(
                    name.clone(),
                    crate::types::ColumnType::Text,
                ));
                qualified.push((None, name));
            }
        }
    }
    Ok(PlanSchema::new(Schema::new(columns), qualified))
}

fn expr_label(expr: &Expression) -> String {
    match expr {
        Expression::ColumnRef { table, column } => match table {
            Some(t) => format!("{t}.{column}"),
            None => column.clone(),
        },
        Expression::Literal(v) => v.to_string(),
        _ => "?column?".to_string(),
    }
}

/// Evaluate a scalar expression against `row`, which is laid out according
/// to `schema`.
pub fn eval_expr(expr: &Expression, row: &Row, schema: &PlanSchema) -> Result<Value> {
    match expr {
        Expression::Literal(v) => Ok(v.clone()),
        Expression::ColumnRef { table, column } => {
            let idx = schema.resolve(table.as_deref(), column)?;
            Ok(row
                .get(idx)
                .cloned()
                .unwrap_or(Value::Null))
        }
        Expression::BinaryOp { op, left, right } => {
            let lv = eval_expr(left, row, schema)?;
            let rv = eval_expr(right, row, schema)?;
            eval_binary(*op, &lv, &rv)
        }
        Expression::UnaryOp { op, expr } => {
            let v = eval_expr(expr, row, schema)?;
            eval_unary(*op, &v)
        }
        Expression::IsNull { expr, negated } => {
            let v = eval_expr(expr, row, schema)?;
            let is_null = matches!(v, Value::Null);
            Ok(Value::Bool(if *negated { !is_null } else { is_null }))
        }
    }
}

fn eval_binary(op: BinaryOperator, lv: &Value, rv: &Value) -> Result<Value> {
    use crate::types::{tri_and, tri_or};
    match op {
        BinaryOperator::And => {
            let lb = lv.to_bool();
            let rb = rv.to_bool();
            Ok(match tri_and(lb, rb) {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            })
        }
        BinaryOperator::Or => {
            let lb = lv.to_bool();
            let rb = rv.to_bool();
            Ok(match tri_or(lb, rb) {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            })
        }
        BinaryOperator::Eq
        | BinaryOperator::Neq
        | BinaryOperator::Lt
        | BinaryOperator::Le
        | BinaryOperator::Gt
        | BinaryOperator::Ge => eval_comparison(op, lv, rv),
        BinaryOperator::Add
        | BinaryOperator::Sub
        | BinaryOperator::Mul
        | BinaryOperator::Div
        | BinaryOperator::Mod => eval_arithmetic(op, lv, rv),
    }
}

fn eval_comparison(op: BinaryOperator, lv: &Value, rv: &Value) -> Result<Value> {
    use std::cmp::Ordering;
    let ord = crate::types::compare_values(lv, rv);
    let Some(ord) = ord else {
        return Ok(Value::Null);
    };
    let b = match op {
        BinaryOperator::Eq => ord == Ordering::Equal,
        BinaryOperator::Neq => ord != Ordering::Equal,
        BinaryOperator::Lt => ord == Ordering::Less,
        BinaryOperator::Le => matches!(ord, Ordering::Less | Ordering::Equal),
        BinaryOperator::Gt => ord == Ordering::Greater,
        BinaryOperator::Ge => matches!(ord, Ordering::Greater | Ordering::Equal),
        _ => unreachable!(),
    };
    Ok(Value::Bool(b))
}

fn eval_arithmetic(op: BinaryOperator, lv: &Value, rv: &Value) -> Result<Value> {
    if matches!(lv, Value::Null) || matches!(rv, Value::Null) {
        return Ok(Value::Null);
    }
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => {
            let v = match op {
                BinaryOperator::Add => a.checked_add(*b),
                BinaryOperator::Sub => a.checked_sub(*b),
                BinaryOperator::Mul => a.checked_mul(*b),
                BinaryOperator::Div => {
                    if *b == 0 {
                        return Err(crate::error::MiniDbError::Runtime(
                            "division by zero".into(),
                        ));
                    }
                    Some(a / b)
                }
                BinaryOperator::Mod => {
                    if *b == 0 {
                        return Err(crate::error::MiniDbError::Runtime("mod by zero".into()));
                    }
                    Some(a % b)
                }
                _ => unreachable!(),
            };
            v.map(Value::Int).ok_or_else(|| {
                crate::error::MiniDbError::Runtime("integer overflow".into())
            })
        }
        _ => {
            // Promote both sides to f64.
            let a = as_f64(lv)?;
            let b = as_f64(rv)?;
            let v = match op {
                BinaryOperator::Add => a + b,
                BinaryOperator::Sub => a - b,
                BinaryOperator::Mul => a * b,
                BinaryOperator::Div => {
                    if b == 0.0 {
                        return Err(crate::error::MiniDbError::Runtime(
                            "division by zero".into(),
                        ));
                    }
                    a / b
                }
                BinaryOperator::Mod => {
                    if b == 0.0 {
                        return Err(crate::error::MiniDbError::Runtime("mod by zero".into()));
                    }
                    a % b
                }
                _ => unreachable!(),
            };
            Ok(Value::Real(v))
        }
    }
}

fn as_f64(v: &Value) -> Result<f64> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Real(r) => Ok(*r),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(crate::error::MiniDbError::Type(format!(
            "cannot treat {} as numeric",
            v.type_name()
        ))),
    }
}

fn eval_unary(op: UnaryOperator, v: &Value) -> Result<Value> {
    match op {
        UnaryOperator::Neg => match v {
            Value::Null => Ok(Value::Null),
            Value::Int(i) => i
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| crate::error::MiniDbError::Runtime("integer overflow".into())),
            Value::Real(r) => Ok(Value::Real(-*r)),
            other => Err(crate::error::MiniDbError::Type(format!(
                "cannot negate {}",
                other.type_name()
            ))),
        },
        UnaryOperator::Not => {
            let b = v.to_bool();
            Ok(match crate::types::tri_not(b) {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            })
        }
    }
}

/// Helper: evaluate a predicate and return `true` only if it cleanly
/// evaluates to `true` (NULL and false are filtered out).
pub fn is_true(expr: &Expression, row: &Row, schema: &PlanSchema) -> Result<bool> {
    let v = eval_expr(expr, row, schema)?;
    Ok(matches!(v, Value::Bool(true)) || matches!(v.to_bool(), Some(true)))
}

/// Resolve an [`IndexPredicate`] into concrete `Value`s using a constant-
/// folding pass. All expressions inside an `IndexPredicate` must be
/// constant at this stage.
pub fn resolve_index_predicate(pred: &IndexPredicate) -> Result<ResolvedIndexPredicate> {
    let constant = |e: &Expression| -> Result<Value> {
        let empty_schema = PlanSchema::new(Schema::default(), vec![]);
        let empty_row: Row = Vec::new();
        eval_expr(e, &empty_row, &empty_schema)
    };
    Ok(match pred {
        IndexPredicate::Eq(e) => ResolvedIndexPredicate::Eq(constant(e)?),
        IndexPredicate::Range { lo, hi } => ResolvedIndexPredicate::Range {
            lo: match lo {
                Some((e, inc)) => Some((constant(e)?, *inc)),
                None => None,
            },
            hi: match hi {
                Some((e, inc)) => Some((constant(e)?, *inc)),
                None => None,
            },
        },
    })
}

/// Concrete-value counterpart of [`IndexPredicate`].
#[derive(Debug, Clone)]
pub enum ResolvedIndexPredicate {
    Eq(Value),
    Range {
        lo: Option<(Value, bool)>,
        hi: Option<(Value, bool)>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_int_add() {
        let v = eval_binary(BinaryOperator::Add, &Value::Int(2), &Value::Int(3)).unwrap();
        assert_eq!(v, Value::Int(5));
    }

    #[test]
    fn division_by_zero() {
        let e = eval_binary(BinaryOperator::Div, &Value::Int(1), &Value::Int(0));
        assert!(e.is_err());
    }
}
