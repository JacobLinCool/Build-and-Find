# minidb

A single-binary, on-disk relational database written in Rust.

## Build & Install

```bash
cargo build --release
```

The binary will be at `target/release/minidb`.

## Quick Start

```bash
# Create a database and run some SQL
./target/release/minidb --db demo.db --eval "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT DEFAULT 0)"
./target/release/minidb --db demo.db --eval "INSERT INTO users VALUES (1, 'Alice', 30), (2, 'Bob', 25)"
./target/release/minidb --db demo.db --eval "SELECT * FROM users WHERE age > 20"

# Interactive REPL
./target/release/minidb --db demo.db

# Execute a SQL script
./target/release/minidb --db demo.db --file script.sql

# Output formats
./target/release/minidb --db demo.db --format json --eval "SELECT * FROM users"
./target/release/minidb --db demo.db --format csv --eval "SELECT * FROM users"
```

## CLI Options

| Option | Description |
|---|---|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--eval, -e <SQL>` | Execute SQL statement and exit |
| `--file, -f <PATH>` | Execute SQL script file |
| `--init` | Create empty database and exit |
| `--repl` | Force interactive REPL mode |
| `--format <FMT>` | Output format: `table`, `csv`, `json` (default: `table`) |
| `--quiet, -q` | Suppress per-statement summary lines |
| `--continue-on-error` | Continue script execution on error |
| `--version, -V` | Print version |
| `--help, -h` | Print help |

## SQL Reference

### Data Types

| Type | Description |
|---|---|
| `INT` / `INTEGER` | 64-bit signed integer |
| `BIGINT` | 64-bit signed integer (alias) |
| `REAL` / `FLOAT` / `DOUBLE` | 64-bit floating point |
| `TEXT` / `VARCHAR` / `CHAR` | Variable-length string |
| `BOOL` / `BOOLEAN` | Boolean (true/false) |

### DDL Statements

```sql
-- Create table with constraints
CREATE TABLE products (
    id INT PRIMARY KEY,
    name TEXT NOT NULL,
    price REAL DEFAULT 0.0,
    sku TEXT UNIQUE
);

-- Create index
CREATE INDEX idx_name ON products (name);

-- Drop table (also drops associated indexes)
DROP TABLE products;

-- Drop index
DROP INDEX idx_name;
```

### DML Statements

```sql
-- Insert
INSERT INTO products (id, name, price) VALUES (1, 'Widget', 9.99);
INSERT INTO products VALUES (2, 'Gadget', 19.99, 'GAD-001');

-- Select with expressions, functions, filtering, sorting, pagination
SELECT name, price * 1.1 AS taxed FROM products WHERE price > 5 ORDER BY price DESC LIMIT 10;

-- Update with filter
UPDATE products SET price = price * 0.9 WHERE name = 'Widget';

-- Delete with filter
DELETE FROM products WHERE id = 2;
```

### Expressions

Arithmetic: `+`, `-`, `*`, `/`
Comparison: `=`, `<>`, `!=`, `<`, `<=`, `>`, `>=`
Logical: `AND`, `OR`, `NOT`
String: `||` (concatenation)
Special: `IS NULL`, `IS NOT NULL`, `IN (...)`, `BETWEEN ... AND ...`, `CASE WHEN ... THEN ... ELSE ... END`

### Built-in Functions

| Function | Description |
|---|---|
| `LENGTH(s)` | String length |
| `LOWER(s)` | Lowercase |
| `UPPER(s)` | Uppercase |
| `SUBSTR(s, start, len)` | Substring (1-indexed) |
| `ABS(n)` | Absolute value |
| `COALESCE(a, b, ...)` | First non-NULL value |

### Transactions

```sql
BEGIN;
INSERT INTO products VALUES (3, 'Thingy', 5.00, 'THI-001');
INSERT INTO products VALUES (4, 'Doohickey', 7.50, 'DOO-001');
COMMIT;

-- Or rollback
BEGIN;
DELETE FROM products;
ROLLBACK;
```

### Meta Commands (REPL)

| Command | Description |
|---|---|
| `.tables` | List all tables |
| `.schema` | Show all table schemas |
| `.indexes` | List all indexes |
| `.format <kind>` | Change output format |
| `.exit` | Exit the REPL |

## Architecture

```
src/
  main.rs          -- Entry point
  cli.rs           -- CLI parsing, REPL, engine, output formatting
  lexer.rs         -- Tokenizer (keywords, literals, operators)
  parser/
    mod.rs         -- Recursive descent parser with precedence climbing
    ast.rs         -- AST node definitions
  plan/
    mod.rs         -- Planner (logical plan, index optimization)
    logical.rs     -- Logical plan nodes
    physical.rs    -- Physical plan nodes
  exec/
    mod.rs         -- Executor, expression evaluator, query context
    operators.rs   -- Volcano-style iterator trait
  types.rs         -- Value types, arithmetic, comparisons, serialization
  error.rs         -- Error types
  catalog.rs       -- Table/index catalog (persisted as JSON on page 0)
  txn.rs           -- Transaction manager with WAL
  storage/
    mod.rs         -- Pager (page-level I/O)
    page.rs        -- Slotted page format (4096 bytes)
    heap.rs        -- Heap file (row storage across pages)
    btree.rs       -- B-tree secondary index
    wal.rs         -- Write-ahead log with CRC32
```

### Key Design Decisions

- **Slotted pages**: 4096-byte pages with a slot directory growing forward and row data growing backward, maximizing space utilization.
- **WAL crash recovery**: Every page write is logged with CRC32 checksums before being applied. On startup, committed but unapplied writes are replayed.
- **Catalog on page 0**: Table and index metadata is serialized as JSON on the first page, loaded into memory at startup.
- **Volcano-style execution**: Query operators implement `open/next/close` for pull-based evaluation with lazy row processing.
- **Single-writer transactions**: Auto-commit by default; explicit `BEGIN/COMMIT/ROLLBACK` for multi-statement transactions.

## Running Tests

```bash
cargo test
```

## License

MIT
