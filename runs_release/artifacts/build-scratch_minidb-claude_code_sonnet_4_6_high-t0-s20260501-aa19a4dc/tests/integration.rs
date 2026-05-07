/// End-to-end integration tests for minidb.
/// Tests are organized by feature area.
use minidb::cli::{run_eval, Session};
use minidb::exec::ExecResult;
use minidb::types::Value;
use tempfile::TempDir;

fn open_session(dir: &TempDir) -> Session {
    Session::open(dir.path().join("test.db"), true).unwrap()
}

fn exec(session: &mut Session, sql: &str) -> ExecResult {
    let mut results = session.execute_sql(sql).unwrap();
    results.remove(0).0
}

fn exec_rows(session: &mut Session, sql: &str) -> Vec<Vec<Value>> {
    match exec(session, sql) {
        ExecResult::Rows { rows, .. } => rows,
        other => panic!("expected rows, got {:?}", other),
    }
}

fn exec_count(session: &mut Session, sql: &str) -> usize {
    match exec(session, sql) {
        ExecResult::RowsAffected(n) => n,
        other => panic!("expected RowsAffected, got {:?}", other),
    }
}

// ─── DDL ─────────────────────────────────────────────────────────────────────

#[test]
fn test_create_and_drop_table() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, name TEXT);");
    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 0);

    exec(&mut s, "DROP TABLE t;");
    let result = s.execute_sql("SELECT * FROM t;");
    assert!(result.is_err(), "should error after drop");
}

#[test]
fn test_create_table_if_not_exists() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    // Should not error
    exec(&mut s, "CREATE TABLE IF NOT EXISTS t (id INT);");
}

#[test]
fn test_create_index_and_drop() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE users (id INT, name TEXT);");
    exec(&mut s, "CREATE INDEX idx_users_id ON users (id);");
    exec(&mut s, "INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob');");

    let rows = exec_rows(&mut s, "SELECT * FROM users WHERE id = 1;");
    assert_eq!(rows.len(), 1);

    exec(&mut s, "DROP INDEX idx_users_id;");
}

// ─── DML ─────────────────────────────────────────────────────────────────────

#[test]
fn test_insert_positional() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, name TEXT, score REAL);");
    let count = exec_count(&mut s, "INSERT INTO t VALUES (1, 'Alice', 99.5);");
    assert_eq!(count, 1);

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[0][1], Value::Text("Alice".into()));
}

#[test]
fn test_insert_named_columns() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(
        &mut s,
        "CREATE TABLE t (id INT, name TEXT, score REAL DEFAULT 0.0);",
    );
    exec(&mut s, "INSERT INTO t (id, name) VALUES (1, 'Bob');");

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::Int(1));
    // score should be default 0.0
    assert_eq!(rows[0][2], Value::Real(0.0));
}

#[test]
fn test_insert_multiple_rows() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    let count = exec_count(&mut s, "INSERT INTO t VALUES (1), (2), (3);");
    assert_eq!(count, 3);

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 3);
}

#[test]
fn test_update() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, val TEXT);");
    exec(&mut s, "INSERT INTO t VALUES (1, 'hello'), (2, 'world');");

    let count = exec_count(&mut s, "UPDATE t SET val = 'updated' WHERE id = 1;");
    assert_eq!(count, 1);

    let rows = exec_rows(&mut s, "SELECT val FROM t WHERE id = 1;");
    assert_eq!(rows[0][0], Value::Text("updated".into()));
}

#[test]
fn test_delete() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, val TEXT);");
    exec(&mut s, "INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');");

    let count = exec_count(&mut s, "DELETE FROM t WHERE id = 2;");
    assert_eq!(count, 1);

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_delete_all() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    exec(&mut s, "INSERT INTO t VALUES (1), (2), (3);");
    exec_count(&mut s, "DELETE FROM t;");

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 0);
}

// ─── SELECT features ─────────────────────────────────────────────────────────

#[test]
fn test_select_where() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, name TEXT);");
    exec(
        &mut s,
        "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie');",
    );

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE id > 1;");
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_select_order_by() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, name TEXT);");
    exec(&mut s, "INSERT INTO t VALUES (3, 'C'), (1, 'A'), (2, 'B');");

    let rows = exec_rows(&mut s, "SELECT * FROM t ORDER BY id ASC;");
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[1][0], Value::Int(2));
    assert_eq!(rows[2][0], Value::Int(3));
}

#[test]
fn test_select_order_by_desc() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    exec(&mut s, "INSERT INTO t VALUES (1), (2), (3);");

    let rows = exec_rows(&mut s, "SELECT * FROM t ORDER BY id DESC;");
    assert_eq!(rows[0][0], Value::Int(3));
    assert_eq!(rows[2][0], Value::Int(1));
}

#[test]
fn test_select_limit_offset() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    exec(&mut s, "INSERT INTO t VALUES (1), (2), (3), (4), (5);");

    let rows = exec_rows(&mut s, "SELECT * FROM t ORDER BY id LIMIT 2 OFFSET 1;");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], Value::Int(2));
    assert_eq!(rows[1][0], Value::Int(3));
}

#[test]
fn test_select_projection() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, name TEXT, score REAL);");
    exec(&mut s, "INSERT INTO t VALUES (1, 'Alice', 95.0);");

    let rows = exec_rows(&mut s, "SELECT id, name FROM t;");
    assert_eq!(rows[0].len(), 2);
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[0][1], Value::Text("Alice".into()));
}

#[test]
fn test_select_expressions() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (a INT, b INT);");
    exec(&mut s, "INSERT INTO t VALUES (3, 4);");

    let rows = exec_rows(&mut s, "SELECT a + b, a * b FROM t;");
    assert_eq!(rows[0][0], Value::Int(7));
    assert_eq!(rows[0][1], Value::Int(12));
}

#[test]
fn test_select_is_null() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, name TEXT);");
    exec(&mut s, "INSERT INTO t (id) VALUES (1);");
    exec(&mut s, "INSERT INTO t VALUES (2, 'Bob');");

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE name IS NULL;");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::Int(1));

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE name IS NOT NULL;");
    assert_eq!(rows.len(), 1);
}

#[test]
fn test_select_functions() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (name TEXT);");
    exec(&mut s, "INSERT INTO t VALUES ('Hello World');");

    let rows = exec_rows(
        &mut s,
        "SELECT LOWER(name), UPPER(name), LENGTH(name) FROM t;",
    );
    assert_eq!(rows[0][0], Value::Text("hello world".into()));
    assert_eq!(rows[0][1], Value::Text("HELLO WORLD".into()));
    assert_eq!(rows[0][2], Value::Int(11));
}

#[test]
fn test_select_substr() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (s TEXT);");
    exec(&mut s, "INSERT INTO t VALUES ('abcdef');");

    let rows = exec_rows(&mut s, "SELECT SUBSTR(s, 2, 3) FROM t;");
    assert_eq!(rows[0][0], Value::Text("bcd".into()));
}

#[test]
fn test_select_coalesce() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (a INT, b INT);");
    exec(&mut s, "INSERT INTO t (b) VALUES (42);");

    let rows = exec_rows(&mut s, "SELECT COALESCE(a, b, 0) FROM t;");
    assert_eq!(rows[0][0], Value::Int(42));
}

#[test]
fn test_select_abs() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (x INT);");
    exec(&mut s, "INSERT INTO t VALUES (-5);");

    let rows = exec_rows(&mut s, "SELECT ABS(x) FROM t;");
    assert_eq!(rows[0][0], Value::Int(5));
}

#[test]
fn test_string_concat() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (first TEXT, last TEXT);");
    exec(&mut s, "INSERT INTO t VALUES ('John', 'Doe');");

    let rows = exec_rows(&mut s, "SELECT first || ' ' || last FROM t;");
    assert_eq!(rows[0][0], Value::Text("John Doe".into()));
}

// ─── JOIN ─────────────────────────────────────────────────────────────────────

#[test]
fn test_inner_join() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(
        &mut s,
        "CREATE TABLE orders (id INT, user_id INT, amount REAL);",
    );
    exec(&mut s, "CREATE TABLE users (id INT, name TEXT);");
    exec(&mut s, "INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob');");
    exec(
        &mut s,
        "INSERT INTO orders VALUES (1, 1, 100.0), (2, 1, 200.0), (3, 2, 50.0);",
    );

    let rows = exec_rows(
        &mut s,
        "SELECT users.name, orders.amount FROM users INNER JOIN orders ON users.id = orders.user_id;",
    );
    assert_eq!(rows.len(), 3);
}

// ─── Transactions ─────────────────────────────────────────────────────────────

#[test]
fn test_transaction_commit() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    exec(&mut s, "BEGIN;");
    exec(&mut s, "INSERT INTO t VALUES (1), (2);");
    exec(&mut s, "COMMIT;");

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_transaction_rollback() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    exec(&mut s, "INSERT INTO t VALUES (1);");

    exec(&mut s, "BEGIN;");
    exec(&mut s, "INSERT INTO t VALUES (2), (3);");
    exec(&mut s, "ROLLBACK;");

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 1);
}

// ─── Constraints ─────────────────────────────────────────────────────────────

#[test]
fn test_not_null_constraint() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT NOT NULL, name TEXT);");
    let result = s.execute_sql("INSERT INTO t (name) VALUES ('Alice');");
    // Should fail since id is NOT NULL and has no default
    assert!(result.is_err(), "should fail NOT NULL constraint");
}

#[test]
fn test_default_value() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(
        &mut s,
        "CREATE TABLE t (id INT, status TEXT DEFAULT 'active');",
    );
    exec(&mut s, "INSERT INTO t (id) VALUES (1);");

    let rows = exec_rows(&mut s, "SELECT status FROM t;");
    assert_eq!(rows[0][0], Value::Text("active".into()));
}

// ─── NULL handling ────────────────────────────────────────────────────────────

#[test]
fn test_null_arithmetic() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (a INT);");
    exec(&mut s, "INSERT INTO t (a) VALUES (NULL);");

    let rows = exec_rows(&mut s, "SELECT a + 1, a * 2 FROM t;");
    assert!(rows[0][0].is_null());
    assert!(rows[0][1].is_null());
}

#[test]
fn test_null_comparison() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (a INT);");
    exec(&mut s, "INSERT INTO t VALUES (NULL), (1), (2);");

    // WHERE a = NULL should return no rows (three-valued logic)
    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE a IS NULL;");
    assert_eq!(rows.len(), 1);
    assert!(rows[0][0].is_null());
}

// ─── Data types ───────────────────────────────────────────────────────────────

#[test]
fn test_bool_type() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT, active BOOL);");
    exec(&mut s, "INSERT INTO t VALUES (1, true), (2, false);");

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE active = true;");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::Int(1));
}

#[test]
fn test_real_type() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (x REAL);");
    exec(&mut s, "INSERT INTO t VALUES (3.14), (2.71);");

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE x > 3.0;");
    assert_eq!(rows.len(), 1);
}

// ─── Index scan ───────────────────────────────────────────────────────────────

#[test]
fn test_index_scan_equality() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE users (id INT, name TEXT);");
    exec(&mut s, "CREATE INDEX idx_id ON users (id);");

    for i in 1..=100i64 {
        s.execute_sql(&format!("INSERT INTO users VALUES ({}, 'user{}');", i, i))
            .unwrap();
    }

    let rows = exec_rows(&mut s, "SELECT * FROM users WHERE id = 42;");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::Int(42));
}

// ─── Complex queries ──────────────────────────────────────────────────────────

#[test]
fn test_and_or_conditions() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (a INT, b INT);");
    exec(
        &mut s,
        "INSERT INTO t VALUES (1, 10), (2, 20), (3, 30), (4, 40);",
    );

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE a > 1 AND b < 35;");
    assert_eq!(rows.len(), 2);

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE a = 1 OR a = 4;");
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_not_condition() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE t (id INT);");
    exec(&mut s, "INSERT INTO t VALUES (1), (2), (3);");

    let rows = exec_rows(&mut s, "SELECT * FROM t WHERE NOT id = 2;");
    assert_eq!(rows.len(), 2);
}

// ─── Persistence (WAL replay) ─────────────────────────────────────────────────

#[test]
fn test_persistence_across_sessions() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("persistent.db");

    {
        let mut s = Session::open(&db_path, true).unwrap();
        exec(&mut s, "CREATE TABLE t (id INT, name TEXT);");
        exec(&mut s, "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob');");
        s.flush().unwrap();
    }

    // Reopen and verify data persists
    {
        let mut s = Session::open(&db_path, true).unwrap();
        let rows = exec_rows(&mut s, "SELECT * FROM t;");
        assert_eq!(rows.len(), 2);
    }
}

// ─── Meta commands ────────────────────────────────────────────────────────────

#[test]
fn test_meta_tables() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(&mut s, "CREATE TABLE users (id INT);");
    exec(&mut s, "CREATE TABLE orders (id INT);");

    let stmts = minidb::parser::Parser::parse_from_str(".tables").unwrap();
    let output = s.handle_meta_command(&stmts[0]).unwrap();
    assert!(output.contains("users"));
    assert!(output.contains("orders"));
}

#[test]
fn test_meta_schema() {
    let dir = TempDir::new().unwrap();
    let mut s = open_session(&dir);

    exec(
        &mut s,
        "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);",
    );

    let stmts = minidb::parser::Parser::parse_from_str(".schema").unwrap();
    let output = s.handle_meta_command(&stmts[0]).unwrap();
    assert!(output.contains("users"));
    assert!(output.contains("id"));
    assert!(output.contains("name"));
}

// ─── Batch mode ───────────────────────────────────────────────────────────────

#[test]
fn test_batch_eval() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("batch.db");
    let mut s = Session::open(&db_path, true).unwrap();

    run_eval(
        &mut s,
        "CREATE TABLE t (id INT, val TEXT); INSERT INTO t VALUES (1, 'x');",
        false,
    )
    .unwrap();

    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert_eq!(rows.len(), 1);
}

#[test]
fn test_batch_continue_on_error() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("batch.db");
    let mut s = Session::open(&db_path, true).unwrap();

    exec(&mut s, "CREATE TABLE t (id INT);");

    // Should continue even after the bad statement
    let result = run_eval(
        &mut s,
        "INSERT INTO t VALUES (1); SELECT * FROM nonexistent; INSERT INTO t VALUES (2);",
        true,
    );
    // Even with error, it should return Ok because continue_on_error is true
    assert!(result.is_ok());

    // At least the first insert should have succeeded
    let rows = exec_rows(&mut s, "SELECT * FROM t;");
    assert!(!rows.is_empty());
}
