# minidb

An embeddable, on-disk relational database implementing a meaningful subset of SQL. Written in Rust, `minidb` includes a hand-written lexer, recursive-descent parser, Volcano-style physical executor, page-based storage engine with a write-ahead log (WAL), B-tree secondary indexes, and transaction management.

## Build & Install

```bash
# Build (requires stable Rust toolchain)
cargo build --release

# The binary is at:
./target/release/minidb

# Run tests
cargo test
```

## Usage

### Interactive REPL

```bash
minidb --db mydata.db
# or force REPL mode:
minidb --db mydata.db --repl
```

### Batch evaluation

```bash
# Evaluate a single statement (or semicolon-separated list)
minidb --db mydata.db --eval "SELECT * FROM users;"

# Execute a SQL script file
minidb --db mydata.db --file schema.sql

# Continue on errors in file mode
minidb --db mydata.db --file import.sql --continue-on-error
```

### Global flags

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file (default: `./minidb.db`) |
| `--format {table,csv,json}` | Output format (default: `table`) |
| `--quiet` / `-q` | Suppress per-statement summary lines |
| `--init` | Create a fresh empty database file and exit |
| `--version` | Print version and exit |
| `--help` | Print help and exit |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | SQL error |
| 2 | CLI usage error |
| 3 | Storage / IO error |

## SQL Reference

### Data Types

| Type | Description |
|------|-------------|
| `INT` / `INTEGER` | 64-bit signed integer |
| `BIGINT` | Alias for INT |
| `REAL` / `FLOAT` | 64-bit floating point |
| `TEXT` / `VARCHAR` | UTF-8 variable-length string |
| `BOOL` / `BOOLEAN` | Boolean (true/false) |
| `NULL` | The null literal |

NULL propagates through comparisons and arithmetic (SQL three-valued logic).

### DDL

```sql
-- Create a table
CREATE TABLE users (
    id    INT  PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    age   INT  DEFAULT 0,
    name  TEXT,
    CHECK (age >= 0)
);

-- Create an index (speeds up equality and range queries)
CREATE INDEX idx_email ON users (email);
CREATE UNIQUE INDEX idx_email ON users (email);

-- Drop
DROP TABLE users;
DROP TABLE IF EXISTS users;
DROP INDEX idx_email;
DROP INDEX IF EXISTS idx_email;
```

Column constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <literal>`, `CHECK (<expr>)`

### DML

```sql
-- Insert (positional)
INSERT INTO users VALUES (1, 'alice@example.com', 30, 'Alice');

-- Insert (named columns; omitted columns get DEFAULT or NULL)
INSERT INTO users (id, email) VALUES (2, 'bob@example.com');

-- Update
UPDATE users SET age = 31 WHERE id = 1;

-- Delete
DELETE FROM users WHERE age > 100;
```

### SELECT

```sql
-- Basic projection
SELECT id, name FROM users;
SELECT * FROM users;
SELECT u.id, u.name FROM users u;

-- WHERE with comparison, boolean logic, IS NULL
SELECT * FROM users WHERE age >= 18 AND name IS NOT NULL;
SELECT * FROM users WHERE id IN (1, 2, 3);
SELECT * FROM users WHERE age BETWEEN 20 AND 30;
SELECT * FROM users WHERE name LIKE 'A%';

-- INNER JOIN
SELECT u.name, o.total
FROM users u
INNER JOIN orders o ON u.id = o.user_id;

-- ORDER BY, LIMIT, OFFSET
SELECT * FROM users ORDER BY age DESC LIMIT 10 OFFSET 20;
```

### Expressions

- Arithmetic: `+`, `-`, `*`, `/`, `%`
- String concatenation: `||`
- Comparisons: `=`, `<>`, `!=`, `<`, `<=`, `>`, `>=`
- Boolean: `AND`, `OR`, `NOT`
- NULL tests: `IS NULL`, `IS NOT NULL`
- `CASE WHEN ... THEN ... ELSE ... END`
- `IN (...)`, `BETWEEN ... AND ...`, `LIKE`

### Scalar Functions

| Function | Description |
|----------|-------------|
| `LENGTH(s)` | String length in characters |
| `LOWER(s)` | Convert to lowercase |
| `UPPER(s)` | Convert to uppercase |
| `SUBSTR(s, start[, len])` | Substring (1-indexed) |
| `ABS(n)` | Absolute value |
| `COALESCE(a, b, ...)` | First non-NULL value |

### Transactions

```sql
BEGIN;
INSERT INTO users VALUES (3, 'carol@example.com', 25, 'Carol');
UPDATE users SET age = 26 WHERE id = 3;
COMMIT;

-- Or discard changes:
ROLLBACK;
```

Statements outside an explicit `BEGIN`/`COMMIT` are auto-committed.

### Meta Commands

| Command | Description |
|---------|-------------|
| `.tables` | List all tables |
| `.schema` | Show CREATE statements for all tables and indexes |
| `.indexes` | List all indexes |
| `.format <kind>` | Set output format (`table`, `csv`, `json`) |
| `.exit` / `.quit` | Exit the REPL |

### Output Formats

```bash
# Table (default)
minidb --format table --eval "SELECT * FROM t;"

# CSV (RFC 4180)
minidb --format csv --eval "SELECT * FROM t;"

# JSON (array of row objects)
minidb --format json --eval "SELECT * FROM t;"
```

## Example Session

```
$ minidb --db demo.db
minidb> CREATE TABLE products (id INT PRIMARY KEY, name TEXT NOT NULL, price REAL DEFAULT 0.0);
ok table 'products' created
minidb> INSERT INTO products VALUES (1, 'Widget', 9.99);
ok 1 row affected
minidb> INSERT INTO products (id, name) VALUES (2, 'Gadget');
ok 1 row affected
minidb> SELECT * FROM products ORDER BY price DESC;
id | name   | price
---+--------+------
1  | Widget | 9.99
2  | Gadget | 0
ok 2 rows
minidb> CREATE INDEX idx_price ON products (price);
ok index 'idx_price' created
minidb> SELECT name FROM products WHERE price > 5.0;
name
------
Widget
ok 1 row
minidb> .tables
table_name
----------
products
ok 1 row
minidb> .exit
```

## Architecture

```
src/
├── main.rs           Entry point, CLI argument parsing
├── error.rs          Crate-wide MiniDbError enum
├── types.rs          Value, DataType, Row, tri-valued logic (Tribool)
├── lexer.rs          SQL tokenizer (Token stream)
├── parser/
│   ├── ast.rs        AST node definitions (Stmt, Expr, etc.)
│   └── mod.rs        Recursive-descent parser
├── catalog/
│   └── mod.rs        TableSchema, IndexSchema, Catalog (JSON-serialized)
├── storage/
│   ├── page.rs       PageId, Page, HeapPage layout (4096-byte pages)
│   ├── page_manager.rs File-backed page cache
│   ├── wal.rs        Write-ahead log (append-only, CRC32 per record)
│   ├── heap.rs       Heap file (slot-array pages for table rows)
│   ├── btree.rs      In-memory B+tree index (serialized to page)
│   └── mod.rs        StorageEngine — DDL/DML coordination
├── txn/
│   └── mod.rs        TxnManager — begin/commit/rollback, WAL framing
├── plan/
│   ├── logical.rs    LogicalPlan enum
│   ├── physical.rs   PhysicalPlan enum, IndexPredicate
│   └── mod.rs        Planner — logical → physical (index selection)
├── exec/
│   ├── mod.rs        PhysicalOperator trait, EvalContext, Schema
│   ├── filter.rs     Filter operator + expression evaluator
│   ├── seq_scan.rs   SeqScan operator
│   ├── index_scan.rs IndexScan operator
│   ├── project.rs    Project (SELECT list evaluation)
│   ├── nested_loop_join.rs  NestedLoopJoin
│   ├── sort.rs       Sort (in-memory)
│   ├── limit.rs      Limit/Offset
│   ├── insert.rs     Insert
│   ├── update.rs     Update
│   ├── delete.rs     Delete
│   └── engine.rs     Executor — dispatches stmts to operators
└── cli/
    └── mod.rs        REPL, batch eval, file mode, output formatting
tests/
└── integration.rs    End-to-end SQL integration tests
```

### Key Design Points

**Storage**: The database file is divided into 4096-byte pages. Page 0 stores the catalog (JSON-encoded schema). Table data lives in heap pages using a slot array layout. Secondary indexes use in-memory B+trees serialized to dedicated pages.

**WAL**: A separate `.wal` file records `Begin`/`Write`/`Commit`/`Rollback` records, each with a CRC32 checksum. On startup, committed transactions are replayed and uncommitted prefixes are discarded.

**Transactions**: Single-writer model. `TxnManager` wraps WAL begin/commit/rollback. Statements outside `BEGIN`/`COMMIT` are auto-committed.

**Execution**: Volcano-style iterator model. Each physical operator implements `open()`, `next()`, `close()` and `schema()`. The planner selects index scans when a WHERE predicate matches an indexed column.

**Logging**: All diagnostic output goes through the `log` crate. Set `RUST_LOG=debug` for verbose output.
