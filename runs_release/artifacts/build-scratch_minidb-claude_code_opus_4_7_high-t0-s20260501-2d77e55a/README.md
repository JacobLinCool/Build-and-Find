# minidb

An embeddable SQL-subset relational database, distributed as a single Rust binary.
It includes a lexer, recursive-descent parser, logical+physical planner, a Volcano-style
iterator executor, a slotted-page heap storage engine with a write-ahead log, B-tree
secondary indexes, a transaction manager, and a REPL plus batch-evaluation CLI.

## Build

Requires a stable Rust toolchain (1.70+).

```sh
cargo build --release
```

The release binary is written to `target/release/minidb`.

To run the test suite:

```sh
cargo test
```

## Quick start

```sh
# Create an empty database directory.
minidb --init --db ./mydb

# Run SQL one-shot.
minidb --db ./mydb --eval "CREATE TABLE t(id INT, name TEXT); INSERT INTO t VALUES (1,'alice');"

# Read SQL from a script file.
minidb --db ./mydb --file script.sql

# Interactive REPL.
minidb --db ./mydb --repl
```

In the REPL, statements end with `;`. Meta commands begin with `.`:

```
minidb> .tables
minidb> .schema
minidb> .indexes
minidb> .format json
minidb> .exit
```

## CLI options

| Flag | Meaning |
|------|---------|
| `--db <PATH>` | Database directory (default `./minidb.db`). |
| `--eval <SQL>` | Run a single SQL string (or `;`-separated list) and exit. |
| `--file <PATH>` | Run SQL from a script file. |
| `--repl` | Force interactive REPL. |
| `--format <FORMAT>` | Output format for queries: `table`, `csv`, `json`. |
| `--continue-on-error` | Don't abort a script on the first failing statement. |
| `--quiet` | Suppress per-statement progress messages on stderr. |
| `--init` | Create a fresh empty database directory and exit. |
| `--version` / `-V` | Print the version. |
| `--help` / `-h` | Print help. |

Exit codes: `0` success, `1` SQL/runtime error, `2` CLI usage error, `3` storage/IO error.

## Supported SQL

minidb supports a useful subset of SQL:

- DDL: `CREATE TABLE` (with `INT`/`REAL`/`TEXT`/`BOOL`, `NOT NULL`, `PRIMARY KEY`,
  `UNIQUE`, `CHECK`), `DROP TABLE [IF EXISTS]`, `CREATE INDEX`, `DROP INDEX`.
- DML: `INSERT INTO ... VALUES (...)`, `UPDATE ... SET ... [WHERE ...]`,
  `DELETE FROM ... [WHERE ...]`.
- Queries: `SELECT` with column lists, `*`, expressions, aliases, `FROM` with
  `INNER JOIN ... ON ...`, `WHERE`, `ORDER BY ... [ASC|DESC]`, `LIMIT N [OFFSET M]`.
- Transactions: `BEGIN`, `COMMIT`, `ROLLBACK`. Statements outside an explicit
  transaction run in implicit auto-commit mode.
- Built-in scalar functions: `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`.
- Three-valued logic for `NULL` comparisons; `IS NULL` / `IS NOT NULL` for explicit
  NULL tests.

## Architecture

```
src/
  lexer.rs            tokenizer
  parser/             recursive-descent parser → AST
  plan/
    logical.rs        AST → LogicalPlan
    physical.rs       LogicalPlan → PhysicalPlan (picks index vs seq scan)
  exec/
    expr.rs           expression evaluator with three-valued logic
    operators.rs      Volcano operators: SeqScan, IndexScan, Filter,
                      Project, NestedLoopJoin, Sort, Limit,
                      Insert, Update, Delete
  storage/
    page.rs           4096-byte page abstraction
    pager.rs          page cache, dirty tracking, flush
    heap.rs           slotted-page heap files (linked list of pages)
    btree.rs          BTreeMap-backed secondary index keyed by (Value, RowId)
    wal.rs            CRC32-framed write-ahead log; page-image redo
  catalog.rs          on-disk JSON catalog of tables and indexes
  txn.rs              transaction state machine
  engine.rs           ties everything together; auto-commit + recovery
  cli/                args, REPL loop, output formatting
```

A database is a *directory* with this layout:

```
mydb/
  data            heap pages
  catalog.json    table+index catalog (atomically rewritten on commit)
  wal.log         write-ahead log
  indexes/        one file per index, bincode-encoded BTreeMap
```

## Durability and recovery

Each statement writes its dirty page images to the WAL before they reach the heap
file. On `COMMIT`, minidb appends a commit marker, fsyncs the WAL, flushes the
pager, atomically rewrites the catalog, persists dirty indexes, and finally
truncates the WAL. A crash mid-commit leaves the WAL with no commit marker, so
recovery only redoes fully-committed transactions on the next open.
