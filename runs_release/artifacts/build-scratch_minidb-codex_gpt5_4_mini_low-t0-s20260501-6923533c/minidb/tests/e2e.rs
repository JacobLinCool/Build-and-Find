use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn eval_create_insert_select() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.args([
        "--db",
        db.to_str().unwrap(),
        "--eval",
        "CREATE TABLE t (id INT); INSERT INTO t VALUES (1); SELECT * FROM t;",
    ])
    .assert()
    .success();
}
