//! Physical plan: lowers a logical plan to a tree of physical operators.
//!
//! At this stage we choose between SeqScan and IndexScan when a single
//! equality or range predicate references an indexed column.

use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::{BinOp, Expr, OrderKey, Projection};

use super::logical::LogicalPlan;

#[derive(Clone, Debug)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index: String,
        column: String,
        bounds: IndexBounds,
        residual: Option<Expr>,
    },
    Filter {
        input: Box<PhysicalPlan>,
        pred: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        exprs: Vec<Projection>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<PhysicalPlan>,
        keys: Vec<OrderKey>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: i64,
        offset: i64,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        pred: Option<Expr>,
    },
    Delete {
        table: String,
        pred: Option<Expr>,
    },
    Ddl(LogicalPlan),
}

#[derive(Clone, Debug, Default)]
pub struct IndexBounds {
    pub eq: Option<Expr>,
    pub lower: Option<(Expr, bool)>, // inclusive flag
    pub upper: Option<(Expr, bool)>,
}

impl IndexBounds {
    pub fn is_empty(&self) -> bool {
        self.eq.is_none() && self.lower.is_none() && self.upper.is_none()
    }
}

pub fn build_physical(plan: LogicalPlan, catalog: &Catalog) -> Result<PhysicalPlan> {
    Ok(match plan {
        LogicalPlan::Scan { table, alias } => PhysicalPlan::SeqScan { table, alias },
        LogicalPlan::Filter { input, pred } => match *input {
            LogicalPlan::Scan { table, alias } => {
                if let Some((index_name, column, bounds, residual)) =
                    try_match_index(&table, &alias, &pred, catalog)
                {
                    PhysicalPlan::IndexScan {
                        table,
                        alias,
                        index: index_name,
                        column,
                        bounds,
                        residual,
                    }
                } else {
                    PhysicalPlan::Filter {
                        input: Box::new(PhysicalPlan::SeqScan { table, alias }),
                        pred,
                    }
                }
            }
            other => PhysicalPlan::Filter {
                input: Box::new(build_physical(other, catalog)?),
                pred,
            },
        },
        LogicalPlan::Project { input, exprs } => PhysicalPlan::Project {
            input: Box::new(build_physical(*input, catalog)?),
            exprs,
        },
        LogicalPlan::Join { left, right, on } => PhysicalPlan::NestedLoopJoin {
            left: Box::new(build_physical(*left, catalog)?),
            right: Box::new(build_physical(*right, catalog)?),
            on,
        },
        LogicalPlan::Sort { input, keys } => PhysicalPlan::Sort {
            input: Box::new(build_physical(*input, catalog)?),
            keys,
        },
        LogicalPlan::Limit {
            input,
            limit,
            offset,
        } => PhysicalPlan::Limit {
            input: Box::new(build_physical(*input, catalog)?),
            limit,
            offset,
        },
        LogicalPlan::Insert {
            table,
            columns,
            rows,
        } => PhysicalPlan::Insert {
            table,
            columns,
            rows,
        },
        LogicalPlan::Update {
            table,
            assignments,
            pred,
        } => PhysicalPlan::Update {
            table,
            assignments,
            pred,
        },
        LogicalPlan::Delete { table, pred } => PhysicalPlan::Delete { table, pred },
        other @ (LogicalPlan::CreateTable(_)
        | LogicalPlan::DropTable(_)
        | LogicalPlan::CreateIndex(_)
        | LogicalPlan::DropIndex(_)
        | LogicalPlan::Begin
        | LogicalPlan::Commit
        | LogicalPlan::Rollback
        | LogicalPlan::Meta(_)) => PhysicalPlan::Ddl(other),
    })
}

/// Detect a top-level conjunction containing exactly one supported predicate
/// against an indexed column. Returns the index, column, bounds, and any
/// residual predicate that should still be applied above the scan.
fn try_match_index(
    table: &str,
    alias: &Option<String>,
    pred: &Expr,
    catalog: &Catalog,
) -> Option<(String, String, IndexBounds, Option<Expr>)> {
    let conjuncts = split_and(pred);
    let mut chosen: Option<(usize, String, String, IndexBounds)> = None;
    for (idx, conj) in conjuncts.iter().enumerate() {
        if let Some((column, bounds)) = match_indexable(conj, table, alias) {
            // Check that an index exists on this column.
            let idxs = catalog.indexes_for_column(table, &column);
            if let Some(i) = idxs.first() {
                chosen = Some((idx, i.name.clone(), column, bounds));
                break;
            }
        }
    }
    let (consumed_idx, index_name, column, bounds) = chosen?;
    let residual: Vec<&Expr> = conjuncts
        .iter()
        .enumerate()
        .filter_map(|(i, e)| if i == consumed_idx { None } else { Some(*e) })
        .collect();
    let residual_expr = if residual.is_empty() {
        None
    } else {
        let mut out = residual[0].clone();
        for r in &residual[1..] {
            out = Expr::And(Box::new(out), Box::new((*r).clone()));
        }
        Some(out)
    };
    Some((index_name, column, bounds, residual_expr))
}

fn split_and(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::And(a, b) => {
            let mut v = split_and(a);
            v.extend(split_and(b));
            v
        }
        other => vec![other],
    }
}

fn match_indexable(e: &Expr, table: &str, alias: &Option<String>) -> Option<(String, IndexBounds)> {
    match e {
        Expr::Binary { op, left, right } => {
            let (col_name, val_expr, op) = if let Some(col) = extract_column(left, table, alias) {
                (col, (**right).clone(), *op)
            } else if let Some(col) = extract_column(right, table, alias) {
                (col, (**left).clone(), reverse_op(*op))
            } else {
                return None;
            };
            // The value side must be free of column references for index probe.
            if has_column(&val_expr) {
                return None;
            }
            let mut bounds = IndexBounds::default();
            match op {
                BinOp::Eq => bounds.eq = Some(val_expr),
                BinOp::Gt => bounds.lower = Some((val_expr, false)),
                BinOp::Ge => bounds.lower = Some((val_expr, true)),
                BinOp::Lt => bounds.upper = Some((val_expr, false)),
                BinOp::Le => bounds.upper = Some((val_expr, true)),
                _ => return None,
            }
            Some((col_name, bounds))
        }
        _ => None,
    }
}

fn extract_column(e: &Expr, table: &str, alias: &Option<String>) -> Option<String> {
    match e {
        Expr::Column { table: t, name } => {
            if let Some(qual) = t {
                let want_alias = alias.as_deref().map(str::to_ascii_lowercase);
                let qual_lc = qual.to_ascii_lowercase();
                if qual_lc == table.to_ascii_lowercase() || Some(qual_lc) == want_alias {
                    Some(name.clone())
                } else {
                    None
                }
            } else {
                Some(name.clone())
            }
        }
        _ => None,
    }
}

fn has_column(e: &Expr) -> bool {
    match e {
        Expr::Column { .. } => true,
        Expr::Literal(_) => false,
        Expr::Binary { left, right, .. } => has_column(left) || has_column(right),
        Expr::And(a, b) | Expr::Or(a, b) => has_column(a) || has_column(b),
        Expr::Not(x) | Expr::Neg(x) | Expr::IsNull(x) => has_column(x),
        Expr::Func { args, .. } => args.iter().any(has_column),
    }
}

fn reverse_op(op: BinOp) -> BinOp {
    match op {
        BinOp::Eq => BinOp::Eq,
        BinOp::NotEq => BinOp::NotEq,
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, ColumnSchema, IndexSchema, TableSchema};
    use crate::parser::parse;
    use crate::storage::PageId;
    use crate::types::DataType;

    fn cat_with_index() -> Catalog {
        let mut c = Catalog::new();
        c.add_table(TableSchema {
            name: "t".into(),
            columns: vec![ColumnSchema {
                name: "id".into(),
                dtype: DataType::Int,
                not_null: false,
                primary_key: false,
                unique: false,
                default: None,
            }],
            checks: vec![],
            heap_head: PageId(1),
        })
        .unwrap();
        c.add_index(IndexSchema {
            name: "i".into(),
            table: "t".into(),
            column: "id".into(),
        })
        .unwrap();
        c
    }

    #[test]
    fn picks_index_scan_for_equality() {
        let stmt = parse("SELECT * FROM t WHERE id = 5").unwrap()[0].clone();
        let lp = crate::plan::logical::build_logical(stmt).unwrap();
        let pp = build_physical(lp, &cat_with_index()).unwrap();
        // Project over IndexScan.
        match pp {
            PhysicalPlan::Project { input, .. } => {
                assert!(matches!(*input, PhysicalPlan::IndexScan { .. }));
            }
            _ => panic!("expected project"),
        }
    }

    #[test]
    fn picks_seq_scan_when_no_index() {
        let stmt = parse("SELECT * FROM t WHERE id <> 5").unwrap()[0].clone();
        let lp = crate::plan::logical::build_logical(stmt).unwrap();
        let pp = build_physical(lp, &cat_with_index()).unwrap();
        match pp {
            PhysicalPlan::Project { input, .. } => match *input {
                PhysicalPlan::Filter { input, .. } => {
                    assert!(matches!(*input, PhysicalPlan::SeqScan { .. }));
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
