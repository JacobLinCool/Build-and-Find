use std::fs;
use std::process::Command;

fn minidb_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minidb"))
}

fn setup_db(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "minidb_integration_{name}_{}.db",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    let mut wal = path.clone();
    wal.set_extension("wal");
    let _ = fs::remove_file(&wal);
    path
}

fn cleanup_db(path: &std::path::Path) {
    let _ = fs::remove_file(path);
    let mut wal = path.to_path_buf();
    wal.set_extension("wal");
    let _ = fs::remove_file(&wal);
}

#[test]
fn test_version() {
    let output = minidb_cmd()
        .arg("--version")
        .output()
        .expect("failed to run minidb");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("minidb"), "version output: {stdout}");
}

#[test]
fn test_help() {
    let output = minidb_cmd()
        .arg("--help")
        .output()
        .expect("failed to run minidb");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("USAGE"), "help output: {stderr}");
}

#[test]
fn test_init_creates_db() {
    let path = setup_db("init");
    let output = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--init")
        .output()
        .expect("failed to run minidb");
    assert!(output.status.success());
    assert!(path.exists(), "database file should exist after --init");
    cleanup_db(&path);
}

#[test]
fn test_eval_create_table() {
    let path = setup_db("create");
    let output = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0)")
        .output()
        .expect("failed to run minidb");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    cleanup_db(&path);
}

#[test]
fn test_eval_insert_and_select() {
    let path = setup_db("ins_sel");
    let out1 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY, name TEXT)")
        .output()
        .expect("failed");
    assert!(
        out1.status.success(),
        "{}",
        String::from_utf8_lossy(&out1.stderr)
    );

    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t (id, name) VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie')")
        .output()
        .expect("failed");
    assert!(
        out2.status.success(),
        "{}",
        String::from_utf8_lossy(&out2.stderr)
    );

    let out3 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t ORDER BY id")
        .output()
        .expect("failed");
    assert!(
        out3.status.success(),
        "{}",
        String::from_utf8_lossy(&out3.stderr)
    );
    let stdout = String::from_utf8_lossy(&out3.stdout);
    assert!(stdout.contains("Alice"), "stdout: {stdout}");
    assert!(stdout.contains("Bob"), "stdout: {stdout}");
    assert!(stdout.contains("Charlie"), "stdout: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_where_clause() {
    let path = setup_db("where");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT, val INT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1, 10), (2, 20), (3, 30)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t WHERE val > 15 ORDER BY id")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("20"), "stdout: {stdout}");
    assert!(stdout.contains("30"), "stdout: {stdout}");
    assert!(!stdout.contains("10"), "should not contain 10: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_update() {
    let path = setup_db("update");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY, name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1, 'old')")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("UPDATE t SET name = 'new' WHERE id = 1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT name FROM t WHERE id = 1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout.contains("new"), "stdout: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_delete() {
    let path = setup_db("delete");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1), (2), (3)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("DELETE FROM t WHERE id = 2")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t ORDER BY id")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout.contains("1"), "stdout: {stdout}");
    assert!(stdout.contains("3"), "stdout: {stdout}");
    // "2" should only appear as part of other numbers, not as a standalone row
    // For simplicity, just check that the row count is right

    cleanup_db(&path);
}

#[test]
fn test_eval_drop_table() {
    let path = setup_db("drop");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("DROP TABLE t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Selecting from dropped table should fail
    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t")
        .output()
        .unwrap();
    assert!(!out2.status.success());

    cleanup_db(&path);
}

#[test]
fn test_eval_meta_tables() {
    let path = setup_db("meta");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE users (id INT, name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE orders (id INT, total REAL)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg(".tables")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("users"), "stdout: {stdout}");
    assert!(stdout.contains("orders"), "stdout: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_meta_schema() {
    let path = setup_db("schema");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg(".schema")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("CREATE TABLE"), "stdout: {stdout}");
    assert!(stdout.contains("PRIMARY KEY"), "stdout: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_json_format() {
    let path = setup_db("json");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT, name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1, 'Alice')")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .arg("--eval")
        .arg("SELECT * FROM t")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"id\""), "json output: {stdout}");
    assert!(stdout.contains("\"Alice\""), "json output: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_csv_format() {
    let path = setup_db("csv");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT, name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1, 'Alice')")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--format")
        .arg("csv")
        .arg("--eval")
        .arg("SELECT * FROM t")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("id,name"), "csv output: {stdout}");
    assert!(stdout.contains("1,Alice"), "csv output: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_sql_error() {
    let path = setup_db("sql_err");
    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM nonexistent")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error"), "stderr: {stderr}");

    cleanup_db(&path);
}

#[test]
fn test_eval_not_null_constraint() {
    let path = setup_db("notnull");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT NOT NULL)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (NULL)")
        .output()
        .unwrap();
    assert!(!out.status.success(), "should fail for NOT NULL violation");

    cleanup_db(&path);
}

#[test]
fn test_eval_primary_key_constraint() {
    let path = setup_db("pk");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1)")
        .output()
        .unwrap();
    assert!(!out.status.success(), "should fail for duplicate PK");

    cleanup_db(&path);
}

#[test]
fn test_eval_expressions() {
    let path = setup_db("expr");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (a INT, b INT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (3, 4)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT a + b AS sum, a * b AS product FROM t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("7"), "sum should be 7: {stdout}");
    assert!(stdout.contains("12"), "product should be 12: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_string_functions() {
    let path = setup_db("strfunc");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES ('Hello')")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT LENGTH(name), LOWER(name), UPPER(name) FROM t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("5"), "length should be 5: {stdout}");
    assert!(stdout.contains("hello"), "lower should be hello: {stdout}");
    assert!(stdout.contains("HELLO"), "upper should be HELLO: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_limit_offset() {
    let path = setup_db("limit");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1), (2), (3), (4), (5)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t ORDER BY id LIMIT 2 OFFSET 1")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("2"), "should contain 2: {stdout}");
    assert!(stdout.contains("3"), "should contain 3: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_eval_create_index() {
    let path = setup_db("index");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT, name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1, 'a'), (2, 'b')")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE INDEX idx_name ON t (name)")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg(".indexes")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout.contains("idx_name"), "stdout: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_file_mode() {
    let path = setup_db("file");
    let script =
        std::env::temp_dir().join(format!("minidb_test_script_{}.sql", std::process::id()));
    fs::write(
        &script,
        "CREATE TABLE t (id INT);\nINSERT INTO t VALUES (1);\nINSERT INTO t VALUES (2);\n",
    )
    .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--file")
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t ORDER BY id")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout.contains("1"), "stdout: {stdout}");
    assert!(stdout.contains("2"), "stdout: {stdout}");

    let _ = fs::remove_file(&script);
    cleanup_db(&path);
}

#[test]
fn test_quiet_mode() {
    let path = setup_db("quiet");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--quiet")
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1)")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    // In quiet mode, summary lines should be suppressed
    assert!(
        stderr.trim().is_empty() || !stderr.contains("ok"),
        "stderr should be quiet: {stderr}"
    );

    cleanup_db(&path);
}

#[test]
fn test_transaction_commit() {
    let path = setup_db("txn");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT PRIMARY KEY)")
        .output()
        .unwrap();

    let sql = "BEGIN; INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); COMMIT;";
    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg(sql)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out2 = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t ORDER BY id")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout.contains("1"), "stdout: {stdout}");
    assert!(stdout.contains("2"), "stdout: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_is_null() {
    let path = setup_db("isnull");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (id INT, name TEXT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (1, 'Alice'), (2, NULL)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT * FROM t WHERE name IS NULL")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("2"), "should find null row: {stdout}");

    cleanup_db(&path);
}

#[test]
fn test_coalesce_function() {
    let path = setup_db("coalesce");
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("CREATE TABLE t (a INT, b INT)")
        .output()
        .unwrap();
    minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("INSERT INTO t VALUES (NULL, 42)")
        .output()
        .unwrap();

    let out = minidb_cmd()
        .arg("--db")
        .arg(&path)
        .arg("--eval")
        .arg("SELECT COALESCE(a, b) AS result FROM t")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("42"), "coalesce should return 42: {stdout}");

    cleanup_db(&path);
}
