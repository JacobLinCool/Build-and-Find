# minidb

An embeddable, single-file SQL-subset relational database written in Rust.

## Build

```sh
cargo build --release
cargo test
```

The binary is produced at `target/release/minidb`.

## Usage

```sh
# Interactive REPL
minidb --db mydata.db

# Evaluate one or more semicolon-separated statements
minidb --db mydata.db --eval "CREATE TABLE t (id INT PRIMARY KEY, n TEXT); INSERT INTO t VALUES (1,'a'); SELECT * FROM t;"

# Execute a SQL script file
minidb --db mydata.db --file script.sql

# Output formats: table (default), csv, json
minidb --db mydata.db --format csv --eval "SELECT * FROM t;"
```

REPL meta-commands: `.exit`, `.tables`, `.indexes`, `.schema [name]`, `.format <table|csv|json>`.

Exit codes: `0` success, `1` SQL/runtime error, `2` CLI usage error.

## SQL subset

- DDL: `CREATE TABLE` (with `PRIMARY KEY`, `UNIQUE`, `NOT NULL`, `DEFAULT`, `CHECK`), `DROP TABLE`, `CREATE INDEX`, `DROP INDEX`.
- DML: `INSERT` (positional & named, multi-row), `SELECT` (projections, `INNER JOIN ... ON`, `WHERE`, `ORDER BY`, `LIMIT`/`OFFSET`), `UPDATE`, `DELETE`.
- Types: `INT`, `REAL`, `TEXT`, `BOOL`, with SQL `NULL` and three-valued logic.
- Expressions: arithmetic (`+ - * / %`), comparisons (`= <> < <= > >=`), `AND`/`OR`/`NOT`, `IS NULL`/`IS NOT NULL`, string `||`, scalar functions `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`.
- Transactions: `BEGIN` / `COMMIT` / `ROLLBACK` (auto-commit when no explicit transaction is active).

## Architecture

- **lexer / parser** — tokenizer + recursive-descent SQL parser producing an AST.
- **plan** — logical plan with predicate extraction, then physical plan with index-scan pushdown for single eq/range predicates on indexed columns.
- **exec** — Volcano-style operators (`SeqScan`, `IndexScan`, `Filter`, `Project`, `NestedLoopJoin`, `Sort`, `Limit`) and a `Database` engine that ties statement execution, the catalog, indexes, and the transaction manager together.
- **storage** — 4096-byte paged file with a slotted-page heap, secondary B-tree indexes (sidecar-persisted), and a CRC32-framed append-only WAL.
- **transactions** — single-writer "no-steal, force-at-commit": mutations apply in memory, snapshots of catalog/indexes/page-allocator are kept for `ROLLBACK`, and `COMMIT` writes WAL records then fsyncs WAL → flushes pager → truncates WAL. On open, any committed-but-unflushed WAL records are replayed idempotently (records are keyed by physical `(page, slot)`).

## Layout

```
src/
  lexer.rs           tokenizer
  parser/            AST + parser
  plan/              logical & physical plans
  exec/              expr evaluator, operators, Database engine
  storage/           pager, slotted heap, B-tree, WAL
  catalog.rs         table & index schemas (sidecar-persisted)
  txn.rs             transaction manager
  cli.rs / main.rs   CLI + REPL
tests/integration.rs end-to-end tests against the public API and the CLI binary
```
