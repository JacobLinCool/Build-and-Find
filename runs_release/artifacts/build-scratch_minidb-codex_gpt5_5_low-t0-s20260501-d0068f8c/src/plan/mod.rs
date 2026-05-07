use crate::{
    error::Result,
    exec::{self, PhysicalOperator},
    parser::ast::Select,
    storage::StorageEngine,
};

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Select(Select),
}
pub struct PhysicalPlan {
    pub root: Box<dyn PhysicalOperator>,
}

pub fn logical_select(select: Select) -> LogicalPlan {
    LogicalPlan::Select(select)
}

pub fn physical(db: &StorageEngine, plan: LogicalPlan) -> Result<PhysicalPlan> {
    match plan {
        LogicalPlan::Select(s) => {
            let mut root = exec::scan_for_select(db, &s.from, &s.selection)?;
            if let Some(j) = s.join.clone() {
                let right = exec::scan_for_select(db, &j.right, &None)?;
                root = Box::new(exec::NestedLoopJoin::new(root, right, j.on)?);
            }
            if let Some(w) = s.selection.clone() {
                root = Box::new(exec::Filter::new(root, w));
            }
            if let Some(o) = s.order.clone() {
                root = Box::new(exec::Sort::new(root, o.expr, o.asc));
            }
            root = Box::new(exec::Project::new(root, s.projection));
            if s.limit.is_some() || s.offset.is_some() {
                root = Box::new(exec::Limit::new(root, s.limit, s.offset.map_or(0, |v| v)));
            }
            Ok(PhysicalPlan { root })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logical_wraps() {
        let _ = LogicalPlan::Select(crate::parser::ast::Select {
            projection: vec![crate::parser::ast::SelectItem::Star],
            from: "t".into(),
            join: None,
            selection: None,
            order: None,
            limit: None,
            offset: None,
        });
    }
}
