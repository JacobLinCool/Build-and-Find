//! End-to-end tests that exercise minidb through its public Database API.
//! A separate "binary" test exercises the assembled CLI via a subprocess.

use minidb::exec::{AffectedKind, Database, StatementResult};
use minidb::types::Value;
use std::path::PathBuf;
use tempfile::tempdir;

fn db_in(dir: &std::path::Path, name: &str) -> PathBuf {
    dir.join(name)
}

fn rows_of(r: &StatementResult) -> Vec<Vec<Value>> {
    match r {
        StatementResult::Rows { rows, .. } => rows.clone(),
        _ => panic!("expected rows, got {:?}", r),
    }
}

fn affected(r: &StatementResult) -> (AffectedKind, usize) {
    match r {
        StatementResult::Affected { kind, n } => (*kind, *n),
        _ => panic!("expected affected, got {:?}", r),
    }
}

#[test]
fn create_insert_select() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "a.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, n TEXT NOT NULL);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'alpha'), (2, 'beta');")
        .unwrap();
    let r = &db.execute_sql("SELECT * FROM t ORDER BY id;").unwrap()[0];
    let rows = rows_of(r);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[1][1], Value::Text("beta".into()));
}

#[test]
fn unique_violation() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "u.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1);").unwrap();
    let err = db.execute_sql("INSERT INTO t VALUES (1);").unwrap_err();
    assert!(format!("{}", err).contains("UNIQUE"));
}

#[test]
fn check_violation_rolls_back() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "c.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (id INT, v INT, CHECK (v > 0));")
        .unwrap();
    let err = db.execute_sql("INSERT INTO t VALUES (1, -5);").unwrap_err();
    assert!(format!("{}", err).contains("CHECK"));
    let r = &db.execute_sql("SELECT * FROM t;").unwrap()[0];
    assert!(rows_of(r).is_empty());
}

#[test]
fn update_and_delete() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "ud.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (id INT, v INT);").unwrap();
    db.execute_sql("INSERT INTO t VALUES (1,10),(2,20),(3,30);")
        .unwrap();
    let r = &db
        .execute_sql("UPDATE t SET v = v + 1 WHERE id < 3;")
        .unwrap()[0];
    assert_eq!(affected(r), (AffectedKind::Updated, 2));
    let r = &db.execute_sql("DELETE FROM t WHERE id = 3;").unwrap()[0];
    assert_eq!(affected(r), (AffectedKind::Deleted, 1));
    let r = &db.execute_sql("SELECT v FROM t ORDER BY id;").unwrap()[0];
    let rows = rows_of(r);
    assert_eq!(rows[0][0], Value::Int(11));
    assert_eq!(rows[1][0], Value::Int(21));
}

#[test]
fn rollback_undoes_insert() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "rb.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (id INT);").unwrap();
    db.execute_sql("INSERT INTO t VALUES (1);").unwrap();
    db.execute_sql("BEGIN; INSERT INTO t VALUES (2); ROLLBACK;")
        .unwrap();
    let r = &db.execute_sql("SELECT * FROM t;").unwrap()[0];
    assert_eq!(rows_of(r).len(), 1);
}

#[test]
fn explicit_commit_persists() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "cm.db");
    {
        let mut db = Database::open(&p).unwrap();
        db.execute_sql("CREATE TABLE t (id INT);").unwrap();
        db.execute_sql("BEGIN; INSERT INTO t VALUES (7); COMMIT;")
            .unwrap();
    }
    let mut db = Database::open(&p).unwrap();
    let r = &db.execute_sql("SELECT * FROM t;").unwrap()[0];
    assert_eq!(rows_of(r)[0][0], Value::Int(7));
}

#[test]
fn join_and_index_scan() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "j.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE a (id INT, x INT);").unwrap();
    db.execute_sql("CREATE TABLE b (id INT, y INT);").unwrap();
    db.execute_sql("CREATE INDEX ai ON a (id);").unwrap();
    db.execute_sql("INSERT INTO a VALUES (1,100),(2,200);")
        .unwrap();
    db.execute_sql("INSERT INTO b VALUES (1,10),(2,20);")
        .unwrap();
    let r = &db
        .execute_sql("SELECT a.x, b.y FROM a INNER JOIN b ON a.id = b.id ORDER BY a.id;")
        .unwrap()[0];
    let rows = rows_of(r);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], Value::Int(100));
    assert_eq!(rows[1][1], Value::Int(20));
    let r = &db.execute_sql("SELECT * FROM a WHERE id = 1;").unwrap()[0];
    assert_eq!(rows_of(r).len(), 1);
}

#[test]
fn null_three_valued_logic() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "n.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (a INT);").unwrap();
    db.execute_sql("INSERT INTO t VALUES (1), (NULL), (3);")
        .unwrap();
    let r = &db.execute_sql("SELECT * FROM t WHERE a IS NULL;").unwrap()[0];
    assert_eq!(rows_of(r).len(), 1);
    let r = &db.execute_sql("SELECT * FROM t WHERE a > 0;").unwrap()[0];
    assert_eq!(rows_of(r).len(), 2);
}

#[test]
fn limit_offset() {
    let dir = tempdir().unwrap();
    let p = db_in(dir.path(), "l.db");
    let mut db = Database::open(&p).unwrap();
    db.execute_sql("CREATE TABLE t (id INT);").unwrap();
    for i in 0..5 {
        db.execute_sql(&format!("INSERT INTO t VALUES ({});", i))
            .unwrap();
    }
    let r = &db
        .execute_sql("SELECT id FROM t ORDER BY id LIMIT 2 OFFSET 1;")
        .unwrap()[0];
    let rows = rows_of(r);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[1][0], Value::Int(2));
}

#[test]
fn cli_subprocess() {
    use std::process::Command;
    let dir = tempdir().unwrap();
    let p = dir.path().join("cli.db");
    let exe = env!("CARGO_BIN_EXE_minidb");
    // Help works
    let out = Command::new(exe).arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("minidb"));
    // Version works
    let out = Command::new(exe).arg("--version").output().unwrap();
    assert!(out.status.success());
    // Exec a SQL session
    let out = Command::new(exe)
        .arg("--db")
        .arg(&p)
        .arg("--eval")
        .arg("CREATE TABLE x (id INT); INSERT INTO x VALUES (1),(2); SELECT * FROM x ORDER BY id;")
        .arg("--format")
        .arg("csv")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("id"));
    assert!(stdout.contains("1"));
    assert!(stdout.contains("2"));
    // SQL error → exit 1
    let out = Command::new(exe)
        .arg("--db")
        .arg(&p)
        .arg("--eval")
        .arg("SELECT * FROM no_such_table;")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    // CLI usage error → exit 2
    let out = Command::new(exe).arg("--bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
