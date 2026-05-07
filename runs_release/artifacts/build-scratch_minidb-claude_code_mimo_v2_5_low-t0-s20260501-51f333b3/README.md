# minidb

An embeddable SQL-subset relational database written in Rust.

## Build & Install

```bash
cargo build --release
# Binary at target/release/minidb
```

## Usage

### Interactive REPL

```bash
minidb --db mydata.db
```

### Execute SQL directly

```bash
minidb --eval "CREATE TABLE users (id INT PRIMARY KEY, name TEXT);"
minidb --eval "INSERT INTO users VALUES (1, 'Alice');"
minidb --eval "SELECT * FROM users;"
```

### Execute a SQL file

```bash
minidb --file schema.sql
minidb --file data.sql --continue-on-error
```

### Output formats

```bash
minidb --format csv --eval "SELECT * FROM users;"
minidb --format json --eval "SELECT * FROM users;"
```

### Other options

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--eval "<SQL>"` | Execute SQL statement(s) and exit |
| `--file <PATH>` | Execute SQL script file |
| `--continue-on-error` | Continue on errors in batch mode |
| `--repl` | Force interactive REPL |
| `--init` | Create fresh empty database and exit |
| `--format <FMT>` | Output format: table (default), csv, json |
| `--quiet` | Suppress summary output |
| `--version` | Print version |
| `--help` | Print help |

## SQL Subset Reference

### Data Types

| Type | Description |
|------|-------------|
| `INT` / `BIGINT` | 64-bit integer |
| `REAL` / `FLOAT` / `DOUBLE` | 64-bit floating point |
| `TEXT` / `VARCHAR` / `STRING` | Variable-length UTF-8 string |
| `BOOL` / `BOOLEAN` | Boolean (true/false) |
| `NULL` | Null literal |

### DDL Statements

```sql
CREATE TABLE t (
    id INT PRIMARY KEY,
    name TEXT NOT NULL,
    age INT DEFAULT 0,
    email TEXT UNIQUE,
    CHECK (age >= 0)
);

DROP TABLE t;

CREATE INDEX idx_name ON users (name);
DROP INDEX idx_name;
```

### DML Statements

```sql
-- Insert
INSERT INTO users VALUES (1, 'Alice');
INSERT INTO users (id, name) VALUES (2, 'Bob');

-- Select
SELECT * FROM users;
SELECT name, age FROM users WHERE age > 18;
SELECT * FROM users ORDER BY name ASC;
SELECT * FROM users LIMIT 10 OFFSET 5;
SELECT u.name, o.total FROM users u INNER JOIN orders o ON u.id = o.user_id;

-- Update
UPDATE users SET name = 'Charlie' WHERE id = 1;
UPDATE users SET age = age + 1;

-- Delete
DELETE FROM users WHERE id = 1;
DELETE FROM users;
```

### Expressions

- Arithmetic: `+`, `-`, `*`, `/`
- String concatenation: `||`
- Comparisons: `=`, `<>`, `<`, `<=`, `>`, `>=`
- Boolean: `AND`, `OR`, `NOT`
- Null checks: `IS NULL`, `IS NOT NULL`

### Built-in Functions

| Function | Description |
|----------|-------------|
| `LENGTH(s)` | String length |
| `LOWER(s)` | Lowercase |
| `UPPER(s)` | Uppercase |
| `SUBSTR(s, start, len)` | Substring (1-indexed) |
| `ABS(n)` | Absolute value |
| `COALESCE(a, b, ...)` | First non-null value |

### Transactions

```sql
BEGIN;
INSERT INTO users VALUES (1, 'Alice');
COMMIT;

BEGIN;
DELETE FROM users WHERE id = 1;
ROLLBACK;
```

### Meta Commands (REPL only)

| Command | Description |
|---------|-------------|
| `.schema` | Show table schemas |
| `.tables` | List all tables |
| `.indexes` | List all indexes |
| `.format <fmt>` | Set output format (table, csv, json) |
| `.exit` | Exit the REPL |

## Example Session

```
$ minidb --db test.db
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, email TEXT);
ok 1 table created
minidb> INSERT INTO users VALUES (1, 'Alice', 'alice@example.com');
ok 1 row inserted
minidb> INSERT INTO users (id, name) VALUES (2, 'Bob');
ok 1 row inserted
minidb> SELECT * FROM users;
id | name  | email
---+-------+------------------
1  | Alice | alice@example.com
2  | Bob   | NULL
2 rows
minidb> .schema
CREATE TABLE users (
  id INT PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT
)

minidb> .exit
```

## Architecture

```
src/
├── main.rs           # CLI entry point, argument parsing
├── error/            # Crate-wide MiniDbError enum
├── types/            # Value, Column, Row, tri-valued logic
├── lexer/            # Tokenizer (Token stream)
├── parser/           # SQL parser → AST
│   └── ast.rs        # Abstract syntax tree types
├── catalog/          # Schema and index metadata
├── storage/          # Page-based storage engine
│   ├── page.rs       # Page type (4096 bytes)
│   ├── heap.rs       # Heap file (row storage)
│   ├── wal.rs        # Write-ahead log with CRC32
│   ├── btree.rs      # B-tree secondary index
│   └── manager.rs    # Storage manager (coordinates components)
├── txn/              # Transaction manager
├── plan/             # Query planning
│   ├── logical.rs    # Logical plan nodes
│   └── physical.rs   # Physical plan nodes
├── exec/             # Volcano-style executor
│   ├── evaluator.rs  # Expression evaluator
│   ├── planner.rs    # AST → Physical plan
│   └── operators/    # Physical operators
│       ├── seq_scan.rs
│       ├── index_scan.rs
│       ├── filter.rs
│       ├── project.rs
│       ├── join.rs
│       ├── sort.rs
│       ├── limit.rs
│       ├── insert.rs
│       ├── update.rs
│       └── delete_op.rs
└── cli/              # REPL and batch execution
```

### Key Design Decisions

- **Volcano-style iterator model**: Each physical operator implements `open()`, `next()`, `close()` for composable query execution.
- **Page-based storage**: 4096-byte pages with slotted heap files for row storage.
- **Write-ahead log**: Append-only WAL with CRC32 checksums for crash recovery.
- **Single-writer transactions**: Simple transaction model without concurrency control.
- **Separated concerns**: Lexer and parser are independent from storage; the planner bridges them.
