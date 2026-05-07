use crate::error::Result;
use crate::exec::{EvalContext, PhysicalOperator, Schema};
use crate::parser::ast::Expr;
use crate::types::Row;

pub struct Filter {
    pub input: Box<dyn PhysicalOperator>,
    pub predicate: Expr,
}

impl Filter {
    pub fn new(input: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        Filter { input, predicate }
    }
}

impl PhysicalOperator for Filter {
    fn schema(&self) -> &Schema {
        self.input.schema()
    }

    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            match self.input.next()? {
                None => return Ok(None),
                Some(row) => {
                    let ctx = EvalContext::from_row(self.input.schema(), &row);
                    let val = eval_expr(&self.predicate, &ctx)?;
                    if val.is_truthy().is_true() {
                        return Ok(Some(row));
                    }
                }
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
}

use crate::error::MiniDbError;
use crate::parser::ast::{BinOp, UnaryOp};
use crate::types::{Tribool, Value};

pub fn eval_expr(expr: &Expr, ctx: &EvalContext) -> Result<Value> {
    match expr {
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Col(name, table) => Ok(ctx.lookup(name, table.as_deref())),
        Expr::BinOp(left, op, right) => {
            let lv = eval_expr(left, ctx)?;
            let rv = eval_expr(right, ctx)?;
            eval_binop(&lv, op, &rv)
        }
        Expr::UnaryOp(op, inner) => {
            let v = eval_expr(inner, ctx)?;
            match op {
                UnaryOp::Neg => v.negate(),
                UnaryOp::Not => match v {
                    Value::Null => Ok(Value::Null),
                    Value::Bool(b) => Ok(Value::Bool(!b)),
                    _ => Ok(Value::Bool(!matches!(v.is_truthy(), Tribool::True))),
                },
            }
        }
        Expr::IsNull(inner) => {
            let v = eval_expr(inner, ctx)?;
            Ok(Value::Bool(v.is_null()))
        }
        Expr::IsNotNull(inner) => {
            let v = eval_expr(inner, ctx)?;
            Ok(Value::Bool(!v.is_null()))
        }
        Expr::Function(name, args) => eval_function(name, args, ctx),
        Expr::In(expr, list) => {
            let v = eval_expr(expr, ctx)?;
            if v.is_null() {
                return Ok(Value::Null);
            }
            let mut any_unknown = false;
            for item_expr in list {
                let item = eval_expr(item_expr, ctx)?;
                match v.sql_eq(&item) {
                    Tribool::True => return Ok(Value::Bool(true)),
                    Tribool::Unknown => any_unknown = true,
                    Tribool::False => {}
                }
            }
            if any_unknown {
                Ok(Value::Null)
            } else {
                Ok(Value::Bool(false))
            }
        }
        Expr::Between(val_expr, low_expr, high_expr) => {
            let v = eval_expr(val_expr, ctx)?;
            let lo = eval_expr(low_expr, ctx)?;
            let hi = eval_expr(high_expr, ctx)?;
            let ge_lo = v.sql_cmp(&lo).map(|o| o != std::cmp::Ordering::Less);
            let le_hi = v.sql_cmp(&hi).map(|o| o != std::cmp::Ordering::Greater);
            match (ge_lo, le_hi) {
                (Some(true), Some(true)) => Ok(Value::Bool(true)),
                (Some(false), _) | (_, Some(false)) => Ok(Value::Bool(false)),
                _ => Ok(Value::Null),
            }
        }
        Expr::Like(val_expr, pattern) => {
            let v = eval_expr(val_expr, ctx)?;
            match v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Bool(like_match(&s, pattern))),
                _ => Ok(Value::Bool(false)),
            }
        }
        Expr::Case {
            operand,
            branches,
            else_branch,
        } => {
            let operand_val = if let Some(op) = operand {
                Some(eval_expr(op, ctx)?)
            } else {
                None
            };
            for (cond, result) in branches {
                let matches = if let Some(ref ov) = operand_val {
                    let cv = eval_expr(cond, ctx)?;
                    ov.sql_eq(&cv).is_true()
                } else {
                    let cv = eval_expr(cond, ctx)?;
                    cv.is_truthy().is_true()
                };
                if matches {
                    return eval_expr(result, ctx);
                }
            }
            if let Some(else_e) = else_branch {
                eval_expr(else_e, ctx)
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn eval_binop(lv: &Value, op: &BinOp, rv: &Value) -> Result<Value> {
    match op {
        BinOp::Add => lv.add(rv),
        BinOp::Sub => lv.sub(rv),
        BinOp::Mul => lv.mul(rv),
        BinOp::Div => lv.div(rv),
        BinOp::Mod => match (lv, rv) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(MiniDbError::Runtime("modulo by zero".to_string()))
                } else {
                    Ok(Value::Int(a % b))
                }
            }
            _ => Err(MiniDbError::Type("MOD requires integers".to_string())),
        },
        BinOp::Concat => lv.concat(rv),
        BinOp::Eq => {
            let t = lv.sql_eq(rv);
            match t {
                Tribool::True => Ok(Value::Bool(true)),
                Tribool::False => Ok(Value::Bool(false)),
                Tribool::Unknown => Ok(Value::Null),
            }
        }
        BinOp::Ne => {
            let t = lv.sql_eq(rv);
            match t {
                Tribool::True => Ok(Value::Bool(false)),
                Tribool::False => Ok(Value::Bool(true)),
                Tribool::Unknown => Ok(Value::Null),
            }
        }
        BinOp::Lt => cmp_op(lv, rv, |o| o == std::cmp::Ordering::Less),
        BinOp::Le => cmp_op(lv, rv, |o| o != std::cmp::Ordering::Greater),
        BinOp::Gt => cmp_op(lv, rv, |o| o == std::cmp::Ordering::Greater),
        BinOp::Ge => cmp_op(lv, rv, |o| o != std::cmp::Ordering::Less),
        BinOp::And => {
            let lt = lv.is_truthy();
            let rt = rv.is_truthy();
            let result = lt.and(rt);
            Ok(tribool_to_value(result))
        }
        BinOp::Or => {
            let lt = lv.is_truthy();
            let rt = rv.is_truthy();
            let result = lt.or(rt);
            Ok(tribool_to_value(result))
        }
    }
}

fn cmp_op(lv: &Value, rv: &Value, pred: impl Fn(std::cmp::Ordering) -> bool) -> Result<Value> {
    match lv.sql_cmp(rv) {
        None => Ok(Value::Null),
        Some(ord) => Ok(Value::Bool(pred(ord))),
    }
}

fn tribool_to_value(t: Tribool) -> Value {
    match t {
        Tribool::True => Value::Bool(true),
        Tribool::False => Value::Bool(false),
        Tribool::Unknown => Value::Null,
    }
}

fn eval_function(name: &str, args: &[Expr], ctx: &EvalContext) -> Result<Value> {
    let eval_args: Result<Vec<Value>> = args.iter().map(|a| eval_expr(a, ctx)).collect();
    let args = eval_args?;

    match name.to_uppercase().as_str() {
        "LENGTH" => match args.first() {
            Some(Value::Null) | None => Ok(Value::Null),
            Some(Value::Text(s)) => Ok(Value::Int(s.len() as i64)),
            Some(v) => Ok(Value::Int(v.to_display_string().len() as i64)),
        },
        "LOWER" => match args.first() {
            Some(Value::Null) | None => Ok(Value::Null),
            Some(Value::Text(s)) => Ok(Value::Text(s.to_lowercase())),
            Some(v) => Ok(Value::Text(v.to_display_string().to_lowercase())),
        },
        "UPPER" => match args.first() {
            Some(Value::Null) | None => Ok(Value::Null),
            Some(Value::Text(s)) => Ok(Value::Text(s.to_uppercase())),
            Some(v) => Ok(Value::Text(v.to_display_string().to_uppercase())),
        },
        "SUBSTR" | "SUBSTRING" => match args.as_slice() {
            [Value::Null, ..] => Ok(Value::Null),
            [Value::Text(s), Value::Int(start), rest @ ..] => {
                let start = (*start).max(1) as usize - 1;
                let chars: Vec<char> = s.chars().collect();
                if start >= chars.len() {
                    return Ok(Value::Text(String::new()));
                }
                let len = if let Some(Value::Int(n)) = rest.first() {
                    (*n as usize).min(chars.len() - start)
                } else {
                    chars.len() - start
                };
                Ok(Value::Text(chars[start..start + len].iter().collect()))
            }
            _ => Err(MiniDbError::Type(
                "SUBSTR expects (text, int[, int])".to_string(),
            )),
        },
        "ABS" => match args.first() {
            Some(Value::Null) | None => Ok(Value::Null),
            Some(Value::Int(n)) => Ok(Value::Int(n.abs())),
            Some(Value::Real(r)) => Ok(Value::Real(r.abs())),
            _ => Err(MiniDbError::Type("ABS expects numeric".to_string())),
        },
        "COALESCE" => {
            for arg in &args {
                if !arg.is_null() {
                    return Ok(arg.clone());
                }
            }
            Ok(Value::Null)
        }
        _ => Err(MiniDbError::Runtime(format!("unknown function: {}", name))),
    }
}

fn like_match(s: &str, pattern: &str) -> bool {
    // Simple LIKE: % matches any sequence, _ matches one char
    let s: Vec<char> = s.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    like_match_chars(&s, &p)
}

fn like_match_chars(s: &[char], p: &[char]) -> bool {
    match (s, p) {
        (_, []) => s.is_empty(),
        (_, ['%', rest @ ..]) => {
            for i in 0..=s.len() {
                if like_match_chars(&s[i..], rest) {
                    return true;
                }
            }
            false
        }
        ([], _) => false,
        ([sc, s_rest @ ..], ['_', p_rest @ ..]) => like_match_chars(s_rest, p_rest),
        ([sc, s_rest @ ..], [pc, p_rest @ ..]) => sc == pc && like_match_chars(s_rest, p_rest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::EvalContext;
    use crate::parser::ast::Expr;
    use crate::types::Value;

    fn ctx_from_pairs(pairs: &[(&str, Value)]) -> EvalContext {
        let mut ctx = EvalContext::default();
        for (k, v) in pairs {
            ctx.columns.insert(k.to_string(), v.clone());
        }
        ctx
    }

    #[test]
    fn test_eval_arithmetic() {
        let ctx = EvalContext::default();
        let expr = Expr::BinOp(
            Box::new(Expr::Lit(Value::Int(3))),
            BinOp::Add,
            Box::new(Expr::Lit(Value::Int(4))),
        );
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), Value::Int(7));
    }

    #[test]
    fn test_eval_coalesce() {
        let ctx = EvalContext::default();
        let expr = Expr::Function(
            "COALESCE".to_string(),
            vec![Expr::Lit(Value::Null), Expr::Lit(Value::Int(42))],
        );
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_eval_length() {
        let ctx = EvalContext::default();
        let expr = Expr::Function(
            "LENGTH".to_string(),
            vec![Expr::Lit(Value::Text("hello".to_string()))],
        );
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_eval_column_lookup() {
        let ctx = ctx_from_pairs(&[("x", Value::Int(10))]);
        let expr = Expr::Col("x".to_string(), None);
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), Value::Int(10));
    }

    #[test]
    fn test_eval_and_or() {
        let ctx = EvalContext::default();
        let t = Expr::Lit(Value::Bool(true));
        let f = Expr::Lit(Value::Bool(false));
        let n = Expr::Lit(Value::Null);

        let and_expr = Expr::BinOp(Box::new(t.clone()), BinOp::And, Box::new(f.clone()));
        assert_eq!(eval_expr(&and_expr, &ctx).unwrap(), Value::Bool(false));

        let or_expr = Expr::BinOp(Box::new(f.clone()), BinOp::Or, Box::new(t.clone()));
        assert_eq!(eval_expr(&or_expr, &ctx).unwrap(), Value::Bool(true));

        let null_and = Expr::BinOp(Box::new(n.clone()), BinOp::And, Box::new(f.clone()));
        assert_eq!(eval_expr(&null_and, &ctx).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_like_match() {
        assert!(like_match("hello", "h%lo"));
        assert!(like_match("hello", "h_llo"));
        assert!(!like_match("hello", "world"));
        assert!(like_match("abc", "%"));
    }
}
