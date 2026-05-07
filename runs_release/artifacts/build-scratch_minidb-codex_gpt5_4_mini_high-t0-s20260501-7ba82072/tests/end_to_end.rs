use std::fs;
use std::process::Command;

fn bin() -> String {
    std::env::var("CARGO_BIN_EXE_minidb").expect("binary path")
}

fn run(args: &[&str]) -> (std::process::Output, String) {
    let bin = bin();
    let output = Command::new(bin).args(args).output().expect("run minidb");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output, stderr)
}

#[test]
fn eval_persists_across_processes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("mini.db");

    let (init_out, _) = run(&["--db", db.to_str().unwrap(), "--init"]);
    assert!(init_out.status.success());

    let (create_out, create_err) = run(&[
        "--db",
        db.to_str().unwrap(),
        "--eval",
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL); INSERT INTO t VALUES (1, 'a');",
    ]);
    assert!(create_out.status.success(), "{create_err}");
    assert!(create_err.contains("ok 0 rows affected"));
    assert!(create_err.contains("ok 1 row affected"));

    let (select_out, select_err) = run(&[
        "--db",
        db.to_str().unwrap(),
        "--format",
        "csv",
        "--eval",
        "SELECT * FROM t;",
    ]);
    assert!(select_out.status.success(), "{select_err}");
    let stdout = String::from_utf8_lossy(&select_out.stdout);
    assert!(stdout.contains("t.id,t.name"));
    assert!(stdout.contains("1,a"));
    assert!(select_err.contains("ok 1 row"));
}

#[test]
fn rollback_discards_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("mini.db");

    let (init_out, _) = run(&["--db", db.to_str().unwrap(), "--init"]);
    assert!(init_out.status.success());

    let (txn_out, txn_err) = run(&[
        "--db",
        db.to_str().unwrap(),
        "--eval",
        "BEGIN; CREATE TABLE x (id INT); ROLLBACK;",
    ]);
    assert!(txn_out.status.success(), "{txn_err}");

    let (select_out, select_err) =
        run(&["--db", db.to_str().unwrap(), "--eval", "SELECT * FROM x;"]);
    assert!(!select_out.status.success());
    assert!(select_err.contains("unknown table x"));
}

#[test]
fn file_mode_runs_scripts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("mini.db");
    let script = dir.path().join("script.sql");

    fs::write(
        &script,
        "CREATE TABLE y (id INT PRIMARY KEY, name TEXT);\nINSERT INTO y VALUES (1, 'one');\nSELECT * FROM y;\n",
    )
    .expect("write script");

    let (init_out, _) = run(&["--db", db.to_str().unwrap(), "--init"]);
    assert!(init_out.status.success());

    let (out, err) = run(&[
        "--db",
        db.to_str().unwrap(),
        "--file",
        script.to_str().unwrap(),
        "--format",
        "csv",
    ]);
    assert!(out.status.success(), "{err}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("1,one"));
}
