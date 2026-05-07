/// Expression evaluator: evaluates AST expressions against a row + schema.
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::types::{tvl, Row, Schema, Value};

/// Evaluate an expression given a row and its schema.
/// For JOINs, combined_schema and combined_row contain both sides.
pub fn eval_expr(expr: &Expr, row: &Row, schema: &Schema) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),

        Expr::Column(col_ref) => {
            let idx = if let Some(tbl) = &col_ref.table {
                // Try table.column lookup
                schema
                    .columns
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(&col_ref.column))
            } else {
                schema.find_column(&col_ref.column)
            };

            match idx {
                Some(i) if i < row.len() => Ok(row[i].clone()),
                Some(_) => Err(MiniDbError::Runtime(format!(
                    "column index out of range for '{}'",
                    col_ref.column
                ))),
                None => Err(MiniDbError::Runtime(format!(
                    "column '{}' not found",
                    col_ref.column
                ))),
            }
        }

        Expr::BinOp { op, left, right } => {
            let lv = eval_expr(left, row, schema)?;
            let rv = eval_expr(right, row, schema)?;

            match op {
                BinOp::Add => lv.add(&rv),
                BinOp::Sub => lv.sub(&rv),
                BinOp::Mul => lv.mul(&rv),
                BinOp::Div => lv.div(&rv),
                BinOp::Concat => Ok(lv.concat(&rv)),
                BinOp::Eq => Ok(Value::Bool(tvl::eq(&lv, &rv).unwrap_or(false))),
                BinOp::Neq => Ok(Value::Bool(tvl::neq(&lv, &rv).unwrap_or(false))),
                BinOp::Lt => Ok(Value::Bool(tvl::lt(&lv, &rv).unwrap_or(false))),
                BinOp::Le => Ok(Value::Bool(tvl::le(&lv, &rv).unwrap_or(false))),
                BinOp::Gt => Ok(Value::Bool(tvl::gt(&lv, &rv).unwrap_or(false))),
                BinOp::Ge => Ok(Value::Bool(tvl::ge(&lv, &rv).unwrap_or(false))),
                BinOp::And => {
                    let lb = eval_as_bool(&lv);
                    let rb = eval_as_bool(&rv);
                    Ok(match tvl::and(lb, rb) {
                        Some(b) => Value::Bool(b),
                        None => Value::Null,
                    })
                }
                BinOp::Or => {
                    let lb = eval_as_bool(&lv);
                    let rb = eval_as_bool(&rv);
                    Ok(match tvl::or(lb, rb) {
                        Some(b) => Value::Bool(b),
                        None => Value::Null,
                    })
                }
            }
        }

        Expr::UnaryOp { op, expr } => {
            let v = eval_expr(expr, row, schema)?;
            match op {
                UnaryOp::Neg => match &v {
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Real(r) => Ok(Value::Real(-r)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::Type(format!(
                        "cannot negate {:?}",
                        v.data_type()
                    ))),
                },
                UnaryOp::Not => Ok(match eval_as_bool(&v) {
                    Some(b) => Value::Bool(!b),
                    None => Value::Null,
                }),
            }
        }

        Expr::IsNull { expr, negated } => {
            let v = eval_expr(expr, row, schema)?;
            let is_null = v.is_null();
            Ok(Value::Bool(if *negated { !is_null } else { is_null }))
        }

        Expr::FunctionCall { name, args } => eval_function(name, args, row, schema),

        Expr::Wildcard | Expr::QualifiedWildcard(_) => Err(MiniDbError::Runtime(
            "wildcard not valid in this context".into(),
        )),
    }
}

fn eval_as_bool(v: &Value) -> Option<bool> {
    v.is_truthy()
}

fn eval_function(name: &str, args: &[Expr], row: &Row, schema: &Schema) -> Result<Value> {
    match name.to_uppercase().as_str() {
        "LENGTH" => {
            let v = eval_one_arg(args, name, row, schema)?;
            match &v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                _ => Err(MiniDbError::Type(format!(
                    "LENGTH expects TEXT, got {:?}",
                    v.data_type()
                ))),
            }
        }
        "LOWER" => {
            let v = eval_one_arg(args, name, row, schema)?;
            match &v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                _ => Err(MiniDbError::Type(format!("LOWER expects TEXT"))),
            }
        }
        "UPPER" => {
            let v = eval_one_arg(args, name, row, schema)?;
            match &v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                _ => Err(MiniDbError::Type(format!("UPPER expects TEXT"))),
            }
        }
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(MiniDbError::Runtime(format!(
                    "SUBSTR requires 2 or 3 arguments, got {}",
                    args.len()
                )));
            }
            let s = eval_expr(&args[0], row, schema)?;
            let start = eval_expr(&args[1], row, schema)?;
            if s.is_null() || start.is_null() {
                return Ok(Value::Null);
            }
            let s = match &s {
                Value::Text(t) => t.clone(),
                _ => return Err(MiniDbError::Type("SUBSTR first arg must be TEXT".into())),
            };
            let start_idx = start.coerce_to_int()? as usize;
            let start_idx = if start_idx > 0 { start_idx - 1 } else { 0 };
            if args.len() == 3 {
                let len = eval_expr(&args[2], row, schema)?.coerce_to_int()? as usize;
                let end = (start_idx + len).min(s.len());
                Ok(Value::Text(s[start_idx.min(s.len())..end].to_string()))
            } else {
                Ok(Value::Text(s[start_idx.min(s.len())..].to_string()))
            }
        }
        "ABS" => {
            let v = eval_one_arg(args, name, row, schema)?;
            match &v {
                Value::Null => Ok(Value::Null),
                Value::Int(i) => Ok(Value::Int(i.abs())),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                _ => Err(MiniDbError::Type("ABS expects numeric".into())),
            }
        }
        "COALESCE" => {
            for arg in args {
                let v = eval_expr(arg, row, schema)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Ok(Value::Null)
        }
        "COUNT" => {
            // COUNT(*) is handled at aggregate level, but handle COUNT(expr) here as a passthrough
            if args.len() == 1 {
                if matches!(&args[0], Expr::Wildcard) {
                    return Ok(Value::Int(1)); // will be counted by aggregator
                }
                let v = eval_expr(&args[0], row, schema)?;
                Ok(if v.is_null() {
                    Value::Int(0)
                } else {
                    Value::Int(1)
                })
            } else {
                Err(MiniDbError::Runtime("COUNT takes 1 argument".into()))
            }
        }
        _ => Err(MiniDbError::Runtime(format!("unknown function '{}'", name))),
    }
}

fn eval_one_arg(args: &[Expr], name: &str, row: &Row, schema: &Schema) -> Result<Value> {
    if args.len() != 1 {
        return Err(MiniDbError::Runtime(format!(
            "{} takes exactly 1 argument, got {}",
            name,
            args.len()
        )));
    }
    eval_expr(&args[0], row, schema)
}

/// Evaluate a boolean predicate, returning the three-valued result.
pub fn eval_predicate(expr: &Expr, row: &Row, schema: &Schema) -> Result<Option<bool>> {
    let v = eval_expr(expr, row, schema)?;
    Ok(v.is_truthy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, DataType, Schema};

    fn make_schema() -> Schema {
        Schema::new(vec![
            Column::new("id", DataType::Int),
            Column::new("name", DataType::Text),
            Column::new("score", DataType::Real),
        ])
    }

    fn make_row() -> Row {
        vec![
            Value::Int(1),
            Value::Text("Alice".into()),
            Value::Real(95.5),
        ]
    }

    #[test]
    fn test_eval_literal() {
        let schema = make_schema();
        let row = make_row();
        let expr = Expr::Literal(Value::Int(42));
        let result = eval_expr(&expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_eval_column() {
        let schema = make_schema();
        let row = make_row();
        let expr = Expr::Column(ColumnRef {
            table: None,
            column: "name".into(),
        });
        let result = eval_expr(&expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Text("Alice".into()));
    }

    #[test]
    fn test_eval_arithmetic() {
        let schema = make_schema();
        let row = make_row();
        let expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Column(ColumnRef {
                table: None,
                column: "id".into(),
            })),
            right: Box::new(Expr::Literal(Value::Int(10))),
        };
        let result = eval_expr(&expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Int(11));
    }

    #[test]
    fn test_eval_comparison() {
        let schema = make_schema();
        let row = make_row();
        let expr = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef {
                table: None,
                column: "id".into(),
            })),
            right: Box::new(Expr::Literal(Value::Int(1))),
        };
        let result = eval_expr(&expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_eval_is_null() {
        let schema = make_schema();
        let row = vec![Value::Null, Value::Null, Value::Null];
        let expr = Expr::IsNull {
            expr: Box::new(Expr::Column(ColumnRef {
                table: None,
                column: "id".into(),
            })),
            negated: false,
        };
        let result = eval_expr(&expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_eval_functions() {
        let schema = make_schema();
        let row = make_row();

        let lower_expr = Expr::FunctionCall {
            name: "LOWER".into(),
            args: vec![Expr::Column(ColumnRef {
                table: None,
                column: "name".into(),
            })],
        };
        let result = eval_expr(&lower_expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Text("alice".into()));

        let len_expr = Expr::FunctionCall {
            name: "LENGTH".into(),
            args: vec![Expr::Column(ColumnRef {
                table: None,
                column: "name".into(),
            })],
        };
        let result = eval_expr(&len_expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn test_coalesce() {
        let schema = Schema::new(vec![Column::new("x", DataType::Int)]);
        let row = vec![Value::Null];
        let expr = Expr::FunctionCall {
            name: "COALESCE".into(),
            args: vec![
                Expr::Column(ColumnRef {
                    table: None,
                    column: "x".into(),
                }),
                Expr::Literal(Value::Int(99)),
            ],
        };
        let result = eval_expr(&expr, &row, &schema).unwrap();
        assert_eq!(result, Value::Int(99));
    }
}
