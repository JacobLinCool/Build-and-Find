//! End-to-end tests against the `minidb` library API. Exercise the full
//! pipeline (parse → plan → execute) without going through the binary, so
//! they're cheap to run on every `cargo test`.

use minidb::{run_sql, Engine, MiniDbError, StatementOutcome};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_db() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut p = std::env::temp_dir();
    p.push(format!("minidb_it_{}_{}.db", std::process::id(), n));
    let _ = std::fs::remove_file(&p);
    let mut wp = p.clone().into_os_string();
    wp.push(".wal");
    let _ = std::fs::remove_file(PathBuf::from(wp));
    p
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let mut wp = path.as_os_str().to_owned();
    wp.push(".wal");
    let _ = std::fs::remove_file(PathBuf::from(wp));
}

fn select_rows(eng: &mut Engine, sql: &str) -> Vec<Vec<minidb::types::Value>> {
    match run_sql(eng, sql).unwrap() {
        StatementOutcome::Select(rs) => rs.rows,
        StatementOutcome::Write(_) => panic!("expected SELECT"),
    }
}

fn run(eng: &mut Engine, sql: &str) -> StatementOutcome {
    run_sql(eng, sql).unwrap()
}

#[test]
fn create_insert_select_roundtrip() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(
        &mut e,
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL)",
    );
    run(&mut e, "INSERT INTO t VALUES (1, 'alice'), (2, 'bob')");
    let rows = select_rows(&mut e, "SELECT id, name FROM t ORDER BY id");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 2);
    drop(e);
    cleanup(&path);
}

#[test]
fn transaction_commit_and_rollback() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (id INT)");
    run(&mut e, "BEGIN");
    run(&mut e, "INSERT INTO t VALUES (1)");
    run(&mut e, "INSERT INTO t VALUES (2)");
    run(&mut e, "ROLLBACK");
    assert_eq!(select_rows(&mut e, "SELECT id FROM t").len(), 0);

    run(&mut e, "BEGIN");
    run(&mut e, "INSERT INTO t VALUES (3)");
    run(&mut e, "COMMIT");
    assert_eq!(select_rows(&mut e, "SELECT id FROM t").len(), 1);
    drop(e);

    // Reopen and confirm persistence.
    let mut e2 = Engine::open(&path).unwrap();
    assert_eq!(select_rows(&mut e2, "SELECT id FROM t").len(), 1);
    drop(e2);
    cleanup(&path);
}

#[test]
fn null_three_valued_logic() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (a INT, b INT)");
    run(&mut e, "INSERT INTO t VALUES (1, NULL), (NULL, 2), (3, 3)");
    // a = b doesn't match NULLs.
    let rows = select_rows(&mut e, "SELECT a, b FROM t WHERE a = b");
    assert_eq!(rows.len(), 1);
    // IS NULL works.
    let rows = select_rows(&mut e, "SELECT a FROM t WHERE a IS NULL");
    assert_eq!(rows.len(), 1);
    let rows = select_rows(&mut e, "SELECT a FROM t WHERE a IS NOT NULL");
    assert_eq!(rows.len(), 2);
    drop(e);
    cleanup(&path);
}

#[test]
fn inner_join() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE a (id INT, x INT)");
    run(&mut e, "CREATE TABLE b (aid INT, y TEXT)");
    run(&mut e, "INSERT INTO a VALUES (1, 10), (2, 20), (3, 30)");
    run(&mut e, "INSERT INTO b VALUES (1, 'one'), (2, 'two')");
    let rows = select_rows(
        &mut e,
        "SELECT a.id, b.y FROM a INNER JOIN b ON a.id = b.aid ORDER BY a.id",
    );
    assert_eq!(rows.len(), 2);
    drop(e);
    cleanup(&path);
}

#[test]
fn order_limit_offset() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (id INT)");
    run(
        &mut e,
        "INSERT INTO t VALUES (3), (1), (4), (1), (5), (9), (2), (6)",
    );
    let rows = select_rows(&mut e, "SELECT id FROM t ORDER BY id ASC LIMIT 3 OFFSET 2");
    let vals: Vec<i64> = rows
        .iter()
        .map(|r| match &r[0] {
            minidb::types::Value::Int(n) => *n,
            other => panic!("not int: {:?}", other),
        })
        .collect();
    assert_eq!(vals, vec![2, 3, 4]);
    drop(e);
    cleanup(&path);
}

#[test]
fn unique_and_not_null_violations() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(
        &mut e,
        "CREATE TABLE t (id INT PRIMARY KEY, n INT NOT NULL)",
    );
    run(&mut e, "INSERT INTO t VALUES (1, 10)");
    let r = run_sql(&mut e, "INSERT INTO t VALUES (1, 20)");
    assert!(matches!(r, Err(MiniDbError::Constraint(_))));
    let r = run_sql(&mut e, "INSERT INTO t VALUES (2, NULL)");
    assert!(matches!(r, Err(MiniDbError::Constraint(_))));
    drop(e);
    cleanup(&path);
}

#[test]
fn update_and_delete() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (id INT, n INT)");
    run(&mut e, "INSERT INTO t VALUES (1, 10), (2, 20), (3, 30)");
    run(&mut e, "UPDATE t SET n = n + 1 WHERE id = 2");
    let rows = select_rows(&mut e, "SELECT n FROM t WHERE id = 2");
    assert!(matches!(rows[0][0], minidb::types::Value::Int(21)));
    run(&mut e, "DELETE FROM t WHERE id = 1");
    assert_eq!(select_rows(&mut e, "SELECT id FROM t").len(), 2);
    drop(e);
    cleanup(&path);
}

#[test]
fn index_lookup_used() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (id INT PRIMARY KEY, name TEXT)");
    run(&mut e, "CREATE INDEX idx_t_name ON t(name)");
    for i in 0..50 {
        run_sql(
            &mut e,
            &format!("INSERT INTO t VALUES ({}, 'name{}')", i, i),
        )
        .unwrap();
    }
    let rows = select_rows(&mut e, "SELECT id FROM t WHERE name = 'name37'");
    assert_eq!(rows.len(), 1);
    drop(e);
    cleanup(&path);
}

#[test]
fn expressions_and_functions() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (s TEXT, n INT)");
    run(&mut e, "INSERT INTO t VALUES ('Hello', -3), ('World', 0)");
    let rows = select_rows(
        &mut e,
        "SELECT LENGTH(s), LOWER(s), UPPER(s), ABS(n), s || '!' FROM t ORDER BY n",
    );
    assert_eq!(rows.len(), 2);
    // ABS(-3) = 3
    let abs = &rows[0][3];
    assert!(matches!(abs, minidb::types::Value::Int(3)));
    drop(e);
    cleanup(&path);
}

#[test]
fn defaults_and_named_insert() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(
        &mut e,
        "CREATE TABLE t (id INT PRIMARY KEY, n INT DEFAULT 7, s TEXT DEFAULT 'x')",
    );
    run(&mut e, "INSERT INTO t (id) VALUES (1)");
    let rows = select_rows(&mut e, "SELECT id, n, s FROM t");
    assert!(matches!(rows[0][1], minidb::types::Value::Int(7)));
    assert!(matches!(rows[0][2], minidb::types::Value::Text(ref s) if s == "x"));
    drop(e);
    cleanup(&path);
}

#[test]
fn drop_table_cascades_indexes() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    run(&mut e, "CREATE TABLE t (id INT PRIMARY KEY)");
    run(&mut e, "DROP TABLE t");
    assert!(matches!(
        run_sql(&mut e, "SELECT * FROM t"),
        Err(MiniDbError::Catalog(_))
    ));
    run(&mut e, "DROP TABLE IF EXISTS t");
    drop(e);
    cleanup(&path);
}

#[test]
fn parse_error_has_position() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    let err = run_sql(&mut e, "SELECT FROM").unwrap_err();
    match err {
        MiniDbError::Parse { line, .. } => assert_eq!(line, 1),
        other => panic!("wrong error: {:?}", other),
    }
    drop(e);
    cleanup(&path);
}

#[test]
fn select_without_from() {
    let path = tmp_db();
    let mut e = Engine::open(&path).unwrap();
    let rows = select_rows(&mut e, "SELECT 1 + 2 AS sum, LENGTH('hi') AS l");
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows[0][0], minidb::types::Value::Int(3)));
    assert!(matches!(rows[0][1], minidb::types::Value::Int(2)));
    drop(e);
    cleanup(&path);
}
