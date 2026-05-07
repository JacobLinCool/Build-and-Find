use crate::catalog::Catalog;
use crate::parser::ast::{BinaryOp, Expr, Select, Statement, TableRef};
use crate::types::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    Create,
    Drop,
    Insert,
    Select(LogicalSelect),
    Update,
    Delete,
    Transaction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalSelect {
    pub from: TableRef,
    pub predicate: Option<Expr>,
    pub projection_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PhysicalPlan {
    Ddl,
    Dml,
    Txn,
    SeqScan {
        table: String,
    },
    IndexScan {
        table: String,
        index: String,
        predicate: IndexPredicate,
    },
    Filter {
        input: Box<PhysicalPlan>,
    },
    Project {
        input: Box<PhysicalPlan>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
    },
    Sort {
        input: Box<PhysicalPlan>,
    },
    Limit {
        input: Box<PhysicalPlan>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexPredicate {
    pub column: String,
    pub op: BinaryOp,
    pub value: Value,
}

pub fn logical(statement: &Statement) -> LogicalPlan {
    match statement {
        Statement::CreateTable { .. }
        | Statement::DropTable { .. }
        | Statement::CreateIndex { .. }
        | Statement::DropIndex { .. } => LogicalPlan::Create,
        Statement::Insert { .. } => LogicalPlan::Insert,
        Statement::Select(select) => LogicalPlan::Select(LogicalSelect {
            from: select.from.clone(),
            predicate: select.selection.clone(),
            projection_count: select.projection.len(),
        }),
        Statement::Update { .. } => LogicalPlan::Update,
        Statement::Delete { .. } => LogicalPlan::Delete,
        Statement::Begin | Statement::Commit | Statement::Rollback => LogicalPlan::Transaction,
    }
}

pub fn physical(statement: &Statement, catalog: &Catalog) -> PhysicalPlan {
    match statement {
        Statement::Select(select) => physical_select(select, catalog),
        Statement::Begin | Statement::Commit | Statement::Rollback => PhysicalPlan::Txn,
        Statement::Insert { .. } | Statement::Update { .. } | Statement::Delete { .. } => {
            PhysicalPlan::Dml
        }
        _ => PhysicalPlan::Ddl,
    }
}

fn physical_select(select: &Select, catalog: &Catalog) -> PhysicalPlan {
    let mut root = match &select.from {
        TableRef::Table(table) => scan_for_table(table, select.selection.as_ref(), catalog),
        TableRef::Join { left, right, .. } => PhysicalPlan::NestedLoopJoin {
            left: Box::new(scan_for_table(left, None, catalog)),
            right: Box::new(scan_for_table(right, None, catalog)),
        },
    };
    if select.selection.is_some() {
        root = PhysicalPlan::Filter {
            input: Box::new(root),
        };
    }
    if !select.order_by.is_empty() {
        root = PhysicalPlan::Sort {
            input: Box::new(root),
        };
    }
    if select.limit.is_some() {
        root = PhysicalPlan::Limit {
            input: Box::new(root),
        };
    }
    PhysicalPlan::Project {
        input: Box::new(root),
    }
}

fn scan_for_table(table: &str, predicate: Option<&Expr>, catalog: &Catalog) -> PhysicalPlan {
    let table = table.to_ascii_lowercase();
    if let Some(index_predicate) = predicate.and_then(|expr| index_predicate(&table, expr)) {
        if let Some((index, _)) = catalog
            .indexes
            .iter()
            .find(|(_, idx)| idx.table == table && idx.column == index_predicate.column)
        {
            return PhysicalPlan::IndexScan {
                table,
                index: index.clone(),
                predicate: index_predicate,
            };
        }
    }
    PhysicalPlan::SeqScan { table }
}

fn index_predicate(table: &str, expr: &Expr) -> Option<IndexPredicate> {
    let Expr::Binary { left, op, right } = expr else {
        return None;
    };
    match (&**left, &**right) {
        (Expr::Column { table: t, name }, Expr::Literal(value))
            if t.as_ref().is_none_or(|x| x.eq_ignore_ascii_case(table))
                && matches!(
                    op,
                    BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
                ) =>
        {
            Some(IndexPredicate {
                column: name.to_ascii_lowercase(),
                op: *op,
                value: value.clone(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::IndexSchema;
    use crate::parser::parse_sql;

    #[test]
    fn emits_index_scan_for_indexed_predicate() {
        let mut catalog = Catalog::default();
        catalog.indexes.insert(
            "i".into(),
            IndexSchema {
                name: "i".into(),
                table: "t".into(),
                column: "id".into(),
            },
        );
        let stmt = parse_sql("select * from t where id = 1;")
            .unwrap()
            .remove(0);
        let plan = physical(&stmt, &catalog);
        assert!(format!("{plan:?}").contains("IndexScan"));
    }
}
