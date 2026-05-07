# minidb

A single-binary, on-disk relational database implementing a meaningful subset of SQL.

## Build / Install

```bash
cargo build --release
```

The binary will be at `target/release/minidb`.

## Usage

### Interactive REPL

```bash
minidb --db mydb.db
```

The REPL is entered automatically when no `--eval` or `--file` is given.

### Batch execution

```bash
# Execute a single statement
minidb --db mydb.db --eval "CREATE TABLE t (id INT, name TEXT);"

# Execute a script file
minidb --db mydb.db --file schema.sql

# Continue on errors in script mode
minidb --db mydb.db --file seed.sql --continue-on-error
```

### Output formats

```bash
minidb --db mydb.db --format csv  --eval "SELECT * FROM t;"
minidb --db mydb.db --format json --eval "SELECT * FROM t;"
```

### CLI options

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--repl` | Force interactive REPL mode |
| `--eval <SQL>` | Execute SQL and exit |
| `--file <PATH>` | Execute SQL script and exit |
| `--continue-on-error` | Continue script on error |
| `--format <FMT>` | Output: `table`, `csv`, `json` |
| `--quiet` | Suppress per-statement summary |
| `--init` | Create fresh empty database and exit |
| `--version` | Print version |
| `--help` | Print help |

## SQL Subset Reference

### Data types

| Type | Description |
|------|-------------|
| `INT` / `BIGINT` | 64-bit signed integer |
| `REAL` | 64-bit floating point |
| `TEXT` | Variable-length UTF-8 string |
| `BOOL` | Boolean (`true` / `false`) |

All types support `NULL`. Three-valued logic propagates through comparisons and arithmetic.

### DDL statements

```sql
CREATE TABLE users (
    id INT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    name TEXT,
    age INT DEFAULT 0,
    CHECK (age >= 0)
);

DROP TABLE users;

CREATE INDEX idx_email ON users (email);
DROP INDEX idx_email;
```

**Constraints:** `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <literal>`, `CHECK (<expr>)`

### DML statements

```sql
-- Insert (positional)
INSERT INTO users VALUES (1, 'alice@example.com', 'Alice', 30);

-- Insert (named columns, defaults fill the rest)
INSERT INTO users (id, email, name) VALUES (2, 'bob@example.com', 'Bob');

-- Insert multiple rows
INSERT INTO users VALUES (3, 'c@x.com', 'C', 25), (4, 'd@x.com', 'D', 35);

-- Update
UPDATE users SET age = age + 1 WHERE name = 'Alice';

-- Delete
DELETE FROM users WHERE id = 4;
```

### Query

```sql
-- Projection
SELECT * FROM users;
SELECT name, age FROM users;
SELECT u.name FROM users u;

-- Filtering
SELECT * FROM users WHERE age > 25 AND age < 40;
SELECT * FROM users WHERE email IS NOT NULL;

-- Joins
SELECT u.name, p.title
FROM users u
INNER JOIN posts p ON u.id = p.user_id;

-- Ordering
SELECT * FROM users ORDER BY age DESC, name ASC;

-- Limit / Offset
SELECT * FROM users ORDER BY id LIMIT 10 OFFSET 5;

-- Expressions
SELECT name || ' (' || age || ')' AS label FROM users;
SELECT ABS(age - 30) AS diff FROM users;
```

### Expressions

Arithmetic: `+`, `-`, `*`, `/`
String: `||` (concatenation)
Comparison: `=`, `<>`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`
Logic: `AND`, `OR`, `NOT`

### Built-in functions

| Function | Description |
|----------|-------------|
| `LENGTH(s)` | String length |
| `LOWER(s)` | Lowercase |
| `UPPER(s)` | Uppercase |
| `SUBSTR(s, start [, len])` | Substring |
| `ABS(n)` | Absolute value |
| `COALESCE(a, b, ...)` | First non-NULL value |

### Transactions

```sql
BEGIN;
INSERT INTO users VALUES (5, 'e@x.com', 'E', 40);
UPDATE users SET age = 99 WHERE id = 1;
COMMIT;

-- Or rollback
BEGIN;
DELETE FROM users;
ROLLBACK;
```

Statements outside `BEGIN`/`COMMIT` are auto-committed.

### Meta-commands

| Command | Description |
|---------|-------------|
| `.schema` | Show all table and index definitions |
| `.tables` | List all tables |
| `.indexes` | List all indexes |
| `.format <kind>` | Set output format (table, csv, json) |
| `.exit` | Exit the REPL |

## Architecture

```
src/
  main.rs           Entry point, CLI dispatch
  error.rs          Crate-wide error enum (MiniDbError)
  types.rs          Value, ColumnType, tri-valued logic, arithmetic
  lexer/mod.rs      Tokenizer: SQL text -> Token stream
  parser/
    mod.rs          Parser: Token stream -> AST
    ast.rs          AST node definitions
  plan/mod.rs       Logical planner: AST -> LogicalPlan (with index optimization)
  exec/
    mod.rs          Engine: executes plans via catalog + storage
    operators.rs    Physical operator trait + SeqScan, IndexScan, Filter, Project,
                    NestedLoopJoin, Sort, Limit, Insert, Update, Delete
  storage/
    page.rs         Page manager: 4096-byte page allocation and I/O
    wal.rs          Write-ahead log: append-only, CRC32-protected records
    heap.rs         Heap file: slotted-page row storage
    btree.rs        B-tree secondary index
  catalog/mod.rs    Schema and index metadata (serialized to page 0)
  txn/mod.rs        Transaction manager: begin, commit, rollback
  cli/mod.rs        REPL, batch mode, output formatting (table/csv/json)
tests/
  integration.rs    End-to-end tests via the binary
```

### Execution model

The planner converts SQL AST into a `LogicalPlan` tree. The engine evaluates this tree recursively:

1. **SeqScan** / **IndexScan** produce rows from storage
2. **Filter** applies WHERE predicates
3. **Project** evaluates SELECT expressions
4. **NestedLoopJoin** joins two row streams
5. **Sort** and **Limit** handle ORDER BY / LIMIT / OFFSET

The engine uses a `table_map` to track column offsets in joined rows, enabling correct qualified column resolution (e.g., `b.info` in a join context).

### Storage

- **Pages**: Fixed 4096-byte pages, managed by `PageManager`
- **Heap**: Slotted-page layout for row storage. Each page has a header, a slot directory, and row data growing from the end of the page
- **WAL**: Append-only log with CRC32 per record. On startup, committed records are replayed; uncommitted records are discarded
- **B-tree**: Secondary indexes keyed by `(index_key, row_page, row_slot)`. Supports equality and range scans
- **Catalog**: Stored as JSON on page 0. Contains table schemas and index metadata

## Example Session

```
$ minidb --db demo.db
minidb> CREATE TABLE products (
    ...>     id INT PRIMARY KEY,
    ...>     name TEXT NOT NULL,
    ...>     price REAL DEFAULT 0.0
    ...> );
ok
minidb> INSERT INTO products VALUES (1, 'Widget', 9.99);
ok 1 row affected
minidb> INSERT INTO products VALUES (2, 'Gadget', 24.50);
ok 1 row affected
minidb> INSERT INTO products (id, name) VALUES (3, 'Doohickey');
ok 1 row affected
minidb> SELECT name, price FROM products WHERE price > 0 ORDER BY price;
name      | price
----------+------
Widget    | 9.9
Gadget    | 24.5
ok 2 rows
minidb> SELECT COALESCE(name, 'unknown') AS n, ABS(price - 15) AS diff FROM products;
n         | diff
----------+------
Widget    | 5.01
Gadget    | 9.5
Doohickey | 15.0
ok 3 rows
minidb> .exit
```
