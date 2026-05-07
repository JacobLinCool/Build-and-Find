# minidb

`minidb` is a small embeddable relational database implemented as one Rust binary. It supports a practical SQL subset, an interactive REPL, batch execution, a page-file placeholder with a fixed 4096 byte page size, an append-only CRC-protected write-ahead log, single-writer transactions, a catalog, heap tables, and secondary B-tree indexes.

## Build and Install

```sh
cargo build --release
cargo test
```

Run the binary from Cargo:

```sh
cargo run -- --db ./example.db --init
cargo run -- --db ./example.db --eval "create table users (id int primary key, name text);"
```

## CLI Usage

```text
minidb --db <PATH> --repl
minidb --db <PATH> --eval "<sql>"
minidb --db <PATH> --file script.sql [--continue-on-error]
minidb --db <PATH> --format table|csv|json
minidb --db <PATH> --init
minidb --help
minidb --version
```

The default database path is `./minidb.db`. The WAL lives next to it with a `.wal` extension. Successful statements print a summary on stderr unless `--quiet` is passed, keeping query data on stdout.

Exit codes are:

- `0`: success
- `1`: SQL, parse, type, or runtime error
- `2`: CLI usage error
- `3`: storage or I/O error

## SQL Subset

Supported statements:

```sql
CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x', CHECK (id > 0));
DROP TABLE t;
CREATE INDEX idx_t_id ON t (id);
DROP INDEX idx_t_id;
INSERT INTO t VALUES (1, 'Ada');
INSERT INTO t (id) VALUES (2);
SELECT * FROM t WHERE id >= 1 ORDER BY id DESC LIMIT 10 OFFSET 0;
UPDATE t SET name = upper(name) WHERE id = 1;
DELETE FROM t WHERE id = 2;
BEGIN;
COMMIT;
ROLLBACK;
```

Types are `INT`, `BIGINT`, `REAL`, `TEXT`, `BOOL`, and `NULL`. Column constraints include `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, and `DEFAULT <literal>`. Table checks use `CHECK (<expr>)`.

Expressions support arithmetic, string concatenation with `||`, comparisons, `IS NULL`, `IS NOT NULL`, boolean `AND`/`OR`/`NOT`, and scalar functions `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, and `COALESCE`. Comparisons involving `NULL` evaluate to unknown, which does not pass `WHERE`.

`SELECT` supports projection, `*`, qualified `table.column` references, a single table or a two-table inner join, `WHERE`, `ORDER BY`, and `LIMIT/OFFSET`. A single equality or range predicate on an indexed column is planned as an index scan.

Meta-commands:

```text
.tables
.indexes
.schema
.format table|csv|json
.exit
```

## Example Session

```text
$ minidb --db demo.db --repl
minidb> create table users (id int primary key, name text not null);
ok
minidb> insert into users values (1, 'Ada');
ok 1 row affected
minidb> select id, upper(name) as name from users;
id | name
---+-----
1  | ADA
ok 1 row
```

JSON and CSV output are selected with `--format json`, `--format csv`, or `.format`.

## Architecture

The crate is split into focused modules:

- `lexer`: tokenizes SQL with source positions.
- `parser`: builds an AST without accessing storage.
- `plan`: exposes logical and physical plan descriptions, including index-scan selection.
- `exec`: implements Volcano-style physical operators and statement execution.
- `storage`: owns page constants, typed page ids, heap rows, B-tree indexes, and WAL replay.
- `txn`: manages explicit and auto-commit transactions.
- `catalog`: stores table and index metadata.
- `types`: stores values, column types, casts, and three-valued logic.
- `cli`: contains all terminal I/O, output formatting, and meta-command handling.
- `error`: defines the crate-wide error enum.

Durability is WAL-first. Each logical operation is written as a CRC32-checked record inside an explicit transaction frame. Startup recovery replays committed frames and ignores incomplete or rolled-back frames.
