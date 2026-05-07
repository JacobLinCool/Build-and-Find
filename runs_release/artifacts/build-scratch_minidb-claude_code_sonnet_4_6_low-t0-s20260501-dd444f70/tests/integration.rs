use std::process::Command;
use tempfile::NamedTempFile;

fn minidb_binary() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    // When running tests, the binary is in deps/
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("minidb");
    path.to_string_lossy().to_string()
}

fn run_sql(db_path: &str, sql: &str) -> (String, String, i32) {
    let output = Command::new(minidb_binary())
        .args(["--db", db_path, "--eval", sql])
        .output()
        .expect("failed to run minidb");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

fn run_sql_quiet(db_path: &str, sql: &str) -> String {
    let output = Command::new(minidb_binary())
        .args(["--db", db_path, "--quiet", "--eval", sql])
        .output()
        .expect("failed to run minidb");
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_create_table_and_insert_select() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let (_, _, code) = run_sql(
        &db,
        "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);",
    );
    assert_eq!(code, 0);

    let (_, _, code) = run_sql(&db, "INSERT INTO users VALUES (1, 'alice');");
    assert_eq!(code, 0);

    let (_, _, code) = run_sql(&db, "INSERT INTO users VALUES (2, 'bob');");
    assert_eq!(code, 0);

    let out = run_sql_quiet(&db, "SELECT * FROM users;");
    assert!(out.contains("alice"));
    assert!(out.contains("bob"));
}

#[test]
fn test_select_where() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, 'a');");
    run_sql(&db, "INSERT INTO t VALUES (2, 'b');");
    run_sql(&db, "INSERT INTO t VALUES (3, 'c');");

    let out = run_sql_quiet(&db, "SELECT val FROM t WHERE id > 1;");
    assert!(out.contains("b"));
    assert!(out.contains("c"));
    assert!(!out.contains("| a |") && !out.contains("| a"));
}

#[test]
fn test_named_insert() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(
        &db,
        "CREATE TABLE t (id INT, name TEXT, age INT DEFAULT 25);",
    );
    run_sql(&db, "INSERT INTO t (id, name) VALUES (1, 'alice');");

    let out = run_sql_quiet(&db, "SELECT age FROM t WHERE id = 1;");
    assert!(out.contains("25"));
}

#[test]
fn test_update() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val INT);");
    run_sql(&db, "INSERT INTO t VALUES (1, 10);");
    run_sql(&db, "UPDATE t SET val = 99 WHERE id = 1;");

    let out = run_sql_quiet(&db, "SELECT val FROM t;");
    assert!(out.contains("99"));
    assert!(!out.contains("10"));
}

#[test]
fn test_delete() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, 'keep');");
    run_sql(&db, "INSERT INTO t VALUES (2, 'remove');");
    run_sql(&db, "DELETE FROM t WHERE id = 2;");

    let out = run_sql_quiet(&db, "SELECT * FROM t;");
    assert!(out.contains("keep"));
    assert!(!out.contains("remove"));
}

#[test]
fn test_order_by() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val INT);");
    run_sql(&db, "INSERT INTO t VALUES (3, 30);");
    run_sql(&db, "INSERT INTO t VALUES (1, 10);");
    run_sql(&db, "INSERT INTO t VALUES (2, 20);");

    let out = run_sql_quiet(&db, "SELECT val FROM t ORDER BY val ASC;");
    let pos10 = out.find("10").unwrap();
    let pos20 = out.find("20").unwrap();
    let pos30 = out.find("30").unwrap();
    assert!(pos10 < pos20 && pos20 < pos30);
}

#[test]
fn test_limit_offset() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT);");
    for i in 1..=10 {
        run_sql(&db, &format!("INSERT INTO t VALUES ({});", i));
    }

    let out = run_sql_quiet(&db, "SELECT id FROM t ORDER BY id LIMIT 3;");
    assert!(out.contains("1"));
    assert!(out.contains("2"));
    assert!(out.contains("3"));
    assert!(!out.contains("4"));
}

#[test]
fn test_inner_join() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE a (id INT, name TEXT);");
    run_sql(&db, "CREATE TABLE b (id INT, score INT);");
    run_sql(&db, "INSERT INTO a VALUES (1, 'alice');");
    run_sql(&db, "INSERT INTO a VALUES (2, 'bob');");
    run_sql(&db, "INSERT INTO b VALUES (1, 95);");
    run_sql(&db, "INSERT INTO b VALUES (2, 87);");

    let out = run_sql_quiet(
        &db,
        "SELECT a.name, b.score FROM a INNER JOIN b ON a.id = b.id;",
    );
    assert!(out.contains("alice"));
    assert!(out.contains("95"));
    assert!(out.contains("bob"));
    assert!(out.contains("87"));
}

#[test]
fn test_null_handling() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, NULL);");
    run_sql(&db, "INSERT INTO t VALUES (2, 'hello');");

    let out = run_sql_quiet(&db, "SELECT * FROM t WHERE val IS NULL;");
    assert!(out.contains("1"));
    assert!(!out.contains("hello"));

    let out2 = run_sql_quiet(&db, "SELECT * FROM t WHERE val IS NOT NULL;");
    assert!(out2.contains("hello"));
}

#[test]
fn test_functions() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let out = run_sql_quiet(&db, "SELECT LENGTH('hello'), UPPER('world'), LOWER('ABC');");
    assert!(out.contains("5"));
    assert!(out.contains("WORLD"));
    assert!(out.contains("abc"));
}

#[test]
fn test_coalesce() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, NULL);");

    let out = run_sql_quiet(&db, "SELECT COALESCE(val, 'default') FROM t;");
    assert!(out.contains("default"));
}

#[test]
fn test_drop_table() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT);");
    let (_, _, code) = run_sql(&db, "DROP TABLE t;");
    assert_eq!(code, 0);

    let (_, stderr, code) = run_sql(&db, "SELECT * FROM t;");
    assert_ne!(code, 0);
    assert!(stderr.contains("not found") || stderr.contains("error") || stderr.contains("Error"));
}

#[test]
fn test_create_and_use_index() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE users (id INT, age INT, name TEXT);");
    run_sql(&db, "INSERT INTO users VALUES (1, 25, 'alice');");
    run_sql(&db, "INSERT INTO users VALUES (2, 30, 'bob');");
    run_sql(&db, "INSERT INTO users VALUES (3, 25, 'carol');");
    run_sql(&db, "CREATE INDEX idx_age ON users (age);");

    let out = run_sql_quiet(&db, "SELECT name FROM users WHERE age = 25;");
    assert!(out.contains("alice"));
    assert!(out.contains("carol"));
    assert!(!out.contains("bob"));
}

#[test]
fn test_transaction_commit() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT);");
    run_sql(&db, "BEGIN; INSERT INTO t VALUES (42); COMMIT;");

    let out = run_sql_quiet(&db, "SELECT * FROM t;");
    assert!(out.contains("42"));
}

#[test]
fn test_exit_code_sql_error() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let (_, _, code) = run_sql(&db, "SELECT * FROM nonexistent_table;");
    assert_ne!(code, 0);
}

#[test]
fn test_csv_format() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, name TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, 'alice');");

    let output = Command::new(minidb_binary())
        .args([
            "--db",
            &db,
            "--quiet",
            "--format",
            "csv",
            "--eval",
            "SELECT * FROM t;",
        ])
        .output()
        .expect("failed to run minidb");
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(out.contains("id,name") || out.contains("id") && out.contains("name"));
    assert!(out.contains("1") && out.contains("alice"));
}

#[test]
fn test_json_format() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, 'hello');");

    let output = Command::new(minidb_binary())
        .args([
            "--db",
            &db,
            "--quiet",
            "--format",
            "json",
            "--eval",
            "SELECT * FROM t;",
        ])
        .output()
        .expect("failed to run minidb");
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(out.contains("{") && out.contains("hello"));
}

#[test]
fn test_file_mode() {
    let tmp_db = NamedTempFile::new().unwrap();
    let db = tmp_db.path().to_string_lossy().to_string();

    let sql_content =
        "CREATE TABLE t (id INT, val TEXT);\nINSERT INTO t VALUES (1, 'from_file');\n";
    let sql_file = tempfile::Builder::new().suffix(".sql").tempfile().unwrap();
    std::fs::write(sql_file.path(), sql_content).unwrap();

    let output = Command::new(minidb_binary())
        .args([
            "--db",
            &db,
            "--quiet",
            "--file",
            &sql_file.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run minidb");
    assert_eq!(output.status.code().unwrap_or(-1), 0);

    let out = run_sql_quiet(&db, "SELECT * FROM t;");
    assert!(out.contains("from_file"));
}

#[test]
fn test_init_flag() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let output = Command::new(minidb_binary())
        .args(["--db", &db, "--init"])
        .output()
        .expect("failed to run minidb");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    assert!(tmp.path().exists());
}

#[test]
fn test_string_concat() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let out = run_sql_quiet(&db, "SELECT 'hello' || ' ' || 'world';");
    assert!(out.contains("hello world"));
}

#[test]
fn test_persistence_across_restarts() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, msg TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, 'persistent');");

    // Second process reads the data
    let out = run_sql_quiet(&db, "SELECT msg FROM t;");
    assert!(out.contains("persistent"));
}

#[test]
fn test_not_null_constraint() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT NOT NULL, val TEXT);");
    let (_, stderr, code) = run_sql(&db, "INSERT INTO t VALUES (NULL, 'test');");
    assert_ne!(code, 0);
    assert!(stderr.len() > 0);
}

#[test]
fn test_arithmetic_expressions() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (a INT, b INT);");
    run_sql(&db, "INSERT INTO t VALUES (10, 3);");

    let out = run_sql_quiet(&db, "SELECT a + b, a - b, a * b FROM t;");
    assert!(out.contains("13"));
    assert!(out.contains("7"));
    assert!(out.contains("30"));
}

#[test]
fn test_substr_function() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let out = run_sql_quiet(&db, "SELECT SUBSTR('hello world', 7, 5);");
    assert!(out.contains("world"));
}

#[test]
fn test_abs_function() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    let out = run_sql_quiet(&db, "SELECT ABS(-42);");
    assert!(out.contains("42"));
}

#[test]
fn test_is_null_in_expression() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val TEXT);");
    run_sql(&db, "INSERT INTO t VALUES (1, NULL);");
    run_sql(&db, "INSERT INTO t VALUES (2, 'hi');");

    // NULL = NULL is UNKNOWN (not true), so only IS NULL works
    let out = run_sql_quiet(&db, "SELECT id FROM t WHERE val IS NULL;");
    assert!(out.contains("1"));
    assert!(!out.contains("2"));
}

#[test]
fn test_meta_tables() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE my_table (id INT);");
    let out = run_sql_quiet(&db, ".tables");
    assert!(out.to_lowercase().contains("my_table"));
}

#[test]
fn test_meta_schema() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE my_table (id INT, name TEXT);");
    let out = run_sql_quiet(&db, ".schema");
    assert!(out.to_lowercase().contains("my_table") || out.contains("MY_TABLE"));
}

#[test]
fn test_drop_index() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT, val INT);");
    run_sql(&db, "CREATE INDEX idx ON t (val);");
    let (_, _, code) = run_sql(&db, "DROP INDEX idx;");
    assert_eq!(code, 0);
}

#[test]
fn test_if_not_exists() {
    let tmp = NamedTempFile::new().unwrap();
    let db = tmp.path().to_string_lossy().to_string();

    run_sql(&db, "CREATE TABLE t (id INT);");
    let (_, _, code) = run_sql(&db, "CREATE TABLE IF NOT EXISTS t (id INT);");
    assert_eq!(code, 0);
}
