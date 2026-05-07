//! High-level database engine: opens the database, drives transactions,
//! applies the WAL on recovery, and executes statements end-to-end.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use log::{debug, info};

use crate::catalog::{Catalog, ColumnSchema, IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::exec::{build_operator, ExecCtx, OperatorSchema, Row};
use crate::parser::{ast, parse};
use crate::plan::{build_logical, build_physical, LogicalPlan, PhysicalPlan};
use crate::storage::btree::PersistentBTree;
use crate::storage::pager::Pager;
use crate::storage::wal::{replay, Wal, WalRecord};
use crate::storage::PageId;
use crate::txn::TxnManager;
use crate::types::Value;

/// Default file names placed inside the database directory.
const CATALOG_FILE: &str = "catalog.json";
const DATA_FILE: &str = "data";
const WAL_FILE: &str = "wal.log";
const INDEX_DIR: &str = "indexes";

#[derive(Debug)]
pub enum ExecResult {
    Query {
        schema: OperatorSchema,
        rows: Vec<Row>,
    },
    Affected {
        verb: AffectedVerb,
        count: i64,
    },
    Ok,
    Begin,
    Commit,
    Rollback,
    Meta(Vec<(String, Value)>),
    SchemaList(Vec<(String, String)>), // (table, column-or-detail)
    TableList(Vec<String>),
    IndexList(Vec<(String, String, String)>), // (index, table, column)
    Exit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AffectedVerb {
    Inserted,
    Updated,
    Deleted,
}

pub struct Engine {
    pub root: PathBuf,
    pub catalog: Catalog,
    pub indexes: HashMap<String, PersistentBTree>,
    pager: Pager,
    wal: Wal,
    txn: TxnManager,
    catalog_dirty: bool,
    indexes_dirty: HashSet<String>,
    catalog_snapshot_for_txn: Option<Catalog>,
}

impl Engine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        if root.exists() && root.is_file() {
            return Err(MiniDbError::storage(format!(
                "database path {:?} is a regular file; expected a directory",
                root
            )));
        }
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join(INDEX_DIR))?;

        // Recover from WAL before opening the pager so that pages are correct.
        Self::recover(&root)?;

        let pager = Pager::open(root.join(DATA_FILE))?;
        let catalog = Self::load_catalog(&root)?;
        let mut indexes = HashMap::new();
        for idx in catalog.indexes.values() {
            let p = root.join(INDEX_DIR).join(format!("{}.bin", idx.name));
            indexes.insert(idx.name.to_ascii_lowercase(), PersistentBTree::open(&p)?);
        }
        let wal = Wal::open(root.join(WAL_FILE))?;
        Ok(Self {
            root,
            catalog,
            indexes,
            pager,
            wal,
            txn: TxnManager::new(),
            catalog_dirty: false,
            indexes_dirty: HashSet::new(),
            catalog_snapshot_for_txn: None,
        })
    }

    pub fn create_empty(path: impl AsRef<Path>) -> Result<()> {
        let root = path.as_ref().to_path_buf();
        if root.exists() && root.is_file() {
            return Err(MiniDbError::storage(format!(
                "cannot init: {:?} is a file",
                root
            )));
        }
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join(INDEX_DIR))?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(root.join(DATA_FILE))?;
        // Empty catalog
        let catalog = Catalog::new();
        let bytes = catalog.to_json()?;
        let cat_path = root.join(CATALOG_FILE);
        let tmp = cat_path.with_extension("tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &cat_path)?;
        // Empty WAL
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(root.join(WAL_FILE))?;
        Ok(())
    }

    fn load_catalog(root: &Path) -> Result<Catalog> {
        let p = root.join(CATALOG_FILE);
        if !p.exists() {
            return Ok(Catalog::new());
        }
        let bytes = std::fs::read(&p)?;
        Catalog::from_json(&bytes)
    }

    fn save_catalog(&self) -> Result<()> {
        let p = self.root.join(CATALOG_FILE);
        let tmp = p.with_extension("tmp");
        let bytes = self.catalog.to_json()?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    fn recover(root: &Path) -> Result<()> {
        let wal_path = root.join(WAL_FILE);
        if !wal_path.exists() {
            return Ok(());
        }
        let mut wal = Wal::open(&wal_path)?;
        let recs = wal.read_all()?;
        if recs.is_empty() {
            return Ok(());
        }
        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(root.join(DATA_FILE))?;
        if let Some(cat_bytes) = replay(recs, &mut data)? {
            let cat_path = root.join(CATALOG_FILE);
            let tmp = cat_path.with_extension("tmp");
            std::fs::write(&tmp, &cat_bytes)?;
            std::fs::rename(&tmp, &cat_path)?;
            info!("WAL replay applied catalog snapshot");
        }
        wal.truncate()?;
        Ok(())
    }

    pub fn execute(&mut self, sql: &str) -> Result<Vec<ExecResult>> {
        let stmts = parse(sql)?;
        let mut out = Vec::new();
        for stmt in stmts {
            out.push(self.execute_one(stmt)?);
        }
        Ok(out)
    }

    pub fn execute_one(&mut self, stmt: ast::Statement) -> Result<ExecResult> {
        // Meta commands handled outside of plan/exec.
        if let ast::Statement::Meta(m) = &stmt {
            return self.exec_meta(m);
        }
        // Transaction control.
        match &stmt {
            ast::Statement::Begin => return self.do_begin(),
            ast::Statement::Commit => return self.do_commit(),
            ast::Statement::Rollback => return self.do_rollback(),
            _ => {}
        }
        // For all other statements: ensure a transaction is active (auto-commit).
        let auto = !self.txn.is_active();
        if auto {
            self.do_begin_implicit()?;
        }
        let result = self.execute_in_txn(stmt);
        match result {
            Ok(r) => {
                if auto {
                    self.do_commit_internal()?;
                }
                Ok(r)
            }
            Err(e) => {
                if auto {
                    let _ = self.do_rollback_internal();
                }
                Err(e)
            }
        }
    }

    fn execute_in_txn(&mut self, stmt: ast::Statement) -> Result<ExecResult> {
        let logical = build_logical(stmt)?;
        // Handle DDL directly (avoids re-entering the executor).
        match logical {
            LogicalPlan::CreateTable(ct) => self.exec_create_table(ct),
            LogicalPlan::DropTable(dt) => self.exec_drop_table(dt),
            LogicalPlan::CreateIndex(ci) => self.exec_create_index(ci),
            LogicalPlan::DropIndex(di) => self.exec_drop_index(di),
            other => {
                let phys = build_physical(other, &self.catalog)?;
                self.run_physical(phys)
            }
        }
    }

    fn run_physical(&mut self, plan: PhysicalPlan) -> Result<ExecResult> {
        let verb = match &plan {
            PhysicalPlan::Insert { .. } => Some(AffectedVerb::Inserted),
            PhysicalPlan::Update { .. } => Some(AffectedVerb::Updated),
            PhysicalPlan::Delete { .. } => Some(AffectedVerb::Deleted),
            _ => None,
        };
        let mut catalog_dirty = self.catalog_dirty;
        let mut indexes_dirty = std::mem::take(&mut self.indexes_dirty);
        let result = {
            let mut ctx = ExecCtx {
                pager: &mut self.pager,
                catalog: &mut self.catalog,
                indexes: &mut self.indexes,
                catalog_dirty: &mut catalog_dirty,
                indexes_dirty: &mut indexes_dirty,
            };
            let mut op = build_operator(plan, &ctx)?;
            op.open(&mut ctx)?;
            let mut rows: Vec<Row> = Vec::new();
            while let Some(r) = op.next(&mut ctx)? {
                rows.push(r);
            }
            op.close(&mut ctx)?;
            let schema = op.schema().clone();
            (schema, rows)
        };
        self.catalog_dirty = catalog_dirty;
        self.indexes_dirty = indexes_dirty;
        let (schema, rows) = result;
        if let Some(v) = verb {
            // DML emits a single summary row with the affected count.
            let count = rows
                .first()
                .and_then(|r| r.first())
                .cloned()
                .unwrap_or(Value::Int(0));
            let count = match count {
                Value::Int(i) => i,
                _ => 0,
            };
            Ok(ExecResult::Affected { verb: v, count })
        } else {
            Ok(ExecResult::Query { schema, rows })
        }
    }

    // ---------------- DDL ----------------

    fn exec_create_table(&mut self, ct: ast::CreateTable) -> Result<ExecResult> {
        let columns: Vec<ColumnSchema> = ct
            .columns
            .iter()
            .map(|c| ColumnSchema {
                name: c.name.clone(),
                dtype: c.dtype,
                not_null: c.not_null,
                primary_key: c.primary_key,
                unique: c.unique,
                default: c.default.clone(),
            })
            .collect();
        // Validate at most one PRIMARY KEY.
        let pk_count = columns.iter().filter(|c| c.primary_key).count();
        if pk_count > 1 {
            return Err(MiniDbError::ty(
                "CREATE TABLE: at most one PRIMARY KEY column",
            ));
        }
        // Render CHECK exprs back to source text.
        let checks: Vec<String> = ct
            .checks
            .iter()
            .map(operators_internal::render_expr)
            .collect();
        let head = self.create_heap()?;
        let table = TableSchema {
            name: ct.name.clone(),
            columns,
            checks,
            heap_head: head,
        };
        self.catalog.add_table(table)?;
        self.catalog_dirty = true;
        Ok(ExecResult::Ok)
    }

    fn exec_drop_table(&mut self, dt: ast::DropTable) -> Result<ExecResult> {
        let removed = self.catalog.drop_table(&dt.name)?;
        self.catalog_dirty = true;
        // Drop dependent index files (best-effort: kept until commit).
        let to_drop: Vec<String> = self
            .indexes
            .keys()
            .filter(|_| true)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .filter(|k| {
                // Keep only indexes whose table no longer exists in catalog.
                !self.catalog.indexes.contains_key(k)
            })
            .collect();
        for k in to_drop {
            if let Some(b) = self.indexes.remove(&k) {
                let _ = b.delete_file();
            }
        }
        // (We leak heap pages; a real DB would have a free list.)
        let _ = removed;
        Ok(ExecResult::Ok)
    }

    fn exec_create_index(&mut self, ci: ast::CreateIndex) -> Result<ExecResult> {
        let table = self.catalog.get_table(&ci.table)?.clone();
        let col_idx = table.column_index(&ci.column).ok_or_else(|| {
            MiniDbError::catalog(format!(
                "column '{}' not found on table '{}'",
                ci.column, ci.table
            ))
        })?;
        let idx_schema = IndexSchema {
            name: ci.name.clone(),
            table: ci.table.clone(),
            column: ci.column.clone(),
        };
        self.catalog.add_index(idx_schema)?;
        self.catalog_dirty = true;
        // Build the index.
        let idx_path = self.root.join(INDEX_DIR).join(format!("{}.bin", ci.name));
        let mut tree = PersistentBTree::open(&idx_path)?;
        let rows = crate::storage::heap::Heap::new(&mut self.pager).scan(table.heap_head)?;
        for (rid, t) in rows {
            tree.tree.insert(t.values[col_idx].clone(), rid)?;
        }
        let key = ci.name.to_ascii_lowercase();
        self.indexes.insert(key.clone(), tree);
        self.indexes_dirty.insert(key);
        Ok(ExecResult::Ok)
    }

    fn exec_drop_index(&mut self, di: ast::DropIndex) -> Result<ExecResult> {
        let _removed = self.catalog.drop_index(&di.name)?;
        self.catalog_dirty = true;
        let key = di.name.to_ascii_lowercase();
        if let Some(b) = self.indexes.remove(&key) {
            // Defer file deletion to commit.
            let _ = b.delete_file();
        }
        Ok(ExecResult::Ok)
    }

    fn create_heap(&mut self) -> Result<PageId> {
        crate::storage::heap::Heap::new(&mut self.pager).create()
    }

    // ---------------- Meta ----------------

    fn exec_meta(&mut self, m: &ast::MetaCommand) -> Result<ExecResult> {
        match m.name.as_str() {
            "tables" => Ok(ExecResult::TableList(
                self.catalog
                    .tables
                    .values()
                    .map(|t| t.name.clone())
                    .collect(),
            )),
            "schema" => {
                let mut out = Vec::new();
                let target = m.arg.clone();
                for (_, t) in self.catalog.tables.iter() {
                    if let Some(name) = &target {
                        if !t.name.eq_ignore_ascii_case(name) {
                            continue;
                        }
                    }
                    let mut col_descs: Vec<String> = Vec::new();
                    for c in &t.columns {
                        let mut s = format!("{} {}", c.name, c.dtype);
                        if c.primary_key {
                            s.push_str(" PRIMARY KEY");
                        }
                        if c.not_null && !c.primary_key {
                            s.push_str(" NOT NULL");
                        }
                        if c.unique {
                            s.push_str(" UNIQUE");
                        }
                        if let Some(d) = &c.default {
                            s.push_str(&format!(" DEFAULT {}", d));
                        }
                        col_descs.push(s);
                    }
                    let descr = format!("CREATE TABLE {} ({})", t.name, col_descs.join(", "));
                    out.push((t.name.clone(), descr));
                }
                Ok(ExecResult::SchemaList(out))
            }
            "indexes" => Ok(ExecResult::IndexList(
                self.catalog
                    .indexes
                    .values()
                    .map(|i| (i.name.clone(), i.table.clone(), i.column.clone()))
                    .collect(),
            )),
            "exit" => Ok(ExecResult::Exit),
            "format" => {
                // Engine doesn't track format directly; the CLI does. Echo.
                Ok(ExecResult::Meta(vec![(
                    "format".into(),
                    Value::Text(m.arg.clone().unwrap_or_else(|| "table".into())),
                )]))
            }
            other => Err(MiniDbError::cli(format!(
                "unknown meta command: .{}",
                other
            ))),
        }
    }

    // ---------------- Transactions ----------------

    fn do_begin(&mut self) -> Result<ExecResult> {
        self.do_begin_internal(false)?;
        Ok(ExecResult::Begin)
    }

    fn do_begin_implicit(&mut self) -> Result<()> {
        self.do_begin_internal(true)
    }

    fn do_begin_internal(&mut self, auto: bool) -> Result<()> {
        let id = self.txn.begin(auto)?;
        self.wal.append(&WalRecord::Begin { txn_id: id.0 })?;
        self.catalog_snapshot_for_txn = Some(self.catalog.clone());
        self.catalog_dirty = false;
        self.indexes_dirty.clear();
        Ok(())
    }

    fn do_commit(&mut self) -> Result<ExecResult> {
        if !self.txn.is_active() {
            return Err(MiniDbError::txn("COMMIT without active transaction"));
        }
        self.do_commit_internal()?;
        Ok(ExecResult::Commit)
    }

    fn do_commit_internal(&mut self) -> Result<()> {
        let id = self.txn.current().expect("active");
        // Collect dirty page images and append redo records.
        let dirty = self.pager.dirty_snapshot();
        for (pid, buf) in &dirty {
            self.wal.append(&WalRecord::PageImage {
                txn_id: id.0,
                page_id: *pid,
                bytes: buf.to_vec(),
            })?;
        }
        // Catalog snapshot if anything changed.
        if self.catalog_dirty {
            let bytes = self.catalog.to_json()?;
            self.wal.append(&WalRecord::CatalogSnapshot {
                txn_id: id.0,
                bytes,
            })?;
        }
        self.wal.append(&WalRecord::Commit { txn_id: id.0 })?;
        self.wal.fsync()?;
        // Now flush data pages.
        self.pager.flush()?;
        if self.catalog_dirty {
            self.save_catalog()?;
        }
        // Persist dirty indexes.
        for name in std::mem::take(&mut self.indexes_dirty) {
            if let Some(idx) = self.indexes.get(&name) {
                idx.persist()?;
            }
        }
        // Truncate WAL on successful checkpoint.
        self.wal.truncate()?;
        self.catalog_dirty = false;
        self.catalog_snapshot_for_txn = None;
        self.txn.finish()?;
        debug!("committed txn {:?}", id);
        Ok(())
    }

    fn do_rollback(&mut self) -> Result<ExecResult> {
        if !self.txn.is_active() {
            return Err(MiniDbError::txn("ROLLBACK without active transaction"));
        }
        self.do_rollback_internal()?;
        Ok(ExecResult::Rollback)
    }

    fn do_rollback_internal(&mut self) -> Result<()> {
        let id = self.txn.current().expect("active");
        // Discard dirty pages.
        self.pager.discard_dirty();
        // Restore catalog snapshot.
        if let Some(snap) = self.catalog_snapshot_for_txn.take() {
            self.catalog = snap;
        }
        self.catalog_dirty = false;
        // Reload indexes from disk to discard in-memory mutations.
        let mut new_idx = HashMap::new();
        for idx in self.catalog.indexes.values() {
            let p = self.root.join(INDEX_DIR).join(format!("{}.bin", idx.name));
            new_idx.insert(idx.name.to_ascii_lowercase(), PersistentBTree::open(&p)?);
        }
        self.indexes = new_idx;
        self.indexes_dirty.clear();
        // Append rollback marker (and truncate WAL since nothing committed
        // requires durability now).
        self.wal.append(&WalRecord::Rollback { txn_id: id.0 })?;
        self.wal.fsync()?;
        self.wal.truncate()?;
        self.txn.finish()?;
        Ok(())
    }
}

/// Internal helper module: re-exports a tiny utility that operators.rs needs
/// to keep CHECK predicates round-trippable as text.
pub mod operators_internal {
    use crate::parser::ast::{BinOp, Expr};
    use crate::types::Value;

    pub fn render_expr(e: &Expr) -> String {
        match e {
            Expr::Literal(v) => match v {
                Value::Null => "NULL".to_string(),
                Value::Int(i) => i.to_string(),
                Value::Real(r) => {
                    if r.fract() == 0.0 && r.is_finite() {
                        format!("{:.1}", r)
                    } else {
                        format!("{}", r)
                    }
                }
                Value::Text(s) => format!("'{}'", s.replace('\'', "''")),
                Value::Bool(b) => {
                    if *b {
                        "TRUE".into()
                    } else {
                        "FALSE".into()
                    }
                }
            },
            Expr::Column { table, name } => match table {
                Some(t) => format!("{}.{}", t, name),
                None => name.clone(),
            },
            Expr::Binary { op, left, right } => format!(
                "({} {} {})",
                render_expr(left),
                op_str(*op),
                render_expr(right)
            ),
            Expr::And(a, b) => format!("({} AND {})", render_expr(a), render_expr(b)),
            Expr::Or(a, b) => format!("({} OR {})", render_expr(a), render_expr(b)),
            Expr::Not(x) => format!("(NOT {})", render_expr(x)),
            Expr::Neg(x) => format!("(-{})", render_expr(x)),
            Expr::IsNull(x) => format!("({} IS NULL)", render_expr(x)),
            Expr::Func { name, args } => {
                let parts: Vec<String> = args.iter().map(render_expr).collect();
                format!("{}({})", name, parts.join(", "))
            }
        }
    }

    fn op_str(o: BinOp) -> &'static str {
        match o {
            BinOp::Eq => "=",
            BinOp::NotEq => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Concat => "||",
        }
    }
}
