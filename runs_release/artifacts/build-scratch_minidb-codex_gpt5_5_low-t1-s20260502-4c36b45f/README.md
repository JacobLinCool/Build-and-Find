# minidb

`minidb` is a small embeddable relational database implemented as a single Rust binary. It supports a practical subset of SQL, durable on-disk storage, an append-only write-ahead log, explicit transactions, secondary indexes, a REPL, and script/batch execution.

## Build and Install

```sh
cargo build --release
cargo test
```

Run the binary:

```sh
target/release/minidb --db ./minidb.db --repl
target/release/minidb --db ./minidb.db --eval "select * from t;"
target/release/minidb --db ./minidb.db --file script.sql
```

Useful flags:

- `--init` creates a fresh empty database file.
- `--format table|csv|json` selects result formatting.
- `--quiet` suppresses per-statement progress summaries on stderr.
- `--continue-on-error` keeps executing a script after SQL errors.
- `--help` and `--version` print CLI metadata.

## SQL Subset

Supported statements:

```sql
CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x', CHECK (id > 0));
DROP TABLE t;
CREATE INDEX idx_t_id ON t (id);
DROP INDEX idx_t_id;
INSERT INTO t VALUES (1, 'Ada');
INSERT INTO t (id) VALUES (2);
SELECT id, UPPER(name) FROM t WHERE id >= 1 ORDER BY id DESC LIMIT 10 OFFSET 0;
UPDATE t SET name = LOWER(name) WHERE id = 1;
DELETE FROM t WHERE id = 2;
BEGIN;
COMMIT;
ROLLBACK;
```

Types are `INT`, `BIGINT`, `REAL`, `TEXT`, `BOOL`, and `NULL`. Expressions include arithmetic, string concatenation with `||`, boolean logic, comparisons, `IS NULL`, `IS NOT NULL`, and scalar functions `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, and `COALESCE`.

Meta-commands:

```sql
.tables
.indexes
.schema
.format json
.exit
```

## Example Session

```text
$ minidb --db demo.db --repl
minidb> create table users (id int primary key, name text not null);
ok 0
minidb> insert into users values (1, 'Ada');
ok 1 row affected
minidb> select id, lower(name) from users;
id | lower
---+------
1  | ada
ok 1 row
```

Progress summaries are written to stderr so stdout can be used as a data stream in scripts.

## Architecture

The crate is split into focused modules:

- `lexer` tokenizes SQL text.
- `parser` builds an AST without touching storage.
- `plan` contains logical and physical planning helpers, including index-scan selection.
- `exec` contains Volcano-style physical operators and statement execution.
- `storage` owns the page constant, typed page ids, heap image, B-tree indexes, and WAL.
- `txn` provides explicit `begin`, `commit`, and `rollback` handling.
- `catalog` stores table and index metadata.
- `types` defines values, column types, and SQL three-valued logic.
- `cli` implements the REPL, batch modes, output formats, and process exit behavior.
- `error` defines the crate-wide error enum.

The database image is persisted as a structured on-disk file. The WAL is append-only and record-oriented, stores logical records, and includes a CRC32 per record so recovery can ignore torn writes. On startup, committed WAL transactions are replayed and uncommitted trailing records are discarded.
