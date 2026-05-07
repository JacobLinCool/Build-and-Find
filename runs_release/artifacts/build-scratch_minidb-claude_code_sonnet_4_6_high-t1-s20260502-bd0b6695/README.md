# MiniDB

An embeddable, single-binary, on-disk relational database implementing a meaningful subset of SQL. Includes a hand-written lexer, recursive-descent parser, logical+physical query planner, a Volcano-style executor, a page-based storage engine with write-ahead logging (WAL), transaction management, and a REPL plus batch-evaluation CLI.

## Features

- Full SQL DML: `SELECT`, `INSERT`, `UPDATE`, `DELETE`
- DDL: `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX`, `DROP INDEX`
- Transactions: `BEGIN`, `COMMIT`, `ROLLBACK` with WAL-based crash recovery
- B+ tree secondary indexes with equality and range scan support
- Column types: `INT`, `BIGINT`, `REAL`, `TEXT`, `BOOL`, `NULL`
- Column constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT`, `CHECK`
- Expressions: arithmetic, comparisons, `AND`/`OR`/`NOT`, `IS NULL`, `LIKE`, `IN`
- Functions: `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`
- Joins: `INNER JOIN` with `ON` condition
- `ORDER BY` (ASC/DESC), `LIMIT`, `OFFSET`
- Output formats: table (default), CSV, JSON
- Meta-commands: `.tables`, `.schema`, `.indexes`, `.format`, `.exit`

## Build & Install

**Requirements:** Rust stable toolchain (1.70+)

```bash
cargo build --release
# Binary: target/release/minidb
```

To install to your PATH:

```bash
cargo install --path .
```

## Usage

### Interactive REPL

```bash
minidb --db mydata.db
# or explicitly:
minidb --db mydata.db --repl
```

```
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0);
ok table users created
minidb> INSERT INTO users VALUES (1, 'Alice', 30);
ok 1 row affected
minidb> SELECT * FROM users;
+----+-------+-----+
| id | name  | age |
+----+-------+-----+
| 1  | Alice | 30  |
+----+-------+-----+
ok 1 row
minidb> .exit
```

### Batch Mode (--eval)

```bash
# Execute SQL inline
minidb --db mydata.db --eval "SELECT * FROM users WHERE age > 25;"

# Multiple statements separated by ;
minidb --db mydata.db --eval "BEGIN; INSERT INTO t VALUES (1); COMMIT;"
```

### File Mode (--file)

```bash
minidb --db mydata.db --file script.sql
# Continue despite errors:
minidb --db mydata.db --file script.sql --continue-on-error
```

### Output Formats

```bash
minidb --db mydata.db --format csv --eval "SELECT * FROM users;"
minidb --db mydata.db --format json --eval "SELECT * FROM users;"
minidb --db mydata.db --format table --eval "SELECT * FROM users;"
```

You can also switch format inside a session:
```
minidb> .format csv
```

### Other Flags

```bash
minidb --version
minidb --help
minidb --init --db fresh.db      # Create empty database and exit
minidb --quiet --eval "..."      # Suppress summary lines (data only on stdout)
```

## SQL Reference

### Types

| SQL Type     | Internal   | Description             |
|-------------|------------|-------------------------|
| `INT`        | i64        | 64-bit signed integer   |
| `BIGINT`     | i64        | Alias for INT            |
| `REAL`       | f64        | 64-bit float            |
| `TEXT`       | String     | UTF-8 variable-length   |
| `BOOL`       | bool       | true/false              |
| `NULL`       | Null       | Three-valued logic NULL  |

### CREATE TABLE

```sql
CREATE TABLE employees (
    id   INT PRIMARY KEY,
    name TEXT NOT NULL,
    dept TEXT,
    salary REAL DEFAULT 0.0,
    active BOOL DEFAULT true,
    CHECK (salary >= 0)
);
```

Column constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <literal>`, `CHECK (<expr>)`.

Table constraints: `PRIMARY KEY (col, ...)`, `UNIQUE (col, ...)`, `CHECK (<expr>)`.

### INSERT

```sql
-- Positional (all columns)
INSERT INTO employees VALUES (1, 'Alice', 'Engineering', 90000.0, true);

-- Named columns (omitted columns get their DEFAULT or NULL)
INSERT INTO employees (id, name, dept) VALUES (2, 'Bob', 'Marketing');

-- Multiple rows
INSERT INTO employees VALUES (3, 'Charlie', 'Engineering', 85000.0, true),
                             (4, 'Dave', 'HR', 70000.0, false);
```

### SELECT

```sql
SELECT id, name, salary
FROM employees
WHERE dept = 'Engineering' AND salary > 80000
ORDER BY salary DESC
LIMIT 10 OFFSET 0;

-- All columns
SELECT * FROM employees;

-- Qualified columns (for joins)
SELECT e.name, d.dept_name
FROM employees e
INNER JOIN departments d ON e.dept_id = d.id;

-- Expressions and functions
SELECT name, LENGTH(name), UPPER(name), salary * 1.1 AS new_salary
FROM employees;
```

### UPDATE

```sql
UPDATE employees SET salary = salary * 1.1 WHERE dept = 'Engineering';
UPDATE employees SET active = false;  -- no WHERE = update all rows
```

### DELETE

```sql
DELETE FROM employees WHERE id = 5;
DELETE FROM employees;  -- delete all rows
```

### Indexes

```sql
CREATE INDEX idx_name ON employees (name);
CREATE UNIQUE INDEX idx_email ON users (email);
DROP INDEX idx_name;
```

The planner automatically uses an index scan when the `WHERE` clause contains an equality or range predicate on an indexed column.

### Transactions

```sql
BEGIN;
INSERT INTO accounts (id, balance) VALUES (1, 1000);
UPDATE accounts SET balance = balance - 100 WHERE id = 1;
UPDATE accounts SET balance = balance + 100 WHERE id = 2;
COMMIT;

-- Rollback on error:
BEGIN;
INSERT INTO t VALUES (1);
ROLLBACK;
```

Statements outside `BEGIN/COMMIT` are auto-committed individually.

### Expressions

```sql
-- Arithmetic
a + b, a - b, a * b, a / b

-- Comparisons (three-valued: returns NULL if either operand is NULL)
a = b, a <> b, a < b, a <= b, a > b, a >= b
a IS NULL, a IS NOT NULL

-- Logical (short-circuit)
expr AND expr, expr OR expr, NOT expr

-- String
s1 || s2           -- concatenation

-- Functions
LENGTH(s)          -- character count
LOWER(s)           -- lowercase
UPPER(s)           -- uppercase
SUBSTR(s, start)   -- substring from pos (1-indexed)
SUBSTR(s, start, len)
ABS(n)             -- absolute value
COALESCE(a, b, ...) -- first non-NULL argument

-- Pattern matching
s LIKE 'pattern%'  -- % = any string, _ = any char
s NOT LIKE 'pattern'
```

### Meta-commands

| Command          | Description                         |
|-----------------|-------------------------------------|
| `.tables`        | List all tables                     |
| `.schema`        | Show column definitions             |
| `.indexes`       | List all indexes                    |
| `.format table`  | Switch to table output (default)    |
| `.format csv`    | Switch to CSV output                |
| `.format json`   | Switch to JSON output               |
| `.exit` / `.quit`| Exit the REPL                       |

## Architecture Overview

```
src/
├── error.rs          MiniDbError enum (crate-wide)
├── types/            Value, DataType, Schema, Row, tri-valued logic,
│                     row serialization codec
├── lexer/            Hand-written tokenizer; produces Token stream with positions
├── parser/           Recursive-descent parser; produces AST (parser::ast)
├── catalog/          Table + index metadata; JSON-persisted alongside the DB file
├── storage/
│   ├── page.rs       PAGE_SIZE = 4096, PageId(u64), PageManager (file-backed)
│   ├── heap.rs       Slotted heap pages, RowId, row insert/scan/update/delete
│   ├── btree.rs      B+ tree secondary index (page-based, leaf-linked)
│   └── wal.rs        Append-only WAL with CRC32 per record, replay on open
├── txn/              TxnManager: begin/commit/rollback, WAL framing
├── plan/
│   ├── logical.rs    Logical plan tree
│   └── mod.rs        Planner: AST → logical → physical plan
├── exec/             Volcano iterator model (PhysicalOperator trait);
│                     operators: SeqScan, IndexScan, Filter, Project,
│                     NestedLoopJoin, Sort, Limit, Insert, Update, Delete
├── cli/
│   ├── format.rs     Table/CSV/JSON formatters
│   ├── repl.rs       Interactive REPL (rustyline)
│   └── mod.rs        --eval and --file batch modes
└── main.rs           clap-based CLI entry point
tests/
└── integration.rs    End-to-end SQL tests via the library API
```

### Storage Model

The database uses three files:
- `<name>.db` — main page file (heap + B-tree pages)
- `<name>.db-wal` — write-ahead log
- `<name>.db.catalog` — table/index metadata (JSON)

**Heap pages** use a slotted layout (4096 bytes): a header tracks free space boundaries; a slot array maps slot index → (offset, length); row data grows from the end. Deleted slots are marked with length = 0.

**B+ tree** leaf nodes store `(encoded_key, row_id)` pairs in sorted order with a next-leaf pointer for range scans. Internal nodes store separator keys and child page IDs.

**WAL** records are `[length: u32][crc32: u32][type: u8][txn_id: u64][data...]`. On startup, committed transactions that haven't been checkpointed are replayed; uncommitted prefixes are discarded.

### Query Execution

The planner converts the AST to a physical plan. If a `WHERE` clause contains a single equality/range predicate on an indexed column, an `IndexScan` is substituted for `SeqScan`. The executor builds a tree of physical operators (Volcano model): each operator's `open()` materializes its input, and `next()` returns rows one at a time from an internal buffer.

### Logging

Set `RUST_LOG=info` (or `debug`) to see WAL replay messages and query execution details.

## Example Session

```
$ minidb --db shop.db

minidb> CREATE TABLE products (id INT PRIMARY KEY, name TEXT NOT NULL, price REAL, stock INT DEFAULT 0);
ok table products created

minidb> INSERT INTO products VALUES (1, 'Widget', 9.99, 100);
ok 1 row affected
minidb> INSERT INTO products VALUES (2, 'Gadget', 24.99, 50);
ok 1 row affected
minidb> INSERT INTO products (id, name, price) VALUES (3, 'Doohickey', 4.99);
ok 1 row affected

minidb> SELECT * FROM products ORDER BY price DESC;
+----+-----------+-------+-------+
| id | name      | price | stock |
+----+-----------+-------+-------+
| 2  | Gadget    | 24.99 | 50    |
| 1  | Widget    | 9.99  | 100   |
| 3  | Doohickey | 4.99  | 0     |
+----+-----------+-------+-------+
ok 3 rows

minidb> CREATE INDEX idx_price ON products (price);
ok index idx_price created

minidb> SELECT name, price FROM products WHERE price < 10.0 ORDER BY name;
+-----------+-------+
| name      | price |
+-----------+-------+
| Doohickey | 4.99  |
| Widget    | 9.99  |
+-----------+-------+
ok 2 rows

minidb> BEGIN;
ok transaction started
minidb> UPDATE products SET stock = stock - 5 WHERE id = 1;
ok 1 row affected
minidb> DELETE FROM products WHERE stock = 0;
ok 1 row affected
minidb> COMMIT;
ok committed

minidb> .tables
+----------+
| name     |
+----------+
| products |
+----------+
ok 1 row

minidb> .exit
```

## Exit Codes

| Code | Meaning                              |
|------|--------------------------------------|
| 0    | Success                              |
| 1    | SQL error (parse, constraint, etc.)  |
| 2    | CLI usage error                      |
| 3    | Storage / IO error                   |
