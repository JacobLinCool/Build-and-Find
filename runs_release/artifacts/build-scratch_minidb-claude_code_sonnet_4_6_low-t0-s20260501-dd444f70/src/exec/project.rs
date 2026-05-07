use crate::error::Result;
use crate::exec::filter::eval_expr;
use crate::exec::{EvalContext, PhysicalOperator, Schema, SchemaEntry};
use crate::parser::ast::Expr;
use crate::types::{Row, Value};

pub struct Project {
    pub input: Box<dyn PhysicalOperator>,
    pub projections: Vec<(Expr, Option<String>)>,
    output_schema: Schema,
}

impl Project {
    pub fn new(input: Box<dyn PhysicalOperator>, projections: Vec<(Expr, Option<String>)>) -> Self {
        let input_schema = input.schema().to_vec();
        let output_schema = build_output_schema(&projections, &input_schema);
        Project {
            input,
            projections,
            output_schema,
        }
    }
}

fn build_output_schema(projections: &[(Expr, Option<String>)], input_schema: &Schema) -> Schema {
    let mut schema = Vec::new();
    for (expr, alias) in projections {
        match expr {
            Expr::Col(name, table) if name == "*" => {
                // Expand star
                if let Some(tbl) = table {
                    for entry in input_schema {
                        if let Some(t) = &entry.table {
                            if t.eq_ignore_ascii_case(tbl) {
                                schema.push(entry.clone());
                            }
                        }
                    }
                } else {
                    schema.extend(input_schema.iter().cloned());
                }
            }
            _ => {
                let name = alias.clone().unwrap_or_else(|| expr_name(expr));
                let table = match expr {
                    Expr::Col(_, t) => t.clone(),
                    _ => None,
                };
                schema.push(SchemaEntry { name, table });
            }
        }
    }
    schema
}

fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Col(name, _) => name.clone(),
        Expr::Lit(v) => v.to_display_string(),
        Expr::Function(name, _) => name.clone(),
        _ => "?".to_string(),
    }
}

impl PhysicalOperator for Project {
    fn schema(&self) -> &Schema {
        &self.output_schema
    }

    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        let input_row = match self.input.next()? {
            None => return Ok(None),
            Some(r) => r,
        };
        let ctx = EvalContext::from_row(self.input.schema(), &input_row);
        let mut out_row = Vec::new();

        for (expr, _alias) in &self.projections {
            match expr {
                Expr::Col(name, table) if name == "*" => {
                    // Expand star
                    let input_schema = self.input.schema();
                    if let Some(tbl) = table {
                        for (i, entry) in input_schema.iter().enumerate() {
                            if let Some(t) = &entry.table {
                                if t.eq_ignore_ascii_case(tbl) {
                                    out_row.push(input_row.get(i).cloned().unwrap_or(Value::Null));
                                }
                            }
                        }
                    } else {
                        out_row.extend(input_row.iter().cloned());
                    }
                }
                _ => {
                    let val = eval_expr(expr, &ctx)?;
                    out_row.push(val);
                }
            }
        }

        Ok(Some(out_row))
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
}
