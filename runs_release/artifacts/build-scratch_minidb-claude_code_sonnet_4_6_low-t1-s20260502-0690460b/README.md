# minidb

An embeddable, single-binary, on-disk relational database implementing a meaningful subset of SQL.

## Features

- **Full SQL subset**: CREATE/DROP TABLE, CREATE/DROP INDEX, INSERT, SELECT, UPDATE, DELETE, BEGIN/COMMIT/ROLLBACK
- **ACID transactions** with write-ahead log (WAL) and crash recovery
- **B-tree secondary indexes** with automatic index-scan optimization
- **Three-valued logic** (NULL semantics per SQL standard)
- **Multiple output formats**: table (human-readable), CSV, JSON
- **Interactive REPL** with line editing and **batch mode** (--eval, --file)
- **Page-based storage** (4096-byte pages) with in-memory heap and persisted B-trees

## Build & Install

```bash
# Build (requires stable Rust toolchain)
cargo build --release

# Run tests
cargo test

# Install to PATH
cargo install --path .
```

## Usage

### Interactive REPL

```bash
minidb --db mydata.db --repl
# Or just (if stdin is a TTY):
minidb --db mydata.db
```

```
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0);
ok
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

### Batch mode

```bash
# Execute a SQL string
minidb --db mydata.db --eval "SELECT * FROM users;"

# Execute a SQL script file
minidb --db mydata.db --file script.sql

# Continue on errors
minidb --db mydata.db --file script.sql --continue-on-error

# Choose output format
minidb --db mydata.db --eval "SELECT * FROM users;" --format csv
minidb --db mydata.db --eval "SELECT * FROM users;" --format json
```

### Initialize a fresh database

```bash
minidb --db mydata.db --init
```

### Flags

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--repl` | Force interactive REPL |
| `--eval <SQL>` | Execute SQL and exit |
| `--file <PATH>` | Execute SQL script file and exit |
| `--continue-on-error` | Don't stop on first error (with `--file`) |
| `--format table\|csv\|json` | Output format for SELECT results |
| `--quiet` | Suppress per-statement summary lines |
| `--init` | Create fresh empty database and exit |
| `--version` | Print version |
| `--help` | Print help |

## SQL Reference

### Data Types

| Type | Description |
|------|-------------|
| `INT` / `INTEGER` | 64-bit signed integer |
| `BIGINT` | Alias for INT |
| `REAL` / `FLOAT` | 64-bit IEEE 754 float |
| `TEXT` / `VARCHAR` | UTF-8 variable-length string |
| `BOOL` / `BOOLEAN` | Boolean (TRUE / FALSE) |
| `NULL` | Null value |

### CREATE TABLE

```sql
CREATE TABLE [IF NOT EXISTS] table_name (
    col1 INT PRIMARY KEY,
    col2 TEXT NOT NULL,
    col3 REAL DEFAULT 0.0,
    col4 BOOL UNIQUE,
    CHECK (col3 >= 0)
);
```

Column constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <literal>`
Table constraints: `CHECK (<expr>)`

### DROP TABLE

```sql
DROP TABLE [IF EXISTS] table_name;
```

### CREATE INDEX

```sql
CREATE [UNIQUE] INDEX [IF NOT EXISTS] idx_name ON table_name (column);
```

### DROP INDEX

```sql
DROP INDEX [IF EXISTS] idx_name;
```

### INSERT

```sql
-- Positional (full row)
INSERT INTO t VALUES (1, 'hello', 3.14);

-- Named columns (partial row; omitted columns get DEFAULT or NULL)
INSERT INTO t (col1, col3) VALUES (1, 3.14);
```

### SELECT

```sql
SELECT [DISTINCT] col1, t.col2, expr AS alias, *
FROM table1 [AS alias1]
     [INNER JOIN table2 [AS alias2] ON condition]
[WHERE condition]
[ORDER BY col ASC|DESC [, ...]]
[LIMIT n [OFFSET m]];
```

Supported operators in WHERE/expressions:
- Comparison: `=`, `<>`, `<`, `<=`, `>`, `>=`
- Boolean: `AND`, `OR`, `NOT`
- Null checks: `IS NULL`, `IS NOT NULL`
- Arithmetic: `+`, `-`, `*`, `/`
- String concatenation: `||`

### UPDATE

```sql
UPDATE table SET col1 = expr [, col2 = expr] [WHERE condition];
```

### DELETE

```sql
DELETE FROM table [WHERE condition];
```

### Transactions

```sql
BEGIN;
INSERT INTO t VALUES (1, 'a');
INSERT INTO t VALUES (2, 'b');
COMMIT;    -- or ROLLBACK to discard
```

Statements outside an explicit `BEGIN`/`COMMIT` are auto-committed.

### Built-in Functions

| Function | Description |
|----------|-------------|
| `LENGTH(s)` | Length of string in characters |
| `LOWER(s)` | Convert string to lowercase |
| `UPPER(s)` | Convert string to uppercase |
| `SUBSTR(s, start [, len])` | Substring (1-indexed) |
| `ABS(n)` | Absolute value |
| `COALESCE(a, b, ...)` | First non-NULL argument |

### Meta-commands

| Command | Description |
|---------|-------------|
| `.schema` | Show CREATE statements for all tables and indexes |
| `.tables` | List all table names |
| `.indexes` | List all indexes |
| `.format table\|csv\|json` | Change output format for current session |
| `.exit` | Exit the REPL |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | SQL / runtime error |
| 2 | CLI usage error |
| 3 | Storage / IO error |

## Example Session

```sql
-- Create a schema
CREATE TABLE products (
    id   INT  PRIMARY KEY,
    name TEXT NOT NULL,
    price REAL DEFAULT 0.0
);
CREATE INDEX idx_price ON products (price);

-- Insert data
INSERT INTO products VALUES (1, 'Widget', 9.99);
INSERT INTO products VALUES (2, 'Gadget', 24.99);
INSERT INTO products (id, name) VALUES (3, 'Thingamajig');

-- Query with index scan
SELECT name, price FROM products WHERE price < 15.0 ORDER BY price DESC;

-- Update
UPDATE products SET price = 12.99 WHERE id = 1;

-- Delete
DELETE FROM products WHERE price IS NULL;

-- Join
CREATE TABLE orders (product_id INT, qty INT);
INSERT INTO orders VALUES (1, 3), (2, 1);

SELECT p.name, o.qty, p.price * o.qty AS total
FROM products p
INNER JOIN orders o ON p.id = o.product_id;
```

## Architecture Overview

```
┌─────────────────────────────────────┐
│  CLI (src/cli/)                     │
│  REPL, batch eval, output formatting│
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  Database Engine (src/db.rs)         │
│  Statement dispatch, auto-commit     │
└──┬──────────┬────────────┬──────────┘
   │          │            │
┌──▼──┐  ┌───▼──┐  ┌──────▼──────────┐
│Plan │  │Exec  │  │TxnManager       │
│     │  │      │  │(src/txn/)       │
│Logical  │Volcano│  │begin/commit/    │
│→Physical│iterator  rollback, undo log│
│plan  │  │tree  │  └─────────────────┘
│(src/ │  │(src/ │
│plan/)│  │exec/)│
└──────┘  └──┬───┘
             │
┌────────────▼──────────────────────┐
│  Storage (src/storage/)            │
│  ┌────────┐ ┌──────┐ ┌─────────┐  │
│  │ Heap   │ │BTree │ │  WAL    │  │
│  │(tables)│ │(idxs)│ │CRC32    │  │
│  └────────┘ └──────┘ │ records │  │
│  ┌────────┐           └─────────┘  │
│  │ Pager  │  4096-byte pages       │
│  └────────┘                        │
└───────────────────────────────────┘

┌──────────┐  ┌────────┐  ┌─────────┐
│  Lexer   │→ │ Parser │→ │Catalog  │
│  Tokens  │  │  AST   │  │Schemas  │
│(src/     │  │(src/   │  │Indexes  │
│lexer/)   │  │parser/)│  │(src/    │
└──────────┘  └────────┘  │catalog/)│
                           └─────────┘
```

### Module Guide

| Module | Responsibility |
|--------|---------------|
| `error` | Crate-wide `MiniDbError` enum |
| `types` | `Value` enum, `DataType`, tri-valued logic, row serialization |
| `lexer` | Tokenizer producing `Token` stream with position |
| `parser` | Recursive-descent parser; produces `parser::ast` nodes |
| `catalog` | `TableSchema`, `IndexMeta`, in-memory catalog with serialization |
| `storage::page` | `PageId`, `Page` (4096-byte), compile-time `PAGE_SIZE` constant |
| `storage::pager` | Page cache and file I/O |
| `storage::wal` | Append-only WAL with CRC32 per record; replay on startup |
| `storage::heap` | Variable-length row storage per table |
| `storage::btree` | In-memory B-tree for secondary indexes keyed by `(value, row_id)` |
| `txn` | Single-writer transaction manager with undo log |
| `plan` | Logical → physical planner; index-scan pushdown |
| `exec` | Volcano-style `PhysicalOperator` trait; `SeqScan`, `IndexScan`, `Filter`, `Project`, `NestedLoopJoin`, `Sort`, `Limit`, `Insert`, `Update`, `Delete` |
| `exec::eval` | Expression evaluator with three-valued logic and built-in functions |
| `cli` | REPL (rustyline), batch mode, table/CSV/JSON output |
| `db` | Top-level `Database` struct; WAL replay and crash recovery |

### WAL Format

Each record:
```
[4 bytes: u32 LE record length]
[N bytes: JSON-encoded WalRecord]
[4 bytes: u32 LE CRC32 of (length_bytes || record_bytes)]
```

On startup, minidb reads all WAL records, identifies committed transactions
(those with a matching `Commit` record), replays their operations into the heap
and B-trees, and truncates the WAL.

### Transaction Flow

1. `BEGIN` → WAL record written, `TxnManager::active` set
2. Each DML → WAL record + undo log entry appended
3. `COMMIT` → WAL `Commit` record, fsync, heap/btree persisted
4. `ROLLBACK` → undo log applied in reverse, WAL `Rollback` record
5. Auto-commit wraps each standalone statement in an implicit transaction

## License

MIT
