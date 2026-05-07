# minidb

An embeddable SQL-subset relational database written in Rust.

## Build & Install

```bash
cargo build --release
```

The binary will be at `target/release/minidb`.

## Usage

```bash
# Interactive REPL
minidb

# Execute SQL and exit
minidb --eval "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'alice'); SELECT * FROM t;"

# Execute a SQL file
minidb --file script.sql

# Use a specific database file
minidb --db /path/to/data.db

# Output formats
minidb --format csv   # CSV output
minidb --format json  # JSON output
minidb --format table # Table output (default)

# Quiet mode (suppress per-statement summary)
minidb --quiet

# Initialize empty database without entering REPL
minidb --init

# Continue on error in file mode
minidb --file script.sql --continue-on-error
```

## SQL Reference

### Statements

| Statement | Example |
|-----------|---------|
| CREATE TABLE | `CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0);` |
| DROP TABLE | `DROP TABLE users;` |
| CREATE INDEX | `CREATE INDEX idx_name ON users (name);` |
| DROP INDEX | `DROP INDEX idx_name;` |
| INSERT | `INSERT INTO users (id, name, age) VALUES (1, 'alice', 30);` |
| SELECT | `SELECT * FROM users WHERE age > 25 ORDER BY name LIMIT 10;` |
| UPDATE | `UPDATE users SET age = 31 WHERE id = 1;` |
| DELETE | `DELETE FROM users WHERE id = 1;` |
| BEGIN | `BEGIN;` |
| COMMIT | `COMMIT;` |
| ROLLBACK | `ROLLBACK;` |

### Column Types

| Type | Description |
|------|-------------|
| INT / INTEGER | 64-bit signed integer |
| BIGINT | 64-bit signed integer |
| REAL / FLOAT / DOUBLE | 64-bit floating point |
| TEXT / VARCHAR / STRING | Variable-length string |
| BOOL / BOOLEAN | True/false |

### Expressions

- **Arithmetic**: `+`, `-`, `*`, `/`
- **Comparison**: `=`, `<>`, `<`, `<=`, `>`, `>=`
- **Logical**: `AND`, `OR`, `NOT`
- **String**: `||` (concatenation)
- **Null checks**: `IS NULL`, `IS NOT NULL`
- **Functions**: `LENGTH()`, `LOWER()`, `UPPER()`, `SUBSTR()`, `ABS()`, `COALESCE()`
- **Grouping**: `(expr)`
- **Dotted references**: `table.column`

### SELECT Features

```sql
-- Star projection
SELECT * FROM t;

-- Column projection with expressions
SELECT id, name, age * 2 AS doubled FROM users;

-- WHERE clause
SELECT * FROM users WHERE age > 25 AND name = 'alice';

-- INNER JOIN
SELECT u.name, o.total FROM users u INNER JOIN orders o ON u.id = o.user_id;

-- ORDER BY (ASC or DESC)
SELECT * FROM users ORDER BY name ASC, age DESC;

-- LIMIT and OFFSET
SELECT * FROM users ORDER BY id LIMIT 10 OFFSET 20;

-- IS NULL
SELECT * FROM users WHERE email IS NULL;
```

### Column Constraints

- `PRIMARY KEY` - Implies NOT NULL and unique
- `NOT NULL` - Column cannot contain NULL values
- `UNIQUE` - Column values must be unique
- `DEFAULT value` - Default value when not specified

### Meta-Commands (REPL only)

| Command | Description |
|---------|-------------|
| `.tables` | List all tables |
| `.schema [table]` | Show CREATE TABLE statement |
| `.indexes` | List all indexes |
| `.format <fmt>` | Set output format (table/csv/json) |
| `.exit` | Exit the REPL |

## Example Session

```
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, email TEXT);
ok table created

minidb> INSERT INTO users VALUES (1, 'alice', 'alice@example.com');
ok 1 row affected

minidb> INSERT INTO users VALUES (2, 'bob', NULL);
ok 1 row affected

minidb> SELECT * FROM users;
+----+-------+-------------------+
| id | name  | email             |
+----+-------+-------------------+
| 1  | alice | alice@example.com |
| 2  | bob   | NULL              |
+----+-------+-------------------+
2 rows

minidb> SELECT name FROM users WHERE email IS NULL;
+------+
| name |
+------+
| bob  |
+------+
1 row

minidb> .schema users;
CREATE TABLE users (
  id INT PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT
)
```

## Architecture

```
src/
├── main.rs          # CLI argument parsing and entry point
├── cli.rs           # REPL, batch execution, and meta-commands
├── lexer.rs         # SQL tokenizer
├── parser.rs        # Recursive descent SQL parser
│   └── ast.rs       # Abstract syntax tree types
├── plan/
│   ├── logical.rs   # Logical query plans
│   └── physical.rs  # Physical query plans
├── exec/
│   ├── mod.rs       # Query executor (Volcano-style)
│   └── expression.rs # Expression evaluation
├── storage/
│   ├── mod.rs       # Storage constants and traits
│   ├── page.rs      # Page abstraction
│   ├── heap.rs      # Heap file (append-based row storage)
│   ├── wal.rs       # Write-ahead log with CRC32 checksums
│   └── btree.rs     # B-tree secondary index
├── catalog.rs       # Table and index metadata (JSON persistence)
├── txn.rs           # Transaction manager (begin/commit/rollback)
├── types.rs         # Value types and type coercion
└── error.rs         # Error types
```

### Key Design Decisions

- **Append-based heap storage**: Rows are appended to the heap file. Updates mark old rows as deleted and append new versions. Deleted rows are skipped during reads.
- **Write-ahead log (WAL)**: All mutations are logged before being applied. On startup, uncommitted transactions are replayed for crash recovery.
- **B-tree indexes**: Secondary indexes use `BTreeMap` for O(log n) lookups. Supports equality and range predicates (>, <, >=, <=).
- **JSON catalog**: Table and index schemas are persisted as JSON files alongside the database.
- **Tri-valued logic**: NULL propagates through comparisons and arithmetic. `NULL = NULL` evaluates to NULL, not TRUE.
- **CLI modes**: Interactive REPL, single-statement eval (`--eval`), and file execution (`--file`). Stdin piping is supported.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | SQL error (parse, runtime, constraint) |
| 2 | CLI usage error (invalid arguments) |
| 3 | Storage/IO error |
