# minidb

An embeddable SQL-subset relational database written in Rust. Supports an interactive REPL, batch SQL evaluation, a page-based storage engine with write-ahead logging, B-tree secondary indexes, and a Volcano-style query executor.

---

## Build & Install

**Prerequisites:** Rust stable toolchain (1.70+).

```bash
# Build release binary
cargo build --release

# Install to PATH
cargo install --path .

# Run tests
cargo test
```

The binary is at `target/release/minidb`.

---

## Usage

### Interactive REPL

```bash
minidb --db ./mydb.db
minidb --db ./mydb.db --repl   # force REPL even if stdin is not a TTY
```

Type SQL statements terminated by `;`. Special meta-commands do not need a semicolon.

### Batch Evaluation

```bash
# Execute SQL string and exit
minidb --db ./mydb.db --eval "SELECT * FROM users;"

# Execute a SQL script file
minidb --db ./mydb.db --file schema.sql

# Continue on errors instead of stopping
minidb --db ./mydb.db --file script.sql --continue-on-error
```

### Options

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--eval <SQL>` | Execute SQL string and exit |
| `--file <PATH>` | Execute SQL script file and exit |
| `--repl` | Force interactive REPL mode |
| `--continue-on-error` | Do not stop on first error in batch mode |
| `--quiet` / `-q` | Suppress per-statement summary output on stderr |
| `--format {table,csv,json}` | Output format for SELECT results |
| `--init` | Create a fresh database file and exit |
| `--version` | Print version |
| `--help` | Print help |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | SQL error |
| 2 | CLI usage error |
| 3 | Storage / IO error |

---

## SQL Reference

### Data Types

| Type | Description |
|------|-------------|
| `INT` / `INTEGER` | 64-bit signed integer |
| `BIGINT` | Alias for INT |
| `REAL` / `FLOAT` / `DOUBLE` | 64-bit IEEE 754 float |
| `TEXT` / `VARCHAR` / `CHAR` | Variable-length UTF-8 string |
| `BOOL` / `BOOLEAN` | Boolean (`true` / `false`) |
| `NULL` | The null literal |

All types support three-valued logic: `NULL` propagates through comparisons and arithmetic per the SQL standard.

### Statements

#### CREATE TABLE

```sql
CREATE TABLE [IF NOT EXISTS] table_name (
  column_name data_type [constraints...],
  ...
  [table_constraints...]
);
```

Column constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <literal>`  
Table constraints: `PRIMARY KEY (col, ...)`, `UNIQUE (col, ...)`, `CHECK (expr)`

#### DROP TABLE

```sql
DROP TABLE [IF EXISTS] table_name;
```

#### CREATE INDEX

```sql
CREATE [UNIQUE] INDEX [IF NOT EXISTS] index_name ON table_name (column);
```

#### DROP INDEX

```sql
DROP INDEX [IF EXISTS] index_name;
```

#### INSERT

```sql
-- Positional (must match all columns)
INSERT INTO table_name VALUES (v1, v2, ...);

-- Named columns (omitted columns get their DEFAULT or NULL)
INSERT INTO table_name (col1, col2) VALUES (v1, v2);

-- Multiple rows
INSERT INTO table_name VALUES (v1), (v2), ...;
```

#### SELECT

```sql
SELECT [DISTINCT] expression [AS alias], ...
FROM table_or_join
[WHERE condition]
[ORDER BY expr [ASC|DESC], ...]
[LIMIT n [OFFSET m]];
```

**Projections:** `*`, `table.*`, column references, expressions  
**FROM:** single table or `INNER JOIN / LEFT JOIN / RIGHT JOIN / CROSS JOIN ... ON condition`  
**WHERE operators:** `=`, `<>`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`, `AND`, `OR`, `NOT`

#### UPDATE

```sql
UPDATE table_name SET col = expr [, ...] [WHERE condition];
```

Returns number of rows affected.

#### DELETE

```sql
DELETE FROM table_name [WHERE condition];
```

Returns number of rows deleted.

#### Transactions

```sql
BEGIN;
-- statements ...
COMMIT;    -- or ROLLBACK;
```

Statements outside an explicit transaction are auto-committed.

### Expressions

| Category | Operators |
|----------|-----------|
| Arithmetic | `+ - * /` |
| String concat | `\|\|` |
| Comparison | `= <> < <= > >=` |
| Logical | `AND OR NOT` |
| Null checks | `IS NULL`, `IS NOT NULL` |

### Builtin Functions

| Function | Description |
|----------|-------------|
| `LENGTH(s)` | Length of string in bytes |
| `LOWER(s)` | Lowercase string |
| `UPPER(s)` | Uppercase string |
| `SUBSTR(s, start [, len])` | Substring (1-indexed) |
| `ABS(n)` | Absolute value |
| `COALESCE(v1, v2, ...)` | First non-NULL value |

### Meta-Commands (REPL only)

| Command | Description |
|---------|-------------|
| `.tables` | List all tables |
| `.schema` | Show CREATE TABLE statements for all tables |
| `.indexes` | List all indexes |
| `.format table\|csv\|json` | Change output format |
| `.exit` | Exit the REPL |

---

## Example Session

```
$ minidb --db demo.db
minidb v0.1.0
Enter SQL statements terminated by ';'. Type '.exit' to quit.
minidb> CREATE TABLE users (
     ->   id INT PRIMARY KEY,
     ->   name TEXT NOT NULL,
     ->   email TEXT,
     ->   score REAL DEFAULT 0.0
     -> );
ok
minidb> INSERT INTO users VALUES (1, 'Alice', 'alice@example.com', 95.5);
ok 1 row affected
minidb> INSERT INTO users (id, name) VALUES (2, 'Bob');
ok 1 row affected
minidb> SELECT * FROM users;
| id | name  | email             | score |
|----|-------|-------------------|-------|
| 1  | Alice | alice@example.com | 95.5  |
| 2  | Bob   | NULL              | 0.0   |
ok 2 rows
minidb> SELECT UPPER(name), LENGTH(name) FROM users ORDER BY id DESC;
| UPPER | LENGTH |
|-------|--------|
| BOB   | 3      |
| ALICE | 5      |
ok 2 rows
minidb> CREATE INDEX idx_score ON users (score);
ok
minidb> SELECT name FROM users WHERE score > 50.0;
| name  |
|-------|
| Alice |
ok 1 row
minidb> BEGIN;
ok
minidb> UPDATE users SET score = score + 10.0 WHERE id = 2;
ok 1 row affected
minidb> ROLLBACK;
ok
minidb> SELECT id, score FROM users;
| id | score |
|----|-------|
| 1  | 95.5  |
| 2  | 0.0   |
ok 2 rows
minidb> .exit
bye
```

### CSV and JSON output

```bash
minidb --db demo.db --format csv --eval "SELECT * FROM users;"
minidb --db demo.db --format json --eval "SELECT * FROM users;"
```

---

## Architectural Overview

```
minidb/
├── src/
│   ├── main.rs           Entry point, CLI argument parsing (clap)
│   ├── lib.rs            Crate re-exports for integration tests
│   ├── error.rs          MiniDbError enum (crate-wide error type)
│   ├── types/            Value, DataType, Schema, Row, three-valued logic
│   ├── lexer/            SQL tokenizer → Token stream
│   ├── parser/           Recursive descent parser → AST (parser::ast)
│   ├── catalog/          TableMeta, IndexMeta, schema/index registry
│   ├── storage/
│   │   ├── page.rs       PAGE_SIZE constant, PageId, Page struct
│   │   ├── pager.rs      Page manager (read/write/cache)
│   │   ├── heap.rs       Heap file (slot-based row storage)
│   │   ├── btree.rs      B-tree secondary index (keyed by value + row_id)
│   │   └── wal.rs        Write-ahead log (append-only, CRC32 per record)
│   ├── txn/              TransactionManager (begin/commit/rollback, WAL framing)
│   ├── plan/
│   │   ├── logical.rs    Logical plan tree
│   │   └── physical.rs   Physical plan (with index scan selection)
│   ├── exec/
│   │   ├── operators.rs  Volcano-style PhysicalOperator trait + implementations
│   │   │                 SeqScan, IndexScan, Filter, Project, NestedLoopJoin,
│   │   │                 Sort, Limit, Insert, Update, Delete
│   │   ├── eval.rs       Expression evaluator (three-valued logic, functions)
│   │   └── mod.rs        Top-level execute() dispatcher
│   └── cli/              Session, REPL, batch eval, output formatters
└── tests/
    └── integration.rs    End-to-end SQL tests via the Session API
```

### Key Design Decisions

**Storage:** Pages are 4096 bytes (`PAGE_SIZE` constant). A heap file stores rows as slot-directory pages; each row is prefixed with its 8-byte `row_id`. The B-tree index is keyed by `(serialized_value_bytes, row_id)` for unique ordering even with duplicate keys.

**WAL:** Append-only, record-oriented. Each record is `[payload_len: u32][crc32: u32][JSON payload]`. On startup, records are replayed; only committed transactions are applied. Torn writes (CRC mismatch) are treated as a clean tail and ignored. The WAL is truncated after every full flush to disk.

**Query execution:** The planner first builds a `LogicalPlan` (predicate pushdown, join planning), then converts it to a `PhysicalPlan`. The physical planner checks the catalog for matching single-column indexes and emits `IndexScan` instead of `SeqScan` for equality and range predicates. The executor follows the Volcano iterator model: `open` → repeated `next` → `close`.

**Transactions:** Single-writer model. `BEGIN` assigns a transaction ID and writes a WAL `Begin` record. `COMMIT` writes a `Commit` record and calls `fsync`. `ROLLBACK` writes a `Rollback` record and applies undo from the in-memory pending record list.

**Logging:** All internal log output uses the `log` crate. `println!`/`eprintln!` is only used at the CLI boundary. Set `RUST_LOG=debug` for verbose output.

---

## Development

```bash
# Run tests
cargo test

# Run a specific test
cargo test test_inner_join

# Build with release optimizations
cargo build --release

# Format code
cargo fmt

# Lint
cargo clippy
```
