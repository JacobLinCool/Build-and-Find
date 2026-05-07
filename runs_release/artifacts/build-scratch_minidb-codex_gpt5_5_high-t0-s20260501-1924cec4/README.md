# minidb

`minidb` is a single-binary, on-disk relational database that implements a practical subset of SQL. It is intended to be embedded or scripted, while still exposing the main parts of a database stack: lexer, parser, planner, Volcano-style executor, page/storage layer, write-ahead log, transaction manager, catalog, and CLI.

## Build and Install

```sh
cargo build --release
cargo test
```

The binary is produced at `target/release/minidb`.

Create an empty database:

```sh
minidb --db ./app.db --init
```

Run a one-shot script:

```sh
minidb --db ./app.db --eval "CREATE TABLE users (id INT PRIMARY KEY, name TEXT); INSERT INTO users VALUES (1, 'Ada'); SELECT * FROM users;"
```

Run a SQL file:

```sh
minidb --db ./app.db --file ./schema.sql
```

Start the REPL:

```sh
minidb --db ./app.db --repl
```

## CLI Reference

- `--db <PATH>` selects the database file. The default is `./minidb.db`.
- `--eval "<sql>"` executes one statement or a semicolon-separated list.
- `--file <PATH>` executes a SQL script.
- `--continue-on-error` keeps processing a file after SQL errors.
- `--format table|csv|json` selects result formatting.
- `--quiet` suppresses per-statement summary lines on stderr.
- `--init` creates a fresh empty database and exits.
- `--help` and `--version` print command metadata.

Exit codes are: `0` success, `1` SQL or parse error, `2` CLI usage error, and `3` storage or IO error.

## SQL Subset

Supported statements:

```sql
CREATE TABLE t (
  id INT PRIMARY KEY,
  name TEXT NOT NULL,
  score REAL DEFAULT 0,
  active BOOL DEFAULT TRUE,
  CHECK (score >= 0)
);

DROP TABLE t;
CREATE INDEX idx_name ON t (name);
DROP INDEX idx_name;

INSERT INTO t VALUES (1, 'Ada', 10.5, TRUE);
INSERT INTO t (id, name) VALUES (2, 'Grace');

SELECT id, LOWER(name) AS name
FROM t
WHERE score >= 0 AND active IS NOT NULL
ORDER BY id DESC
LIMIT 10 OFFSET 5;

UPDATE t SET score = score + 1 WHERE id = 1;
DELETE FROM t WHERE id = 2;

BEGIN;
COMMIT;
ROLLBACK;
```

Types are `INT`, `BIGINT`, `REAL`, `TEXT`, `BOOL`, and `NULL`. `BIGINT` is an alias of `INT`.

Expressions support:

- Arithmetic: `+`, `-`, `*`, `/`
- Concatenation: `||`
- Comparisons: `=`, `<>`, `!=`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`
- Boolean logic: `AND`, `OR`, `NOT`
- Functions: `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`

`SELECT` supports projection, `*`, qualified `table.column` references, a single table, or an inner join of two tables with `JOIN ... ON ...`.

Meta-commands:

```sql
.schema
.tables
.indexes
.format json
.exit
```

## Example Session

```text
$ minidb --db demo.db --repl
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
ok 0 rows affected
minidb> INSERT INTO users VALUES (1, 'Ada');
ok 1 row affected
minidb> SELECT * FROM users;
+====+======+
| id | name |
+----+------+
| 1  | Ada  |
+====+======+
ok 1 row
```

## Architecture

The code is split into top-level modules:

- `lexer`: tokenizes SQL into positioned tokens.
- `parser`: builds an AST without touching storage.
- `plan`: represents logical and physical plans and chooses index scans for simple indexed predicates.
- `exec`: implements Volcano-style physical operators including sequential scan, index scan, filter, project, nested loop join, sort, limit, insert, update, and delete.
- `storage`: owns the page constant, typed page IDs, heap tables, B-tree secondary indexes, checkpoint file, and WAL.
- `txn`: provides explicit `begin`, `commit`, and `rollback` boundaries.
- `catalog`: stores table and index metadata.
- `types`: defines values, column types, and three-valued logic helpers.
- `cli`: contains all terminal IO and output rendering.
- `error`: defines the single crate-wide error enum.

Storage uses a readable checkpoint file plus an append-only record WAL. Each WAL record is length-delimited and protected by a CRC32. On startup, committed transactions after the checkpoint are replayed and incomplete transactions are ignored. Secondary indexes are keyed by `(index_key, row_id)` through the stored key map and are rebuilt from heap rows after recovery or mutation.
