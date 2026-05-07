# MiniDB

An embeddable SQL-subset relational database written in Rust.

## Build & Install

```bash
cargo build --release
```

The binary will be at `target/release/minidb`.

## Quick Start

```bash
# Initialize a new database
minidb --init --db mydb.db

# Execute SQL directly
minidb --db mydb.db --eval "CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);"

# Start interactive REPL
minidb --db mydb.db --repl

# Execute SQL from a file
minidb --db mydb.db --file script.sql

# Output as CSV or JSON
minidb --db mydb.db --format csv --eval "SELECT * FROM users;"
minidb --db mydb.db --format json --eval "SELECT * FROM users;"
```

## SQL Reference

### Data Types

| Type | Description |
|------|-------------|
| `INT` | 64-bit signed integer |
| `BIGINT` | 64-bit signed integer (alias for INT) |
| `REAL` | 64-bit floating point |
| `TEXT` | Variable-length string |
| `BOOL` | Boolean (`TRUE` / `FALSE`) |

### Constraints

- `PRIMARY KEY` - Unique, not null identifier
- `NOT NULL` - Disallow NULL values
- `UNIQUE` - All values in column must be distinct
- `DEFAULT <expr>` - Default value when not specified
- `CHECK (<expr>)` - Row-level condition

### DDL Statements

```sql
CREATE TABLE table_name (
    col1 INT PRIMARY KEY,
    col2 TEXT NOT NULL,
    col3 REAL DEFAULT 0.0,
    col4 BOOL UNIQUE,
    col5 INT CHECK (col5 >= 0)
);

DROP TABLE table_name;

CREATE INDEX idx_name ON table_name (column_name);
DROP INDEX idx_name;
```

### DML Statements

```sql
-- Insert
INSERT INTO t VALUES (1, 'Alice', 3.14);
INSERT INTO t (id, name) VALUES (2, 'Bob');

-- Select
SELECT * FROM t;
SELECT id, name FROM t WHERE id > 0 ORDER BY name LIMIT 10 OFFSET 5;
SELECT DISTINCT category FROM products;

-- Update
UPDATE t SET name = 'Alicia' WHERE id = 1;

-- Delete
DELETE FROM t WHERE id = 1;
```

### Joins

```sql
SELECT a.name, b.value
FROM a INNER JOIN b ON a.id = b.a_id;

SELECT a.name, b.value
FROM a LEFT JOIN b ON a.id = b.a_id;

SELECT a.name, b.value
FROM a CROSS JOIN b;
```

### Expressions

```sql
-- Arithmetic
SELECT price * quantity AS total FROM orders;

-- String concatenation
SELECT first_name || ' ' || last_name AS full_name FROM users;

-- Built-in functions
SELECT LENGTH(name), LOWER(email), UPPER(code) FROM t;
SELECT ABS(delta), SUBSTR(name, 1, 3) FROM t;
SELECT COALESCE(nickname, name, 'unknown') FROM users;

-- CASE expression
SELECT CASE WHEN score >= 90 THEN 'A' ELSE 'B' END AS grade FROM students;

-- NULL checks
SELECT * FROM t WHERE val IS NULL;
SELECT * FROM t WHERE val IS NOT NULL;

-- IN list
SELECT * FROM t WHERE status IN ('active', 'pending');

-- BETWEEN
SELECT * FROM t WHERE age BETWEEN 18 AND 65;

-- LIKE pattern matching
SELECT * FROM t WHERE name LIKE 'Al%';
```

### Transactions

```sql
BEGIN;
INSERT INTO accounts VALUES (1, 1000);
UPDATE accounts SET balance = balance - 100 WHERE id = 1;
UPDATE accounts SET balance = balance + 100 WHERE id = 2;
COMMIT;

BEGIN;
DELETE FROM t WHERE id = 1;
ROLLBACK;  -- undo the delete
```

### Meta Commands (REPL)

```
.schema      Show all table schemas
.tables      List all tables
.indexes     List all indexes
.format csv  Change output format
.exit        Exit the REPL
```

## Example Session

```
$ minidb --db demo.db --repl
minidb> CREATE TABLE employees (
    ...>     id INT PRIMARY KEY,
    ...>     name TEXT NOT NULL,
    ...>     department TEXT,
    ...>     salary REAL
    ...> );
ok
minidb> INSERT INTO employees VALUES (1, 'Alice', 'Engineering', 120000);
ok 1 row affected
minidb> INSERT INTO employees VALUES (2, 'Bob', 'Marketing', 95000);
ok 1 row affected
minidb> INSERT INTO employees VALUES (3, 'Charlie', 'Engineering', 110000);
ok 1 row affected
minidb> SELECT name, salary FROM employees WHERE department = 'Engineering' ORDER BY salary DESC;
   name | salary
--------+--------
Charlie | 110000
  Alice | 120000
ok 2 rows
minidb> SELECT department, SUM(salary) FROM employees GROUP BY department;
department | SUM(salary)
-----------+------------
Engineering |      230000
  Marketing |       95000
ok 2 rows
minidb> .exit
```

## Architecture

MiniDB follows a classic database architecture with these layers:

```
SQL text
  |
  v
Lexer (tokenizer)  -->  Parser (AST)  -->  Planner (logical plan)
                                                |
                                                v
                                         Optimizer (pushdown filters, index selection)
                                                |
                                                v
                                         Physical Plan  -->  Executor (Volcano iterator model)
                                                                  |
                                                                  v
                                                            Storage Engine
                                                           /      |      \
                                                     Pager    Heap    B-tree
                                                       |       |        |
                                                       v       v        v
                                                    Pages   Rows    Index
                                                       |
                                                       v
                                                     WAL (crash recovery)
```

- **Lexer/Parser**: Tokenizes SQL and builds an AST with operator precedence
- **Planner**: Converts AST to logical plan, optimizes filter pushdown and index usage
- **Executor**: Volcano-style iterator model (open/next/close) for pipeline execution
- **Storage**: Page-based (4096 bytes) with slotted page heap files and B-tree indexes
- **WAL**: Write-ahead logging with CRC32 checksums for crash recovery
- **Transactions**: Single-writer model with auto-commit and explicit BEGIN/COMMIT/ROLLBACK

## Testing

```bash
cargo test
```

## License

MIT
