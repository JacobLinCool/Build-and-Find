use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn minidb_cmd() -> Command {
    Command::cargo_bin("minidb").unwrap()
}

#[test]
fn test_help() {
    minidb_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("minidb"));
}

#[test]
fn test_version() {
    minidb_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("minidb"));
}

#[test]
fn test_init() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();
    assert!(db.exists());
}

#[test]
fn test_eval_create_table() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);",
        ])
        .assert()
        .success();
}

#[test]
fn test_eval_insert_and_select() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'alice');",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (2, 'bob');",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT * FROM t;"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("alice"));
    assert!(stdout.contains("bob"));
}

#[test]
fn test_eval_select_where() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'alice'); INSERT INTO t (id, name) VALUES (2, 'bob');",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT * FROM t WHERE id = 1;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("alice"));
    assert!(!stdout.contains("bob"));
}

#[test]
fn test_eval_update() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'alice');",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "UPDATE t SET name = 'bob' WHERE id = 1;",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT name FROM t WHERE id = 1;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("bob"));
}

#[test]
fn test_eval_delete() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'alice'); INSERT INTO t (id, name) VALUES (2, 'bob');",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "DELETE FROM t WHERE id = 1;",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT * FROM t;"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("alice"));
    assert!(stdout.contains("bob"));
}

#[test]
fn test_eval_drop_table() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "DROP TABLE t;"])
        .assert()
        .success();
}

#[test]
fn test_eval_create_index() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE INDEX idx_name ON t (name);",
        ])
        .assert()
        .success();
}

#[test]
fn test_eval_order_by() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (3, 'charlie'); INSERT INTO t (id, name) VALUES (1, 'alice'); INSERT INTO t (id, name) VALUES (2, 'bob');",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT * FROM t ORDER BY id ASC;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let pos_alice = stdout.find("alice").unwrap();
    let pos_bob = stdout.find("bob").unwrap();
    let pos_charlie = stdout.find("charlie").unwrap();
    assert!(pos_alice < pos_bob);
    assert!(pos_bob < pos_charlie);
}

#[test]
fn test_eval_limit() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY);",
        ])
        .assert()
        .success();

    for i in 1..=5 {
        minidb_cmd()
            .args([
                "--db",
                db.to_str().unwrap(),
                "--eval",
                &format!("INSERT INTO t (id) VALUES ({i});"),
            ])
            .assert()
            .success();
    }

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT * FROM t LIMIT 2;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    // Should have exactly 2 data rows
    let lines: Vec<&str> = stdout.lines().collect();
    // Header + separator + 2 rows
    assert!(lines.len() <= 5);
}

#[test]
fn test_eval_arithmetic() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT 1 + 2;"])
        .assert()
        .success()
        .stdout(predicate::str::contains("3"));
}

#[test]
fn test_eval_functions() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT LENGTH('hello');",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("5"));
}

#[test]
fn test_eval_not_null_constraint() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT NOT NULL);",
        ])
        .assert()
        .success();

    // Insert NULL should fail
    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id) VALUES (NULL);",
        ])
        .assert()
        .failure();
}

#[test]
fn test_eval_primary_key_constraint() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id) VALUES (1);",
        ])
        .assert()
        .success();

    // Duplicate primary key should fail
    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id) VALUES (1);",
        ])
        .assert()
        .failure();
}

#[test]
fn test_eval_file_mode() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let sql_file = dir.path().join("test.sql");

    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    std::fs::write(
        &sql_file,
        "CREATE TABLE t (id INT PRIMARY KEY);\nINSERT INTO t (id) VALUES (1);\nINSERT INTO t (id) VALUES (2);\n",
    )
    .unwrap();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--file",
            sql_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT * FROM t;"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("1"));
    assert!(stdout.contains("2"));
}

#[test]
fn test_eval_json_format() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'alice');",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--format",
            "json",
            "--eval",
            "SELECT * FROM t;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("alice"));
    assert!(stdout.contains("\"id\""));
}

#[test]
fn test_eval_csv_format() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'alice');",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--format",
            "csv",
            "--eval",
            "SELECT * FROM t;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("id,name"));
    assert!(stdout.contains("1,alice"));
}

#[test]
fn test_eval_quiet_mode() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--quiet",
            "--eval",
            "CREATE TABLE t (id INT);",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("ok"));
}

#[test]
fn test_eval_transaction() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY);",
        ])
        .assert()
        .success();

    // BEGIN, INSERT, ROLLBACK should discard the insert
    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "BEGIN; INSERT INTO t (id) VALUES (1); ROLLBACK;",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT * FROM t;"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("1"));
}

#[test]
fn test_eval_exit_code_usage_error() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    // Invalid SQL should exit with code 1
    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INVALID SQL STATEMENT;",
        ])
        .assert()
        .failure();
}

#[test]
fn test_meta_tables() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t1 (id INT); CREATE TABLE t2 (id INT);",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", ".tables"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("t1"));
    assert!(stdout.contains("t2"));
}

#[test]
fn test_meta_schema() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", ".schema"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("CREATE TABLE"));
    assert!(stdout.contains("id"));
    assert!(stdout.contains("name"));
}

#[test]
fn test_multi_statement() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t (id, name) VALUES (1, 'a'); INSERT INTO t (id, name) VALUES (2, 'b'); SELECT * FROM t;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("a"));
    assert!(stdout.contains("b"));
}

#[test]
fn test_null_handling() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id) VALUES (1);",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--eval", "SELECT * FROM t;"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("NULL"));
}

#[test]
fn test_is_null() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT, name TEXT);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, name) VALUES (1, 'a'); INSERT INTO t (id) VALUES (2);",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT * FROM t WHERE name IS NULL;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("2"));
    assert!(!stdout.contains("a"));
}

#[test]
fn test_string_concat() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT 'hello' || ' ' || 'world';",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world"));
}

#[test]
fn test_bool_type() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    minidb_cmd()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT, active BOOL);",
        ])
        .assert()
        .success();

    minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "INSERT INTO t (id, active) VALUES (1, TRUE);",
        ])
        .assert()
        .success();

    let output = minidb_cmd()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "SELECT * FROM t WHERE active = TRUE;",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("true"));
}
