use minidb::cli::{self, MiniDb, OutputFormat};
use tempfile::tempdir;

fn setup_db() -> (tempfile::TempDir, MiniDb) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    cli::init_db(&db_path).unwrap();
    let db = MiniDb::open(&db_path, true).unwrap();
    (dir, db)
}

#[test]
fn test_create_table_and_insert() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT);")
        .unwrap();

    db.execute_sql("INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30);")
        .unwrap();
    db.execute_sql("INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25);")
        .unwrap();
    db.execute_sql("INSERT INTO users (id, name, age) VALUES (3, 'Charlie', 35);")
        .unwrap();

    let result = db.execute_sql("SELECT * FROM users;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 3),
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_select_with_where() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, val TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'a');").unwrap();
    db.execute_sql("INSERT INTO t VALUES (2, 'b');").unwrap();
    db.execute_sql("INSERT INTO t VALUES (3, 'c');").unwrap();

    let result = db.execute_sql("SELECT * FROM t WHERE id = 2;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], minidb::types::Value::Int(2));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_select_order_by_limit() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, val TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (3, 'c');").unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'a');").unwrap();
    db.execute_sql("INSERT INTO t VALUES (2, 'b');").unwrap();

    let result = db.execute_sql("SELECT * FROM t ORDER BY id ASC;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0][0], minidb::types::Value::Int(1));
            assert_eq!(rows[1][0], minidb::types::Value::Int(2));
            assert_eq!(rows[2][0], minidb::types::Value::Int(3));
        }
        _ => panic!("expected rows"),
    }

    let result = db
        .execute_sql("SELECT * FROM t ORDER BY id DESC LIMIT 2;")
        .unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0][0], minidb::types::Value::Int(3));
            assert_eq!(rows[1][0], minidb::types::Value::Int(2));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_select_limit_offset() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY);")
        .unwrap();
    for i in 1..=10 {
        db.execute_sql(&format!("INSERT INTO t VALUES ({});", i))
            .unwrap();
    }

    let result = db
        .execute_sql("SELECT * FROM t ORDER BY id LIMIT 3 OFFSET 2;")
        .unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0][0], minidb::types::Value::Int(3));
            assert_eq!(rows[1][0], minidb::types::Value::Int(4));
            assert_eq!(rows[2][0], minidb::types::Value::Int(5));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_update() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'Alice');")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (2, 'Bob');").unwrap();

    db.execute_sql("UPDATE t SET name = 'Charlie' WHERE id = 1;")
        .unwrap();

    let result = db.execute_sql("SELECT * FROM t WHERE id = 1;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][1], minidb::types::Value::Text("Charlie".into()));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_delete() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1);").unwrap();
    db.execute_sql("INSERT INTO t VALUES (2);").unwrap();
    db.execute_sql("INSERT INTO t VALUES (3);").unwrap();

    db.execute_sql("DELETE FROM t WHERE id = 2;").unwrap();

    let result = db.execute_sql("SELECT * FROM t ORDER BY id;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0][0], minidb::types::Value::Int(1));
            assert_eq!(rows[1][0], minidb::types::Value::Int(3));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_insert_positional() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT, age INT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'Alice', 30);")
        .unwrap();

    let result = db.execute_sql("SELECT * FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], minidb::types::Value::Int(1));
            assert_eq!(rows[0][1], minidb::types::Value::Text("Alice".into()));
            assert_eq!(rows[0][2], minidb::types::Value::Int(30));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_insert_with_defaults() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t (id) VALUES (1);").unwrap();

    let result = db.execute_sql("SELECT * FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], minidb::types::Value::Int(1));
            // name should be default (empty string)
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_expressions_arithmetic() {
    let (_dir, mut db) = setup_db();

    let result = db.execute_sql("SELECT 1 + 2 * 3;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Int(7));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT 10 / 3;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Int(3));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_expressions_string_concat() {
    let (_dir, mut db) = setup_db();

    let result = db.execute_sql("SELECT 'hello' || ' ' || 'world';").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Text("hello world".into()));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_scalar_functions() {
    let (_dir, mut db) = setup_db();

    let result = db.execute_sql("SELECT LENGTH('hello');").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Int(5));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT LOWER('HELLO');").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Text("hello".into()));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT UPPER('hello');").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Text("HELLO".into()));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT ABS(-42);").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Int(42));
        }
        _ => panic!("expected rows"),
    }

    let result = db
        .execute_sql("SELECT COALESCE(NULL, NULL, 'default');")
        .unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Text("default".into()));
        }
        _ => panic!("expected rows"),
    }

    let result = db
        .execute_sql("SELECT SUBSTR('hello world', 7, 5);")
        .unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Text("world".into()));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_null_handling() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, val TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t (id) VALUES (1);").unwrap();

    let result = db.execute_sql("SELECT val IS NULL FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Bool(true));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT val IS NOT NULL FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Bool(false));
        }
        _ => panic!("expected rows"),
    }

    // NULL propagation
    let result = db.execute_sql("SELECT 1 + NULL;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Null);
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_boolean_operators() {
    let (_dir, mut db) = setup_db();

    let result = db.execute_sql("SELECT TRUE AND FALSE;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Bool(false));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT TRUE OR FALSE;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Bool(true));
        }
        _ => panic!("expected rows"),
    }

    let result = db.execute_sql("SELECT NOT TRUE;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][0], minidb::types::Value::Bool(false));
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_drop_table() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1);").unwrap();
    db.execute_sql("DROP TABLE t;").unwrap();

    let result = db.execute_sql("SELECT * FROM t;");
    assert!(result.is_err());
}

#[test]
fn test_create_index() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute_sql("CREATE INDEX idx_name ON t (name);")
        .unwrap();

    db.execute_sql("INSERT INTO t VALUES (1, 'Alice');")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (2, 'Bob');").unwrap();

    let result = db
        .execute_sql("SELECT * FROM t WHERE name = 'Alice';")
        .unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_transactions() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY);")
        .unwrap();

    // Auto-commit
    db.execute_sql("INSERT INTO t VALUES (1);").unwrap();
    let result = db.execute_sql("SELECT * FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 1),
        _ => panic!("expected rows"),
    }

    // Explicit transaction with commit
    db.execute_sql("BEGIN;").unwrap();
    db.execute_sql("INSERT INTO t VALUES (2);").unwrap();
    db.execute_sql("COMMIT;").unwrap();
    let result = db.execute_sql("SELECT * FROM t ORDER BY id;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
        _ => panic!("expected rows"),
    }

    // Transaction with rollback
    db.execute_sql("BEGIN;").unwrap();
    db.execute_sql("INSERT INTO t VALUES (3);").unwrap();
    db.execute_sql("ROLLBACK;").unwrap();
    let result = db.execute_sql("SELECT * FROM t ORDER BY id;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_not_null_constraint() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);")
        .unwrap();
    let result = db.execute_sql("INSERT INTO t (id) VALUES (1);");
    assert!(result.is_err());
}

#[test]
fn test_unique_constraint() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY UNIQUE, name TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'Alice');")
        .unwrap();
    let result = db.execute_sql("INSERT INTO t VALUES (1, 'Bob');");
    assert!(result.is_err());
}

#[test]
fn test_meta_commands() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE users (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute_sql("CREATE TABLE orders (id INT PRIMARY KEY, user_id INT);")
        .unwrap();

    let result = db.meta_command(".tables").unwrap().unwrap();
    assert!(result.contains("users"));
    assert!(result.contains("orders"));

    let result = db.meta_command(".schema").unwrap().unwrap();
    assert!(result.contains("CREATE TABLE users"));
    assert!(result.contains("CREATE TABLE orders"));
}

#[test]
fn test_output_formats() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'Alice');")
        .unwrap();

    // Table format (default)
    let result = db.execute_sql("SELECT * FROM t;").unwrap();
    let output = db.format_result(&result);
    assert!(output.contains("id"));
    assert!(output.contains("name"));

    // CSV format
    db.set_format(OutputFormat::Csv);
    let output = db.format_result(&result);
    assert!(output.contains("id,name"));
    assert!(output.contains("1,Alice"));

    // JSON format
    db.set_format(OutputFormat::Json);
    let output = db.format_result(&result);
    assert!(output.contains("\"id\""));
    assert!(output.contains("\"name\""));
    assert!(output.contains("\"Alice\""));
}

#[test]
fn test_comparisons() {
    let (_dir, mut db) = setup_db();

    let tests = vec![
        ("SELECT 1 < 2;", true),
        ("SELECT 2 < 1;", false),
        ("SELECT 1 <= 1;", true),
        ("SELECT 1 > 2;", false),
        ("SELECT 2 >= 2;", true),
        ("SELECT 1 <> 2;", true),
        ("SELECT 1 = 1;", true),
        ("SELECT 'a' < 'b';", true),
    ];

    for (sql, expected) in tests {
        let result = db.execute_sql(sql).unwrap();
        match result {
            minidb::exec::ExecResult::Rows { rows, .. } => {
                assert_eq!(
                    rows[0][0],
                    minidb::types::Value::Bool(expected),
                    "failed: {}",
                    sql
                );
            }
            _ => panic!("expected rows for: {}", sql),
        }
    }
}

#[test]
fn test_column_projection() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, a TEXT, b TEXT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'x', 'y');")
        .unwrap();

    let result = db.execute_sql("SELECT a, b FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { columns, rows, .. } => {
            assert_eq!(columns, vec!["a".to_string(), "b".to_string()]);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].len(), 2);
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_multiple_inserts_one_statement() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1), (2), (3);")
        .unwrap();

    let result = db.execute_sql("SELECT * FROM t ORDER BY id;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 3);
        }
        _ => panic!("expected rows"),
    }
}

#[test]
fn test_error_on_nonexistent_table() {
    let (_dir, mut db) = setup_db();

    let result = db.execute_sql("SELECT * FROM nonexistent;");
    assert!(result.is_err());
}

#[test]
fn test_update_multiple_columns() {
    let (_dir, mut db) = setup_db();

    db.execute_sql("CREATE TABLE t (id INT PRIMARY KEY, a TEXT, b INT);")
        .unwrap();
    db.execute_sql("INSERT INTO t VALUES (1, 'old', 10);")
        .unwrap();

    db.execute_sql("UPDATE t SET a = 'new', b = 20 WHERE id = 1;")
        .unwrap();

    let result = db.execute_sql("SELECT * FROM t;").unwrap();
    match result {
        minidb::exec::ExecResult::Rows { rows, .. } => {
            assert_eq!(rows[0][1], minidb::types::Value::Text("new".into()));
            assert_eq!(rows[0][2], minidb::types::Value::Int(20));
        }
        _ => panic!("expected rows"),
    }
}
