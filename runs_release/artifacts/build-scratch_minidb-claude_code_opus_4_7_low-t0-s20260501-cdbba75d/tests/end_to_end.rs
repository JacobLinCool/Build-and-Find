//! End-to-end integration tests that exercise the `minidb` binary over SQL.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_minidb"))
}

fn temp_db() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("test.db");
    (d, p)
}

fn run_sql(db: &PathBuf, sql: &str) -> (i32, String, String) {
    let out = Command::new(bin())
        .args(["--db", db.to_str().unwrap(), "--eval", sql, "--quiet"])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .expect("run binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn create_insert_select_basic() {
    let (_g, db) = temp_db();
    let (code, _stdout, _stderr) = run_sql(
        &db,
        "CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES (1,'x'),(2,'y');",
    );
    assert_eq!(code, 0);
    let (code, stdout, _stderr) = run_sql(&db, "SELECT a,b FROM t WHERE a=2;");
    assert_eq!(code, 0);
    assert!(stdout.contains("2"));
    assert!(stdout.contains("y"));
}

#[test]
fn csv_format_output() {
    let (_g, db) = temp_db();
    run_sql(
        &db,
        "CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES (1,'a,b'),(2,'q\"u');",
    );
    let out = Command::new(bin())
        .args([
            "--db",
            db.to_str().unwrap(),
            "--format",
            "csv",
            "--quiet",
            "--eval",
            "SELECT a,b FROM t ORDER BY a;",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "a,b");
    assert!(lines[1].contains("\"a,b\""));
    assert!(lines[2].contains("\"q\"\"u\""));
}

#[test]
fn json_format_output() {
    let (_g, db) = temp_db();
    run_sql(
        &db,
        "CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES (1,'x');",
    );
    let out = Command::new(bin())
        .args([
            "--db",
            db.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
            "--eval",
            "SELECT a,b FROM t;",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("[{\"a\":1,\"b\":\"x\"}]"));
}

#[test]
fn errors_have_exit_codes() {
    let (_g, db) = temp_db();
    let (code, _o, _e) = run_sql(&db, "SELECT * FROM nope;");
    assert_eq!(code, 1);

    let bad = Command::new(bin())
        .args(["--db", db.to_str().unwrap(), "--unknown"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn transaction_commit_persists() {
    let (_g, db) = temp_db();
    run_sql(&db, "CREATE TABLE t(a INT);");
    run_sql(&db, "BEGIN; INSERT INTO t VALUES (1); COMMIT;");
    let (_, stdout, _) = run_sql(&db, "SELECT a FROM t;");
    assert!(stdout.contains('1'));
}

#[test]
fn transaction_rollback_discards() {
    let (_g, db) = temp_db();
    run_sql(&db, "CREATE TABLE t(a INT);");
    run_sql(&db, "BEGIN; INSERT INTO t VALUES (1); ROLLBACK;");
    let (_, _stdout, stderr) = Command::new(bin())
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT a FROM t;"])
        .output()
        .map(|o| {
            (
                o.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&o.stdout).into_owned(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            )
        })
        .unwrap();
    assert!(stderr.contains("0 rows"));
}

#[test]
fn join_and_order_by() {
    let (_g, db) = temp_db();
    run_sql(
        &db,
        "CREATE TABLE a(id INT, x INT); CREATE TABLE b(id INT, y TEXT); \
         INSERT INTO a VALUES (1,10),(2,20); \
         INSERT INTO b VALUES (1,'hello'),(2,'world');",
    );
    let (code, stdout, _) = run_sql(
        &db,
        "SELECT a.x, b.y FROM a INNER JOIN b ON a.id = b.id ORDER BY a.x DESC;",
    );
    assert_eq!(code, 0);
    let i_world = stdout.find("world").unwrap();
    let i_hello = stdout.find("hello").unwrap();
    assert!(i_world < i_hello, "DESC order: world should come first");
}

#[test]
fn index_speeds_up_eq_lookup() {
    let (_g, db) = temp_db();
    run_sql(
        &db,
        "CREATE TABLE t(a INT, b INT); CREATE INDEX ix ON t(a);",
    );
    // insert many rows
    let mut sql = String::from("INSERT INTO t VALUES ");
    for i in 0..200 {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str(&format!("({},{})", i, i * 10));
    }
    sql.push(';');
    run_sql(&db, &sql);
    let (_, stdout, _) = run_sql(&db, "SELECT b FROM t WHERE a = 42;");
    assert!(stdout.contains("420"));
}

#[test]
fn check_meta_commands_and_schema() {
    let (_g, db) = temp_db();
    run_sql(
        &db,
        "CREATE TABLE t(a INT PRIMARY KEY, b TEXT NOT NULL); CREATE INDEX ix ON t(b);",
    );
    let mut child = Command::new(bin())
        .args(["--db", db.to_str().unwrap(), "--quiet", "--repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, ".tables").unwrap();
        writeln!(stdin, ".indexes").unwrap();
        writeln!(stdin, ".schema").unwrap();
        writeln!(stdin, ".exit").unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("t"));
    assert!(stdout.contains("ix"));
    assert!(stdout.contains("CREATE TABLE"));
}

#[test]
fn file_mode_continue_on_error() {
    let (g, db) = temp_db();
    let script = g.path().join("script.sql");
    std::fs::write(
        &script,
        "CREATE TABLE t(a INT);\nINSERT INTO bad VALUES (1);\nINSERT INTO t VALUES (1);\n",
    )
    .unwrap();
    let out = Command::new(bin())
        .args([
            "--db",
            db.to_str().unwrap(),
            "--file",
            script.to_str().unwrap(),
            "--continue-on-error",
            "--quiet",
        ])
        .output()
        .unwrap();
    // Should have continued past the error and inserted into t.
    let (_, stdout, _) = run_sql(&db, "SELECT a FROM t;");
    assert!(stdout.contains('1'));
    // exit code reflects the last error
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn null_propagation_in_arithmetic_and_comparison() {
    let (_g, db) = temp_db();
    run_sql(
        &db,
        "CREATE TABLE t(a INT, b INT); INSERT INTO t VALUES (1, NULL),(2, 5);",
    );
    // a + b is NULL for the first row -> WHERE a + b > 0 only matches the second
    let (_, stdout, _) = run_sql(&db, "SELECT a FROM t WHERE a + b > 0;");
    assert!(stdout.contains('2'));
    assert!(!stdout.contains(" 1 "));
}

#[test]
fn version_and_help_exit_zero() {
    let v = Command::new(bin()).arg("--version").output().unwrap();
    assert_eq!(v.status.code(), Some(0));
    let h = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(h.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&h.stdout).contains("minidb"));
}

#[test]
fn init_creates_fresh_db() {
    let g = tempfile::tempdir().unwrap();
    let db = g.path().join("fresh.db");
    let out = Command::new(bin())
        .args(["--db", db.to_str().unwrap(), "--init"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(db.exists());
}
