# minidb

`minidb` is a small embeddable SQL-subset database written in Rust.

## Build

```bash
cargo build --release
```

## Run

```bash
./target/release/minidb --init --db ./minidb.db
./target/release/minidb --db ./minidb.db --eval "CREATE TABLE t (id INT); INSERT INTO t VALUES (1); SELECT * FROM t;"
./target/release/minidb --db ./minidb.db --repl
```

## Supported SQL

- `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX`, `DROP INDEX`
- `INSERT`, `SELECT`, `UPDATE`, `DELETE`
- `BEGIN`, `COMMIT`, `ROLLBACK`
- meta commands: `.schema`, `.tables`, `.indexes`, `.exit`, `.format <kind>`

Supported types include `INT`, `BIGINT`, `REAL`, `TEXT`, `BOOL`, and `NULL` values.

## Architecture

- `lexer` tokenizes SQL text.
- `parser` turns tokens into AST nodes.
- `plan` lowers AST into executable plan nodes.
- `exec` executes statements against the catalog and tables.
- `storage` persists catalog state and manages the WAL.
- `txn` manages explicit transaction boundaries.
- `catalog` holds schema and index metadata.
- `types` defines values, expressions, and schema primitives.
- `cli` provides batch and REPL entry points.

