use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn minidb_cmd() -> Command {
    Command::cargo_bin("minidb").unwrap()
}

fn with_db(test: impl FnOnce(&str)) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db_str = db_path.to_str().unwrap();
    test(db_str);
}

#[test]
fn test_help() {
    minidb_cmd().arg("--help").assert().success();
}

#[test]
fn test_version() {
    minidb_cmd()
        .arg("--version")
        .assert()
        .success()
        .stderr(predicate::str::contains("minidb"));
}

#[test]
fn test_init() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success()
            .stderr(predicate::str::contains("ok"));
    });
}

#[test]
fn test_create_table() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);",
            ])
            .assert()
            .success()
            .stderr(predicate::str::contains("ok"));
    });
}

#[test]
fn test_insert_and_select() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 'Alice');"])
            .assert()
            .success()
            .stderr(predicate::str::contains("1 row"));

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (2, 'Bob');"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT * FROM t;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Alice"))
            .stdout(predicate::str::contains("Bob"));
    });
}

#[test]
fn test_insert_named_columns() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT, name TEXT, age INT DEFAULT 0);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "INSERT INTO t (id, name) VALUES (1, 'Alice');",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT * FROM t;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Alice"));
    });
}

#[test]
fn test_update() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 'Alice');"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "UPDATE t SET name = 'Bob' WHERE id = 1;",
            ])
            .assert()
            .success()
            .stderr(predicate::str::contains("1 row"));

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT name FROM t WHERE id = 1;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Bob"));
    });
}

#[test]
fn test_delete() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 'Alice');"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "DELETE FROM t WHERE id = 1;"])
            .assert()
            .success()
            .stderr(predicate::str::contains("1 row"));

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT * FROM t;"])
            .assert()
            .success();
    });
}

#[test]
fn test_where_clause() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT, name TEXT, age INT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 'Alice', 30); INSERT INTO t VALUES (2, 'Bob', 25); INSERT INTO t VALUES (3, 'Charlie', 35);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT name FROM t WHERE age > 28;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Alice"))
            .stdout(predicate::str::contains("Charlie"));
    });
}

#[test]
fn test_order_by() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "CREATE TABLE t (id INT, name TEXT);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (3, 'Charlie'); INSERT INTO t VALUES (1, 'Alice'); INSERT INTO t VALUES (2, 'Bob');"])
            .assert()
            .success();

        let output = minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT name FROM t ORDER BY id;"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let alice_pos = stdout.find("Alice").unwrap();
        let bob_pos = stdout.find("Bob").unwrap();
        let charlie_pos = stdout.find("Charlie").unwrap();
        assert!(alice_pos < bob_pos);
        assert!(bob_pos < charlie_pos);
    });
}

#[test]
fn test_limit_offset() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "CREATE TABLE t (id INT);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); INSERT INTO t VALUES (3); INSERT INTO t VALUES (4); INSERT INTO t VALUES (5);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "SELECT id FROM t ORDER BY id LIMIT 2 OFFSET 1;",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("2"))
            .stdout(predicate::str::contains("3"));
    });
}

#[test]
fn test_create_index() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "CREATE INDEX idx_name ON t (name);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", ".indexes"])
            .assert()
            .success()
            .stdout(predicate::str::contains("idx_name"));
    });
}

#[test]
fn test_drop_table() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "CREATE TABLE t (id INT);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "DROP TABLE t;"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", ".tables"])
            .assert()
            .success();
    });
}

#[test]
fn test_meta_tables() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE users (id INT); CREATE TABLE posts (id INT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", ".tables"])
            .assert()
            .success()
            .stdout(predicate::str::contains("users"))
            .stdout(predicate::str::contains("posts"));
    });
}

#[test]
fn test_meta_schema() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", ".schema"])
            .assert()
            .success()
            .stdout(predicate::str::contains("CREATE TABLE"))
            .stdout(predicate::str::contains("PRIMARY KEY"));
    });
}

#[test]
fn test_null_handling() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "CREATE TABLE t (id INT, name TEXT);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, NULL);"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "SELECT name FROM t WHERE name IS NULL;",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("NULL"));

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "SELECT name FROM t WHERE name IS NOT NULL;",
            ])
            .assert()
            .success();
    });
}

#[test]
fn test_expressions() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT 1 + 2 AS result;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("3"));
    });
}

#[test]
fn test_functions() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (name TEXT); INSERT INTO t VALUES ('Hello');",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT LENGTH(name) FROM t;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("5"));

        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT UPPER(name) FROM t;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("HELLO"));
    });
}

#[test]
fn test_csv_format() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT, name TEXT); INSERT INTO t VALUES (1, 'Alice');",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--format", "csv", "--eval", "SELECT * FROM t;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("id,name"))
            .stdout(predicate::str::contains("1,Alice"));
    });
}

#[test]
fn test_json_format() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT, name TEXT); INSERT INTO t VALUES (1, 'Alice');",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--format", "json", "--eval", "SELECT * FROM t;"])
            .assert()
            .success()
            .stdout(predicate::str::contains("\"id\""))
            .stdout(predicate::str::contains("\"name\""));
    });
}

#[test]
fn test_unique_constraint() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT UNIQUE, name TEXT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 'Alice');"])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 'Bob');"])
            .assert()
            .failure();
    });
}

#[test]
fn test_not_null_constraint() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT, name TEXT NOT NULL);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, NULL);"])
            .assert()
            .failure();
    });
}

#[test]
fn test_file_mode() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let script_path = dir.path().join("test.sql");
    let db_str = db_path.to_str().unwrap();
    let script_str = script_path.to_str().unwrap();

    minidb_cmd()
        .args(&["--db", db_str, "--init"])
        .assert()
        .success();

    std::fs::write(&script_path, "CREATE TABLE t (id INT, name TEXT);\nINSERT INTO t VALUES (1, 'Alice');\nINSERT INTO t VALUES (2, 'Bob');\n").unwrap();

    minidb_cmd()
        .args(&["--db", db_str, "--file", script_str])
        .assert()
        .success();

    minidb_cmd()
        .args(&["--db", db_str, "--eval", "SELECT * FROM t;"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Alice"))
        .stdout(predicate::str::contains("Bob"));
}

#[test]
fn test_quiet_mode() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init", "--quiet"])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
    });
}

#[test]
fn test_exit_code_on_error() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        // SQL error should exit with code 1
        minidb_cmd()
            .args(&["--db", db, "--eval", "SELECT * FROM nonexistent;"])
            .assert()
            .failure();
    });
}

#[test]
fn test_join() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE a (id INT, val INT); CREATE TABLE b (aid INT, info TEXT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO a VALUES (1, 10); INSERT INTO a VALUES (2, 20); INSERT INTO b VALUES (1, 'info1');"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "SELECT a.val, b.info FROM a INNER JOIN b ON a.id = b.aid;",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("10"))
            .stdout(predicate::str::contains("info1"));
    });
}

#[test]
fn test_and_or() {
    with_db(|db| {
        minidb_cmd()
            .args(&["--db", db, "--init"])
            .assert()
            .success();

        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "CREATE TABLE t (id INT, a INT, b INT);",
            ])
            .assert()
            .success();

        minidb_cmd()
            .args(&["--db", db, "--eval", "INSERT INTO t VALUES (1, 1, 1); INSERT INTO t VALUES (2, 1, 0); INSERT INTO t VALUES (3, 0, 1); INSERT INTO t VALUES (4, 0, 0);"])
            .assert()
            .success();

        // AND
        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "SELECT id FROM t WHERE a = 1 AND b = 1;",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("1"));

        // OR
        minidb_cmd()
            .args(&[
                "--db",
                db,
                "--eval",
                "SELECT id FROM t WHERE a = 1 OR b = 1;",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("1"))
            .stdout(predicate::str::contains("2"))
            .stdout(predicate::str::contains("3"));
    });
}
