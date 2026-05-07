use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn minidb(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db").arg(dir.path().join("test.db"));
    cmd
}

fn eval(dir: &TempDir, sql: &str) -> assert_cmd::assert::Assert {
    minidb(dir).arg("--eval").arg(sql).arg("--quiet").assert()
}

#[test]
fn test_version() {
    Command::cargo_bin("minidb")
        .unwrap()
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn test_help() {
    Command::cargo_bin("minidb")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn test_create_and_select() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT, name TEXT);").success();
    eval(&dir, "INSERT INTO t VALUES (1, 'hello');").success();
    eval(&dir, "SELECT * FROM t;")
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn test_exit_code_sql_error() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "SELECT * FROM nonexistent;").failure().code(1);
}

#[test]
fn test_format_csv() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT, name TEXT);").success();
    eval(&dir, "INSERT INTO t VALUES (1, 'alice');").success();
    minidb(&dir)
        .arg("--eval")
        .arg("SELECT * FROM t;")
        .arg("--format")
        .arg("csv")
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("id,name"))
        .stdout(predicate::str::contains("1,alice"));
}

#[test]
fn test_format_json() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT, name TEXT);").success();
    eval(&dir, "INSERT INTO t VALUES (1, 'alice');").success();
    minidb(&dir)
        .arg("--eval")
        .arg("SELECT * FROM t;")
        .arg("--format")
        .arg("json")
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("alice"));
}

#[test]
fn test_file_mode() {
    let dir = TempDir::new().unwrap();
    let sql_file = dir.path().join("script.sql");
    fs::write(
        &sql_file,
        "CREATE TABLE t (x INT); INSERT INTO t VALUES (42); SELECT x FROM t;",
    )
    .unwrap();
    minidb(&dir)
        .arg("--file")
        .arg(&sql_file)
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("42"));
}

#[test]
fn test_file_mode_stop_on_error() {
    let dir = TempDir::new().unwrap();
    let sql_file = dir.path().join("script.sql");
    // Second statement will fail
    fs::write(
        &sql_file,
        "CREATE TABLE t (x INT);\nSELECT * FROM nonexistent;\nSELECT 1;\n",
    )
    .unwrap();
    minidb(&dir)
        .arg("--file")
        .arg(&sql_file)
        .arg("--quiet")
        .assert()
        .failure()
        .code(1);
}

#[test]
fn test_file_mode_continue_on_error() {
    let dir = TempDir::new().unwrap();
    let sql_file = dir.path().join("script.sql");
    fs::write(
        &sql_file,
        "CREATE TABLE t (x INT);\nSELECT * FROM nonexistent;\nINSERT INTO t VALUES (1);\n",
    )
    .unwrap();
    minidb(&dir)
        .arg("--file")
        .arg(&sql_file)
        .arg("--continue-on-error")
        .arg("--quiet")
        .assert()
        .success();
    // Verify the insert worked
    eval(&dir, "SELECT * FROM t;").success();
}

#[test]
fn test_persistence_across_invocations() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE users (id INT, name TEXT);").success();
    eval(&dir, "INSERT INTO users VALUES (1, 'alice');").success();
    eval(&dir, "INSERT INTO users VALUES (2, 'bob');").success();
    // New invocation
    eval(&dir, "SELECT * FROM users;")
        .success()
        .stdout(predicate::str::contains("alice"))
        .stdout(predicate::str::contains("bob"));
}

#[test]
fn test_transactions_commit() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    minidb(&dir)
        .arg("--eval")
        .arg("BEGIN; INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); COMMIT;")
        .arg("--quiet")
        .assert()
        .success();
    eval(&dir, "SELECT * FROM t;")
        .success()
        .stdout(predicate::str::contains("1"))
        .stdout(predicate::str::contains("2"));
}

#[test]
fn test_transactions_rollback() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    minidb(&dir)
        .arg("--eval")
        .arg("BEGIN; INSERT INTO t VALUES (99); ROLLBACK;")
        .arg("--quiet")
        .assert()
        .success();
    // Should be empty - table with just header and no data rows
    let out = minidb(&dir)
        .arg("--eval")
        .arg("SELECT * FROM t;")
        .arg("--quiet")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    // No data rows: text should not contain "99"
    assert!(!text.contains("99"), "rolled-back row should not appear");
}

#[test]
fn test_where_clause() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT, val INT);").success();
    for i in 1..=5 {
        eval(&dir, &format!("INSERT INTO t VALUES ({}, {});", i, i * 10)).success();
    }
    eval(&dir, "SELECT val FROM t WHERE id > 3;")
        .success()
        .stdout(predicate::str::contains("40"))
        .stdout(predicate::str::contains("50"));
}

#[test]
fn test_order_by() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    for i in [5, 3, 1, 4, 2] {
        eval(&dir, &format!("INSERT INTO t VALUES ({});", i)).success();
    }
    let output = minidb(&dir)
        .arg("--eval")
        .arg("SELECT x FROM t ORDER BY x ASC;")
        .arg("--quiet")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    let nums: Vec<i32> = text
        .lines()
        .filter_map(|l| l.trim().trim_matches('|').trim().parse().ok())
        .collect();
    assert_eq!(nums, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_limit_offset() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    for i in 1..=10 {
        eval(&dir, &format!("INSERT INTO t VALUES ({});", i)).success();
    }
    eval(&dir, "SELECT x FROM t ORDER BY x LIMIT 3 OFFSET 2;")
        .success()
        .stdout(predicate::str::contains("3"))
        .stdout(predicate::str::contains("4"))
        .stdout(predicate::str::contains("5"));
}

#[test]
fn test_update() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT, name TEXT);").success();
    eval(&dir, "INSERT INTO t VALUES (1, 'alice');").success();
    eval(&dir, "UPDATE t SET name = 'bob' WHERE id = 1;").success();
    eval(&dir, "SELECT name FROM t;")
        .success()
        .stdout(predicate::str::contains("bob"));
}

#[test]
fn test_delete() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT);").success();
    eval(
        &dir,
        "INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); INSERT INTO t VALUES (3);",
    )
    .success();
    eval(&dir, "DELETE FROM t WHERE id = 2;").success();
    let out = minidb(&dir)
        .arg("--eval")
        .arg("SELECT * FROM t;")
        .arg("--quiet")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(
        !text.contains("| 2 |") && !text.contains("| 2|"),
        "row 2 should be deleted"
    );
}

#[test]
fn test_not_null_constraint() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT NOT NULL);").success();
    eval(&dir, "INSERT INTO t VALUES (NULL);").failure().code(1);
}

#[test]
fn test_default_value() {
    let dir = TempDir::new().unwrap();
    eval(
        &dir,
        "CREATE TABLE t (id INT, status TEXT DEFAULT 'active');",
    )
    .success();
    eval(&dir, "INSERT INTO t (id) VALUES (1);").success();
    eval(&dir, "SELECT status FROM t;")
        .success()
        .stdout(predicate::str::contains("active"));
}

#[test]
fn test_create_index_and_use() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT, val TEXT);").success();
    eval(&dir, "CREATE INDEX idx_id ON t (id);").success();
    for i in 1..=20 {
        eval(&dir, &format!("INSERT INTO t VALUES ({}, 'v{}');", i, i)).success();
    }
    eval(&dir, "SELECT val FROM t WHERE id = 10;")
        .success()
        .stdout(predicate::str::contains("v10"));
}

#[test]
fn test_drop_table() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    eval(&dir, "DROP TABLE t;").success();
    eval(&dir, "SELECT * FROM t;").failure();
}

#[test]
fn test_inner_join() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE a (id INT, val INT);").success();
    eval(&dir, "CREATE TABLE b (aid INT, name TEXT);").success();
    eval(
        &dir,
        "INSERT INTO a VALUES (1, 10); INSERT INTO a VALUES (2, 20);",
    )
    .success();
    eval(
        &dir,
        "INSERT INTO b VALUES (1, 'one'); INSERT INTO b VALUES (2, 'two');",
    )
    .success();
    eval(
        &dir,
        "SELECT a.val, b.name FROM a INNER JOIN b ON a.id = b.aid WHERE a.id = 1;",
    )
    .success()
    .stdout(predicate::str::contains("one"))
    .stdout(predicate::str::contains("10"));
}

#[test]
fn test_null_semantics() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT, y INT);").success();
    eval(
        &dir,
        "INSERT INTO t VALUES (1, NULL); INSERT INTO t VALUES (2, 2);",
    )
    .success();
    eval(&dir, "SELECT x FROM t WHERE y IS NULL;")
        .success()
        .stdout(predicate::str::contains("1"));
    eval(&dir, "SELECT x FROM t WHERE y IS NOT NULL;")
        .success()
        .stdout(predicate::str::contains("2"));
}

#[test]
fn test_scalar_functions() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (name TEXT);").success();
    eval(&dir, "INSERT INTO t VALUES ('Hello World');").success();
    eval(
        &dir,
        "SELECT LENGTH(name), UPPER(name), LOWER(name) FROM t;",
    )
    .success()
    .stdout(predicate::str::contains("11"))
    .stdout(predicate::str::contains("HELLO WORLD"))
    .stdout(predicate::str::contains("hello world"));
}

#[test]
fn test_summary_on_stderr() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    minidb(&dir)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1);")
        .assert()
        .success()
        .stderr(predicate::str::contains("row"));
}

#[test]
fn test_quiet_flag() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT);").success();
    minidb(&dir)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1);")
        .arg("--quiet")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn test_meta_tables() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE users (id INT);").success();
    eval(&dir, "CREATE TABLE orders (id INT);").success();
    eval(&dir, ".tables")
        .success()
        .stdout(predicate::str::contains("users"))
        .stdout(predicate::str::contains("orders"));
}

#[test]
fn test_meta_indexes() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT);").success();
    eval(&dir, "CREATE INDEX idx_id ON t (id);").success();
    eval(&dir, ".indexes")
        .success()
        .stdout(predicate::str::contains("idx_id"));
}

#[test]
fn test_string_concatenation() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "SELECT 'hello' || ' ' || 'world';")
        .success()
        .stdout(predicate::str::contains("hello world"));
}

#[test]
fn test_arithmetic() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "SELECT 2 + 3 * 4;")
        .success()
        .stdout(predicate::str::contains("14"));
}

#[test]
fn test_coalesce() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x INT, y INT);").success();
    eval(&dir, "INSERT INTO t VALUES (NULL, 42);").success();
    eval(&dir, "SELECT COALESCE(x, y) FROM t;")
        .success()
        .stdout(predicate::str::contains("42"));
}

#[test]
fn test_bool_type() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (active BOOL);").success();
    eval(
        &dir,
        "INSERT INTO t VALUES (TRUE); INSERT INTO t VALUES (FALSE);",
    )
    .success();
    eval(&dir, "SELECT * FROM t WHERE active = TRUE;")
        .success()
        .stdout(predicate::str::contains("TRUE"));
}

#[test]
fn test_real_type() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (x REAL);").success();
    eval(&dir, "INSERT INTO t VALUES (3.14);").success();
    eval(&dir, "SELECT x FROM t;")
        .success()
        .stdout(predicate::str::contains("3.14"));
}

#[test]
fn test_init_flag() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("new.db");
    Command::cargo_bin("minidb")
        .unwrap()
        .arg("--db")
        .arg(&db_path)
        .arg("--init")
        .assert()
        .success();
}

#[test]
fn test_named_insert() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (a INT, b TEXT, c REAL);").success();
    eval(&dir, "INSERT INTO t (b, a) VALUES ('hi', 42);").success();
    eval(&dir, "SELECT a, b, c FROM t;")
        .success()
        .stdout(predicate::str::contains("42"))
        .stdout(predicate::str::contains("hi"))
        .stdout(predicate::str::contains("NULL"));
}

#[test]
fn test_drop_index() {
    let dir = TempDir::new().unwrap();
    eval(&dir, "CREATE TABLE t (id INT);").success();
    eval(&dir, "CREATE INDEX idx ON t (id);").success();
    eval(&dir, "DROP INDEX idx;").success();
    eval(&dir, ".indexes")
        .success()
        .stdout(predicate::str::contains("idx").not());
}

#[test]
fn test_multiple_statements_eval() {
    let dir = TempDir::new().unwrap();
    minidb(&dir)
        .arg("--eval")
        .arg("CREATE TABLE t (x INT); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2);")
        .arg("--quiet")
        .assert()
        .success();
    eval(&dir, "SELECT * FROM t;")
        .success()
        .stdout(predicate::str::contains("1"))
        .stdout(predicate::str::contains("2"));
}
