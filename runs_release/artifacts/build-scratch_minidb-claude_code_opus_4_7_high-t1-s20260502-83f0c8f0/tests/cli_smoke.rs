//! End-to-end tests that spawn the `minidb` binary and check stdout/stderr
//! plus exit codes. These cover the deliverable contract directly.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn binary_path() -> PathBuf {
    // Cargo sets CARGO_BIN_EXE_minidb when running integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_minidb"))
}

fn tmp_db() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut p = std::env::temp_dir();
    p.push(format!("minidb_cli_{}_{}.db", std::process::id(), n));
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

struct RunOutput {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_eval(db: &Path, sql: &str, extra: &[&str]) -> RunOutput {
    let mut cmd = Command::new(binary_path());
    cmd.arg(db);
    for a in extra {
        cmd.arg(a);
    }
    cmd.arg("--eval").arg(sql);
    let out = cmd.output().expect("spawn minidb");
    RunOutput {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code().unwrap_or(-1),
    }
}

fn run_stdin(db: &Path, sql: &str, extra: &[&str]) -> RunOutput {
    let mut cmd = Command::new(binary_path());
    cmd.arg(db);
    for a in extra {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn minidb");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(sql.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    RunOutput {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code().unwrap_or(-1),
    }
}

#[test]
fn version_flag() {
    let out = Command::new(binary_path())
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("minidb"));
}

#[test]
fn unknown_flag_is_cli_error() {
    let out = Command::new(binary_path())
        .arg("--this-does-not-exist")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn select_table_format() {
    let db = tmp_db();
    let r = run_eval(
        &db,
        "CREATE TABLE t(id INT, name TEXT); INSERT INTO t VALUES (1,'a'), (2,'b'); SELECT * FROM t ORDER BY id;",
        &[],
    );
    assert_eq!(r.code, 0, "stderr={}", r.stderr);
    assert!(r.stdout.contains("| id "));
    assert!(r.stdout.contains("| 1  "));
    assert!(r.stdout.contains("| a "));
    cleanup(&db);
}

#[test]
fn select_csv_format_quotes_commas() {
    let db = tmp_db();
    let r = run_eval(
        &db,
        "CREATE TABLE t(s TEXT); INSERT INTO t VALUES ('a,b'); SELECT * FROM t;",
        &["--format", "csv"],
    );
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("\"a,b\""));
    cleanup(&db);
}

#[test]
fn select_json_format_null() {
    let db = tmp_db();
    let r = run_eval(
        &db,
        "CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES (1, NULL); SELECT * FROM t;",
        &["--format", "json"],
    );
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("\"b\":null"));
    cleanup(&db);
}

#[test]
fn sql_error_exit_code_is_one() {
    let db = tmp_db();
    let r = run_eval(&db, "SELECT * FROM nope", &[]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("error"));
    cleanup(&db);
}

#[test]
fn continue_on_error_keeps_going() {
    let db = tmp_db();
    let r = run_eval(
        &db,
        "CREATE TABLE t(id INT); SELECT * FROM nope; INSERT INTO t VALUES (1); SELECT * FROM t;",
        &["--continue-on-error"],
    );
    assert_eq!(r.code, 0, "stderr={}", r.stderr);
    cleanup(&db);
}

#[test]
fn meta_tables_indexes_schema() {
    let db = tmp_db();
    // Setup
    let s = run_eval(
        &db,
        "CREATE TABLE users(id INT PRIMARY KEY, name TEXT NOT NULL); CREATE INDEX idx_name ON users(name);",
        &[],
    );
    assert_eq!(s.code, 0);
    let r = run_stdin(&db, ".tables\n.indexes\n.schema users\n", &[]);
    assert_eq!(r.code, 0, "stderr={}", r.stderr);
    assert!(r.stdout.contains("users"));
    assert!(r.stdout.contains("idx_name"));
    assert!(r.stdout.contains("CREATE TABLE users"));
    cleanup(&db);
}

#[test]
fn persistence_across_invocations() {
    let db = tmp_db();
    let r1 = run_eval(
        &db,
        "CREATE TABLE t(id INT); INSERT INTO t VALUES (1), (2), (3);",
        &[],
    );
    assert_eq!(r1.code, 0);
    let r2 = run_eval(&db, "SELECT id FROM t ORDER BY id;", &["--format", "csv"]);
    assert_eq!(r2.code, 0);
    let lines: Vec<&str> = r2.stdout.trim().lines().collect();
    assert_eq!(lines, vec!["id", "1", "2", "3"]);
    cleanup(&db);
}
