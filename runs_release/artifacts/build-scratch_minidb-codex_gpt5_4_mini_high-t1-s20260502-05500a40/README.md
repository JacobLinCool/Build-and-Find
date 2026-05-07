# minidb

`minidb` is a small embeddable SQL-subset relational database written in Rust.
It stores data on disk, writes a CRC-protected WAL, and supports transactional
statement execution through a command-line interface or interactive REPL.

## Build and install

```bash
cargo build --release
```

The binary is produced at `target/release/minidb`.

## Usage

### REPL

Start an interactive session when stdin is a TTY or by passing `--repl`:

```bash
./target/release/minidb --db ./minidb.db --repl
```

### Batch evaluation

Run one statement or a semicolon-separated script:

```bash
./target/release/minidb --db ./minidb.db --eval "CREATE TABLE t (id INT PRIMARY KEY); INSERT INTO t VALUES (1); SELECT * FROM t;"
```

### Script files

```bash
./target/release/minidb --db ./minidb.db --file script.sql
```

Use `--continue-on-error` to keep running after a failing statement.

### Database initialization

```bash
./target/release/minidb --db ./minidb.db --init
```

## SQL subset

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

Supported column types:

- `INT`
- `BIGINT`
- `REAL`
- `TEXT`
- `BOOL`
- `NULL` values in data and expressions

Supported constraints:

- `PRIMARY KEY`
- `NOT NULL`
- `UNIQUE`
- `DEFAULT <literal>`
- `CHECK (<expr>)`

Supported query features:

- projection with `*`, `table.*`, qualified column names, and expressions
- single-table `FROM` or `INNER JOIN` of two tables
- `WHERE` with `AND`, `OR`, `NOT`, comparisons, and `IS NULL` / `IS NOT NULL`
- `ORDER BY ... ASC|DESC`
- `LIMIT ... [OFFSET ...]`
- scalar functions: `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`

## Output formats

Select results can be rendered as:

- table: default human-readable format
- csv: RFC 4180 style CSV
- json: one JSON array of row objects per result set

Set the format with `--format {table,csv,json}` or the REPL meta-command
`.format <kind>`.

## Meta-commands

- `.schema` prints the schema as `CREATE TABLE` / `CREATE INDEX` statements
- `.tables` prints table names
- `.indexes` prints index names
- `.format <kind>` changes the current output format
- `.exit` exits the REPL

## Architecture

The crate is organized into a few clear layers:

- `lexer` turns SQL text into tokens with line and column information
- `parser` builds the SQL AST
- `plan` chooses logical and physical execution shapes
- `exec` implements iterator-style physical operators and expression evaluation
- `storage` owns the page manager, WAL, heap/index structures, and snapshot I/O
- `txn` manages begin/commit/rollback and statement-level atomicity
- `catalog` stores schema and index metadata
- `types` defines values, row containers, and schema helpers
- `cli` is the top-level binary interface

## Example session

```text
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
ok
minidb> INSERT INTO users VALUES (1, 'Ada');
ok 1 row affected
minidb> SELECT id, name FROM users;
id | name
---+-----
1  | Ada
ok 1 rows
```

