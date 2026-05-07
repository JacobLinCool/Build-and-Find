# minidb

An embeddable SQL-subset relational database written in Rust.

## Build & Install

```bash
# Build release binary
cargo build --release

# The binary is at target/release/minidb
# Or run directly:
cargo run --release -- --help
```

Requires Rust stable toolchain (1.70+).

## Quick Start

```bash
# Initialize a new database
minidb --db mydb.db --init

# Create a table and insert data
minidb --db mydb.db --eval "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT);"
minidb --db mydb.db --eval "INSERT INTO users VALUES (1, 'Alice', 30);"
minidb --db mydb.db --eval "INSERT INTO users VALUES (2, 'Bob', 25);"

# Query data
minidb --db mydb.db --eval "SELECT * FROM users;"
# | 1 | Alice | 30 |
# | 2 | Bob   | 25 |

# Start interactive REPL
minidb --db mydb.db
```

## SQL Reference

### Data Types

| Type    | Description                     |
|---------|---------------------------------|
| `INT`   | 64-bit signed integer           |
| `BIGINT`| Alias for INT (64-bit)          |
| `REAL`  | 64-bit floating point           |
| `TEXT`  | Variable-length string          |
| `BOOL`  | Boolean (`true` / `false`)      |

`NULL` is supported for all types.

### DDL Statements

```sql
CREATE TABLE name (
    col1 INT PRIMARY KEY,
    col2 TEXT NOT NULL,
    col3 REAL UNIQUE,
    col4 BOOL DEFAULT false
);

DROP TABLE name;

CREATE INDEX idx_name ON table_name (column_name);
DROP INDEX idx_name;
```

**Constraints:** `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <value>`

### DML Statements

```sql
-- Insert
INSERT INTO table_name VALUES (1, 'hello', 3.14);
INSERT INTO table_name (col1, col2) VALUES (1, 'hello');

-- Select
SELECT * FROM table_name;
SELECT col1, col2 FROM table_name WHERE col1 > 5;
SELECT * FROM table_name ORDER BY col2 DESC LIMIT 10 OFFSET 5;

-- Update
UPDATE table_name SET col2 = 'world' WHERE col1 = 1;

-- Delete
DELETE FROM table_name WHERE col1 = 1;
```

### Expressions

Arithmetic: `+`, `-`, `*`, `/`
String concatenation: `||`
Comparison: `=`, `<>`, `<`, `<=`, `>`, `>=`
Logical: `AND`, `OR`, `NOT`
Null check: `IS NULL`, `IS NOT NULL`

**Built-in functions:**
- `LENGTH(s)` - string length
- `LOWER(s)` - lowercase
- `UPPER(s)` - uppercase
- `SUBSTR(s, start, len)` - substring
- `ABS(x)` - absolute value
- `COALESCE(a, b, ...)` - first non-null value

### Joins

```sql
SELECT a.name, b.score
FROM students a
JOIN scores b ON a.id = b.student_id;
```

### Transactions

```sql
BEGIN;
INSERT INTO t VALUES (1, 'temp');
ROLLBACK;  -- discards the insert

BEGIN;
INSERT INTO t VALUES (1, 'perm');
COMMIT;    -- persists the insert
```

### Output Formats

```bash
minidb --db mydb.db --format table --eval "SELECT * FROM t;"  # default
minidb --db mydb.db --format csv --eval "SELECT * FROM t;"
minidb --db mydb.db --format json --eval "SELECT * FROM t;"
```

### Meta-Commands (REPL)

| Command   | Description            |
|-----------|------------------------|
| `.schema` | Show all table schemas |
| `.tables` | List all tables        |
| `.indexes`| List all indexes       |
| `.exit`   | Exit the REPL          |
| `.format` | Set output format      |

## CLI Options

```
--db <PATH>              Database file path (default: ./minidb.db)
--format <KIND>          Output format: table, csv, json (default: table)
--quiet                  Suppress per-statement summary lines
--repl                   Force interactive REPL mode
--eval "<SQL>"           Execute SQL and exit
--file <PATH>            Execute SQL script from file
--continue-on-error      Continue executing script on error
--init                   Create a fresh empty database and exit
--version                Print version and exit
--help, -h               Print this help message
```

## Architecture

```
src/
  main.rs           CLI entry point and argument parsing
  cli.rs            REPL, batch execution, output formatting
  error.rs          Unified error type (MiniDbError)
  types.rs          Value, ColumnType, Row, TableSchema
  lexer.rs          SQL tokenizer
  parser/
    ast.rs          AST node definitions
    parser_impl.rs  Recursive descent parser
  plan.rs           Logical planner with index scan optimization
  exec/
    operators.rs    Physical operators (Volcano-style) and expression evaluator
    executor.rs     Statement dispatch and plan execution
  catalog.rs        Metadata stored as JSON on page 0
  txn.rs            Transaction manager with WAL-based commit/rollback
  storage/
    page.rs         Page-based storage (4096-byte pages)
    heap.rs         Slotted-page heap file for row storage
    btree.rs        B-tree secondary indexes
    wal.rs          Write-ahead log with CRC32 per record
```

**Key design decisions:**
- **Page-based storage:** 4096-byte pages managed by PageManager
- **Slotted pages:** Rows stored in heap files with a slot directory for variable-length records
- **WAL with CRC32:** Each WAL record includes a CRC32 checksum for torn-write detection
- **B-tree indexes:** Secondary indexes keyed by (index_key, row_id) for fast lookups
- **Volcano-style iterators:** Physical operators implement open/next/close protocol
- **Catalog on page 0:** Table and index metadata serialized as JSON
- **Single-writer transactions:** BEGIN snapshots the database; ROLLBACK restores from snapshot

## Example Session

```
$ minidb --db school.db --init
Database initialized: school.db

$ minidb --db school.db
minidb> CREATE TABLE students (id INT PRIMARY KEY, name TEXT NOT NULL, gpa REAL);
ok
minidb> INSERT INTO students VALUES (1, 'Alice', 3.9);
ok
minidb> INSERT INTO students VALUES (2, 'Bob', 3.5);
ok
minidb> INSERT INTO students VALUES (3, 'Charlie', 3.7);
ok
minidb> SELECT name, gpa FROM students WHERE gpa >= 3.7 ORDER BY gpa DESC;
| Alice   | 3.9 |
| Charlie | 3.7 |
ok 2 rows
minidb> SELECT LENGTH(name), UPPER(name) FROM students;
| 5 | ALICE   |
| 3 | BOB     |
| 7 | CHARLIE |
ok 3 rows
minidb> .exit
```
