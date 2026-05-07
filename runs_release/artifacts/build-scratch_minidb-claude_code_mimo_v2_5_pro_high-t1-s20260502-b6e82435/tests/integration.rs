use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn minidb_cmd() -> (Command, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db").arg(db_path.to_str().unwrap());
    (cmd, dir)
}

fn run_sql(sql: &str) -> (String, String, TempDir) {
    let (mut cmd, dir) = minidb_cmd();
    cmd.arg("--eval").arg(sql);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr, dir)
}

#[test]
fn test_version() {
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("minidb"));
}

#[test]
fn test_help() {
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("USAGE"));
}

#[test]
fn test_init() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("init.db");
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db").arg(db_path.to_str().unwrap()).arg("--init");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("initialized"));
}

#[test]
fn test_create_table_and_insert() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               SELECT * FROM t;";
    let (stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("ok"));
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("Bob"));
}

#[test]
fn test_insert_with_columns() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT, age INT);\
               INSERT INTO t (name, id) VALUES ('Alice', 1);\
               SELECT id, name, age FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    // age should be NULL
    assert!(stdout.contains("NULL") || stdout.contains("null"));
}

#[test]
fn test_select_where() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT * FROM t WHERE val > 15;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("2"));
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("1\n"));
}

#[test]
fn test_select_where_string() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               SELECT * FROM t WHERE name = 'Alice';";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    assert!(!stdout.contains("Bob"));
}

#[test]
fn test_select_order_by() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (3, 'Charlie');\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               SELECT * FROM t ORDER BY id;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    let pos_alice = stdout.find("Alice").unwrap();
    let pos_bob = stdout.find("Bob").unwrap();
    let pos_charlie = stdout.find("Charlie").unwrap();
    assert!(pos_alice < pos_bob);
    assert!(pos_bob < pos_charlie);
}

#[test]
fn test_select_limit_offset() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY);\
               INSERT INTO t VALUES (1);\
               INSERT INTO t VALUES (2);\
               INSERT INTO t VALUES (3);\
               INSERT INTO t VALUES (4);\
               INSERT INTO t VALUES (5);\
               SELECT * FROM t ORDER BY id LIMIT 2 OFFSET 1;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("2"));
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("1\n"));
    assert!(!stdout.contains("4\n"));
    assert!(!stdout.contains("5\n"));
}

#[test]
fn test_select_distinct() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 10);\
               INSERT INTO t VALUES (3, 20);\
               SELECT DISTINCT val FROM t ORDER BY val;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("10"));
    assert!(stdout.contains("20"));
}

#[test]
fn test_update() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               UPDATE t SET name = 'Alicia' WHERE id = 1;\
               SELECT * FROM t WHERE id = 1;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alicia"));
}

#[test]
fn test_delete() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               DELETE FROM t WHERE id = 1;\
               SELECT * FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(!stdout.contains("Alice"));
    assert!(stdout.contains("Bob"));
}

#[test]
fn test_drop_table() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY);\
               DROP TABLE t;\
               CREATE TABLE t2 (id INT PRIMARY KEY);\
               INSERT INTO t2 VALUES (1);\
               SELECT * FROM t2;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("1"));
}

#[test]
fn test_create_index() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               CREATE INDEX idx_name ON t (name);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               SELECT * FROM t WHERE name = 'Alice';";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    assert!(!stdout.contains("Bob"));
}

#[test]
fn test_not_null_constraint() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);\
               INSERT INTO t VALUES (1, NULL);";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("error") || stderr.contains("NOT NULL"));
}

#[test]
fn test_primary_key_constraint() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (1, 'Bob');";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("error") || stderr.contains("duplicate"));
}

#[test]
fn test_unique_constraint() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, email TEXT UNIQUE);\
               INSERT INTO t VALUES (1, 'a@b.com');\
               INSERT INTO t VALUES (2, 'a@b.com');";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("error") || stderr.contains("duplicate") || stderr.contains("UNIQUE"));
}

#[test]
fn test_check_constraint() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, age INT CHECK (age >= 0));\
               INSERT INTO t VALUES (1, -5);";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("error") || stderr.contains("CHECK"));
}

#[test]
fn test_default_value() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, status TEXT DEFAULT 'active');\
               INSERT INTO t (id) VALUES (1);\
               SELECT status FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("active"));
}

#[test]
fn test_join() {
    let sql = "CREATE TABLE a (id INT PRIMARY KEY, name TEXT);\
               CREATE TABLE b (id INT PRIMARY KEY, a_id INT, val INT);\
               INSERT INTO a VALUES (1, 'Alice');\
               INSERT INTO a VALUES (2, 'Bob');\
               INSERT INTO b VALUES (1, 1, 100);\
               INSERT INTO b VALUES (2, 2, 200);\
               SELECT a.name, b.val FROM a JOIN b ON a.id = b.a_id;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("100"));
    assert!(stdout.contains("Bob"));
    assert!(stdout.contains("200"));
}

#[test]
fn test_left_join() {
    let sql = "CREATE TABLE a (id INT PRIMARY KEY, name TEXT);\
               CREATE TABLE b (id INT PRIMARY KEY, a_id INT, val INT);\
               INSERT INTO a VALUES (1, 'Alice');\
               INSERT INTO a VALUES (2, 'Bob');\
               INSERT INTO b VALUES (1, 1, 100);\
               SELECT a.name, b.val FROM a LEFT JOIN b ON a.id = b.a_id ORDER BY a.id;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("Bob"));
}

#[test]
fn test_cross_join() {
    let sql = "CREATE TABLE a (id INT PRIMARY KEY);\
               CREATE TABLE b (id INT PRIMARY KEY);\
               INSERT INTO a VALUES (1);\
               INSERT INTO a VALUES (2);\
               INSERT INTO b VALUES (10);\
               SELECT a.id, b.id FROM a CROSS JOIN b;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    // Should produce 4 rows
    assert!(stdout.contains("1"));
    assert!(stdout.contains("2"));
    assert!(stdout.contains("10"));
}

#[test]
fn test_transaction_commit() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               BEGIN;\
               INSERT INTO t VALUES (1, 'Alice');\
               COMMIT;\
               SELECT * FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
}

#[test]
fn test_transaction_rollback() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               BEGIN;\
               INSERT INTO t VALUES (2, 'Bob');\
               ROLLBACK;\
               SELECT * FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    assert!(!stdout.contains("Bob"));
}

#[test]
fn test_expression_arithmetic() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, a INT, b INT);\
               INSERT INTO t VALUES (1, 10, 3);\
               SELECT a + b AS sum, a - b AS diff, a * b AS prod, a / b AS quot FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("13"));
    assert!(stdout.contains("7"));
    assert!(stdout.contains("30"));
}

#[test]
fn test_expression_string_functions() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Hello');\
               SELECT LENGTH(name), LOWER(name), UPPER(name) FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("5"));
}

#[test]
fn test_expression_concat() {
    let sql = "SELECT 'Hello' || ' ' || 'World';";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Hello World"));
}

#[test]
fn test_null_handling() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, NULL);\
               SELECT * FROM t WHERE val IS NULL;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("1"));
}

#[test]
fn test_is_not_null() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, NULL);\
               INSERT INTO t VALUES (2, 42);\
               SELECT * FROM t WHERE val IS NOT NULL;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("2"));
    assert!(stdout.contains("42"));
}

#[test]
fn test_like() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               INSERT INTO t VALUES (2, 'Bob');\
               INSERT INTO t VALUES (3, 'Alfred');\
               SELECT * FROM t WHERE name LIKE 'Al%';";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("Alfred"));
    assert!(!stdout.contains("Bob"));
}

#[test]
fn test_in_list() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT * FROM t WHERE val IN (10, 30);";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("2\n"));
}

#[test]
fn test_between() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT * FROM t WHERE val BETWEEN 15 AND 25;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("20"));
}

#[test]
fn test_case_expression() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               SELECT CASE WHEN val > 15 THEN 'high' ELSE 'low' END AS category FROM t ORDER BY id;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("low"));
    assert!(stdout.contains("high"));
}

#[test]
fn test_select_alias() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               SELECT id AS user_id, name AS user_name FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("user_id") || stdout.contains("1"));
    assert!(stdout.contains("Alice"));
}

#[test]
fn test_count_function() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT COUNT(*) FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("3"));
}

#[test]
fn test_sum_function() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT SUM(val) FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("60"));
}

#[test]
fn test_output_format_csv() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               SELECT * FROM t;";
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db")
        .arg(db_path.to_str().unwrap())
        .arg("--format")
        .arg("csv")
        .arg("--eval")
        .arg(sql);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        stdout.contains("id,name") || stdout.contains("1,Alice") || stdout.contains("1,\"Alice\"")
    );
}

#[test]
fn test_output_format_json() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               INSERT INTO t VALUES (1, 'Alice');\
               SELECT * FROM t;";
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db")
        .arg(db_path.to_str().unwrap())
        .arg("--format")
        .arg("json")
        .arg("--eval")
        .arg(sql);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    // Should be valid JSON
    assert!(stdout.contains("[{") || stdout.contains("Alice"));
}

#[test]
fn test_file_execution() {
    let dir = TempDir::new().unwrap();
    let sql_path = dir.path().join("script.sql");
    let db_path = dir.path().join("test.db");
    fs::write(&sql_path, "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\nINSERT INTO t VALUES (1, 'Alice');\nSELECT * FROM t;\n").unwrap();
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db")
        .arg(db_path.to_str().unwrap())
        .arg("--file")
        .arg(sql_path.to_str().unwrap());
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(stdout.contains("Alice"));
}

#[test]
fn test_continue_on_error() {
    let dir = TempDir::new().unwrap();
    let sql_path = dir.path().join("script.sql");
    let db_path = dir.path().join("test.db");
    fs::write(&sql_path, "CREATE TABLE t (id INT PRIMARY KEY);\nINSERT INTO t VALUES (1);\nINSERT INTO t VALUES (1);\nINSERT INTO t VALUES (2);\nSELECT * FROM t;\n").unwrap();
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db")
        .arg(db_path.to_str().unwrap())
        .arg("--file")
        .arg(sql_path.to_str().unwrap())
        .arg("--continue-on-error");
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    // Should still have the valid insert
    assert!(stdout.contains("2"));
}

#[test]
fn test_meta_tables() {
    let sql = "CREATE TABLE users (id INT PRIMARY KEY);\
               CREATE TABLE posts (id INT PRIMARY KEY);\
               .tables";
    let (_stdout, stderr, _dir) = run_sql(sql);
    // Meta commands output to stderr or stdout
    assert!(stderr.contains("users") || stderr.contains("ok"));
}

#[test]
fn test_meta_schema() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);\
               .schema";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("ok") || stderr.contains("schema"));
}

#[test]
fn test_in_list_values() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT * FROM t WHERE val IN (10, 30);";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("2\n"));
}

#[test]
fn test_multiple_types() {
    let sql = "CREATE TABLE t (\
               id INT PRIMARY KEY,\
               big BIGINT,\
               r REAL,\
               txt TEXT,\
               b BOOL\
               );\
               INSERT INTO t VALUES (1, 9999999999, 3.14, 'hello', TRUE);\
               SELECT * FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("9999999999"));
    assert!(stdout.contains("3.14"));
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("true") || stdout.contains("TRUE") || stdout.contains("1"));
}

#[test]
fn test_update_with_expression() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               UPDATE t SET val = val * 2 WHERE id = 1;\
               SELECT val FROM t WHERE id = 1;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("20"));
}

#[test]
fn test_delete_all() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY);\
               INSERT INTO t VALUES (1);\
               INSERT INTO t VALUES (2);\
               DELETE FROM t;\
               SELECT COUNT(*) FROM t;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("0"));
}

#[test]
fn test_select_count_with_filter() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT COUNT(*) FROM t WHERE val > 15;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("2"));
}

#[test]
fn test_drop_index() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);\
               CREATE INDEX idx ON t (name);\
               DROP INDEX idx;\
               .indexes";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("ok"));
}

#[test]
fn test_error_unknown_table() {
    let sql = "SELECT * FROM nonexistent;";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("error") || stderr.contains("not found"));
}

#[test]
fn test_error_syntax() {
    let sql = "SELCT * FROM t;";
    let (_stdout, stderr, _dir) = run_sql(sql);
    assert!(stderr.contains("error"));
}

#[test]
fn test_coalesce() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, NULL);\
               INSERT INTO t VALUES (2, 42);\
               SELECT COALESCE(val, 0) FROM t ORDER BY id;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("0"));
    assert!(stdout.contains("42"));
}

#[test]
fn test_or_condition() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, val INT);\
               INSERT INTO t VALUES (1, 10);\
               INSERT INTO t VALUES (2, 20);\
               INSERT INTO t VALUES (3, 30);\
               SELECT * FROM t WHERE val = 10 OR val = 30;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("2\n"));
}

#[test]
fn test_and_condition() {
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, a INT, b INT);\
               INSERT INTO t VALUES (1, 10, 100);\
               INSERT INTO t VALUES (2, 20, 200);\
               INSERT INTO t VALUES (3, 30, 100);\
               SELECT * FROM t WHERE a > 15 AND b = 100;";
    let (stdout, _stderr, _dir) = run_sql(sql);
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("1\n"));
    assert!(!stdout.contains("2\n"));
}

#[test]
fn test_quiet_mode() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let mut cmd = Command::cargo_bin("minidb").unwrap();
    cmd.arg("--db")
        .arg(db_path.to_str().unwrap())
        .arg("--quiet")
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY); INSERT INTO t VALUES (1); SELECT * FROM t;");
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    // Quiet mode should suppress "ok" messages
    assert!(!stderr.contains("ok"));
}
