//! Logical plan: a tree-shaped declarative description of a SELECT.

use crate::error::Result;
use crate::parser::ast::{BinaryOp, Expr, OrderBy, Projection, Select};

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        projections: Vec<Projection>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<OrderBy>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: Option<i64>,
        offset: Option<i64>,
    },
}

pub fn plan_select(s: &Select) -> Result<LogicalPlan> {
    let mut plan = LogicalPlan::Scan {
        table: s.from.name.clone(),
        alias: s.from.alias.clone(),
    };
    if let Some(j) = &s.join {
        let right = LogicalPlan::Scan {
            table: j.right.name.clone(),
            alias: j.right.alias.clone(),
        };
        plan = LogicalPlan::Join {
            left: Box::new(plan),
            right: Box::new(right),
            on: j.on.clone(),
        };
    }
    if let Some(w) = &s.where_clause {
        plan = LogicalPlan::Filter {
            input: Box::new(plan),
            predicate: w.clone(),
        };
    }
    plan = LogicalPlan::Project {
        input: Box::new(plan),
        projections: s.projections.clone(),
    };
    if !s.order_by.is_empty() {
        plan = LogicalPlan::Sort {
            input: Box::new(plan),
            keys: s.order_by.clone(),
        };
    }
    if s.limit.is_some() || s.offset.is_some() {
        let limit = s.limit.as_ref().map(|e| eval_const_int(e)).transpose()?;
        let offset = s.offset.as_ref().map(|e| eval_const_int(e)).transpose()?;
        plan = LogicalPlan::Limit {
            input: Box::new(plan),
            limit,
            offset,
        };
    }
    Ok(plan)
}

fn eval_const_int(e: &Expr) -> Result<i64> {
    use crate::types::Value;
    match e {
        Expr::Literal(Value::Int(i)) => Ok(*i),
        Expr::UnaryOp {
            op: crate::parser::ast::UnaryOp::Neg,
            expr,
        } => Ok(-eval_const_int(expr)?),
        _ => Err(crate::error::MiniDbError::Type(
            "LIMIT/OFFSET must be a constant integer".into(),
        )),
    }
}

/// Try to extract `(column, op, literal)` triples ANDed together at the top of
/// a predicate. Used for index selection.
pub fn extract_simple_filters(e: &Expr) -> Vec<(String, BinaryOp, crate::types::Value)> {
    use crate::types::Value;
    let mut out = Vec::new();
    walk(e, &mut out);
    fn walk(e: &Expr, out: &mut Vec<(String, BinaryOp, Value)>) {
        match e {
            Expr::BinaryOp {
                op: BinaryOp::And,
                left,
                right,
            } => {
                walk(left, out);
                walk(right, out);
            }
            Expr::BinaryOp { op, left, right } => {
                let interesting = matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::Neq
                        | BinaryOp::Lt
                        | BinaryOp::Lte
                        | BinaryOp::Gt
                        | BinaryOp::Gte
                );
                if !interesting {
                    return;
                }
                if let (Expr::Column { name, .. }, Expr::Literal(v)) =
                    (left.as_ref(), right.as_ref())
                {
                    out.push((name.clone(), *op, v.clone()));
                } else if let (Expr::Literal(v), Expr::Column { name, .. }) =
                    (left.as_ref(), right.as_ref())
                {
                    let flipped = match op {
                        BinaryOp::Lt => BinaryOp::Gt,
                        BinaryOp::Lte => BinaryOp::Gte,
                        BinaryOp::Gt => BinaryOp::Lt,
                        BinaryOp::Gte => BinaryOp::Lte,
                        other => *other,
                    };
                    out.push((name.clone(), flipped, v.clone()));
                }
            }
            _ => {}
        }
    }
    out
}
