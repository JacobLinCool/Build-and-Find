use std::process::Command;
use tempfile::tempdir;

fn run_minidb(db_path: &str, sql: &str, quiet: bool) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_minidb"));
    cmd.arg("--db").arg(db_path);
    if quiet {
        cmd.arg("--quiet");
    }
    cmd.arg("--eval").arg(sql);
    let output = cmd.output().expect("Failed to execute minidb");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

#[test]
fn test_create_table_and_insert() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, stderr, code) = run_minidb(
        db_str,
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'hello');",
        true,
    );
    assert_eq!(code, 0);
    // In quiet mode, no summary on stderr
    assert!(stdout.is_empty());
}

#[test]
fn test_select_all() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT PRIMARY KEY, val TEXT); INSERT INTO t VALUES (1, 'a'); INSERT INTO t VALUES (2, 'b'); SELECT * FROM t;", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("a"));
    assert!(stdout.contains("2"));
    assert!(stdout.contains("b"));
}

#[test]
fn test_where_clause() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT, val TEXT); INSERT INTO t VALUES (1, 'a'); INSERT INTO t VALUES (2, 'b'); INSERT INTO t VALUES (3, 'c'); SELECT * FROM t WHERE id > 1;", true);
    assert_eq!(code, 0);
    assert!(!stdout.contains("| 1"));
    assert!(stdout.contains("b"));
    assert!(stdout.contains("c"));
}

#[test]
fn test_update() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT PRIMARY KEY, val TEXT); INSERT INTO t VALUES (1, 'old'); UPDATE t SET val = 'new' WHERE id = 1; SELECT * FROM t;", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("new"));
}

#[test]
fn test_delete() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT PRIMARY KEY); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); DELETE FROM t WHERE id = 1; SELECT * FROM t;", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("2"));
    assert!(!stdout.contains("| 1 "));
}

#[test]
fn test_order_by() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT, val TEXT); INSERT INTO t VALUES (3, 'c'); INSERT INTO t VALUES (1, 'a'); INSERT INTO t VALUES (2, 'b'); SELECT * FROM t ORDER BY id ASC;", true);
    assert_eq!(code, 0);
    // Should have rows
    assert!(stdout.contains("a"));
    assert!(stdout.contains("b"));
    assert!(stdout.contains("c"));
}

#[test]
fn test_limit_offset() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); INSERT INTO t VALUES (3); INSERT INTO t VALUES (4); INSERT INTO t VALUES (5); SELECT * FROM t ORDER BY id LIMIT 2 OFFSET 2;", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("3"));
    assert!(stdout.contains("4"));
}

#[test]
fn test_drop_table() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (_, _, code) = run_minidb(db_str, "CREATE TABLE t (id INT); DROP TABLE t;", true);
    assert_eq!(code, 0);
}

#[test]
fn test_create_index() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT, name TEXT); CREATE INDEX idx_name ON t (name); INSERT INTO t VALUES (1, 'alice'); SELECT * FROM t WHERE name = 'alice';", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("alice"));
}

#[test]
fn test_begin_commit() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(
        db_str,
        "CREATE TABLE t (id INT); BEGIN; INSERT INTO t VALUES (1); COMMIT; SELECT * FROM t;",
        true,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("1"));
}

#[test]
fn test_rollback_discards() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(
        db_str,
        "CREATE TABLE t (id INT); BEGIN; INSERT INTO t VALUES (1); ROLLBACK; SELECT * FROM t;",
        true,
    );
    assert_eq!(code, 0);
    // Should have no data rows
    let data_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains('|') && !l.contains('-') && !l.contains("id"))
        .collect();
    assert!(
        data_lines.is_empty(),
        "Expected no rows, got: {:?}",
        data_lines
    );
}

#[test]
fn test_not_null_constraint() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (_, stderr, code) = run_minidb(
        db_str,
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL); INSERT INTO t (id) VALUES (1);",
        true,
    );
    assert_ne!(code, 0);
    assert!(
        stderr.contains("required") || stderr.contains("NOT NULL") || stderr.contains("Constraint")
    );
}

#[test]
fn test_column_types() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(
        db_str,
        "CREATE TABLE t (i INT, txt TEXT); INSERT INTO t VALUES (42, 'hello'); SELECT * FROM t;",
        true,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("42"));
    assert!(stdout.contains("hello"));
}

#[test]
fn test_version_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_minidb"))
        .arg("--version")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("minidb"));
    assert!(stderr.contains("0.1.0"));
}

#[test]
fn test_help_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_minidb"))
        .arg("--help")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--db"));
    assert!(stderr.contains("--eval"));
    assert!(stderr.contains("--file"));
}

#[test]
fn test_sql_error_exit_code() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (_, _, code) = run_minidb(db_str, "SELECT * FROM nonexistent_table;", true);
    assert_eq!(code, 1);
}

#[test]
fn test_inner_join() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t1 (id INT, name TEXT); CREATE TABLE t2 (id INT, val TEXT); INSERT INTO t1 VALUES (1, 'alice'); INSERT INTO t2 VALUES (1, 'x'); SELECT * FROM t1 INNER JOIN t2 ON t1.id = t2.id;", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("alice"));
    assert!(stdout.contains("x"));
}

#[test]
fn test_is_null() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let db_str = db.to_str().unwrap();
    let (stdout, _, code) = run_minidb(db_str,
        "CREATE TABLE t (id INT, val TEXT); INSERT INTO t VALUES (1, NULL); INSERT INTO t VALUES (2, 'hello'); SELECT * FROM t WHERE val IS NULL;", true);
    assert_eq!(code, 0);
    assert!(stdout.contains("1"));
    assert!(!stdout.contains("hello"));
}
