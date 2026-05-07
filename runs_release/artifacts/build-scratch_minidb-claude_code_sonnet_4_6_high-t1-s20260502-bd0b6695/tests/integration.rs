/// End-to-end integration tests for minidb.
use minidb::exec::Database;
use minidb::types::Value;
use std::path::PathBuf;
use tempfile::TempDir;

fn open_db(dir: &TempDir) -> Database {
    let path = dir.path().join("test.db");
    Database::open(&path).unwrap()
}

fn exec(db: &mut Database, sql: &str) -> minidb::exec::QueryResult {
    use minidb::parser::parse;
    use minidb::plan::Planner;

    let stmts = parse(sql).expect("parse failed");
    let mut last = minidb::exec::QueryResult::empty();
    for stmt in stmts {
        let planner = Planner::new(&db.catalog);
        let plan = planner.plan_statement(stmt).expect("plan failed");
        last = db.execute_plan(plan).expect("execute failed");
    }
    last
}

#[test]
fn test_create_insert_select() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(
        &mut db,
        "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0);",
    );

    exec(&mut db, "INSERT INTO users VALUES (1, 'Alice', 30);");
    exec(&mut db, "INSERT INTO users VALUES (2, 'Bob', 25);");
    exec(
        &mut db,
        "INSERT INTO users (id, name) VALUES (3, 'Charlie');",
    );

    let res = exec(&mut db, "SELECT * FROM users ORDER BY id;");
    assert_eq!(res.rows.len(), 3);
    assert_eq!(res.rows[0][0], Value::Int(1));
    assert_eq!(res.rows[0][1], Value::Text("Alice".into()));
    assert_eq!(res.rows[0][2], Value::Int(30));
    // Charlie should have default age = 0
    assert_eq!(res.rows[2][2], Value::Int(0));
}

#[test]
fn test_update() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (id INT, val INT);");
    exec(&mut db, "INSERT INTO t VALUES (1, 10);");
    exec(&mut db, "INSERT INTO t VALUES (2, 20);");

    let res = exec(&mut db, "UPDATE t SET val = val + 1 WHERE id = 1;");
    assert_eq!(res.affected_rows, Some(1));

    let rows = exec(&mut db, "SELECT val FROM t WHERE id = 1;");
    assert_eq!(rows.rows[0][0], Value::Int(11));
}

#[test]
fn test_delete() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (id INT, name TEXT);");
    exec(&mut db, "INSERT INTO t VALUES (1, 'a');");
    exec(&mut db, "INSERT INTO t VALUES (2, 'b');");
    exec(&mut db, "INSERT INTO t VALUES (3, 'c');");

    let res = exec(&mut db, "DELETE FROM t WHERE id < 3;");
    assert_eq!(res.affected_rows, Some(2));

    let rows = exec(&mut db, "SELECT * FROM t;");
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0][0], Value::Int(3));
}

#[test]
fn test_where_clauses() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE nums (n INT);");
    for i in 1..=10 {
        exec(&mut db, &format!("INSERT INTO nums VALUES ({});", i));
    }

    let res = exec(&mut db, "SELECT n FROM nums WHERE n > 5 AND n <= 8;");
    assert_eq!(res.rows.len(), 3); // 6, 7, 8
    let vals: Vec<i64> = res
        .rows
        .iter()
        .map(|r| if let Value::Int(i) = r[0] { i } else { 0 })
        .collect();
    assert!(vals.contains(&6));
    assert!(vals.contains(&7));
    assert!(vals.contains(&8));
}

#[test]
fn test_order_by_limit_offset() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (n INT);");
    for i in [5, 2, 8, 1, 9, 3] {
        exec(&mut db, &format!("INSERT INTO t VALUES ({});", i));
    }

    let res = exec(&mut db, "SELECT n FROM t ORDER BY n ASC LIMIT 3 OFFSET 1;");
    assert_eq!(res.rows.len(), 3);
    // Sorted: 1,2,3,5,8,9 → offset 1 → 2,3,5
    assert_eq!(res.rows[0][0], Value::Int(2));
    assert_eq!(res.rows[1][0], Value::Int(3));
    assert_eq!(res.rows[2][0], Value::Int(5));
}

#[test]
fn test_null_handling() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (id INT, val TEXT);");
    exec(&mut db, "INSERT INTO t VALUES (1, NULL);");
    exec(&mut db, "INSERT INTO t VALUES (2, 'hello');");

    let res = exec(&mut db, "SELECT * FROM t WHERE val IS NULL;");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], Value::Int(1));

    let res2 = exec(&mut db, "SELECT * FROM t WHERE val IS NOT NULL;");
    assert_eq!(res2.rows.len(), 1);
    assert_eq!(res2.rows[0][0], Value::Int(2));
}

#[test]
fn test_arithmetic_and_functions() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (s TEXT, n INT);");
    exec(&mut db, "INSERT INTO t VALUES ('Hello World', 42);");

    let res = exec(&mut db, "SELECT LENGTH(s), LOWER(s), UPPER(s) FROM t;");
    assert_eq!(res.rows[0][0], Value::Int(11));
    assert_eq!(res.rows[0][1], Value::Text("hello world".into()));
    assert_eq!(res.rows[0][2], Value::Text("HELLO WORLD".into()));

    let res2 = exec(&mut db, "SELECT ABS(-5), ABS(5) FROM t;");
    assert_eq!(res2.rows[0][0], Value::Int(5));
    assert_eq!(res2.rows[0][1], Value::Int(5));

    let res3 = exec(&mut db, "SELECT SUBSTR(s, 1, 5) FROM t;");
    assert_eq!(res3.rows[0][0], Value::Text("Hello".into()));
}

#[test]
fn test_coalesce() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (a INT, b INT);");
    exec(&mut db, "INSERT INTO t VALUES (NULL, 42);");
    exec(&mut db, "INSERT INTO t VALUES (10, NULL);");

    let res = exec(
        &mut db,
        "SELECT COALESCE(a, b, 0) FROM t ORDER BY COALESCE(a, b, 0);",
    );
    assert_eq!(res.rows[0][0], Value::Int(10));
    assert_eq!(res.rows[1][0], Value::Int(42));
}

#[test]
fn test_string_concat() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (first TEXT, last TEXT);");
    exec(&mut db, "INSERT INTO t VALUES ('John', 'Doe');");

    let res = exec(&mut db, "SELECT first || ' ' || last FROM t;");
    assert_eq!(res.rows[0][0], Value::Text("John Doe".into()));
}

#[test]
fn test_index_scan() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (id INT, name TEXT);");
    for i in 1..=20 {
        exec(
            &mut db,
            &format!("INSERT INTO t VALUES ({}, 'name{}');", i, i),
        );
    }
    exec(&mut db, "CREATE INDEX idx_id ON t (id);");

    let res = exec(&mut db, "SELECT id FROM t WHERE id = 10;");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], Value::Int(10));
}

#[test]
fn test_transactions_commit() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (n INT);");

    exec(&mut db, "BEGIN;");
    exec(&mut db, "INSERT INTO t VALUES (1);");
    exec(&mut db, "INSERT INTO t VALUES (2);");
    exec(&mut db, "COMMIT;");

    let res2 = exec(&mut db, "SELECT n FROM t ORDER BY n;");
    assert_eq!(res2.rows.len(), 2);
}

#[test]
fn test_join() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE users (id INT, name TEXT);");
    exec(&mut db, "CREATE TABLE orders (user_id INT, item TEXT);");
    exec(&mut db, "INSERT INTO users VALUES (1, 'Alice');");
    exec(&mut db, "INSERT INTO users VALUES (2, 'Bob');");
    exec(&mut db, "INSERT INTO orders VALUES (1, 'book');");
    exec(&mut db, "INSERT INTO orders VALUES (1, 'pen');");
    exec(&mut db, "INSERT INTO orders VALUES (2, 'pencil');");

    let res = exec(
        &mut db,
        "SELECT users.name, orders.item FROM users INNER JOIN orders ON users.id = orders.user_id ORDER BY users.name, orders.item;",
    );
    assert_eq!(res.rows.len(), 3);
}

#[test]
fn test_drop_table() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (n INT);");
    exec(&mut db, "INSERT INTO t VALUES (1);");
    exec(&mut db, "DROP TABLE t;");

    // Table should no longer exist
    use minidb::parser::parse;
    use minidb::plan::Planner;
    let stmts = parse("SELECT * FROM t;").unwrap();
    let planner = Planner::new(&db.catalog);
    let result = planner.plan_statement(stmts.into_iter().next().unwrap());
    assert!(result.is_err());
}

#[test]
fn test_create_drop_index() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (id INT, name TEXT);");
    exec(&mut db, "INSERT INTO t VALUES (1, 'Alice');");
    exec(&mut db, "CREATE INDEX idx_id ON t (id);");

    assert!(db.catalog.get_index("idx_id").is_some());
    exec(&mut db, "DROP INDEX idx_id;");
    assert!(db.catalog.get_index("idx_id").is_none());
}

#[test]
fn test_meta_tables() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE users (id INT);");
    exec(&mut db, "CREATE TABLE orders (id INT);");

    let res = exec(&mut db, ".tables");
    assert!(res.rows.len() >= 2);
}

#[test]
fn test_meta_schema() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(
        &mut db,
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL);",
    );
    let res = exec(&mut db, ".schema");
    assert!(res.rows.len() >= 2);
}

#[test]
fn test_not_null_constraint() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (id INT NOT NULL, name TEXT);");

    // Insert with NULL into NOT NULL column should fail
    use minidb::parser::parse;
    use minidb::plan::Planner;
    let stmts = parse("INSERT INTO t VALUES (NULL, 'test');").unwrap();
    let planner = Planner::new(&db.catalog);
    let plan = planner
        .plan_statement(stmts.into_iter().next().unwrap())
        .unwrap();
    let result = db.execute_plan(plan);
    assert!(result.is_err(), "expected error for NOT NULL violation");
}

#[test]
fn test_types() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (i INT, r REAL, t TEXT, b BOOL);");
    exec(&mut db, "INSERT INTO t VALUES (42, 3.14, 'hello', TRUE);");

    let res = exec(&mut db, "SELECT * FROM t;");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], Value::Int(42));
    if let Value::Real(r) = res.rows[0][1] {
        assert!((r - 3.14).abs() < 1e-10);
    } else {
        panic!("expected Real");
    }
    assert_eq!(res.rows[0][2], Value::Text("hello".into()));
    assert_eq!(res.rows[0][3], Value::Bool(true));
}

#[test]
fn test_select_expressions() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (a INT, b INT);");
    exec(&mut db, "INSERT INTO t VALUES (3, 4);");

    let res = exec(&mut db, "SELECT a + b, a * b, a - b FROM t;");
    assert_eq!(res.rows[0][0], Value::Int(7));
    assert_eq!(res.rows[0][1], Value::Int(12));
    assert_eq!(res.rows[0][2], Value::Int(-1));
}

#[test]
fn test_persistence() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("persist.db");

    // Create and insert
    {
        let mut db = Database::open(&db_path).unwrap();
        exec(&mut db, "CREATE TABLE t (id INT, name TEXT);");
        exec(&mut db, "INSERT INTO t VALUES (1, 'Alice');");
        exec(&mut db, "INSERT INTO t VALUES (2, 'Bob');");
    }

    // Reopen and verify
    {
        let mut db = Database::open(&db_path).unwrap();
        let res = exec(&mut db, "SELECT * FROM t ORDER BY id;");
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0][0], Value::Int(1));
        assert_eq!(res.rows[1][0], Value::Int(2));
    }
}

#[test]
fn test_select_star_join() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE a (id INT, val TEXT);");
    exec(&mut db, "CREATE TABLE b (a_id INT, extra TEXT);");
    exec(&mut db, "INSERT INTO a VALUES (1, 'foo');");
    exec(&mut db, "INSERT INTO b VALUES (1, 'bar');");

    let res = exec(&mut db, "SELECT * FROM a INNER JOIN b ON a.id = b.a_id;");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0].len(), 4); // 2 cols from each table
}

#[test]
fn test_or_predicate() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (n INT);");
    for i in 1..=5 {
        exec(&mut db, &format!("INSERT INTO t VALUES ({});", i));
    }

    let res = exec(&mut db, "SELECT n FROM t WHERE n = 1 OR n = 5;");
    assert_eq!(res.rows.len(), 2);
}

#[test]
fn test_update_without_where() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (n INT);");
    exec(&mut db, "INSERT INTO t VALUES (1);");
    exec(&mut db, "INSERT INTO t VALUES (2);");
    exec(&mut db, "INSERT INTO t VALUES (3);");

    let res = exec(&mut db, "UPDATE t SET n = n * 2;");
    assert_eq!(res.affected_rows, Some(3));

    let rows = exec(&mut db, "SELECT n FROM t ORDER BY n;");
    assert_eq!(rows.rows[0][0], Value::Int(2));
    assert_eq!(rows.rows[1][0], Value::Int(4));
    assert_eq!(rows.rows[2][0], Value::Int(6));
}

#[test]
fn test_desc_order() {
    let dir = TempDir::new().unwrap();
    let mut db = open_db(&dir);

    exec(&mut db, "CREATE TABLE t (n INT);");
    for i in [3, 1, 4, 1, 5] {
        exec(&mut db, &format!("INSERT INTO t VALUES ({});", i));
    }

    let res = exec(&mut db, "SELECT n FROM t ORDER BY n DESC;");
    let vals: Vec<i64> = res
        .rows
        .iter()
        .map(|r| if let Value::Int(i) = r[0] { i } else { 0 })
        .collect();
    // Should be descending
    for i in 1..vals.len() {
        assert!(vals[i - 1] >= vals[i]);
    }
}
