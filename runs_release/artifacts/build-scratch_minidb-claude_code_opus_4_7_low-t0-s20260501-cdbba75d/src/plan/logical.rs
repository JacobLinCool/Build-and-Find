use crate::parser::ast::*;

/// Logical plan tree. Each variant corresponds roughly to a relational-algebra
/// node and is produced from the parser AST without consulting the catalog.
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
    NestedLoopJoin {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<(Expr, bool)>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: Option<i64>,
        offset: Option<i64>,
    },
}

pub fn build_logical(sel: &Select) -> LogicalPlan {
    let mut plan = LogicalPlan::Scan {
        table: sel.table.clone(),
        alias: sel.table_alias.clone(),
    };
    if let Some(j) = &sel.join {
        let right = LogicalPlan::Scan {
            table: j.table.clone(),
            alias: j.alias.clone(),
        };
        plan = LogicalPlan::NestedLoopJoin {
            left: Box::new(plan),
            right: Box::new(right),
            on: j.on.clone(),
        };
    }
    if let Some(w) = &sel.where_clause {
        plan = LogicalPlan::Filter {
            input: Box::new(plan),
            predicate: w.clone(),
        };
    }
    // Sort runs before Project so ORDER BY can reference qualified columns
    // from the underlying tables (`a.x`).
    if !sel.order_by.is_empty() {
        plan = LogicalPlan::Sort {
            input: Box::new(plan),
            keys: sel.order_by.clone(),
        };
    }
    plan = LogicalPlan::Project {
        input: Box::new(plan),
        projections: sel.projections.clone(),
    };
    if sel.limit.is_some() || sel.offset.is_some() {
        plan = LogicalPlan::Limit {
            input: Box::new(plan),
            limit: sel.limit,
            offset: sel.offset,
        };
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sql;

    #[test]
    fn simple_select_logical() {
        let stmts = parse_sql("SELECT a FROM t WHERE a = 1 ORDER BY a LIMIT 5;").unwrap();
        let sel = match &stmts[0] {
            Statement::Select(s) => s,
            _ => panic!(),
        };
        let p = build_logical(sel);
        match p {
            LogicalPlan::Limit { input, .. } => match *input {
                LogicalPlan::Project { .. } => {}
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
