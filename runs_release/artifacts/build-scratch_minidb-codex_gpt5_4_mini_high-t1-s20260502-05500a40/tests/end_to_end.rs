use assert_cmd::prelude::*;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn eval_creates_inserts_and_selects() {
    let dir = tempdir().expect("tempdir");
    let db = dir.path().join("minidb.db");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_minidb"));
    cmd.args([
        "--db",
        db.to_str().expect("db path"),
        "--eval",
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL); INSERT INTO t VALUES (1, 'alice'); SELECT id, name FROM t;",
    ]);
    let assert = cmd.assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8");
    assert!(stdout.contains("alice"));
    assert!(stderr.contains("ok"));
}

#[test]
fn data_persists_between_processes() {
    let dir = tempdir().expect("tempdir");
    let db = dir.path().join("persist.db");
    let mut init = Command::new(env!("CARGO_BIN_EXE_minidb"));
    init.args([
        "--db",
        db.to_str().expect("db path"),
        "--eval",
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'persisted');",
    ]);
    init.assert().success();

    let mut select = Command::new(env!("CARGO_BIN_EXE_minidb"));
    select.args([
        "--db",
        db.to_str().expect("db path"),
        "--eval",
        "SELECT name FROM t;",
    ]);
    let assert = select.assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.contains("persisted"));
}

#[test]
fn file_mode_can_continue_after_error() {
    let dir = tempdir().expect("tempdir");
    let db = dir.path().join("file.db");
    let script = dir.path().join("script.sql");
    fs::write(
        &script,
        "INSERT INTO missing VALUES (1);\nCREATE TABLE t (id INT PRIMARY KEY);\nINSERT INTO t VALUES (1);\nSELECT * FROM t;\n",
    )
    .expect("write script");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_minidb"));
    cmd.args([
        "--db",
        db.to_str().expect("db path"),
        "--file",
        script.to_str().expect("script path"),
        "--continue-on-error",
    ]);
    let assert = cmd.assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.contains('1'));
}

#[test]
fn json_output_is_valid() {
    let dir = tempdir().expect("tempdir");
    let db = dir.path().join("json.db");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_minidb"));
    cmd.args([
        "--db",
        db.to_str().expect("db path"),
        "--format",
        "json",
        "--eval",
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'jane'); SELECT id, name FROM t;",
    ]);
    let assert = cmd.assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.trim_start().starts_with('['));
    assert!(stdout.contains("\"jane\""));
}
