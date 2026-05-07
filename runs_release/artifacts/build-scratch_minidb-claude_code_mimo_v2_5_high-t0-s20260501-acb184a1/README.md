# MiniDB

An embeddable SQL-subset relational database written in Rust.

## Build

```bash
cargo build --release
```

The binary will be at `target/release/minidb`.

## Usage

### Interactive REPL

```bash
./target/release/minidb --db mydata.db
```

### Execute SQL from command line

```bash
./target/release/minidb --db mydata.db --eval "SELECT * FROM users;"
```

### Execute SQL from a file

```bash
./target/release/minidb --db mydata.db --file script.sql
```

### Initialize a fresh database

```bash
./target/release/minidb --db mydata.db --init
```

### Options

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--eval "<SQL>"` | Execute SQL statement(s) and exit |
| `--file <PATH>` | Execute SQL script file |
| `--repl` | Force interactive REPL mode |
| `--format <kind>` | Output format: `table`, `csv`, `json` |
| `--quiet`, `-q` | Suppress per-statement summary lines |
| `--continue-on-error` | Continue on errors in `--file` mode |
| `--init` | Create a fresh empty database and exit |
| `--version`, `-v` | Print version |
| `--help`, `-h` | Print help |

## SQL Reference

### Statements

- `CREATE TABLE name (col type [constraints], ...);`
- `DROP TABLE name;`
- `CREATE INDEX idx_name ON table (column);`
- `DROP INDEX idx_name;`
- `INSERT INTO table [(cols)] VALUES (vals), ...;`
- `SELECT [DISTINCT] cols FROM table [JOIN ... ON ...] [WHERE ...] [ORDER BY ...] [LIMIT N [OFFSET M]];`
- `UPDATE table SET col = expr [, ...] [WHERE expr];`
- `DELETE FROM table [WHERE expr];`
- `BEGIN;` / `COMMIT;` / `ROLLBACK;`

### Column Types

| Type | Description |
|------|-------------|
| `INT` / `BIGINT` | 64-bit integer |
| `REAL` / `FLOAT` | 64-bit floating point |
| `TEXT` / `VARCHAR` | Variable-length UTF-8 string |
| `BOOL` / `BOOLEAN` | Boolean (true/false) |

### Column Constraints

- `PRIMARY KEY` - Primary key (implies NOT NULL and UNIQUE)
- `NOT NULL` - Column cannot be NULL
- `UNIQUE` - Column values must be unique
- `DEFAULT <literal>` - Default value when omitted

### Expressions

- Arithmetic: `+`, `-`, `*`, `/`
- String concatenation: `||`
- Comparisons: `=`, `<>`, `<`, `<=`, `>`, `>=`
- Logical: `AND`, `OR`, `NOT`
- Null checks: `IS NULL`, `IS NOT NULL`

### Scalar Functions

| Function | Description |
|----------|-------------|
| `LENGTH(s)` | String length |
| `LOWER(s)` | Lowercase |
| `UPPER(s)` | Uppercase |
| `SUBSTR(s, start, len)` | Substring |
| `ABS(n)` | Absolute value |
| `COALESCE(a, b, ...)` | First non-NULL value |

### Meta Commands (REPL only)

| Command | Description |
|---------|-------------|
| `.schema` | Show table schemas |
| `.tables` | List all tables |
| `.indexes` | List all indexes |
| `.format <kind>` | Set output format |
| `.exit` | Exit the REPL |

## Example Session

```
$ minidb --db example.db
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT, email TEXT);
ok
minidb> INSERT INTO users (id, name, email) VALUES (1, 'Alice', 'alice@example.com');
ok 1 row affected
minidb> INSERT INTO users (id, name, email) VALUES (2, 'Bob', 'bob@example.com');
ok 1 row affected
minidb> SELECT * FROM users;
id | name  | email
---+-------+----------------
1  | Alice | alice@example.com
2  | Bob   | bob@example.com
ok 2 rows
minidb> SELECT name FROM users WHERE id = 1;
name
-----
Alice
ok 1 row
minidb> UPDATE users SET name = 'Alice Smith' WHERE id = 1;
ok 1 row affected
minidb> DELETE FROM users WHERE id = 2;
ok 1 row affected
minidb> .schema
CREATE TABLE users (
  id INT PRIMARY KEY,
  name TEXT,
  email TEXT
);
minidb> .exit
```

## Architecture

MiniDB is organized into the following modules:

- **`lexer`** - SQL tokenizer that produces a `Token` stream
- **`parser`** - SQL parser that produces an AST from tokens
- **`plan`** - Logical and physical query planner
- **`exec`** - Volcano-style iterator execution engine with physical operators
- **`storage`** - Page-based storage engine with heap files, WAL, and B-tree indexes
- **`txn`** - Transaction manager (BEGIN/COMMIT/ROLLBACK)
- **`catalog`** - Schema and index metadata management
- **`types`** - Value types, column types, and tri-valued logic
- **`cli`** - REPL and batch-mode CLI
- **`error`** - Crate-wide error type

### Storage Engine

- Pages are 4096 bytes
- Catalog metadata is stored in page 0 as JSON
- Table data is stored in heap files (one per table)
- Write-Ahead Log (WAL) with CRC32 per record for crash recovery
- B-tree secondary indexes for equality and range queries

### Execution Model

The executor uses a Volcano-style iterator model where each physical operator implements `open`, `next`, `close`, and `schema` methods. Operators compose as a tree, with `next` returning one row at a time.

Physical operators: `SeqScan`, `IndexScan`, `Filter`, `Project`, `NestedLoopJoin`, `Sort`, `Limit`, `Insert`, `Update`, `Delete`.

### Transactions

- Single-writer model (no concurrent transactions)
- Explicit transactions via `BEGIN`/`COMMIT`/`ROLLBACK`
- Auto-commit for statements outside explicit transactions
- WAL-based crash recovery on startup
