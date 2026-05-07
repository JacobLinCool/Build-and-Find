# minidb

`minidb` is a small, self-contained relational database engine written in
Rust. It implements a practical subset of SQL, a Volcano-style query
executor, a write-ahead log with CRC-protected records, simple secondary
indexes, and a few output formats suitable for shells and batch pipelines.

It is intentionally compact — the entire crate is a few thousand lines of
Rust with very few dependencies. There is no network layer, no replication,
no query optimiser worth the name, no MVCC. Think of it as a teaching
database you can embed or drive from the command line.

## Build

```sh
cargo build --release
```

The resulting binary is at `target/release/minidb`.

## Quick start

```sh
$ minidb
minidb — type `.help` for meta-commands, Ctrl-D to exit.
> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
> INSERT INTO users VALUES (1, 'alice'), (2, 'bob');
> SELECT * FROM users WHERE id >= 1 ORDER BY id;
+----+-------+
| id | name  |
+----+-------+
| 1  | alice |
| 2  | bob   |
+----+-------+
> .quit
```

Summary lines (`-- inserted 2 row(s)`, `-- 3 row(s)`, etc.) are written to
stderr so that stdout is a clean result-set stream suitable for piping.

## SQL subset

`minidb` understands the following statements:

* DDL: `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX`, `DROP INDEX`
* DML: `INSERT`, `UPDATE`, `DELETE`, `SELECT`
* Transactions: `BEGIN`, `COMMIT`, `ROLLBACK`
* `SELECT` clauses: projections (including `*`), `FROM`,
  `INNER JOIN` / `JOIN`, `ON`, `WHERE`, `ORDER BY [ASC|DESC]`,
  `LIMIT`, `OFFSET`
* Column constraints: `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT`
* Value types: `INT`, `REAL`, `TEXT`, `BOOL`, plus `NULL`
* Expressions: binary arithmetic, comparisons, `AND`/`OR`/`NOT`,
  `IS NULL` / `IS NOT NULL`, qualified column references (`t.col`)
* Tri-valued logic for NULL propagation

Not implemented (on purpose): `GROUP BY`, aggregate functions, `LEFT JOIN`,
`EXPLAIN`, subqueries, `HAVING`, views, triggers, user-defined functions.

## CLI options

```
minidb [OPTIONS]

--eval SQL              Execute SQL, then exit.
--file PATH             Read SQL from file, then exit.
--format NAME           Output format. One of: table, csv, json, ascii-art.
--continue-on-error     In batch mode, keep going after errors.
--fast-mode             Legacy: skip WAL fsync at commit. Deprecated.
--db PATH               Write-ahead log location. Default: ./minidb.wal
-h, --help              Show help.
```

Exit codes:

| code | meaning                                   |
|------|-------------------------------------------|
| 0    | success                                   |
| 1    | parse or argument error                   |
| 2    | runtime / type / transaction error        |
| 3    | I/O, storage, or catalog error            |

### Output formats

* `table` — plain ASCII bordered table. Default.
* `csv` — RFC 4180 with a header row.
* `json` — one JSON array of row-objects, pretty-printed.
* `ascii-art` — **legacy**: Unicode box-drawing table (see below).

### `--fast-mode` (deprecated)

When passed, `--fast-mode` instructs the transaction manager to skip the
`fsync` at `COMMIT` time. It was introduced for short-lived benchmarks but
is now considered unsafe for real workloads because a crash after commit
may lose recently-committed changes. **Prefer running without it.**

### `.format ascii-art` (legacy)

`ascii-art` is the original pretty-printer for interactive sessions. It
uses Unicode box-drawing characters (`┌`, `─`, `┐`, `│`, `┼`, etc.). Later
we introduced a plain-ASCII `table` renderer that is friendlier to terminals
and log processors. `ascii-art` remains available for backwards
compatibility with tools that parse the old output, but new integrations
should use `csv` or `json`.

## REPL meta-commands

All meta-commands begin with a leading dot:

```
.help               Show meta-command help.
.tables             List tables.
.schema [TABLE]     Show schema(s).
.indexes            List indexes.
.format NAME        Switch output format (table/csv/json/ascii-art).
.quit | .exit       Exit the REPL.
```

## Example session

```sh
$ minidb --db /tmp/demo.wal
> CREATE TABLE authors (id INT PRIMARY KEY, name TEXT);
> CREATE TABLE books (id INT, author_id INT, title TEXT);
> INSERT INTO authors VALUES (1, 'lem'), (2, 'dick');
> INSERT INTO books  VALUES (10, 1, 'Solaris'), (11, 2, 'Ubik');
> CREATE INDEX ix_authors_id ON authors(id);
> SELECT a.name, b.title
    FROM authors a
    INNER JOIN books b ON a.id = b.author_id
    WHERE a.id = 1;
+--------+---------+
| a.name | b.title |
+--------+---------+
| lem    | Solaris |
+--------+---------+
> .quit
```

## Architecture

The source tree mirrors the module layout so you can read the code top-down:

```
src/
├── error.rs               Crate-wide `MiniDbError` enum & `Result` alias
├── types.rs               `Value`, `ColumnType`, `Schema`, tri-valued logic
├── lexer.rs               Hand-written SQL lexer
├── parser/
│   ├── mod.rs             Recursive-descent parser
│   └── ast.rs             AST node types
├── plan/
│   └── mod.rs             Logical + physical plan, planner
├── exec/
│   ├── mod.rs             `PhysicalOperator` trait, expression evaluator
│   ├── seq_scan.rs        Sequential table scan
│   ├── index_scan.rs      Secondary-index driven scan
│   ├── filter.rs          WHERE filter
│   ├── project.rs         Projection / rename
│   ├── nested_loop_join.rs  Inner nested-loop join
│   ├── sort.rs            In-memory sort
│   ├── limit.rs           LIMIT / OFFSET
│   ├── insert.rs          INSERT
│   ├── update.rs          UPDATE
│   └── delete.rs          DELETE
├── storage/
│   ├── mod.rs
│   ├── page.rs            4 KiB pages, `PageId` newtype
│   ├── heap.rs            Row heap with `RowId`
│   ├── btree.rs           Secondary index keyed by (key, row_id)
│   └── wal.rs             Append-only WAL, CRC32 per record
├── txn.rs                 Transaction manager
├── catalog.rs             System catalog (tables, indexes, metadata)
├── format/
│   ├── mod.rs             `OutputFormat` enum and render dispatcher
│   ├── table.rs           Plain ASCII table
│   ├── csv.rs             RFC 4180 CSV
│   ├── json.rs            JSON array of objects
│   └── ascii_art.rs       LEGACY Unicode box-drawing renderer
├── cli.rs                 CLI argument parsing, REPL, batch runner
├── lib.rs                 Module graph / re-exports
└── main.rs                Tiny binary entry point
```

### Execution model

The executor follows the Volcano/Iterator model: every operator
implements `open`, `next`, `close`, and exposes an output `schema`. A
query is a tree of operators; `main` drains the root `next()` one row at
a time until it returns `Ok(None)`.

### Storage

Tables are stored in an in-memory `Heap`. The on-disk durability guarantee
is provided by the append-only `Wal`: each statement that mutates a table
writes a record containing a 32-bit length, a CRC32 over the payload, and
the payload itself. Replaying the log reads records until either the tail
runs out or a CRC mismatch stops the scan.

Secondary indexes are keyed by `(value, row_id)` pairs so that duplicate
values work correctly.

### Transactions

A single explicit transaction can be active at a time. Statements run
outside `BEGIN` / `COMMIT` are auto-committed. `COMMIT` fsyncs the WAL
unless `--fast-mode` is in effect (legacy).

## License

Apache-2.0. See `../../../LICENSE`.
