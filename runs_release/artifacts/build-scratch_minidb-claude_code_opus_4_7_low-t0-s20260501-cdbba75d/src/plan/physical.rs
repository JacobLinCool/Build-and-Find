use super::logical::LogicalPlan;
use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::{BinOp, Expr, Projection};

/// Physical plan: an operator tree consumed by the executor. Each node names
/// the operator implementation the executor should instantiate.
#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index_name: String,
        column: String,
        bound: IndexBound,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        projections: Vec<Projection>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<PhysicalPlan>,
        keys: Vec<(Expr, bool)>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: Option<i64>,
        offset: Option<i64>,
    },
}

#[derive(Debug, Clone)]
pub enum IndexBound {
    Eq(Expr),
    Range {
        lo: Option<(Expr, bool)>, // (value, inclusive)
        hi: Option<(Expr, bool)>,
    },
}

pub fn build_physical(plan: &LogicalPlan, catalog: &Catalog) -> Result<PhysicalPlan> {
    Ok(match plan {
        LogicalPlan::Scan { table, alias } => PhysicalPlan::SeqScan {
            table: table.clone(),
            alias: alias.clone(),
        },
        LogicalPlan::Filter { input, predicate } => {
            // Try to push a single equality/range predicate on an indexed column down into IndexScan.
            if let LogicalPlan::Scan { table, alias } = input.as_ref() {
                if let Some((col, bound)) =
                    extract_indexable_predicate(predicate, table, alias.as_deref())
                {
                    if let Some(idx) = catalog.find_index_for(table, &col) {
                        return Ok(PhysicalPlan::IndexScan {
                            table: table.clone(),
                            alias: alias.clone(),
                            index_name: idx.name.clone(),
                            column: col,
                            bound,
                        });
                    }
                }
            }
            PhysicalPlan::Filter {
                input: Box::new(build_physical(input, catalog)?),
                predicate: predicate.clone(),
            }
        }
        LogicalPlan::Project { input, projections } => PhysicalPlan::Project {
            input: Box::new(build_physical(input, catalog)?),
            projections: projections.clone(),
        },
        LogicalPlan::NestedLoopJoin { left, right, on } => PhysicalPlan::NestedLoopJoin {
            left: Box::new(build_physical(left, catalog)?),
            right: Box::new(build_physical(right, catalog)?),
            on: on.clone(),
        },
        LogicalPlan::Sort { input, keys } => PhysicalPlan::Sort {
            input: Box::new(build_physical(input, catalog)?),
            keys: keys.clone(),
        },
        LogicalPlan::Limit {
            input,
            limit,
            offset,
        } => PhysicalPlan::Limit {
            input: Box::new(build_physical(input, catalog)?),
            limit: *limit,
            offset: *offset,
        },
    })
}

/// If the predicate is a top-level single comparison `col OP literal` (or `literal OP col`),
/// where `col` references the given table, return (column_name, bound).
fn extract_indexable_predicate(
    expr: &Expr,
    table: &str,
    alias: Option<&str>,
) -> Option<(String, IndexBound)> {
    let (op, l, r) = match expr {
        Expr::BinaryOp { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    let table_match = |t: &Option<String>| match t {
        None => true,
        Some(s) => {
            s.eq_ignore_ascii_case(table) || alias.map_or(false, |a| s.eq_ignore_ascii_case(a))
        }
    };
    // Try col OP literal-ish first
    let (col, val_expr, op_eff) = if let (Expr::Column { table: tt, name }, rhs) = (l, r) {
        if !table_match(tt) || !is_literal_only(rhs) {
            return None;
        }
        (name.clone(), rhs.clone(), op)
    } else if let (lhs, Expr::Column { table: tt, name }) = (l, r) {
        if !table_match(tt) || !is_literal_only(lhs) {
            return None;
        }
        // flip operator
        let flipped = match op {
            BinOp::Eq => BinOp::Eq,
            BinOp::NotEq => BinOp::NotEq,
            BinOp::Lt => BinOp::Gt,
            BinOp::LtEq => BinOp::GtEq,
            BinOp::Gt => BinOp::Lt,
            BinOp::GtEq => BinOp::LtEq,
            _ => return None,
        };
        (name.clone(), lhs.clone(), flipped)
    } else {
        return None;
    };

    let bound = match op_eff {
        BinOp::Eq => IndexBound::Eq(val_expr),
        BinOp::Lt => IndexBound::Range {
            lo: None,
            hi: Some((val_expr, false)),
        },
        BinOp::LtEq => IndexBound::Range {
            lo: None,
            hi: Some((val_expr, true)),
        },
        BinOp::Gt => IndexBound::Range {
            lo: Some((val_expr, false)),
            hi: None,
        },
        BinOp::GtEq => IndexBound::Range {
            lo: Some((val_expr, true)),
            hi: None,
        },
        _ => return None,
    };
    Some((col, bound))
}

fn is_literal_only(e: &Expr) -> bool {
    match e {
        Expr::Literal(_) => true,
        Expr::UnaryOp { expr, .. } => is_literal_only(expr),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, IndexMeta};
    use crate::parser::ast::Expr;
    use crate::storage::btree::BTreeIndex;

    #[test]
    fn picks_index_scan_when_eligible() {
        let mut cat = Catalog::new();
        cat.indexes.insert(
            "ix_t_a".into(),
            IndexMeta {
                name: "ix_t_a".into(),
                table: "t".into(),
                column: "a".into(),
                btree: BTreeIndex::new(),
            },
        );
        let scan = LogicalPlan::Scan {
            table: "t".into(),
            alias: None,
        };
        let pred = Expr::BinaryOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column {
                table: None,
                name: "a".into(),
            }),
            right: Box::new(Expr::Literal(crate::types::Value::Int(1))),
        };
        let logical = LogicalPlan::Filter {
            input: Box::new(scan),
            predicate: pred,
        };
        let phys = build_physical(&logical, &cat).unwrap();
        assert!(matches!(phys, PhysicalPlan::IndexScan { .. }));
    }
}
