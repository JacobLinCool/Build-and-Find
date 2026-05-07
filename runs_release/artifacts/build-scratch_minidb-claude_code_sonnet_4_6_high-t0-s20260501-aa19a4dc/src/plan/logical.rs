use crate::parser::ast::*;
use crate::types::{DataType, Schema};

/// Logical plan node
#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
        predicate: Option<Expr>,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        items: Vec<SelectItem>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        join_type: JoinType,
        condition: Option<Expr>,
    },
    Sort {
        input: Box<LogicalPlan>,
        order_by: Vec<OrderByItem>,
    },
    Limit {
        input: Box<LogicalPlan>,
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
        assignments: Vec<(String, Expr)>,
        predicate: Option<Expr>,
    },
    Delete {
        table: String,
        predicate: Option<Expr>,
    },
    CreateTable(CreateTableStmt),
    DropTable(DropTableStmt),
    CreateIndex(CreateIndexStmt),
    DropIndex(DropIndexStmt),
    Empty,
}

/// Build a logical plan from a parsed statement
pub fn build_logical_plan(stmt: &Statement) -> LogicalPlan {
    match stmt {
        Statement::Select(s) => build_select_plan(s),
        Statement::Insert(s) => LogicalPlan::Insert {
            table: s.table.clone(),
            columns: s.columns.clone(),
            values: s.values.clone(),
        },
        Statement::Update(s) => LogicalPlan::Update {
            table: s.table.clone(),
            assignments: s.assignments.clone(),
            predicate: s.where_clause.clone(),
        },
        Statement::Delete(s) => LogicalPlan::Delete {
            table: s.table.clone(),
            predicate: s.where_clause.clone(),
        },
        Statement::CreateTable(s) => LogicalPlan::CreateTable(s.clone()),
        Statement::DropTable(s) => LogicalPlan::DropTable(s.clone()),
        Statement::CreateIndex(s) => LogicalPlan::CreateIndex(s.clone()),
        Statement::DropIndex(s) => LogicalPlan::DropIndex(s.clone()),
        _ => LogicalPlan::Empty,
    }
}

fn build_select_plan(s: &SelectStmt) -> LogicalPlan {
    // Start from FROM clause
    let base = match &s.from {
        None => LogicalPlan::Empty,
        Some(from_item) => build_from_plan(from_item),
    };

    // Apply WHERE filter
    let filtered = match &s.where_clause {
        None => base,
        Some(pred) => LogicalPlan::Filter {
            input: Box::new(base),
            predicate: pred.clone(),
        },
    };

    // Apply projection
    let projected = LogicalPlan::Project {
        input: Box::new(filtered),
        items: s.projections.clone(),
    };

    // Apply ORDER BY
    let sorted = if !s.order_by.is_empty() {
        LogicalPlan::Sort {
            input: Box::new(projected),
            order_by: s.order_by.clone(),
        }
    } else {
        projected
    };

    // Apply LIMIT/OFFSET
    if let Some(limit) = &s.limit {
        LogicalPlan::Limit {
            input: Box::new(sorted),
            limit: limit.clone(),
            offset: s.offset.clone(),
        }
    } else {
        sorted
    }
}

fn build_from_plan(item: &FromItem) -> LogicalPlan {
    match item {
        FromItem::Table { name, alias } => LogicalPlan::Scan {
            table: name.clone(),
            alias: alias.clone(),
            predicate: None,
        },
        FromItem::Join {
            left,
            right,
            join_type,
            condition,
        } => LogicalPlan::Join {
            left: Box::new(build_from_plan(left)),
            right: Box::new(build_from_plan(right)),
            join_type: join_type.clone(),
            condition: condition.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    #[test]
    fn test_build_select_plan() {
        let stmts = Parser::parse_from_str("SELECT * FROM t WHERE id > 0;").unwrap();
        let plan = build_logical_plan(&stmts[0]);
        // Should be: Project -> Filter -> Scan
        assert!(matches!(&plan, LogicalPlan::Project { .. }));
        if let LogicalPlan::Project { input, .. } = &plan {
            assert!(matches!(input.as_ref(), LogicalPlan::Filter { .. }));
        }
    }
}
