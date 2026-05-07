use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn eval_executes_multiple_statements_and_prints_json_rows() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    Command::cargo_bin("minidb")
        .unwrap()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'Ada'); SELECT id, name FROM t;",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""name":"Ada""#))
        .stderr(predicate::str::contains("ok 1 row"));
}

#[test]
fn file_mode_stops_on_constraint_error() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let script = dir.path().join("script.sql");
    std::fs::write(
        &script,
        "CREATE TABLE t (id INT PRIMARY KEY); INSERT INTO t VALUES (1); INSERT INTO t VALUES (1);",
    )
    .unwrap();
    Command::cargo_bin("minidb")
        .unwrap()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--file",
            script.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("UNIQUE constraint failed"));
}

#[test]
fn init_creates_database_file() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("created.db");
    Command::cargo_bin("minidb")
        .unwrap()
        .args(["--db", db.to_str().unwrap(), "--init"])
        .assert()
        .success();
    assert!(db.exists());
}
