use std::process::Command;

fn bin() -> std::path::PathBuf {
    assert!(std::env::var("CARGO_BIN_EXE_minidb").is_ok());
    std::env::var("CARGO_BIN_EXE_minidb").unwrap().into()
}

#[test]
fn init_and_eval_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");

    let init = Command::new(bin())
        .args(["--init", "--db"])
        .arg(&db)
        .output()
        .unwrap();
    assert!(init.status.success());

    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'alice'); SELECT * FROM t;";
    let out = Command::new(bin())
        .args(["--db"])
        .arg(&db)
        .args(["--eval", sql])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("alice"));
}
