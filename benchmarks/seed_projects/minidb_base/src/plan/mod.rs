//! Logical and physical plan representations and the planner that lowers
//! an AST into a tree of physical-operator descriptors.
//!
//! The planner is deliberately small: it picks an `IndexScan` when the
//! WHERE clause contains exactly one equality or range predicate on an
//! indexed column; otherwise it emits a `SeqScan`. Above the access path
//! it layers `Filter`, then the right-hand side of any join, then
//! `Project`, `Sort`, and `Limit` as needed.
//!
//! This module performs parsing-to-plan translation only. Actual
//! execution lives in [`crate::exec`].

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::types::Schema;

/// Logical plan node. The planner constructs a [`LogicalPlan`] tree first
/// and then lowers it to a [`PhysicalPlan`].
#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
    },
    Filter {
        predicate: Expression,
        input: Box<LogicalPlan>,
    },
    Join {
        kind: JoinKind,
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Expression,
    },
    Project {
        projections: Vec<Projection>,
        input: Box<LogicalPlan>,
    },
    Sort {
        order_by: Vec<OrderTerm>,
        input: Box<LogicalPlan>,
    },
    Limit {
        limit: Option<u64>,
        offset: Option<u64>,
        input: Box<LogicalPlan>,
    },
}

/// Physical plan node. One-to-one with the operator files in [`crate::exec`].
#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index: String,
        predicate: IndexPredicate,
    },
    Filter {
        predicate: Expression,
        input: Box<PhysicalPlan>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Expression,
    },
    Project {
        projections: Vec<Projection>,
        input: Box<PhysicalPlan>,
    },
    Sort {
        order_by: Vec<OrderTerm>,
        input: Box<PhysicalPlan>,
    },
    Limit {
        limit: Option<u64>,
        offset: Option<u64>,
        input: Box<PhysicalPlan>,
    },
}

/// Description of the predicate that an [`PhysicalPlan::IndexScan`] applies.
#[derive(Debug, Clone)]
pub enum IndexPredicate {
    Eq(Expression),
    Range {
        lo: Option<(Expression, bool)>, // (bound, inclusive)
        hi: Option<(Expression, bool)>,
    },
}

/// Output schema descriptor produced alongside a plan.
#[derive(Debug, Clone)]
pub struct PlanSchema {
    pub schema: Schema,
    /// Parallel list of `(table_alias, column_name)` for qualified lookup.
    pub qualified: Vec<(Option<String>, String)>,
}

impl PlanSchema {
    pub fn new(schema: Schema, qualified: Vec<(Option<String>, String)>) -> Self {
        Self { schema, qualified }
    }

    pub fn concat(&self, other: &PlanSchema) -> PlanSchema {
        let schema = self.schema.concat(&other.schema);
        let mut qualified = self.qualified.clone();
        qualified.extend(other.qualified.iter().cloned());
        PlanSchema { schema, qualified }
    }

    /// Resolve a column reference to an index within this schema.
    pub fn resolve(&self, table: Option<&str>, column: &str) -> Result<usize> {
        let mut found = None;
        for (idx, (tbl, col)) in self.qualified.iter().enumerate() {
            let name_match = col.eq_ignore_ascii_case(column);
            let table_match = match (table, tbl) {
                (Some(t), Some(a)) => t.eq_ignore_ascii_case(a),
                (None, _) => true,
                (Some(_), None) => false,
            };
            if name_match && table_match {
                if found.is_some() {
                    return Err(MiniDbError::Parse(format!(
                        "ambiguous column reference `{column}`"
                    )));
                }
                found = Some(idx);
            }
        }
        found.ok_or_else(|| {
            let pretty = match table {
                Some(t) => format!("{t}.{column}"),
                None => column.to_string(),
            };
            MiniDbError::Parse(format!("unknown column `{pretty}`"))
        })
    }
}

/// The actual planner.
pub struct Planner<'a> {
    pub catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Self { catalog }
    }

    /// Lower a `SELECT` into a (physical plan, output schema) pair.
    pub fn plan_select(&self, stmt: &SelectStmt) -> Result<(PhysicalPlan, PlanSchema)> {
        let logical = self.build_logical(stmt)?;
        let (physical, schema) = self.lower(&logical)?;
        Ok((physical, schema))
    }

    fn build_logical(&self, stmt: &SelectStmt) -> Result<LogicalPlan> {
        let mut tree = LogicalPlan::Scan {
            table: stmt.from.name.clone(),
            alias: stmt.from.alias.clone(),
        };
        for j in &stmt.joins {
            let right = LogicalPlan::Scan {
                table: j.right.name.clone(),
                alias: j.right.alias.clone(),
            };
            tree = LogicalPlan::Join {
                kind: j.kind,
                left: Box::new(tree),
                right: Box::new(right),
                on: j.on.clone(),
            };
        }
        if let Some(pred) = &stmt.filter {
            tree = LogicalPlan::Filter {
                predicate: pred.clone(),
                input: Box::new(tree),
            };
        }
        tree = LogicalPlan::Project {
            projections: stmt.projections.clone(),
            input: Box::new(tree),
        };
        if !stmt.order_by.is_empty() {
            tree = LogicalPlan::Sort {
                order_by: stmt.order_by.clone(),
                input: Box::new(tree),
            };
        }
        if stmt.limit.is_some() || stmt.offset.is_some() {
            tree = LogicalPlan::Limit {
                limit: stmt.limit,
                offset: stmt.offset,
                input: Box::new(tree),
            };
        }
        Ok(tree)
    }

    fn lower(&self, logical: &LogicalPlan) -> Result<(PhysicalPlan, PlanSchema)> {
        match logical {
            LogicalPlan::Scan { table, alias } => self.lower_scan(table, alias, None),
            LogicalPlan::Filter { predicate, input } => {
                // Try to push a single equality/range on an indexed column
                // down into an IndexScan.
                if let LogicalPlan::Scan { table, alias } = input.as_ref() {
                    if let Some((ip, index_name)) =
                        self.try_index_predicate(table, predicate)?
                    {
                        let (plan, ps) =
                            self.lower_scan(table, alias, Some((ip, index_name)))?;
                        // If the predicate was fully consumed the filter
                        // becomes trivially true; we still keep it for
                        // correctness because IndexScan gives us a
                        // superset when NULLs are involved.
                        return Ok((
                            PhysicalPlan::Filter {
                                predicate: predicate.clone(),
                                input: Box::new(plan),
                            },
                            ps,
                        ));
                    }
                }
                let (child, schema) = self.lower(input)?;
                Ok((
                    PhysicalPlan::Filter {
                        predicate: predicate.clone(),
                        input: Box::new(child),
                    },
                    schema,
                ))
            }
            LogicalPlan::Join {
                kind: _,
                left,
                right,
                on,
            } => {
                let (l, ls) = self.lower(left)?;
                let (r, rs) = self.lower(right)?;
                let combined = ls.concat(&rs);
                Ok((
                    PhysicalPlan::NestedLoopJoin {
                        left: Box::new(l),
                        right: Box::new(r),
                        on: on.clone(),
                    },
                    combined,
                ))
            }
            LogicalPlan::Project { projections, input } => {
                let (child, in_schema) = self.lower(input)?;
                let out_schema = self.project_schema(projections, &in_schema)?;
                Ok((
                    PhysicalPlan::Project {
                        projections: projections.clone(),
                        input: Box::new(child),
                    },
                    out_schema,
                ))
            }
            LogicalPlan::Sort { order_by, input } => {
                let (child, schema) = self.lower(input)?;
                Ok((
                    PhysicalPlan::Sort {
                        order_by: order_by.clone(),
                        input: Box::new(child),
                    },
                    schema,
                ))
            }
            LogicalPlan::Limit {
                limit,
                offset,
                input,
            } => {
                let (child, schema) = self.lower(input)?;
                Ok((
                    PhysicalPlan::Limit {
                        limit: *limit,
                        offset: *offset,
                        input: Box::new(child),
                    },
                    schema,
                ))
            }
        }
    }

    fn lower_scan(
        &self,
        table: &str,
        alias: &Option<String>,
        index: Option<(IndexPredicate, String)>,
    ) -> Result<(PhysicalPlan, PlanSchema)> {
        let t = self.catalog.table(table)?;
        let alias_name = alias.clone().unwrap_or_else(|| table.to_string());
        let qualified: Vec<(Option<String>, String)> = t
            .schema
            .columns
            .iter()
            .map(|c| (Some(alias_name.clone()), c.name.clone()))
            .collect();
        let ps = PlanSchema::new(t.schema.clone(), qualified);
        let plan = match index {
            Some((predicate, index)) => PhysicalPlan::IndexScan {
                table: table.to_string(),
                alias: alias.clone(),
                index,
                predicate,
            },
            None => PhysicalPlan::SeqScan {
                table: table.to_string(),
                alias: alias.clone(),
            },
        };
        Ok((plan, ps))
    }

    fn project_schema(
        &self,
        projections: &[Projection],
        input: &PlanSchema,
    ) -> Result<PlanSchema> {
        let mut columns = Vec::new();
        let mut qualified = Vec::new();
        for p in projections {
            match p {
                Projection::AllColumns => {
                    columns.extend(input.schema.columns.iter().cloned());
                    qualified.extend(input.qualified.iter().cloned());
                }
                Projection::Expr { expr, alias } => {
                    let name = alias.clone().unwrap_or_else(|| expr_to_label(expr));
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

    /// If `predicate` is of the form `col = expr` (or a single comparison)
    /// on an indexed column of `table`, return the index predicate and
    /// the index name.
    fn try_index_predicate(
        &self,
        table: &str,
        predicate: &Expression,
    ) -> Result<Option<(IndexPredicate, String)>> {
        let Expression::BinaryOp { op, left, right } = predicate else {
            return Ok(None);
        };
        let col = match (&**left, &**right) {
            (
                Expression::ColumnRef {
                    table: _,
                    column,
                },
                _,
            ) => column.clone(),
            _ => return Ok(None),
        };
        let defs = self.catalog.index_defs_for(table);
        let Some(def) = defs.iter().find(|d| d.column.eq_ignore_ascii_case(&col)) else {
            return Ok(None);
        };
        let pred = match op {
            BinaryOperator::Eq => IndexPredicate::Eq((**right).clone()),
            BinaryOperator::Lt => IndexPredicate::Range {
                lo: None,
                hi: Some(((**right).clone(), false)),
            },
            BinaryOperator::Le => IndexPredicate::Range {
                lo: None,
                hi: Some(((**right).clone(), true)),
            },
            BinaryOperator::Gt => IndexPredicate::Range {
                lo: Some(((**right).clone(), false)),
                hi: None,
            },
            BinaryOperator::Ge => IndexPredicate::Range {
                lo: Some(((**right).clone(), true)),
                hi: None,
            },
            _ => return Ok(None),
        };
        Ok(Some((pred, def.name.clone())))
    }
}

fn expr_to_label(expr: &Expression) -> String {
    match expr {
        Expression::ColumnRef { table, column } => match table {
            Some(t) => format!("{t}.{column}"),
            None => column.clone(),
        },
        Expression::Literal(v) => v.to_string(),
        _ => "?column?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::types::{Column, ColumnType, Schema};

    fn sample_catalog() -> Catalog {
        let mut c = Catalog::new();
        c.create_table(
            "users",
            Schema::new(vec![
                Column::new("id", ColumnType::Int),
                Column::new("name", ColumnType::Text),
            ]),
        )
        .unwrap();
        c.create_index("ix_id", "users", "id", false).unwrap();
        c
    }

    #[test]
    fn chooses_index_scan_for_eq() {
        let cat = sample_catalog();
        let stmt = parse("SELECT id FROM users WHERE id = 7").unwrap();
        if let crate::parser::ast::Statement::Select(sel) = stmt {
            let (plan, _) = Planner::new(&cat).plan_select(&sel).unwrap();
            let dbg = format!("{plan:?}");
            assert!(dbg.contains("IndexScan"));
        } else {
            panic!();
        }
    }

    #[test]
    fn chooses_seqscan_without_index() {
        let cat = sample_catalog();
        let stmt = parse("SELECT id FROM users WHERE name = 'x'").unwrap();
        if let crate::parser::ast::Statement::Select(sel) = stmt {
            let (plan, _) = Planner::new(&cat).plan_select(&sel).unwrap();
            let dbg = format!("{plan:?}");
            assert!(dbg.contains("SeqScan"));
        } else {
            panic!();
        }
    }
}
