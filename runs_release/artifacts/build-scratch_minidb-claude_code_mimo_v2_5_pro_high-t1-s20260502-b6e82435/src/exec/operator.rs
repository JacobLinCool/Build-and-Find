use crate::error::Result;
use crate::types::{Schema, Value};

/// Volcano-style physical operator trait.
pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Vec<Value>>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> &Schema;
    fn rows_affected(&self) -> u64 {
        0
    }
}

/// Sequential scan operator
pub struct SeqScan {
    pub table: String,
    pub schema: Schema,
    pub rows: Vec<Vec<Value>>,
    pub pos: usize,
    pub filter: Option<crate::plan::CompiledExpr>,
    pub alias: Option<String>,
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        loop {
            if self.pos >= self.rows.len() {
                return Ok(None);
            }
            let row = self.rows[self.pos].clone();
            self.pos += 1;

            if let Some(filter) = &self.filter {
                let result = eval_expr(filter, &row, &self.schema)?;
                match result {
                    Value::Bool(true) => return Ok(Some(row)),
                    Value::Bool(false) | Value::Null => continue,
                    _ => continue,
                }
            }

            return Ok(Some(row));
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Index scan operator
pub struct IndexScan {
    pub table: String,
    pub schema: Schema,
    pub row_ids: Vec<u64>,
    pub rows: Vec<Vec<Value>>,
    pub pos: usize,
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Filter operator
pub struct Filter {
    pub input: Box<dyn PhysicalOperator>,
    pub predicate: crate::plan::CompiledExpr,
    pub schema: Schema,
}

impl PhysicalOperator for Filter {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        loop {
            match self.input.next()? {
                None => return Ok(None),
                Some(row) => {
                    let result = eval_expr(&self.predicate, &row, self.input.schema())?;
                    match result {
                        Value::Bool(true) => return Ok(Some(row)),
                        Value::Bool(false) | Value::Null => continue,
                        _ => continue,
                    }
                }
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Projection operator
pub struct Project {
    pub input: Box<dyn PhysicalOperator>,
    pub exprs: Vec<(crate::plan::CompiledExpr, Option<String>)>,
    pub schema: Schema,
}

impl PhysicalOperator for Project {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        match self.input.next()? {
            None => Ok(None),
            Some(row) => {
                let mut result = Vec::new();
                for (expr, _) in &self.exprs {
                    let val = eval_expr(expr, &row, self.input.schema())?;
                    result.push(val);
                }
                Ok(Some(result))
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Nested loop join operator
pub struct NestedLoopJoin {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    pub condition: crate::plan::CompiledExpr,
    pub schema: Schema,
    pub left_row: Option<Vec<Value>>,
    pub right_rows: Vec<Vec<Value>>,
    pub right_pos: usize,
    pub exhausted: bool,
    pub join_type: crate::plan::JoinType,
    pub matched_left: bool,
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        self.right.open()?;
        // Pre-load all right rows
        loop {
            match self.right.next()? {
                None => break,
                Some(row) => self.right_rows.push(row),
            }
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        loop {
            // Try next right row with current left row
            if let Some(left_row) = &self.left_row {
                while self.right_pos < self.right_rows.len() {
                    let right_row = &self.right_rows[self.right_pos];
                    self.right_pos += 1;

                    let mut combined = left_row.clone();
                    combined.extend(right_row.clone());

                    let result = eval_expr(&self.condition, &combined, &self.schema)?;

                    if matches!(result, Value::Bool(true)) {
                        self.matched_left = true;
                        return Ok(Some(combined));
                    }
                }

                // Exhausted right rows for this left row
                // For LEFT JOIN, emit left row + NULLs if no match
                if self.join_type == crate::plan::JoinType::Left && !self.matched_left {
                    let right_len = if !self.right_rows.is_empty() {
                        self.right_rows[0].len()
                    } else {
                        // Figure out right column count from schema vs left schema
                        let left_len = left_row.len();
                        self.schema.columns.len() - left_len
                    };
                    let mut combined = left_row.clone();
                    for _ in 0..right_len {
                        combined.push(Value::Null);
                    }
                    self.left_row = None;
                    return Ok(Some(combined));
                }
                self.left_row = None;
            }

            // Get next left row
            match self.left.next()? {
                None => return Ok(None),
                Some(row) => {
                    self.left_row = Some(row);
                    self.right_pos = 0;
                    self.matched_left = false;
                }
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.left.close()?;
        self.right.close()
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Sort operator
pub struct Sort {
    pub input: Box<dyn PhysicalOperator>,
    pub keys: Vec<(crate::plan::CompiledExpr, bool)>,
    pub schema: Schema,
    pub sorted_rows: Vec<Vec<Value>>,
    pub pos: usize,
}

impl PhysicalOperator for Sort {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        // Collect all rows
        loop {
            match self.input.next()? {
                None => break,
                Some(row) => self.sorted_rows.push(row),
            }
        }
        self.input.close()?;

        // Sort
        let keys = self.keys.clone();
        let input_schema = self.schema.clone();
        self.sorted_rows.sort_by(|a, b| {
            for (key_expr, ascending) in &keys {
                let val_a = eval_expr(key_expr, a, &input_schema).unwrap_or(Value::Null);
                let val_b = eval_expr(key_expr, b, &input_schema).unwrap_or(Value::Null);

                let cmp = val_a
                    .partial_cmp(&val_b)
                    .unwrap_or(std::cmp::Ordering::Equal);
                if cmp != std::cmp::Ordering::Equal {
                    return if *ascending { cmp } else { cmp.reverse() };
                }
            }
            std::cmp::Ordering::Equal
        });

        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        if self.pos >= self.sorted_rows.len() {
            return Ok(None);
        }
        let row = self.sorted_rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Limit operator
pub struct Limit {
    pub input: Box<dyn PhysicalOperator>,
    pub count: usize,
    pub offset: usize,
    pub schema: Schema,
    pub consumed: usize,
    pub skipped: bool,
}

impl PhysicalOperator for Limit {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        // Skip offset rows first
        if !self.skipped {
            for _ in 0..self.offset {
                if self.input.next()?.is_none() {
                    return Ok(None);
                }
            }
            self.skipped = true;
        }

        if self.consumed >= self.count {
            return Ok(None);
        }

        match self.input.next()? {
            None => Ok(None),
            Some(row) => {
                self.consumed += 1;
                Ok(Some(row))
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Insert operator
pub struct InsertOp {
    pub table: String,
    pub schema: Schema,
    pub rows: Vec<Vec<Value>>,
    pub rows_affected: u64,
}

impl PhysicalOperator for InsertOp {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

/// Update operator
pub struct UpdateOp {
    pub table: String,
    pub schema: Schema,
    pub rows_affected: u64,
}

impl PhysicalOperator for UpdateOp {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

/// Delete operator
pub struct DeleteOp {
    pub table: String,
    pub schema: Schema,
    pub rows_affected: u64,
}

impl PhysicalOperator for DeleteOp {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

/// Evaluate a compiled expression against a row.
pub fn eval_expr(
    expr: &crate::plan::CompiledExpr,
    row: &[Value],
    schema: &Schema,
) -> Result<Value> {
    match expr {
        crate::plan::CompiledExpr::Literal(v) => Ok(v.clone()),
        crate::plan::CompiledExpr::ColumnRef { table, name, index } => {
            if let Some(idx) = index {
                if *idx < row.len() {
                    return Ok(row[*idx].clone());
                }
            }
            // Try qualified name first (table.column), then unqualified
            if let Some(tbl) = table {
                let qualified = format!("{}.{}", tbl, name);
                if let Some(idx) = schema.column_index(&qualified) {
                    if idx < row.len() {
                        return Ok(row[idx].clone());
                    }
                }
            }
            // Try by name
            if let Some(idx) = schema.column_index(name) {
                if idx < row.len() {
                    return Ok(row[idx].clone());
                }
            }
            Ok(Value::Null)
        }
        crate::plan::CompiledExpr::BinaryOp { left, op, right } => {
            let l = eval_expr(left, row, schema)?;
            let r = eval_expr(right, row, schema)?;
            eval_binary_op(*op, &l, &r)
        }
        crate::plan::CompiledExpr::UnaryOp { op, expr } => {
            let val = eval_expr(expr, row, schema)?;
            match op {
                crate::parser::ast::UnaryOp::Not => Ok(val.not()),
                crate::parser::ast::UnaryOp::Neg => match val {
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Real(r) => Ok(Value::Real(-r)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(crate::error::MiniDbError::Type(
                        "cannot negate non-numeric".into(),
                    )),
                },
            }
        }
        crate::plan::CompiledExpr::FunctionCall { name, args } => {
            let arg_values: Vec<Value> = args
                .iter()
                .map(|a| eval_expr(a, row, schema))
                .collect::<Result<Vec<_>>>()?;
            eval_function(name, &arg_values)
        }
        crate::plan::CompiledExpr::IsNull(expr) => {
            let val = eval_expr(expr, row, schema)?;
            Ok(val.is_null_check())
        }
        crate::plan::CompiledExpr::IsNotNull(expr) => {
            let val = eval_expr(expr, row, schema)?;
            Ok(val.is_not_null_check())
        }
        crate::plan::CompiledExpr::Coalesce(args) => {
            for arg in args {
                let val = eval_expr(arg, row, schema)?;
                if !val.is_null() {
                    return Ok(val);
                }
            }
            Ok(Value::Null)
        }
        crate::plan::CompiledExpr::Case {
            when_clauses,
            else_clause,
        } => {
            for (cond, result) in when_clauses {
                let cond_val = eval_expr(cond, row, schema)?;
                if matches!(cond_val, Value::Bool(true)) {
                    return eval_expr(result, row, schema);
                }
            }
            if let Some(else_expr) = else_clause {
                eval_expr(else_expr, row, schema)
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn eval_binary_op(op: crate::parser::ast::BinaryOp, left: &Value, right: &Value) -> Result<Value> {
    use crate::parser::ast::BinaryOp::*;
    match op {
        Eq => Ok(left.sql_eq(right)),
        Ne => Ok(left.sql_ne(right)),
        Lt => Ok(left.sql_lt(right)),
        Le => Ok(left.sql_le(right)),
        Gt => Ok(left.sql_gt(right)),
        Ge => Ok(left.sql_ge(right)),
        Add => left.sql_add(right),
        Sub => left.sql_sub(right),
        Mul => left.sql_mul(right),
        Div => left.sql_div(right),
        Mod => match (left, right) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(crate::error::MiniDbError::Runtime(
                        "division by zero".into(),
                    ))
                } else {
                    Ok(Value::Int(a % b))
                }
            }
            _ => Err(crate::error::MiniDbError::Type(
                "modulo requires integers".into(),
            )),
        },
        And => Ok(left.and(right)),
        Or => Ok(left.or(right)),
        Concat => Ok(left.sql_concat(right)),
        Like => {
            // Simple LIKE implementation: % matches any, _ matches one char
            match (left, right) {
                (Value::Text(s), Value::Text(pattern)) => Ok(Value::Bool(like_match(s, pattern))),
                _ => Ok(Value::Null),
            }
        }
    }
}

fn like_match(s: &str, pattern: &str) -> bool {
    let s_chars: Vec<char> = s.chars().collect();
    let p_chars: Vec<char> = pattern.chars().collect();
    like_match_chars(&s_chars, &p_chars)
}

fn like_match_chars(s: &[char], p: &[char]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if p[0] == '%' {
        // % matches zero or more chars
        for i in 0..=s.len() {
            if like_match_chars(&s[i..], &p[1..]) {
                return true;
            }
        }
        false
    } else if p[0] == '_' {
        // _ matches exactly one char
        if s.is_empty() {
            return false;
        }
        like_match_chars(&s[1..], &p[1..])
    } else {
        if s.is_empty() || s[0] != p[0] {
            return false;
        }
        like_match_chars(&s[1..], &p[1..])
    }
}

fn eval_function(name: &str, args: &[Value]) -> Result<Value> {
    match name.to_uppercase().as_str() {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(crate::error::MiniDbError::Runtime(
                    "LENGTH takes 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Err(crate::error::MiniDbError::Type(
                    "LENGTH requires TEXT argument".into(),
                )),
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Err(crate::error::MiniDbError::Runtime(
                    "LOWER takes 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                Value::Null => Ok(Value::Null),
                _ => Err(crate::error::MiniDbError::Type(
                    "LOWER requires TEXT argument".into(),
                )),
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Err(crate::error::MiniDbError::Runtime(
                    "UPPER takes 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                Value::Null => Ok(Value::Null),
                _ => Err(crate::error::MiniDbError::Type(
                    "UPPER requires TEXT argument".into(),
                )),
            }
        }
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(crate::error::MiniDbError::Runtime(
                    "SUBSTR takes 2 or 3 arguments".into(),
                ));
            }
            match (&args[0], &args[1]) {
                (Value::Text(s), Value::Int(start)) => {
                    let start = (*start).max(0) as usize;
                    let len = if args.len() == 3 {
                        match &args[2] {
                            Value::Int(l) => (*l).max(0) as usize,
                            _ => s.len(),
                        }
                    } else {
                        s.len()
                    };
                    let chars: Vec<char> = s.chars().collect();
                    let end = (start + len).min(chars.len());
                    if start >= chars.len() {
                        Ok(Value::Text(String::new()))
                    } else {
                        Ok(Value::Text(chars[start..end].iter().collect()))
                    }
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(crate::error::MiniDbError::Type(
                    "SUBSTR requires (TEXT, INT) arguments".into(),
                )),
            }
        }
        "ABS" => {
            if args.len() != 1 {
                return Err(crate::error::MiniDbError::Runtime(
                    "ABS takes 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Int(i) => Ok(Value::Int(i.abs())),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                Value::Null => Ok(Value::Null),
                _ => Err(crate::error::MiniDbError::Type(
                    "ABS requires numeric argument".into(),
                )),
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
        "COUNT" => {
            // COUNT is handled specially in aggregation
            Ok(Value::Int(args.len() as i64))
        }
        "SUM" | "MIN" | "MAX" | "AVG" => {
            // Aggregate functions: return the argument value as-is for per-row evaluation
            // Actual aggregation is handled in execute_select
            if args.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(args[0].clone())
            }
        }
        _ => Err(crate::error::MiniDbError::Runtime(format!(
            "unknown function: {}",
            name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnDef, ColumnType};

    fn test_schema() -> Schema {
        Schema::new(vec![
            ColumnDef {
                name: "id".into(),
                col_type: ColumnType::Int,
                constraints: vec![],
            },
            ColumnDef {
                name: "name".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
        ])
    }

    #[test]
    fn test_seq_scan() {
        let schema = test_schema();
        let rows = vec![
            vec![Value::Int(1), Value::Text("Alice".into())],
            vec![Value::Int(2), Value::Text("Bob".into())],
        ];

        let mut scan = SeqScan {
            table: "users".into(),
            schema,
            rows,
            pos: 0,
            filter: None,
            alias: None,
        };

        scan.open().unwrap();
        assert_eq!(
            scan.next().unwrap(),
            Some(vec![Value::Int(1), Value::Text("Alice".into())])
        );
        assert_eq!(
            scan.next().unwrap(),
            Some(vec![Value::Int(2), Value::Text("Bob".into())])
        );
        assert!(scan.next().unwrap().is_none());
        scan.close().unwrap();
    }

    #[test]
    fn test_eval_binary_op() {
        assert_eq!(
            eval_binary_op(
                crate::parser::ast::BinaryOp::Add,
                &Value::Int(1),
                &Value::Int(2)
            )
            .unwrap(),
            Value::Int(3)
        );
        assert_eq!(
            eval_binary_op(
                crate::parser::ast::BinaryOp::Eq,
                &Value::Int(1),
                &Value::Int(1)
            )
            .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_eval_function() {
        assert_eq!(
            eval_function("LENGTH", &[Value::Text("hello".into())]).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            eval_function("LOWER", &[Value::Text("HELLO".into())]).unwrap(),
            Value::Text("hello".into())
        );
        assert_eq!(
            eval_function("ABS", &[Value::Int(-42)]).unwrap(),
            Value::Int(42)
        );
        assert_eq!(
            eval_function("COALESCE", &[Value::Null, Value::Int(42)]).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn test_like_match() {
        assert!(like_match("hello", "h%o"));
        assert!(like_match("hello", "h_llo"));
        assert!(!like_match("hello", "h_l"));
        assert!(like_match("hello", "%"));
        assert!(like_match("hello", "hello"));
    }
}
