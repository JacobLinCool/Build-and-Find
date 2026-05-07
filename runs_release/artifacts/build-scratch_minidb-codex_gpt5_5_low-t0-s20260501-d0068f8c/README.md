# minidb

`minidb` is a small embeddable relational database implemented as one Rust
binary. It supports a practical SQL subset, on-disk persistence, an append-only
write-ahead log with CRC-protected records, explicit transactions, a REPL, and
batch execution.

## Build and Install

```sh
cargo build --release
cargo test
```

Run the binary from `target/release/minidb`, or during development with
`cargo run --`.

## CLI

```sh
minidb --db ./app.db --init
minidb --db ./app.db --eval "CREATE TABLE users (id INT PRIMARY KEY, name TEXT);"
minidb --db ./app.db --file schema.sql
minidb --db ./app.db --repl
```

Flags:

- `--db <PATH>` selects the database file, defaulting to `./minidb.db`.
- `--eval "<sql>"` executes one or more semicolon-separated statements.
- `--file <PATH>` executes a SQL script.
- `--continue-on-error` keeps running a script after a statement error.
- `--format table|csv|json` selects result formatting.
- `--quiet` suppresses per-statement `ok ...` progress lines on stderr.
- `--version`, `--help`, and `--init` are supported.

Exit codes are `0` for success, `1` for SQL errors, `2` for CLI usage errors,
and `3` for storage or IO errors.

## SQL Subset

Supported statements:

```sql
CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL, n INT DEFAULT 0,
                CHECK (n >= 0));
DROP TABLE t;
CREATE INDEX idx_name ON t (name);
DROP INDEX idx_name;
INSERT INTO t VALUES (1, 'Ada', 10);
INSERT INTO t (id, name) VALUES (2, 'Bob');
SELECT * FROM t WHERE n >= 5 ORDER BY name ASC LIMIT 10 OFFSET 5;
UPDATE t SET n = n + 1 WHERE id = 2;
DELETE FROM t WHERE name IS NULL;
BEGIN;
COMMIT;
ROLLBACK;
```

Types are `INT`/`BIGINT`, `REAL`, `TEXT`, `BOOL`, and `NULL`. Expressions
support arithmetic, string concatenation with `||`, boolean logic, comparisons,
`IS NULL`, `IS NOT NULL`, and scalar functions `LENGTH`, `LOWER`, `UPPER`,
`SUBSTR`, `ABS`, and `COALESCE`.

`SELECT` supports projections, `*`, qualified column names, a single table or
one inner join, `WHERE`, `ORDER BY`, and `LIMIT/OFFSET`.

Meta-commands in the REPL and scripts:

```text
.schema
.tables
.indexes
.format table|csv|json
.exit
```

## Example Session

```text
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
ok table created
minidb> INSERT INTO users VALUES (1, 'Ada');
ok 1 row affected
minidb> SELECT id, UPPER(name) AS name FROM users;
id | name
1  | ADA
ok 1 rows
```

## Architecture

The crate is split into explicit layers:

- `lexer` tokenizes SQL text.
- `parser` builds an AST without touching storage.
- `plan` converts AST queries into logical and physical plans.
- `exec` implements Volcano-style physical operators such as `SeqScan`,
  `IndexScan`, `Filter`, `Project`, `NestedLoopJoin`, `Sort`, `Limit`,
  `Insert`, `Update`, and `Delete`.
- `storage` owns page constants, typed page identifiers, heap table state,
  B-tree secondary index helpers, and WAL replay.
- `txn` exposes explicit `begin`, `commit`, and `rollback`.
- `catalog` stores table and index metadata.
- `types` defines values, columns, and SQL three-valued logic.
- `cli` is the only boundary that prints user-facing output.

The database file stores the latest durable image. The WAL is record-oriented:
each logical operation is serialized as one record with a length header and
CRC32 checksum. On startup, committed transactions are replayed and incomplete
or torn records are ignored.
