use std::fs;
use std::process::Command;

fn bin_path() -> String {
    let meta = std::process::Command::new("cargo")
        .args(&["metadata", "--format-version", "1"])
        .output()
        .expect("Failed to get cargo metadata");
    let meta_json: serde_json::Value =
        serde_json::from_slice(&meta.stdout).expect("Invalid metadata JSON");
    let target_dir = meta_json["target_directory"]
        .as_str()
        .expect("No target_directory");
    let mut path = std::path::PathBuf::from(target_dir);
    path.push("release");
    path.push("minidb");
    path.to_string_lossy().to_string()
}

fn run_sql(db: &str, sql: &str) -> (String, String, i32) {
    let output = Command::new(bin_path())
        .args(&["--db", db, "--eval", sql, "--quiet"])
        .output()
        .expect("Failed to execute minidb");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(1);
    (stdout, stderr, code)
}

fn run_sql_format(db: &str, sql: &str, format: &str) -> (String, String, i32) {
    let output = Command::new(bin_path())
        .args(&["--db", db, "--eval", sql, "--quiet", "--format", format])
        .output()
        .expect("Failed to execute minidb");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(1);
    (stdout, stderr, code)
}

fn fresh_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("minidb_test_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("test.db")
}

#[test]
fn test_version() {
    let output = Command::new(bin_path())
        .args(&["--version"])
        .output()
        .expect("Failed");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(stderr.contains("minidb"));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn test_help() {
    let output = Command::new(bin_path())
        .args(&["--help"])
        .output()
        .expect("Failed");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(stderr.contains("USAGE"));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn test_init() {
    let db = fresh_db("init");
    let output = Command::new(bin_path())
        .args(&["--db", db.to_str().unwrap(), "--init"])
        .output()
        .expect("Failed");
    assert_eq!(output.status.code(), Some(0));
    assert!(db.exists());
}

#[test]
fn test_create_insert_select() {
    let db = fresh_db("crud");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE users (id INT PRIMARY KEY, name TEXT, email TEXT); INSERT INTO users (id, name, email) VALUES (1, 'Alice', 'alice@example.com'); SELECT * FROM users;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("alice@example.com"));
}

#[test]
fn test_update_in_session() {
    let db = fresh_db("upd");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (id INT, val TEXT); INSERT INTO t VALUES (1, 'a'); INSERT INTO t VALUES (2, 'b'); UPDATE t SET val = 'x' WHERE id = 1; SELECT val FROM t WHERE id = 1;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("x"));
}

#[test]
fn test_delete_in_session() {
    let db = fresh_db("del");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (id INT); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); DELETE FROM t WHERE id = 1; SELECT * FROM t;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("2"));
    assert!(!stdout.contains("1\n"));
}

#[test]
fn test_order_by_limit() {
    let db = fresh_db("order");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (id INT, name TEXT); INSERT INTO t VALUES (3, 'c'); INSERT INTO t VALUES (1, 'a'); INSERT INTO t VALUES (2, 'b'); SELECT * FROM t ORDER BY id ASC;",
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.lines().collect();
    let data_lines: Vec<&str> = lines
        .iter()
        .skip(2)
        .filter(|l| !l.starts_with('-'))
        .copied()
        .collect();
    assert!(data_lines.len() >= 3);
}

#[test]
fn test_expressions() {
    let db = fresh_db("expr");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (a INT, b INT, s TEXT); INSERT INTO t VALUES (3, 4, 'hello'); SELECT a + b AS sum_val, a * b AS product FROM t;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("7"));
    assert!(stdout.contains("12"));
}

#[test]
fn test_string_functions() {
    let db = fresh_db("strfn");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (s TEXT); INSERT INTO t VALUES ('Hello World'); SELECT LOWER(s) FROM t;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("hello world"));
}

#[test]
fn test_null_handling() {
    let db = fresh_db("null");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (a INT, b INT); INSERT INTO t VALUES (1, NULL); SELECT * FROM t WHERE b IS NULL;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("1"));
}

#[test]
fn test_transactions_in_session() {
    let db = fresh_db("txn");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (id INT); BEGIN; INSERT INTO t VALUES (1); COMMIT; SELECT * FROM t;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("1"));
}

#[test]
fn test_error_codes() {
    let db = fresh_db("err");
    let (_, _, code) = run_sql(db.to_str().unwrap(), "SELECT * FROM nonexistent;");
    assert_eq!(code, 1);
}

#[test]
fn test_csv_output() {
    let db = fresh_db("csv");
    run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (a INT, b TEXT); INSERT INTO t VALUES (1, 'hello');",
    );
    let (stdout, _, code) = run_sql_format(db.to_str().unwrap(), "SELECT * FROM t;", "csv");
    assert_eq!(code, 0);
    assert!(stdout.contains("a"));
    assert!(stdout.contains("1"));
    assert!(stdout.contains("hello"));
}

#[test]
fn test_json_output() {
    let db = fresh_db("json");
    run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (a INT, b TEXT); INSERT INTO t VALUES (1, 'hello');",
    );
    let (stdout, _, code) = run_sql_format(db.to_str().unwrap(), "SELECT * FROM t;", "json");
    assert_eq!(code, 0);
    assert!(stdout.contains("\"a\""));
    assert!(stdout.contains("\"hello\""));
}

#[test]
fn test_create_drop_table() {
    let db = fresh_db("drop");
    run_sql(db.to_str().unwrap(), "CREATE TABLE t (id INT);");
    let (_, stderr, _) = run_sql(db.to_str().unwrap(), ".tables");
    assert!(stderr.contains("t"));
    run_sql(db.to_str().unwrap(), "DROP TABLE t;");
    let (_, stderr, _) = run_sql(db.to_str().unwrap(), ".tables");
    assert!(!stderr.contains("t"));
}

#[test]
fn test_bool_type() {
    let db = fresh_db("bool");
    let (stdout, _, code) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (flag BOOL); INSERT INTO t VALUES (true); INSERT INTO t VALUES (false); SELECT * FROM t WHERE flag = true;",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("true"));
}

#[test]
fn test_is_null_queries() {
    let db = fresh_db("isnull");
    let (stdout, _, _) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (a INT); INSERT INTO t VALUES (1); INSERT INTO t VALUES (NULL); SELECT * FROM t WHERE a IS NOT NULL;",
    );
    assert!(stdout.contains("1"));
}

#[test]
fn test_where_comparisons() {
    let db = fresh_db("where");
    let (stdout, _, _) = run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (a INT); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); INSERT INTO t VALUES (3); SELECT * FROM t WHERE a > 1;",
    );
    assert!(stdout.contains("2"));
    assert!(stdout.contains("3"));
    assert!(!stdout.trim().contains("\n1\n"));
}

#[test]
fn test_persistence() {
    let db = fresh_db("persist");
    run_sql(
        db.to_str().unwrap(),
        "CREATE TABLE t (id INT, name TEXT); INSERT INTO t VALUES (1, 'Alice');",
    );
    let (stdout, _, code) = run_sql(db.to_str().unwrap(), "SELECT name FROM t WHERE id = 1;");
    assert_eq!(code, 0);
    assert!(stdout.contains("Alice"));
}
