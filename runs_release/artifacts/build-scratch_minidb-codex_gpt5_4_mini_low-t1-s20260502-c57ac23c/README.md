# minidb

`minidb` is a single-binary embeddable SQL-subset database with a lexer, parser, planner, executor, storage layer, WAL, and CLI.

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```

## Usage

Initialize a database file:

```bash
minidb --init --db ./minidb.db
```

Run a one-shot SQL statement:

```bash
minidb --db ./minidb.db --eval "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'alice'); SELECT * FROM t;"
```

Start the REPL:

```bash
minidb --repl --db ./minidb.db
```

## Supported SQL

- `CREATE TABLE`, `DROP TABLE`
- `CREATE INDEX`, `DROP INDEX`
- `INSERT`, `SELECT`, `UPDATE`, `DELETE`
- `BEGIN`, `COMMIT`, `ROLLBACK`
- Meta commands: `.schema`, `.tables`, `.indexes`, `.exit`, `.format`

## Architecture

- `lexer`: tokenizes SQL input
- `parser`: converts tokens into an AST
- `plan`: logical and physical plan placeholders
- `exec`: volcano-style operator trait plus statement execution
- `storage`: snapshot persistence, WAL, heap, and btree index representation
- `txn`: transaction manager
- `catalog`: schema and index metadata
- `types`: values, columns, and tri-valued logic
- `cli`: CLI parsing and output formatting

