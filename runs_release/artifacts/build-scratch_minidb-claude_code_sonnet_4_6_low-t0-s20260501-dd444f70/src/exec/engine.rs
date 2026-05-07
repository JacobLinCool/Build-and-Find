use crate::error::{MiniDbError, Result};
use crate::exec::filter::eval_expr;
use crate::exec::{
    Delete, EvalContext, Filter, IndexScan, Insert, Limit, NestedLoopJoin, PhysicalOperator,
    Project, Schema, SeqScan, Sort, Update,
};
use crate::parser::ast::Stmt;
use crate::plan::physical::{IndexPredicate, PhysicalPlan};
use crate::plan::Planner;
use crate::storage::StorageEngine;
use crate::txn::TxnManager;
use crate::types::{Row, Value};

pub struct ExecResult {
    pub schema: Schema,
    pub rows: Vec<Row>,
    pub rows_affected: Option<i64>,
    pub summary: String,
}

pub struct Executor {
    pub engine: StorageEngine,
    pub txn: TxnManager,
    in_explicit_txn: bool,
}

impl Executor {
    pub fn new(engine: StorageEngine) -> Self {
        Executor {
            engine,
            txn: TxnManager::new(),
            in_explicit_txn: false,
        }
    }

    pub fn execute(&mut self, stmt: &Stmt) -> Result<ExecResult> {
        match stmt {
            Stmt::Begin => {
                self.txn.begin(&mut self.engine)?;
                self.in_explicit_txn = true;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::Commit => {
                self.txn.commit(&mut self.engine)?;
                self.in_explicit_txn = false;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::Rollback => {
                self.txn.rollback(&mut self.engine)?;
                self.in_explicit_txn = false;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::CreateTable(ct) => {
                let auto = self.auto_begin()?;
                let result = self.engine.create_table(ct);
                self.auto_end(auto, result.is_ok())?;
                result?;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: format!("ok table '{}' created", ct.table),
                })
            }
            Stmt::DropTable(dt) => {
                let auto = self.auto_begin()?;
                let result = self.engine.drop_table(&dt.table, dt.if_exists);
                self.auto_end(auto, result.is_ok())?;
                result?;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: format!("ok table '{}' dropped", dt.table),
                })
            }
            Stmt::CreateIndex(ci) => {
                let auto = self.auto_begin()?;
                let result = self.engine.create_index(ci);
                self.auto_end(auto, result.is_ok())?;
                result?;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: format!("ok index '{}' created", ci.index_name),
                })
            }
            Stmt::DropIndex(di) => {
                let auto = self.auto_begin()?;
                let result = self.engine.drop_index(&di.index_name, di.if_exists);
                self.auto_end(auto, result.is_ok())?;
                result?;
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: format!("ok index '{}' dropped", di.index_name),
                })
            }
            Stmt::MetaSchema => {
                let schema = vec![crate::exec::SchemaEntry {
                    name: "schema".to_string(),
                    table: None,
                }];
                let mut rows = Vec::new();
                for (name, table) in &self.engine.catalog.data.tables {
                    let mut s = format!("CREATE TABLE {} (\n", table.name);
                    let col_defs: Vec<String> = table
                        .columns
                        .iter()
                        .map(|c| {
                            let mut def = format!("  {} {}", c.name, c.data_type);
                            if c.primary_key {
                                def += " PRIMARY KEY";
                            }
                            if !c.nullable && !c.primary_key {
                                def += " NOT NULL";
                            }
                            if c.unique && !c.primary_key {
                                def += " UNIQUE";
                            }
                            if let Some(ref d) = c.default {
                                def += &format!(" DEFAULT {}", d);
                            }
                            def
                        })
                        .collect();
                    s += &col_defs.join(",\n");
                    s += "\n);";
                    rows.push(vec![Value::Text(s)]);
                }
                for (name, idx) in &self.engine.catalog.data.indexes {
                    let s = format!(
                        "CREATE {}INDEX {} ON {} ({});",
                        if idx.unique { "UNIQUE " } else { "" },
                        idx.name,
                        idx.table,
                        idx.column
                    );
                    rows.push(vec![Value::Text(s)]);
                }
                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::MetaTables => {
                let schema = vec![crate::exec::SchemaEntry {
                    name: "table_name".to_string(),
                    table: None,
                }];
                let rows: Vec<Row> = self
                    .engine
                    .catalog
                    .data
                    .tables
                    .values()
                    .map(|t| vec![Value::Text(t.name.clone())])
                    .collect();
                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::MetaIndexes => {
                let schema = vec![
                    crate::exec::SchemaEntry {
                        name: "index_name".to_string(),
                        table: None,
                    },
                    crate::exec::SchemaEntry {
                        name: "table_name".to_string(),
                        table: None,
                    },
                    crate::exec::SchemaEntry {
                        name: "column".to_string(),
                        table: None,
                    },
                ];
                let rows: Vec<Row> = self
                    .engine
                    .catalog
                    .data
                    .indexes
                    .values()
                    .map(|i| {
                        vec![
                            Value::Text(i.name.clone()),
                            Value::Text(i.table.clone()),
                            Value::Text(i.column.clone()),
                        ]
                    })
                    .collect();
                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::MetaExit => Err(MiniDbError::Runtime("__exit__".to_string())),
            Stmt::MetaFormat(_) => {
                // handled by cli layer
                Ok(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: "ok".to_string(),
                })
            }
            Stmt::Select(_) | Stmt::Insert(_) | Stmt::Update(_) | Stmt::Delete(_) => {
                let planner = Planner::new(&self.engine.catalog);
                let physical_plan = planner.plan_stmt(stmt)?;

                let auto = self.auto_begin()?;
                let eng_ptr: *mut StorageEngine = &mut self.engine;
                let mut op = build_operator(physical_plan, eng_ptr)?;

                op.open()?;
                let schema = op.schema().to_vec();
                let mut rows = Vec::new();
                while let Some(row) = op.next()? {
                    rows.push(row);
                }
                op.close()?;

                self.auto_end(auto, true)?;

                let rows_affected = match stmt {
                    Stmt::Insert(_) | Stmt::Update(_) | Stmt::Delete(_) => {
                        rows.first().and_then(|r| r.first()).and_then(|v| {
                            if let Value::Int(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                    }
                    _ => None,
                };

                let summary = match rows_affected {
                    Some(n) => {
                        if n == 1 {
                            "ok 1 row affected".to_string()
                        } else {
                            format!("ok {} rows affected", n)
                        }
                    }
                    None => {
                        if rows.len() == 1 {
                            "ok 1 row".to_string()
                        } else {
                            format!("ok {} rows", rows.len())
                        }
                    }
                };

                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected,
                    summary,
                })
            }
        }
    }

    fn auto_begin(&mut self) -> Result<bool> {
        if !self.in_explicit_txn {
            self.txn.ensure_active(&mut self.engine)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn auto_end(&mut self, auto: bool, success: bool) -> Result<()> {
        if auto {
            if success {
                self.txn.auto_commit(&mut self.engine)?;
            } else {
                if self.txn.is_active() {
                    let _ = self.txn.rollback(&mut self.engine);
                }
            }
        }
        Ok(())
    }
}

fn build_operator(
    plan: PhysicalPlan,
    engine: *mut StorageEngine,
) -> Result<Box<dyn PhysicalOperator>> {
    Ok(match plan {
        PhysicalPlan::SeqScan { table, alias } => Box::new(SeqScan::new(table, alias, engine)?),
        PhysicalPlan::IndexScan {
            table,
            alias,
            index_name,
            predicate,
        } => Box::new(IndexScan::new(table, alias, index_name, predicate, engine)?),
        PhysicalPlan::Filter { input, predicate } => {
            let input_op = build_operator(*input, engine)?;
            Box::new(Filter::new(input_op, predicate))
        }
        PhysicalPlan::Project { input, projections } => {
            let input_op = build_operator(*input, engine)?;
            Box::new(Project::new(input_op, projections))
        }
        PhysicalPlan::NestedLoopJoin {
            left,
            right,
            condition,
        } => {
            let left_op = build_operator(*left, engine)?;
            let right_op = build_operator(*right, engine)?;
            Box::new(NestedLoopJoin::new(left_op, right_op, condition))
        }
        PhysicalPlan::Sort { input, keys } => {
            let input_op = build_operator(*input, engine)?;
            Box::new(Sort::new(input_op, keys))
        }
        PhysicalPlan::Limit {
            input,
            limit,
            offset,
        } => {
            let input_op = build_operator(*input, engine)?;
            Box::new(Limit::new(input_op, limit, offset))
        }
        PhysicalPlan::Insert {
            table,
            columns,
            rows,
        } => Box::new(Insert::new(table, columns, rows, engine)),
        PhysicalPlan::Update {
            table,
            input,
            assignments,
        } => {
            let input_op = build_operator(*input, engine)?;
            Box::new(Update::new(table, input_op, assignments, engine))
        }
        PhysicalPlan::Delete { table, input } => {
            let input_op = build_operator(*input, engine)?;
            Box::new(Delete::new(table, input_op, engine))
        }
    })
}
