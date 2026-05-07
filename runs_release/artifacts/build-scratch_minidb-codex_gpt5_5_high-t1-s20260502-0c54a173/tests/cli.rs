use assert_cmd::Command;
use predicates::prelude::*;

fn db_path(name: &str) -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join(name);
    std::mem::forget(dir);
    path
}

#[test]
fn eval_runs_multiple_statements_and_prints_data_to_stdout() {
    let db = db_path("eval.db");
    Command::cargo_bin("minidb")
        .expect("binary")
        .args([
            "--db",
            db.to_str().expect("utf8 path"),
            "--eval",
            "create table people (id int primary key, name text); insert into people values (1, 'Ada'); select id, lower(name) as name from people;",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ada"))
        .stderr(predicate::str::contains("ok 1 row affected"));
}

#[test]
fn committed_wal_records_are_visible_to_a_new_process() {
    let db = db_path("recover.db");
    Command::cargo_bin("minidb")
        .expect("binary")
        .args([
            "--db",
            db.to_str().expect("utf8 path"),
            "--eval",
            "create table t (id int primary key); begin; insert into t values (7); commit;",
        ])
        .assert()
        .success();

    Command::cargo_bin("minidb")
        .expect("binary")
        .args([
            "--db",
            db.to_str().expect("utf8 path"),
            "--format",
            "json",
            "--eval",
            "select * from t;",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("7"));
}

#[test]
fn rollback_discards_changes() {
    let db = db_path("rollback.db");
    Command::cargo_bin("minidb")
        .expect("binary")
        .args([
            "--db",
            db.to_str().expect("utf8 path"),
            "--eval",
            "create table t (id int); begin; insert into t values (1); rollback; select * from t;",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("ok 0 rows"));
}

#[test]
fn sql_errors_exit_with_code_one() {
    let db = db_path("error.db");
    Command::cargo_bin("minidb")
        .expect("binary")
        .args([
            "--db",
            db.to_str().expect("utf8 path"),
            "--eval",
            "select from;",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("parse error"));
}
