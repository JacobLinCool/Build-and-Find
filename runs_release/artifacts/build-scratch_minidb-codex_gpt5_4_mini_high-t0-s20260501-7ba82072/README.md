# minidb

`minidb` is a single-binary, on-disk relational database implemented in Rust. It supports a SQL subset with tables, indexes, basic transactions, a page-based snapshot file, and an append-only WAL.

## Build And Install

```bash
cargo build --release
```

The binary is produced at `target/release/minidb`.

## Usage

Create a fresh database:

```bash
minidb --db ./minidb.db --init
```

Run a statement batch:

```bash
minidb --db ./minidb.db --eval "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'alice'); SELECT * FROM t;"
```

Run a SQL script:

```bash
minidb --db ./minidb.db --file ./script.sql
```

Start a REPL:

```bash
minidb --db ./minidb.db --repl
```

## SQL Subset

Supported statements:

- `CREATE TABLE`
- `DROP TABLE`
- `CREATE INDEX`
- `DROP INDEX`
- `INSERT`
- `SELECT`
- `UPDATE`
- `DELETE`
- `BEGIN`
- `COMMIT`
- `ROLLBACK`
- Meta commands: `.schema`, `.tables`, `.indexes`, `.format`, `.exit`

Supported types:

- `INT` / `BIGINT`
- `REAL`
- `TEXT`
- `BOOL`
- `NULL` literals in expressions and defaults

Supported query features:

- Projection, `*`, and qualified `table.column`
- Single-table `FROM` and `INNER JOIN`
- `WHERE` with `AND`, `OR`, `NOT`, `=`, `<>`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`
- `ORDER BY ... ASC|DESC`
- `LIMIT ... OFFSET ...`
- Expressions with `+`, `-`, `*`, `/`, `||`
- Scalar functions `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`

## Example Session

```sql
CREATE TABLE users (
  id INT PRIMARY KEY,
  name TEXT NOT NULL,
  active BOOL DEFAULT TRUE
);

INSERT INTO users VALUES (1, 'alice', TRUE);
INSERT INTO users (id, name) VALUES (2, 'bob');
SELECT id, name FROM users ORDER BY id;
```

Example output:

```text
id | name
---+-----
1  | alice
2  | bob
```

## Architecture

- `lexer`: tokenizes SQL text with line/column tracking.
- `parser`: builds an AST for statements and expressions.
- `plan`: contains logical planning and physical plan assembly.
- `exec`: provides the Volcano-style physical operators and expression evaluation.
- `storage`: page manager, WAL, heap tables, and B-tree indexes.
- `txn`: transaction state, recovery, and commit/rollback handling.
- `catalog`: schema and index metadata.
- `types`: values, columns, rows, and three-valued logic.
- `cli`: command-line interface, REPL, output formatting, and statement dispatch.

## Notes

- The database file is snapshot-based and paired with a `.wal` file.
- Recovery replays committed WAL records after the last snapshot checkpoint.
- Output formats: `table`, `csv`, and `json`.

