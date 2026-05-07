//! Physical plan: a concrete operator tree the executor can evaluate.

use crate::parser::ast::{Expr, OrderBy, Projection};
use crate::types::DataType;

#[derive(Debug, Clone)]
pub struct ColumnRef {
    pub table: Option<String>,
    pub name: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub columns: Vec<ColumnRef>,
}

impl Schema {
    pub fn empty() -> Self {
        Schema { columns: vec![] }
    }
    pub fn find(&self, table: Option<&str>, name: &str) -> Option<usize> {
        let mut found = None;
        for (i, c) in self.columns.iter().enumerate() {
            if c.name != name {
                continue;
            }
            if let Some(t) = table {
                if c.table.as_deref() != Some(t) {
                    continue;
                }
            }
            if found.is_some() {
                return None; // ambiguous
            }
            found = Some(i);
        }
        found
    }
    pub fn names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
        schema: Schema,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index_name: String,
        column: String,
        lo: Option<crate::types::Value>,
        lo_inclusive: bool,
        hi: Option<crate::types::Value>,
        hi_inclusive: bool,
        eq: Option<crate::types::Value>,
        schema: Schema,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
        schema: Schema,
    },
    Project {
        input: Box<PhysicalPlan>,
        projections: Vec<Projection>,
        schema: Schema,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Expr,
        schema: Schema,
    },
    Sort {
        input: Box<PhysicalPlan>,
        keys: Vec<OrderBy>,
        schema: Schema,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: Option<i64>,
        offset: Option<i64>,
        schema: Schema,
    },
}

impl PhysicalPlan {
    pub fn schema(&self) -> &Schema {
        match self {
            PhysicalPlan::SeqScan { schema, .. }
            | PhysicalPlan::IndexScan { schema, .. }
            | PhysicalPlan::Filter { schema, .. }
            | PhysicalPlan::Project { schema, .. }
            | PhysicalPlan::NestedLoopJoin { schema, .. }
            | PhysicalPlan::Sort { schema, .. }
            | PhysicalPlan::Limit { schema, .. } => schema,
        }
    }
}

use crate::catalog::Catalog;
use crate::error::Result;
use crate::plan::logical::{extract_simple_filters, LogicalPlan};

pub fn build_physical(cat: &Catalog, plan: LogicalPlan) -> Result<PhysicalPlan> {
    match plan {
        LogicalPlan::Scan { table, alias } => {
            let t = cat.get_table(&table)?;
            let cols = t
                .columns
                .iter()
                .map(|c| ColumnRef {
                    table: Some(alias.clone().unwrap_or_else(|| table.clone())),
                    name: c.name.clone(),
                    data_type: c.data_type,
                })
                .collect();
            Ok(PhysicalPlan::SeqScan {
                table,
                alias,
                schema: Schema { columns: cols },
            })
        }
        LogicalPlan::Join { left, right, on } => {
            let l = build_physical(cat, *left)?;
            let r = build_physical(cat, *right)?;
            let mut cols = l.schema().columns.clone();
            cols.extend_from_slice(&r.schema().columns);
            Ok(PhysicalPlan::NestedLoopJoin {
                left: Box::new(l),
                right: Box::new(r),
                on,
                schema: Schema { columns: cols },
            })
        }
        LogicalPlan::Filter { input, predicate } => {
            let phys = build_physical(cat, *input)?;
            // Try to push down to IndexScan when predicate is a single eq/range on an indexed column.
            if let PhysicalPlan::SeqScan {
                table,
                alias,
                schema,
            } = &phys
            {
                let simples = extract_simple_filters(&predicate);
                if simples.len() == 1 {
                    let (col, op, val) = &simples[0];
                    let idx_opt = cat
                        .indexes_for(table)
                        .into_iter()
                        .find(|i| i.column == *col);
                    if let Some(idx) = idx_opt {
                        use crate::parser::ast::BinaryOp;
                        let mut eq = None;
                        let (mut lo, mut lo_inc, mut hi, mut hi_inc) = (None, false, None, false);
                        match op {
                            BinaryOp::Eq => eq = Some(val.clone()),
                            BinaryOp::Lt => {
                                hi = Some(val.clone());
                                hi_inc = false;
                            }
                            BinaryOp::Lte => {
                                hi = Some(val.clone());
                                hi_inc = true;
                            }
                            BinaryOp::Gt => {
                                lo = Some(val.clone());
                                lo_inc = false;
                            }
                            BinaryOp::Gte => {
                                lo = Some(val.clone());
                                lo_inc = true;
                            }
                            _ => {}
                        }
                        if eq.is_some() || lo.is_some() || hi.is_some() {
                            let new = PhysicalPlan::IndexScan {
                                table: table.clone(),
                                alias: alias.clone(),
                                index_name: idx.name.clone(),
                                column: col.clone(),
                                lo,
                                lo_inclusive: lo_inc,
                                hi,
                                hi_inclusive: hi_inc,
                                eq,
                                schema: schema.clone(),
                            };
                            return Ok(PhysicalPlan::Filter {
                                schema: new.schema().clone(),
                                input: Box::new(new),
                                predicate,
                            });
                        }
                    }
                }
            }
            let schema = phys.schema().clone();
            Ok(PhysicalPlan::Filter {
                input: Box::new(phys),
                predicate,
                schema,
            })
        }
        LogicalPlan::Project { input, projections } => {
            let phys = build_physical(cat, *input)?;
            let mut cols = Vec::new();
            for p in &projections {
                match p {
                    Projection::Star => cols.extend_from_slice(&phys.schema().columns),
                    Projection::QualifiedStar(t) => {
                        for c in &phys.schema().columns {
                            if c.table.as_deref() == Some(t.as_str()) {
                                cols.push(c.clone());
                            }
                        }
                    }
                    Projection::Expr(e, alias) => {
                        let dt = infer_type(e, phys.schema()).unwrap_or(DataType::Text);
                        let name = alias.clone().unwrap_or_else(|| expr_alias_name(e));
                        cols.push(ColumnRef {
                            table: None,
                            name,
                            data_type: dt,
                        });
                    }
                }
            }
            Ok(PhysicalPlan::Project {
                input: Box::new(phys),
                projections,
                schema: Schema { columns: cols },
            })
        }
        LogicalPlan::Sort { input, keys } => {
            let phys = build_physical(cat, *input)?;
            let schema = phys.schema().clone();
            Ok(PhysicalPlan::Sort {
                input: Box::new(phys),
                keys,
                schema,
            })
        }
        LogicalPlan::Limit {
            input,
            limit,
            offset,
        } => {
            let phys = build_physical(cat, *input)?;
            let schema = phys.schema().clone();
            Ok(PhysicalPlan::Limit {
                input: Box::new(phys),
                limit,
                offset,
                schema,
            })
        }
    }
}

fn expr_alias_name(e: &Expr) -> String {
    match e {
        Expr::Column { name, .. } => name.clone(),
        _ => format!("{}", e),
    }
}

fn infer_type(e: &Expr, schema: &Schema) -> Option<DataType> {
    use crate::parser::ast::BinaryOp;
    use crate::types::Value;
    match e {
        Expr::Literal(v) => v.data_type(),
        Expr::Column { table, name } => schema
            .find(table.as_deref(), name)
            .map(|i| schema.columns[i].data_type),
        Expr::BinaryOp { op, left, right } => match op {
            BinaryOp::Concat => Some(DataType::Text),
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte => Some(DataType::Bool),
            _ => {
                let lt = infer_type(left, schema);
                let rt = infer_type(right, schema);
                if matches!(lt, Some(DataType::Real)) || matches!(rt, Some(DataType::Real)) {
                    Some(DataType::Real)
                } else {
                    Some(DataType::Int)
                }
            }
        },
        Expr::UnaryOp { op, expr } => match op {
            crate::parser::ast::UnaryOp::Neg => infer_type(expr, schema),
            crate::parser::ast::UnaryOp::Not => Some(DataType::Bool),
        },
        Expr::IsNull(_) | Expr::IsNotNull(_) => Some(DataType::Bool),
        Expr::Function { name, args } => match name.to_ascii_uppercase().as_str() {
            "LENGTH" | "ABS" => Some(DataType::Int),
            "LOWER" | "UPPER" | "SUBSTR" => Some(DataType::Text),
            "COALESCE" => args.iter().find_map(|a| infer_type(a, schema)),
            _ => None,
        },
    }
    .or(Some(match e {
        Expr::Literal(_) => DataType::Int,
        _ => DataType::Text,
    }))
    .filter(|_| !matches!(e, Expr::Literal(Value::Null)))
}
