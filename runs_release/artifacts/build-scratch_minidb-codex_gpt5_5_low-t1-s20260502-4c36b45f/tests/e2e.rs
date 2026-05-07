use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn creates_inserts_selects() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    Command::cargo_bin("minidb").unwrap()
        .args(["--db", db.to_str().unwrap(), "--eval", "create table t (id int primary key, name text default 'x'); insert into t (id) values (1); select id,name from t;"])
        .assert()
        .success()
        .stdout(predicate::str::contains("id"))
        .stdout(predicate::str::contains("x"));
}

#[test]
fn rolls_back_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    Command::cargo_bin("minidb")
        .unwrap()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--eval",
            "create table t (id int); begin; insert into t values (1); rollback; select * from t;",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("rollback"));
}
