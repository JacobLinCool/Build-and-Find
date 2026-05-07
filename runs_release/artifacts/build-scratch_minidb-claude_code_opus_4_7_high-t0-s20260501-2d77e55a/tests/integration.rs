//! End-to-end integration tests for the `minidb` binary.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn db_path(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

fn minidb() -> Command {
    Command::cargo_bin("minidb").unwrap()
}

fn init(db: &PathBuf) {
    minidb()
        .arg("--init")
        .arg("--db")
        .arg(db)
        .assert()
        .success();
}

fn eval(db: &PathBuf, sql: &str) -> assert_cmd::assert::Assert {
    minidb()
        .arg("--db")
        .arg(db)
        .arg("--quiet")
        .arg("--eval")
        .arg(sql)
        .assert()
}

#[test]
fn create_insert_select_roundtrip() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d1");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT, b TEXT);").success();
    eval(&db, "INSERT INTO t VALUES (1, 'x'), (2, 'y'), (3, 'z');").success();
    eval(&db, "SELECT a, b FROM t ORDER BY a;")
        .success()
        .stdout(predicate::str::contains("1"))
        .stdout(predicate::str::contains("y"))
        .stdout(predicate::str::contains("z"));
}

#[test]
fn select_filter_and_aggregate_count() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d2");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT);").success();
    eval(&db, "INSERT INTO t VALUES (1), (2), (3), (4), (5);").success();
    eval(&db, "SELECT a FROM t WHERE a > 2 ORDER BY a;")
        .success()
        .stdout(predicate::str::contains("3"))
        .stdout(predicate::str::contains("4"))
        .stdout(predicate::str::contains("5"))
        .stdout(predicate::str::contains("1").not())
        .stdout(predicate::str::contains("2 ").not());
}

#[test]
fn join_two_tables() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d3");
    init(&db);
    eval(
        &db,
        "CREATE TABLE a(id INT, name TEXT); CREATE TABLE b(aid INT, val INT);",
    )
    .success();
    eval(
        &db,
        "INSERT INTO a VALUES (1,'foo'),(2,'bar'); INSERT INTO b VALUES (1, 10),(2, 20),(1, 30);",
    )
    .success();
    eval(
        &db,
        "SELECT a.name, b.val FROM a JOIN b ON a.id = b.aid ORDER BY a.name, b.val;",
    )
    .success()
    .stdout(predicate::str::contains("foo"))
    .stdout(predicate::str::contains("bar"))
    .stdout(predicate::str::contains("10"))
    .stdout(predicate::str::contains("30"));
}

#[test]
fn update_and_delete() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d4");
    init(&db);
    eval(&db, "CREATE TABLE t(id INT, v INT);").success();
    eval(&db, "INSERT INTO t VALUES (1, 10), (2, 20), (3, 30);").success();
    eval(&db, "UPDATE t SET v = v + 1 WHERE id = 2;").success();
    eval(&db, "DELETE FROM t WHERE id = 1;").success();
    eval(&db, "SELECT id, v FROM t ORDER BY id;")
        .success()
        .stdout(predicate::str::contains("21"))
        .stdout(predicate::str::contains("30"))
        .stdout(predicate::str::contains("10").not());
}

#[test]
fn transactions_commit_and_rollback() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d5");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT);").success();
    eval(&db, "BEGIN; INSERT INTO t VALUES (1); COMMIT;").success();
    eval(&db, "BEGIN; INSERT INTO t VALUES (99); ROLLBACK;").success();
    eval(&db, "SELECT a FROM t ORDER BY a;")
        .success()
        .stdout(predicate::str::contains("1"))
        .stdout(predicate::str::contains("99").not());
}

#[test]
fn persistence_across_runs() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d6");
    init(&db);
    eval(&db, "CREATE TABLE t(x INT); INSERT INTO t VALUES (42);").success();
    eval(&db, "SELECT x FROM t;")
        .success()
        .stdout(predicate::str::contains("42"));
}

#[test]
fn output_format_csv() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d7");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT, b TEXT);").success();
    eval(&db, "INSERT INTO t VALUES (1, 'hi, there');").success();
    minidb()
        .arg("--db")
        .arg(&db)
        .arg("--quiet")
        .arg("--format")
        .arg("csv")
        .arg("--eval")
        .arg("SELECT a, b FROM t;")
        .assert()
        .success()
        .stdout(predicate::str::contains("a,b"))
        .stdout(predicate::str::contains("\"hi, there\""));
}

#[test]
fn output_format_json() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d8");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT);").success();
    eval(&db, "INSERT INTO t VALUES (7);").success();
    minidb()
        .arg("--db")
        .arg(&db)
        .arg("--quiet")
        .arg("--format")
        .arg("json")
        .arg("--eval")
        .arg("SELECT a FROM t;")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"a\""))
        .stdout(predicate::str::contains("7"));
}

#[test]
fn null_handling_three_valued_logic() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d9");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT);").success();
    eval(&db, "INSERT INTO t VALUES (1), (NULL), (3);").success();
    // NULL comparisons are unknown, so WHERE filters them out.
    eval(&db, "SELECT a FROM t WHERE a > 0 ORDER BY a;")
        .success()
        .stdout(predicate::str::contains("1"))
        .stdout(predicate::str::contains("3"));
    // IS NULL works.
    eval(&db, "SELECT a FROM t WHERE a IS NULL;").success();
}

#[test]
fn index_scan_works() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d10");
    init(&db);
    eval(&db, "CREATE TABLE t(id INT, name TEXT);").success();
    eval(&db, "CREATE INDEX idx_t_id ON t(id);").success();
    eval(&db, "INSERT INTO t VALUES (1,'a'),(2,'b'),(3,'c'),(4,'d');").success();
    eval(&db, "SELECT name FROM t WHERE id = 3;")
        .success()
        .stdout(predicate::str::contains("c"));
}

#[test]
fn limit_and_offset() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "d11");
    init(&db);
    eval(&db, "CREATE TABLE t(a INT);").success();
    eval(&db, "INSERT INTO t VALUES (1),(2),(3),(4),(5);").success();
    eval(&db, "SELECT a FROM t ORDER BY a LIMIT 2 OFFSET 1;")
        .success()
        .stdout(predicate::str::contains("2"))
        .stdout(predicate::str::contains("3"))
        .stdout(predicate::str::contains("5").not());
}

#[test]
fn parse_error_exit_code_is_one() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "derr");
    init(&db);
    minidb()
        .arg("--db")
        .arg(&db)
        .arg("--eval")
        .arg("THIS IS NOT SQL;")
        .assert()
        .failure()
        .code(1);
}

#[test]
fn cli_error_exit_code_is_two() {
    minidb().arg("--bogus").assert().failure().code(2);
}

#[test]
fn meta_tables_lists_tables() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir, "dm");
    init(&db);
    eval(&db, "CREATE TABLE foo(a INT); CREATE TABLE bar(b INT);").success();
    eval(&db, ".tables")
        .success()
        .stdout(predicate::str::contains("foo"))
        .stdout(predicate::str::contains("bar"));
}
