use std::process::Command;

#[test]
fn create_insert_select_update_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("test.db");
    let exe = env!("CARGO_BIN_EXE_minidb");
    let sql = "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0, CHECK (age >= 0));
INSERT INTO users (id, name, age) VALUES (1, 'Ada', 36);
INSERT INTO users (id, name) VALUES (2, 'Bob');
CREATE INDEX users_age ON users (age);
SELECT name, age FROM users WHERE age >= 10 ORDER BY age DESC;
UPDATE users SET age = age + 1 WHERE id = 2;
DELETE FROM users WHERE id = 1;
SELECT id, age FROM users;";
    let out = Command::new(exe)
        .arg("--db")
        .arg(&db)
        .arg("--eval")
        .arg(sql)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Ada"));
    assert!(stdout.contains("2"));
}

#[test]
fn rollback_discards_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("test.db");
    let exe = env!("CARGO_BIN_EXE_minidb");
    let sql = "CREATE TABLE t (id INT PRIMARY KEY); BEGIN; INSERT INTO t VALUES (1); ROLLBACK; SELECT * FROM t;";
    let out = Command::new(exe)
        .arg("--db")
        .arg(&db)
        .arg("--eval")
        .arg(sql)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("\n1\n"));
}
