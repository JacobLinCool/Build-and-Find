use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::*;
use crate::plan::logical::LogicalPlan;

/// Physical plan node — maps to a concrete physical operator
#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
        predicate: Option<Expr>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index_name: String,
        index_col: String,
        eq_val: Option<Expr>,
        range_lo: Option<Expr>,
        range_hi: Option<Expr>,
        predicate: Option<Expr>,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        items: Vec<SelectItem>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        join_type: JoinType,
        condition: Option<Expr>,
    },
    Sort {
        input: Box<PhysicalPlan>,
        order_by: Vec<OrderByItem>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: Expr,
        offset: Option<Expr>,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        input: Box<PhysicalPlan>,
        assignments: Vec<(String, Expr)>,
    },
    Delete {
        table: String,
        input: Box<PhysicalPlan>,
    },
    CreateTable(CreateTableStmt),
    DropTable(DropTableStmt),
    CreateIndex(CreateIndexStmt),
    DropIndex(DropIndexStmt),
    Empty,
}

/// Convert a logical plan to a physical plan, choosing index scans where applicable.
pub fn build_physical_plan(logical: &LogicalPlan, catalog: &Catalog) -> Result<PhysicalPlan> {
    match logical {
        LogicalPlan::Scan {
            table,
            alias,
            predicate,
        } => {
            // Check if we can use an index
            if let Some(pred) = predicate {
                if let Some((index_name, col, eq_val, lo, hi)) =
                    find_index_predicate(pred, table, catalog)
                {
                    return Ok(PhysicalPlan::IndexScan {
                        table: table.clone(),
                        alias: alias.clone(),
                        index_name,
                        index_col: col,
                        eq_val,
                        range_lo: lo,
                        range_hi: hi,
                        predicate: Some(pred.clone()),
                    });
                }
            }
            Ok(PhysicalPlan::SeqScan {
                table: table.clone(),
                alias: alias.clone(),
                predicate: predicate.clone(),
            })
        }
        LogicalPlan::Filter { input, predicate } => {
            // Try to push filter into scan
            if let LogicalPlan::Scan {
                table,
                alias,
                predicate: scan_pred,
            } = input.as_ref()
            {
                let merged_pred = match scan_pred {
                    None => predicate.clone(),
                    Some(sp) => Expr::BinOp {
                        op: BinOp::And,
                        left: Box::new(sp.clone()),
                        right: Box::new(predicate.clone()),
                    },
                };
                let new_scan = LogicalPlan::Scan {
                    table: table.clone(),
                    alias: alias.clone(),
                    predicate: Some(merged_pred),
                };
                return build_physical_plan(&new_scan, catalog);
            }
            let phys_input = build_physical_plan(input, catalog)?;
            Ok(PhysicalPlan::Filter {
                input: Box::new(phys_input),
                predicate: predicate.clone(),
            })
        }
        LogicalPlan::Project { input, items } => {
            let phys_input = build_physical_plan(input, catalog)?;
            Ok(PhysicalPlan::Project {
                input: Box::new(phys_input),
                items: items.clone(),
            })
        }
        LogicalPlan::Join {
            left,
            right,
            join_type,
            condition,
        } => {
            let phys_left = build_physical_plan(left, catalog)?;
            let phys_right = build_physical_plan(right, catalog)?;
            Ok(PhysicalPlan::NestedLoopJoin {
                left: Box::new(phys_left),
                right: Box::new(phys_right),
                join_type: join_type.clone(),
                condition: condition.clone(),
            })
        }
        LogicalPlan::Sort { input, order_by } => {
            let phys_input = build_physical_plan(input, catalog)?;
            Ok(PhysicalPlan::Sort {
                input: Box::new(phys_input),
                order_by: order_by.clone(),
            })
        }
        LogicalPlan::Limit {
            input,
            limit,
            offset,
        } => {
            let phys_input = build_physical_plan(input, catalog)?;
            Ok(PhysicalPlan::Limit {
                input: Box::new(phys_input),
                limit: limit.clone(),
                offset: offset.clone(),
            })
        }
        LogicalPlan::Insert {
            table,
            columns,
            values,
        } => Ok(PhysicalPlan::Insert {
            table: table.clone(),
            columns: columns.clone(),
            values: values.clone(),
        }),
        LogicalPlan::Update {
            table,
            assignments,
            predicate,
        } => {
            let scan = LogicalPlan::Scan {
                table: table.clone(),
                alias: None,
                predicate: predicate.clone(),
            };
            let phys_scan = build_physical_plan(&scan, catalog)?;
            Ok(PhysicalPlan::Update {
                table: table.clone(),
                input: Box::new(phys_scan),
                assignments: assignments.clone(),
            })
        }
        LogicalPlan::Delete { table, predicate } => {
            let scan = LogicalPlan::Scan {
                table: table.clone(),
                alias: None,
                predicate: predicate.clone(),
            };
            let phys_scan = build_physical_plan(&scan, catalog)?;
            Ok(PhysicalPlan::Delete {
                table: table.clone(),
                input: Box::new(phys_scan),
            })
        }
        LogicalPlan::CreateTable(s) => Ok(PhysicalPlan::CreateTable(s.clone())),
        LogicalPlan::DropTable(s) => Ok(PhysicalPlan::DropTable(s.clone())),
        LogicalPlan::CreateIndex(s) => Ok(PhysicalPlan::CreateIndex(s.clone())),
        LogicalPlan::DropIndex(s) => Ok(PhysicalPlan::DropIndex(s.clone())),
        LogicalPlan::Empty => Ok(PhysicalPlan::Empty),
    }
}

/// Try to find an index that matches a predicate for the given table.
/// Returns (index_name, col, eq_val, range_lo, range_hi) if found.
fn find_index_predicate(
    expr: &Expr,
    table: &str,
    catalog: &Catalog,
) -> Option<(String, String, Option<Expr>, Option<Expr>, Option<Expr>)> {
    match expr {
        Expr::BinOp { op, left, right } => {
            if let Expr::Column(col_ref) = left.as_ref() {
                let col_name = &col_ref.column;
                if let Some(idx) = catalog.find_index_for_column(table, col_name) {
                    let idx_name = idx.name.clone();
                    let col = col_name.clone();
                    match op {
                        BinOp::Eq => {
                            return Some((idx_name, col, Some(*right.clone()), None, None));
                        }
                        BinOp::Lt | BinOp::Le => {
                            return Some((idx_name, col, None, None, Some(*right.clone())));
                        }
                        BinOp::Gt | BinOp::Ge => {
                            return Some((idx_name, col, None, Some(*right.clone()), None));
                        }
                        _ => {}
                    }
                }
            }
            if let Expr::Column(col_ref) = right.as_ref() {
                let col_name = &col_ref.column;
                if let Some(idx) = catalog.find_index_for_column(table, col_name) {
                    let idx_name = idx.name.clone();
                    let col = col_name.clone();
                    match op {
                        BinOp::Eq => {
                            return Some((idx_name, col, Some(*left.clone()), None, None));
                        }
                        _ => {}
                    }
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, IndexMeta, TableMeta};
    use crate::parser::Parser;
    use crate::plan::logical::build_logical_plan;
    use crate::types::{Column, DataType, Schema};

    fn make_catalog_with_index() -> Catalog {
        let mut cat = Catalog::new();
        cat.create_table(TableMeta {
            name: "users".into(),
            schema: Schema::new(vec![
                Column::new("id", DataType::Int),
                Column::new("name", DataType::Text),
            ]),
            check_constraints: vec![],
            heap_root: 0,
            next_row_id: 1,
        })
        .unwrap();
        cat.create_index(IndexMeta {
            name: "idx_users_id".into(),
            table: "users".into(),
            columns: vec!["id".into()],
            unique: true,
            btree_root: 1,
        })
        .unwrap();
        cat
    }

    #[test]
    fn test_index_scan_selection() {
        let cat = make_catalog_with_index();
        let stmts = Parser::parse_from_str("SELECT * FROM users WHERE id = 5;").unwrap();
        let logical = build_logical_plan(&stmts[0]);
        let physical = build_physical_plan(&logical, &cat).unwrap();

        // Should produce Project -> IndexScan
        if let PhysicalPlan::Project { input, .. } = &physical {
            assert!(matches!(input.as_ref(), PhysicalPlan::IndexScan { .. }));
        } else {
            panic!("expected Project as root");
        }
    }

    #[test]
    fn test_seq_scan_without_index() {
        let cat = Catalog::new();
        let mut c = cat;
        c.create_table(TableMeta {
            name: "t".into(),
            schema: Schema::new(vec![Column::new("x", DataType::Int)]),
            check_constraints: vec![],
            heap_root: 0,
            next_row_id: 1,
        })
        .unwrap();
        let stmts = Parser::parse_from_str("SELECT * FROM t WHERE x = 1;").unwrap();
        let logical = build_logical_plan(&stmts[0]);
        let physical = build_physical_plan(&logical, &c).unwrap();

        if let PhysicalPlan::Project { input, .. } = &physical {
            assert!(matches!(input.as_ref(), PhysicalPlan::SeqScan { .. }));
        }
    }
}
