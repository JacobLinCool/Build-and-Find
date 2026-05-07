use crate::error::{MiniDbError, Result};
use crate::parser::ast;
use crate::types::{ColumnType, Value};

pub fn serialize_row(values: &[Value]) -> Vec<u8> {
    let mut data = Vec::new();
    for val in values {
        match val {
            Value::Null => data.push(0x00),
            Value::Int(v) => {
                data.push(0x01);
                data.extend_from_slice(&v.to_be_bytes());
            }
            Value::Real(v) => {
                data.push(0x02);
                data.extend_from_slice(&v.to_be_bytes());
            }
            Value::Text(v) => {
                data.push(0x03);
                let bytes = v.as_bytes();
                data.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                data.extend_from_slice(bytes);
            }
            Value::Bool(v) => {
                data.push(0x04);
                data.push(*v as u8);
            }
        }
    }
    data
}

pub fn deserialize_row(data: &[u8], columns: &[(String, ColumnType)]) -> Vec<Value> {
    let mut values = Vec::new();
    let mut pos = 0;
    for (_, col_type) in columns {
        if pos >= data.len() {
            values.push(col_type.default_value());
            continue;
        }
        let type_byte = data[pos];
        pos += 1;
        match type_byte {
            0x00 => values.push(Value::Null),
            0x01 => {
                if pos + 8 <= data.len() {
                    let v = i64::from_be_bytes(data[pos..pos + 8].try_into().unwrap_or([0; 8]));
                    values.push(Value::Int(v));
                    pos += 8;
                }
            }
            0x02 => {
                if pos + 8 <= data.len() {
                    let v = f64::from_be_bytes(data[pos..pos + 8].try_into().unwrap_or([0; 8]));
                    values.push(Value::Real(v));
                    pos += 8;
                }
            }
            0x03 => {
                if pos + 4 <= data.len() {
                    let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap_or([0; 4]))
                        as usize;
                    pos += 4;
                    if pos + len <= data.len() {
                        let s = String::from_utf8_lossy(&data[pos..pos + len]).to_string();
                        values.push(Value::Text(s));
                        pos += len;
                    }
                }
            }
            0x04 => {
                if pos < data.len() {
                    values.push(Value::Bool(data[pos] != 0));
                    pos += 1;
                }
            }
            _ => values.push(Value::Null),
        }
    }
    values
}

pub fn eval_expr(
    expr: &ast::Expr,
    row: &[Value],
    schema: &[(String, ColumnType)],
) -> Result<Value> {
    match expr {
        ast::Expr::Literal(v) => Ok(v.clone()),
        ast::Expr::ColumnRef { table, column } => {
            let col_name = if let Some(t) = table {
                format!("{}.{}", t, column)
            } else {
                column.clone()
            };
            if let Some(pos) = schema.iter().position(|(name, _)| name == &col_name) {
                Ok(row.get(pos).cloned().unwrap_or(Value::Null))
            } else if let Some(pos) = schema.iter().position(|(name, _)| name == column) {
                Ok(row.get(pos).cloned().unwrap_or(Value::Null))
            } else {
                Err(MiniDbError::Runtime(format!(
                    "Column '{}' not found",
                    col_name
                )))
            }
        }
        ast::Expr::Star => Err(MiniDbError::Runtime(
            "Cannot evaluate star in expression".into(),
        )),
        ast::Expr::BinaryOp { op, left, right } => {
            let l = eval_expr(left, row, schema)?;
            let r = eval_expr(right, row, schema)?;
            eval_binary_op(*op, &l, &r)
        }
        ast::Expr::UnaryOp { op, expr } => {
            let v = eval_expr(expr, row, schema)?;
            eval_unary_op(*op, &v)
        }
        ast::Expr::IsNull(expr) => {
            let v = eval_expr(expr, row, schema)?;
            Ok(Value::Bool(v.is_null()))
        }
        ast::Expr::IsNotNull(expr) => {
            let v = eval_expr(expr, row, schema)?;
            Ok(Value::Bool(!v.is_null()))
        }
        ast::Expr::Function { name, args } => {
            let mut values = Vec::new();
            for arg in args {
                values.push(eval_expr(arg, row, schema)?);
            }
            eval_function(name, &values)
        }
    }
}

fn eval_binary_op(op: ast::BinaryOp, left: &Value, right: &Value) -> Result<Value> {
    match op {
        ast::BinaryOp::Eq => Ok(Value::Bool(left == right)),
        ast::BinaryOp::Ne => Ok(Value::Bool(left != right)),
        ast::BinaryOp::Lt => Ok(Value::Bool(
            left.partial_cmp(right) == Some(std::cmp::Ordering::Less),
        )),
        ast::BinaryOp::Le => Ok(Value::Bool(matches!(
            left.partial_cmp(right),
            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
        ))),
        ast::BinaryOp::Gt => Ok(Value::Bool(
            left.partial_cmp(right) == Some(std::cmp::Ordering::Greater),
        )),
        ast::BinaryOp::Ge => Ok(Value::Bool(matches!(
            left.partial_cmp(right),
            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
        ))),
        ast::BinaryOp::And => {
            let l = left.coerce_to_bool().unwrap_or(false);
            let r = right.coerce_to_bool().unwrap_or(false);
            Ok(Value::Bool(l && r))
        }
        ast::BinaryOp::Or => {
            let l = left.coerce_to_bool().unwrap_or(false);
            let r = right.coerce_to_bool().unwrap_or(false);
            Ok(Value::Bool(l || r))
        }
        ast::BinaryOp::Add => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a + b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 + b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a + *b as f64)),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "Cannot add {} and {}",
                left.sql_type(),
                right.sql_type()
            ))),
        },
        ast::BinaryOp::Sub => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a - b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 - b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a - *b as f64)),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "Cannot subtract {} and {}",
                left.sql_type(),
                right.sql_type()
            ))),
        },
        ast::BinaryOp::Mul => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a * b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 * b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a * *b as f64)),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "Cannot multiply {} and {}",
                left.sql_type(),
                right.sql_type()
            ))),
        },
        ast::BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) | (Value::Int(_), Value::Real(0.0)) => {
                Err(MiniDbError::Runtime("Division by zero".into()))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a / b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 / b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a / *b as f64)),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "Cannot divide {} and {}",
                left.sql_type(),
                right.sql_type()
            ))),
        },
        ast::BinaryOp::Concat => match (left, right) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Ok(Value::Text(format!("{}{}", left, right))),
        },
    }
}

fn eval_unary_op(op: ast::UnaryOp, val: &Value) -> Result<Value> {
    match op {
        ast::UnaryOp::Neg => match val {
            Value::Int(v) => Ok(Value::Int(-v)),
            Value::Real(v) => Ok(Value::Real(-v)),
            Value::Null => Ok(Value::Null),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "Cannot negate {}",
                val.sql_type()
            ))),
        },
        ast::UnaryOp::Not => {
            let b = val.coerce_to_bool().unwrap_or(false);
            Ok(Value::Bool(!b))
        }
    }
}

fn eval_function(name: &str, args: &[Value]) -> Result<Value> {
    match name {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("LENGTH requires 1 argument".into()));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Ok(Value::Int(args[0].coerce_to_text().len() as i64)),
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("LOWER requires 1 argument".into()));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                Value::Null => Ok(Value::Null),
                _ => Ok(Value::Text(args[0].coerce_to_text().to_lowercase())),
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("UPPER requires 1 argument".into()));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                Value::Null => Ok(Value::Null),
                _ => Ok(Value::Text(args[0].coerce_to_text().to_uppercase())),
            }
        }
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(MiniDbError::Runtime("SUBSTR requires 2-3 arguments".into()));
            }
            let s = match &args[0] {
                Value::Text(s) => s.clone(),
                Value::Null => return Ok(Value::Null),
                _ => args[0].coerce_to_text(),
            };
            let start = match args[1].coerce_to_int() {
                Ok(v) => v as usize,
                Err(_) => return Ok(Value::Null),
            };
            let len = if args.len() == 3 {
                match args[2].coerce_to_int() {
                    Ok(v) => v as usize,
                    Err(_) => return Ok(Value::Null),
                }
            } else {
                s.len()
            };
            let start = start.saturating_sub(1).min(s.len());
            let end = (start + len).min(s.len());
            Ok(Value::Text(s[start..end].to_string()))
        }
        "ABS" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("ABS requires 1 argument".into()));
            }
            match &args[0] {
                Value::Int(v) => Ok(Value::Int(v.abs())),
                Value::Real(v) => Ok(Value::Real(v.abs())),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeMismatch(format!(
                    "ABS not supported for {}",
                    args[0].sql_type()
                ))),
            }
        }
        "COALESCE" => {
            for arg in args {
                if !arg.is_null() {
                    return Ok(arg.clone());
                }
            }
            Ok(Value::Null)
        }
        _ => Err(MiniDbError::Runtime(format!("Unknown function: {}", name))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast;

    #[test]
    fn test_eval_literal() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::Literal(Value::Int(42));
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_eval_column_ref() {
        let schema = vec![("id".into(), ColumnType::Int)];
        let row = vec![Value::Int(5)];
        let expr = ast::Expr::ColumnRef {
            table: None,
            column: "id".into(),
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_eval_binary_add() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::BinaryOp {
            op: ast::BinaryOp::Add,
            left: Box::new(ast::Expr::Literal(Value::Int(3))),
            right: Box::new(ast::Expr::Literal(Value::Int(4))),
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(7));
    }

    #[test]
    fn test_eval_binary_eq() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::BinaryOp {
            op: ast::BinaryOp::Eq,
            left: Box::new(ast::Expr::Literal(Value::Int(5))),
            right: Box::new(ast::Expr::Literal(Value::Int(5))),
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Bool(true));
    }

    #[test]
    fn test_eval_function_length() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::Function {
            name: "LENGTH".into(),
            args: vec![ast::Expr::Literal(Value::Text("hello".into()))],
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_eval_function_coalesce() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::Function {
            name: "COALESCE".into(),
            args: vec![
                ast::Expr::Literal(Value::Null),
                ast::Expr::Literal(Value::Int(42)),
            ],
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_eval_is_null() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::IsNull(Box::new(ast::Expr::Literal(Value::Null)));
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Bool(true));
    }

    #[test]
    fn test_eval_negation() {
        let schema = vec![];
        let row = vec![];
        let expr = ast::Expr::UnaryOp {
            op: ast::UnaryOp::Neg,
            expr: Box::new(ast::Expr::Literal(Value::Int(5))),
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(-5));
    }
}
