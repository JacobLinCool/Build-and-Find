use super::expr::eval;
use super::operator::{ExecColumn, ExecSchema, PhysicalOperator, Tuple};
use crate::error::Result;
use crate::parser::ast::{Expr, Projection};

/// Project: rewrites tuples according to a projection list and propagates
/// a flat output schema. Stars and qualified-stars are expanded against the
/// child schema once during `open()` so `next()` is cheap.
pub struct Project {
    pub child: Box<dyn PhysicalOperator>,
    pub projections: Vec<Projection>,
    expanded_exprs: Vec<(Expr, String)>, // (expr, output_name)
    output_schema: ExecSchema,
}

impl Project {
    pub fn new(child: Box<dyn PhysicalOperator>, projections: Vec<Projection>) -> Self {
        Project {
            child,
            projections,
            expanded_exprs: Vec::new(),
            output_schema: ExecSchema::empty(),
        }
    }

    fn expand(&mut self) {
        let child_schema = self.child.schema().clone();
        let mut exprs: Vec<(Expr, String)> = Vec::new();
        let mut out_cols: Vec<ExecColumn> = Vec::new();
        for p in &self.projections {
            match p {
                Projection::Star => {
                    for c in &child_schema.columns {
                        exprs.push((
                            Expr::Column {
                                table: c.qualifier.clone(),
                                name: c.name.clone(),
                            },
                            c.name.clone(),
                        ));
                        out_cols.push(c.clone());
                    }
                }
                Projection::QualifiedStar(t) => {
                    for c in &child_schema.columns {
                        if c.qualifier
                            .as_ref()
                            .map(|q| q.eq_ignore_ascii_case(t))
                            .unwrap_or(false)
                        {
                            exprs.push((
                                Expr::Column {
                                    table: c.qualifier.clone(),
                                    name: c.name.clone(),
                                },
                                c.name.clone(),
                            ));
                            out_cols.push(c.clone());
                        }
                    }
                }
                Projection::Expr(e, alias) => {
                    let name = alias.clone().unwrap_or_else(|| project_default_name(e));
                    exprs.push((e.clone(), name.clone()));
                    out_cols.push(ExecColumn {
                        qualifier: None,
                        name,
                        data_type: None,
                    });
                }
            }
        }
        self.expanded_exprs = exprs;
        self.output_schema = ExecSchema { columns: out_cols };
    }
}

fn project_default_name(e: &Expr) -> String {
    match e {
        Expr::Column { name, .. } => name.clone(),
        _ => format!("{:?}", e)
            .chars()
            .take(64)
            .collect::<String>()
            .replace(' ', ""),
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self) -> Result<()> {
        self.child.open()?;
        self.expand();
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        let child_schema = self.child.schema().clone();
        let Some(row) = self.child.next()? else {
            return Ok(None);
        };
        let mut out = Vec::with_capacity(self.expanded_exprs.len());
        for (e, _) in &self.expanded_exprs {
            out.push(eval(e, &child_schema, &row)?);
        }
        Ok(Some(out))
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> &ExecSchema {
        &self.output_schema
    }
}
