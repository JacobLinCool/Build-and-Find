# minidb

An embeddable SQL-subset relational database written in Rust.

## Building

```bash
cargo build --release
```

The binary will be at `target/release/minidb`.

## Usage

### Interactive REPL

```bash
minidb --db mydata.db
```

### Batch execution

```bash
minidb --eval "CREATE TABLE t (id INT PRIMARY KEY); INSERT INTO t VALUES (1);"
minidb --file script.sql
minidb --file script.sql --continue-on-error
```

### Initialize empty database

```bash
minidb --init --db mydata.db
```

### Command-line options

| Flag | Description |
|------|-------------|
| `--db <PATH>` | Database file path (default: `./minidb.db`) |
| `--eval "<SQL>"` | Execute SQL and exit |
| `--file <PATH>` | Execute SQL file |
| `--continue-on-error` | Don't stop on SQL errors in file mode |
| `--format <FMT>` | Output format: `table`, `csv`, `json` |
| `--quiet`, `-q` | Suppress per-statement summary |
| `--repl` | Force interactive REPL |
| `--init` | Create empty database and exit |
| `--version` | Print version |
| `--help` | Print help |

## SQL Support

### Statements

- `CREATE TABLE` with column constraints (`PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `DEFAULT`, `CHECK`)
- `DROP TABLE`
- `CREATE INDEX` / `DROP INDEX`
- `INSERT INTO ... VALUES (...)`
- `SELECT ... FROM ... WHERE ... ORDER BY ... LIMIT ... OFFSET ...`
- `UPDATE ... SET ... WHERE ...`
- `DELETE FROM ... WHERE ...`
- `BEGIN` / `COMMIT` / `ROLLBACK`

### Column Types

| Type | Description |
|------|-------------|
| `INT` | 64-bit signed integer |
| `BIGINT` | Alias for INT |
| `REAL` | 64-bit floating point |
| `TEXT` | Variable-length UTF-8 string |
| `BOOL` | Boolean |

### Expressions

- Arithmetic: `+`, `-`, `*`, `/`, `%`
- String concatenation: `||`
- Comparisons: `=`, `<>`, `<`, `<=`, `>`, `>=`
- Null tests: `IS NULL`, `IS NOT NULL`
- Boolean: `AND`, `OR`, `NOT`
- Scalar functions: `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`

### Meta-commands (REPL only)

| Command | Description |
|---------|-------------|
| `.schema` | Show CREATE TABLE statements |
| `.tables` | List all tables |
| `.indexes` | List all indexes |
| `.format <kind>` | Set output format (table/csv/json) |
| `.exit` | Exit the REPL |

## Example Session

```
$ minidb --db example.db
minidb> CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, email TEXT);
ok 0 rows

minidb> INSERT INTO users VALUES (1, 'Alice', 'alice@example.com');
ok 1 row

minidb> INSERT INTO users VALUES (2, 'Bob', 'bob@example.com');
ok 1 row

minidb> SELECT * FROM users WHERE id = 1;
   id |  name |        email
------+-------+-------------------
    1 | Alice | alice@example.com
ok 1 row

minidb> UPDATE users SET email = 'newalice@example.com' WHERE id = 1;
ok 1 row

minidb> .schema
CREATE TABLE users (
  id INT PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT
)
```

## Architecture

```
minidb/
  src/
    main.rs          - CLI entry point
    lib.rs           - Library root
    error.rs         - Crate-wide error enum (MiniDbError)
    types.rs         - Value, ColumnType, ColumnDef, Row, RowId
    lexer.rs         - SQL tokenizer
    parser/
      mod.rs         - SQL parser (recursive descent)
      ast.rs         - Abstract syntax tree types
    plan/
      mod.rs         - Plan module
      logical.rs     - Logical plan nodes
      physical.rs    - Physical plan nodes
    exec/
      mod.rs         - Execution module
      operators.rs   - Volcano-style physical operators
    storage/
      mod.rs         - Storage module
      page.rs        - Page manager (4096-byte pages)
      wal.rs         - Write-ahead log with CRC32
      heap.rs        - Heap file storage
      btree.rs       - B-tree secondary index
    catalog.rs       - Schema and index metadata
    txn.rs           - Transaction manager
    cli.rs           - REPL, batch mode, file mode
  tests/
    integration.rs   - End-to-end integration tests
```

### Key Design Decisions

- **Page-based storage**: 4096-byte pages with a page manager that handles allocation and I/O
- **Write-ahead log**: Append-only WAL with CRC32 checksums for crash recovery
- **Volcano-style execution**: Physical operators implement `open`/`next`/`close` iterator protocol
- **B-tree indexes**: Secondary indexes on individual columns for fast lookups
- **Single-writer transactions**: Simple transaction model with WAL framing for atomicity
- **Tri-valued logic**: NULL propagates through comparisons and arithmetic per SQL semantics
