# minidb

`minidb` is a small, embeddable, file-backed, SQL-subset relational database
written in pure Rust. It is shipped as a single self-contained binary plus a
companion library and was built as a from-scratch reference of the major
moving parts of a real RDBMS — a SQL lexer/parser, a logical/physical planner,
a Volcano-style executor, a paged storage engine with a heap and a B-tree, a
write-ahead log with crash recovery, and a transaction manager.

## Quick start

```sh
cargo build --release
./target/release/minidb mydb.db --eval "
  CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
  INSERT INTO users VALUES (1, 'alice'), (2, 'bob');
  SELECT * FROM users ORDER BY id;
"
```

```sh
# REPL
./target/release/minidb mydb.db --repl

# Pipe SQL on stdin (also works without --repl)
echo "SELECT 1 + 1 AS x" | ./target/release/minidb mydb.db --format csv
```

## CLI

```
USAGE:
    minidb [DB_PATH] [OPTIONS]

OPTIONS:
    --repl              Force REPL mode (default when stdin is a TTY)
    --eval SQL          Execute the given SQL string and exit
    --file PATH         Execute SQL from PATH and exit
    --init PATH         Run SQL from PATH before reading further input
    --format FMT        table|csv|json (default: table)
    --quiet             Suppress per-statement summary on stderr
    --continue-on-error Keep going after a SQL error in batch/file modes
    -V, --version       Print version
    -h, --help          Show help
```

Exit codes follow the standard contract:

| Code | Meaning                              |
|------|--------------------------------------|
| 0    | success                              |
| 1    | SQL / runtime error                  |
| 2    | CLI usage error                      |
| 3    | storage / IO error                   |

Meta-commands (REPL or stdin):

- `.tables` — list table names
- `.indexes` — list indexes (name, table, column)
- `.schema [name]` — print CREATE-style schema
- `.format table|csv|json` — change output format
- `.exit` / `.quit` — end the session

## SQL subset

`minidb` implements a deliberately small but coherent subset of SQL — enough
to write tests, run small workloads, and exercise the major code paths.

### Data types

- `INT` (alias `INTEGER`, `BIGINT`) — 64-bit signed integer
- `REAL` (alias `DOUBLE`, `FLOAT`) — 64-bit floating point
- `TEXT` (alias `VARCHAR`, `STRING`) — UTF-8 string
- `BOOL` (alias `BOOLEAN`)
- `NULL` is supported on every column unless `NOT NULL` is set

### DDL

```sql
CREATE TABLE name (
    col1 INT PRIMARY KEY,
    col2 TEXT NOT NULL UNIQUE,
    col3 REAL DEFAULT 0.0,
    col4 BOOL DEFAULT FALSE,
    CHECK (col3 >= 0)
);

DROP TABLE [IF EXISTS] name;

CREATE INDEX idx_name ON table(column);
DROP   INDEX [IF EXISTS] idx_name;
```

A B-tree index is automatically created for `PRIMARY KEY` and `UNIQUE`
columns; user-declared `CREATE INDEX` is non-unique unless the underlying
column is unique.

### DML

```sql
INSERT INTO t VALUES (1, 'a'), (2, 'b');
INSERT INTO t (id, name) VALUES (3, 'c');

UPDATE t SET name = 'x' WHERE id = 1;
DELETE FROM t WHERE id = 2;
```

### Queries

```sql
SELECT *, a.x AS x_aliased
FROM   a INNER JOIN b ON a.id = b.aid
WHERE  a.x > 0 AND (b.y IS NULL OR b.y = 'k')
ORDER BY a.x DESC, a.id ASC
LIMIT 10 OFFSET 5;
```

Supported expression vocabulary:

- arithmetic: `+ - * /`
- comparison: `= != < <= > >=`
- logical: `AND OR NOT`
- null: `IS NULL`, `IS NOT NULL`
- string: `||` (concat)
- functions: `LENGTH(s)`, `LOWER(s)`, `UPPER(s)`, `SUBSTR(s, start[, len])`,
  `ABS(n)`, `COALESCE(v1, v2, ...)`

`NULL` participates in three-valued logic (`AND`/`OR`/`NOT`/comparisons all
propagate `NULL` correctly).

### Transactions

```sql
BEGIN;
INSERT INTO t VALUES (10);
INSERT INTO t VALUES (11);
COMMIT;            -- or ROLLBACK
```

DML statements inside an open transaction are buffered in memory and become
visible only on `COMMIT`. DDL statements (`CREATE`/`DROP TABLE`,
`CREATE`/`DROP INDEX`) implicitly commit any open transaction first, then
auto-commit themselves.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                       cli (REPL/eval)                   │
└──────────┬──────────────────────────────────────────────┘
           │
       parser ──── ast::Statement
           │
        plan/planner ──── PhysicalPlan / WritePlan
           │
        exec/runner ──── Volcano operators
           │
        engine ─────── catalog + transaction manager
           │
        storage ────── pager · heap · btree · WAL
           │
           ▼
       data file (4 KiB pages) + .wal file
```

| Module      | Responsibility                                                   |
|-------------|------------------------------------------------------------------|
| `lexer`     | SQL → tokens with line/column positions                          |
| `parser`    | tokens → AST (`Statement`/`Expr`)                                |
| `plan`      | AST → logical/physical plans + index selection                   |
| `exec`      | Volcano operators (`SeqScan`, `IndexScan`, `Filter`, `Project`,  |
|             | `NestedLoopJoin`, `Sort`, `Limit`, `Insert`, `Update`, `Delete`) |
| `storage`   | 4 KiB pager, slotted heap pages, B-tree leaf chain, WAL          |
| `catalog`   | tables + indexes (persisted as a chain of pages)                 |
| `txn`       | transaction manager + buffered op-log                            |
| `engine`    | top-level read/write API used by the executor                    |
| `cli`       | argument parsing, REPL, output formatting                        |

### Storage model

- Page size: 4096 bytes.
- The pager exposes `allocate_page` / `free_page` over a free list, plus a
  fixed header on page 0 (`MINIDB01` magic, page count, free-list head,
  catalog-root pointer, format version).
- Heap rows are stored in slotted pages. Pages are chained when full.
- Indexes are sorted leaf chains (a single-level B-tree variant); split on
  overflow. Adequate for the small data sets we test against; multi-level
  internal nodes are an obvious next step.
- The catalog is serialised into a chain of pages on every DDL change.

### Crash recovery

All mutations are written to the WAL ahead of any data-file change, fsynced
on commit, and only then applied to the heap/B-tree pages. WAL records are
framed with `[u32 payload_len, u32 crc32, payload]`; truncated tails (a
crash mid-fsync) are detected via CRC and dropped at recovery time.

On startup, `Engine::open()` replays the WAL, applies any **committed**
records, persists the catalog, fsyncs the data file, and truncates the WAL.

## Library API

`minidb` is also a `lib` crate. The simplest possible use:

```rust
use minidb::{run_sql, Engine, StatementOutcome};

fn main() -> minidb::Result<()> {
    let mut db = Engine::open("mydb.db")?;
    run_sql(&mut db, "CREATE TABLE t (id INT, name TEXT)")?;
    run_sql(&mut db, "INSERT INTO t VALUES (1, 'a')")?;
    if let StatementOutcome::Select(rs) = run_sql(&mut db, "SELECT * FROM t")? {
        for row in &rs.rows {
            println!("{:?}", row);
        }
    }
    Ok(())
}
```

## Tests

```sh
cargo test            # unit tests + integration tests
cargo test --lib      # unit tests only
cargo test --test cli_smoke      # binary-level CLI smoke
cargo test --test library_smoke  # in-process library smoke
```

## Limitations

This is a pedagogical implementation; some sharp edges:

- Single-writer; no concurrent transactions.
- B-tree is a single-level sorted leaf chain (works correctly but pays for
  long chains as data grows).
- No `GROUP BY`, no aggregates, no subqueries, no `LEFT`/`RIGHT JOIN`,
  no views, no triggers.
- Column-level `CHECK` constraints are parsed but not enforced beyond
  basic type/NULL/UNIQUE checks.
