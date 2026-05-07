# minidb

`minidb` is a single-binary, on-disk relational database with a meaningful
SQL subset, an interactive REPL, and a batch CLI. It is implemented in
stable Rust and ships with unit tests in every module plus an end-to-end
integration test harness.

## Build / install

```sh
cargo build --release
# resulting binary: target/release/minidb
```

To run the test suite:

```sh
cargo test
```

To format the source tree:

```sh
cargo fmt
```

## Usage

```text
minidb [OPTIONS]

OPTIONS:
    --db <PATH>             database file path (default: ./minidb.db)
    --eval "<SQL>"          execute one SQL statement (or ;-separated list)
    --file <PATH>           execute the SQL script at PATH
    --repl                  force REPL mode even when stdin is not a TTY
    --init                  create a fresh empty database file and exit
    --format {table|csv|json}
    --continue-on-error     do not stop --file on the first error
    --quiet                 suppress per-statement summary lines on stderr
    --version               print version and exit
    --help                  print this help and exit
```

Exit codes:

| Code | Meaning                                  |
| ---- | ---------------------------------------- |
| 0    | success                                  |
| 1    | SQL error (parse / type / runtime / etc) |
| 2    | CLI usage error                          |
| 3    | storage / IO error                       |

The data stream goes to stdout; per-statement summaries
(`ok N rows affected`, `ok N rows`) and error banners go to stderr, so
scripts can pipe results separately from progress.

## SQL subset

Statements: `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX`, `DROP INDEX`,
`INSERT`, `SELECT`, `UPDATE`, `DELETE`, `BEGIN`, `COMMIT`, `ROLLBACK`.

Meta-commands inside the REPL: `.tables`, `.indexes`, `.schema [<name>]`,
`.format {table|csv|json}`, `.exit`.

Column types: `INT` / `BIGINT` (`i64`), `REAL` (`f64`), `TEXT`, `BOOL`,
plus `NULL` (with SQL three-valued logic).

Constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT <literal>`,
table-level `CHECK (<expr>)`.

`SELECT` features:

- projection: `*`, `tbl.*`, `expr [AS alias]`
- single table or `INNER JOIN ... ON`
- `WHERE` with `AND` / `OR` / `NOT`, comparisons (`=`, `<>`, `<`, `<=`,
  `>`, `>=`), `IS NULL` / `IS NOT NULL`
- `ORDER BY ... [ASC|DESC]`
- `LIMIT N [OFFSET M]`

Expressions: arithmetic (`+ - * /`), string concat (`||`), and the
scalar functions `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`.

`INSERT` accepts both positional `INSERT INTO t VALUES (...)` and named
`INSERT INTO t (c1, c2) VALUES (...)`.

`UPDATE t SET c = expr [, ...] [WHERE expr]` and
`DELETE FROM t [WHERE expr]` report the number of affected rows.

## Example session

```
$ minidb --db demo.db --eval "
    CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, age INT);
    INSERT INTO users VALUES (1, 'alice', 30), (2, 'bob', 25);
    CREATE INDEX ix_users_age ON users(age);
    SELECT name FROM users WHERE age > 26 ORDER BY name;
"
+-------+
| name  |
+-------+
| alice |
+-------+
ok 1 row
```

CSV output:

```
$ minidb --db demo.db --format csv --eval "SELECT id, name FROM users ORDER BY id;"
id,name
1,alice
2,bob
```

JSON output:

```
$ minidb --db demo.db --format json --eval "SELECT id, name FROM users ORDER BY id;"
[{"id":1,"name":"alice"},{"id":2,"name":"bob"}]
```

Transactions:

```sql
BEGIN;
UPDATE users SET age = age + 1 WHERE id = 1;
ROLLBACK;     -- discards the update
```

## Architectural overview

Top-level modules under `src/`:

| Module     | Responsibility                                                       |
| ---------- | -------------------------------------------------------------------- |
| `lexer`    | SQL tokenizer (keyword recognition, position tracking, string escapes)
| `parser`   | Recursive-descent parser → `parser::ast` (no storage access)
| `plan`     | `LogicalPlan` and `PhysicalPlan` (single-predicate index pushdown)
| `exec`     | Volcano-style `PhysicalOperator` trait and operators
| `storage`  | Page manager, write-ahead log, B-tree secondary index
| `txn`      | Single-writer transaction manager (begin/commit/rollback)
| `catalog`  | Schema and index metadata, in-memory table data
| `engine`   | Top-level orchestration (open/recover/execute)
| `cli`      | Argument parsing, REPL loop, output formatting (table/CSV/JSON)
| `error`    | Crate-wide `MiniDbError` enum

Volcano operators (in `src/exec/`): `SeqScan`, `IndexScan`, `Filter`,
`Project`, `NestedLoopJoin`, `Sort`, `Limit`, plus the DML structs
`Insert`, `Update`, and `Delete`. Each streaming operator implements
`open` / `next` / `close` and exposes its output schema.

### Storage format

The database file is a sequence of 4 KiB pages addressed by `PageId(u64)`.
Page 0 is the header (magic, version, page count, snapshot length). The
remaining pages hold a bincode-serialized snapshot of the catalog (which
includes table data and B-tree indexes).

### WAL and recovery

Each logical mutation appends a length-prefixed CRC32-protected record
to `<db>.wal`. On commit the WAL is fsynced before the snapshot is
flushed. On startup, replay walks the WAL, applies records belonging to
transactions whose `Commit` marker is present, and discards records of
transactions that were not committed (or were torn at the tail). After
replay the snapshot is rewritten and the WAL is truncated.

### Indexes

Secondary indexes live in `storage::btree` and are keyed by
`(index_key, row_id)`. The physical planner emits an `IndexScan` when
the `WHERE` clause contains exactly one equality / range predicate on
an indexed column.

## Project layout

```
.
├── Cargo.toml
├── README.md
├── src/
│   ├── catalog.rs
│   ├── cli.rs
│   ├── engine.rs
│   ├── error.rs
│   ├── exec/
│   │   ├── delete.rs
│   │   ├── expr.rs
│   │   ├── filter.rs
│   │   ├── index_scan.rs
│   │   ├── insert.rs
│   │   ├── join.rs
│   │   ├── limit.rs
│   │   ├── mod.rs
│   │   ├── operator.rs
│   │   ├── project.rs
│   │   ├── seq_scan.rs
│   │   ├── sort.rs
│   │   └── update.rs
│   ├── lexer.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── parser/
│   │   ├── ast.rs
│   │   └── mod.rs
│   ├── plan/
│   │   ├── logical.rs
│   │   ├── mod.rs
│   │   └── physical.rs
│   ├── storage/
│   │   ├── btree.rs
│   │   ├── heap.rs
│   │   ├── mod.rs
│   │   ├── page.rs
│   │   └── wal.rs
│   ├── txn.rs
│   └── types.rs
└── tests/
    └── end_to_end.rs
```
