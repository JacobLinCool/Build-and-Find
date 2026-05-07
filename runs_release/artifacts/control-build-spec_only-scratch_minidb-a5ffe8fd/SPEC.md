spec_version: "0.1"

task_id: scratch_minidb
task_family: build_from_scratch
title: minidb - an embeddable SQL-subset relational database
objective: >
  Build `minidb`, a single-binary, on-disk relational database that
  implements a meaningful subset of SQL. The system must include a
  lexer, parser, logical+physical planner, an executor with multiple
  physical operators, a page-based storage engine with a write-ahead
  log, a transaction manager, and a REPL plus batch-evaluation CLI.
  The deliverable is a real project, not a toy: it is expected to span
  many thousands of lines of Rust across well-separated modules with
  unit tests in each module and a top-level integration test harness.

functional_requirements:
  - id: FR1
    statement: >
      The binary `minidb` must support an interactive REPL (invoked when
      stdin is a TTY or when `--repl` is passed) that reads SQL
      statements terminated by `;`, executes them against the database
      file given by `--db <PATH>` (default `./minidb.db`), and prints
      results. On EOF it exits cleanly with code 0.
  - id: FR2
    statement: >
      The binary must support a batch mode `--eval "<sql>"` that
      executes a single SQL statement (or semicolon-separated list) and
      exits. Exit codes: 0 on success, 1 on SQL error, 2 on CLI usage
      error, 3 on storage/IO error. Errors are printed on stderr with
      the offending line and column when available.
  - id: FR3
    statement: >
      The binary must support a batch mode `--file <PATH>` that reads a
      SQL script and executes every statement in order, stopping on the
      first error unless `--continue-on-error` is passed.
  - id: FR4
    statement: >
      SQL statements supported: `CREATE TABLE`, `DROP TABLE`,
      `CREATE INDEX`, `DROP INDEX`, `INSERT`, `SELECT`, `UPDATE`,
      `DELETE`, `BEGIN`, `COMMIT`, `ROLLBACK`, plus meta-commands
      `.schema`, `.tables`, `.indexes`, and `.exit`.
  - id: FR5
    statement: >
      Column types: `INT` (i64), `BIGINT` (i64, alias of INT), `REAL`
      (f64), `TEXT` (UTF-8 variable-length), `BOOL`, and the literal
      `NULL`. Values are tri-valued (NULL propagates through comparison
      and arithmetic according to SQL's three-valued logic).
  - id: FR6
    statement: >
      `CREATE TABLE` must accept column-level constraints
      `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, and `DEFAULT <literal>`, and
      the table-level constraint `CHECK (<expr>)`. Violations raise a
      SQL error and roll back the enclosing statement.
  - id: FR7
    statement: >
      `SELECT` must support projection (including `*` and qualified
      `table.col`), `FROM` of a single table or an INNER JOIN of two
      tables, `WHERE` with boolean operators `AND`/`OR`/`NOT` and
      comparisons `=`/`<>`/`<`/`<=`/`>`/`>=`/`IS NULL`/`IS NOT NULL`,
      `ORDER BY` with `ASC`/`DESC`, and `LIMIT N [OFFSET M]`.
  - id: FR8
    statement: >
      Expressions must support integer and real arithmetic
      (`+ - * /`), string concatenation (`||`), and the builtin scalar
      functions `LENGTH`, `LOWER`, `UPPER`, `SUBSTR`, `ABS`, `COALESCE`.
  - id: FR9
    statement: >
      `INSERT` must accept both `INSERT INTO t VALUES (...)` (positional
      full row) and `INSERT INTO t (c1, c2) VALUES (...)` (named
      subset, with defaults or NULL for omitted columns).
  - id: FR10
    statement: >
      `UPDATE t SET c = expr [, ...] [WHERE expr]` updates matching
      rows and returns the number of affected rows in the result
      summary.
  - id: FR11
    statement: >
      `DELETE FROM t [WHERE expr]` deletes matching rows and returns
      the number of affected rows.
  - id: FR12
    statement: >
      `CREATE INDEX idx_name ON t (col)` creates a B-tree secondary
      index that speeds up equality and range predicates on the
      indexed column. The executor must be able to choose an index
      scan when exactly one equality/range predicate matches.
  - id: FR13
    statement: >
      Transactions: `BEGIN` opens an implicit transaction; `COMMIT`
      flushes the WAL and makes changes visible; `ROLLBACK` discards
      them. Statements executed outside an explicit `BEGIN` are
      auto-committed as their own transaction.
  - id: FR14
    statement: >
      Crash recovery: on startup, `minidb` replays the WAL to restore
      any committed-but-unflushed transactions and discards any
      uncommitted prefix.
  - id: FR15
    statement: >
      Output format: by default, `SELECT` prints a human-readable table
      on stdout. The flag `--format {table,csv,json}` (or the
      session-level meta-command `.format <kind>`) selects the output
      style. CSV follows RFC 4180 quoting; JSON emits one JSON array of
      row-objects per result set.
  - id: FR16
    statement: >
      The CLI must print a summary line after each successful statement
      (e.g. `ok 3 rows` / `ok 1 row affected`) on stderr so that
      scripts can distinguish the data stream from progress output.
  - id: FR17
    statement: >
      The binary must support `--version`, `--help`, `--init` (create a
      fresh empty database file and exit), and the global flag
      `--quiet` (suppresses the per-statement summary line).

structural_requirements:
  - id: SR1
    statement: >
      The source tree must be organised into the following top-level
      modules, each in its own file or sub-directory: `lexer`,
      `parser`, `plan` (logical plan + physical plan), `exec`
      (physical operators), `storage` (page manager, WAL, heap,
      btree), `txn` (transaction manager), `catalog` (schema and
      index metadata), `types` (values, columns, tri-valued logic),
      `cli`, and an `error` module with a single crate-wide error
      enum.
  - id: SR2
    statement: >
      SQL parsing must be separated from execution. The lexer produces
      a `Token` stream; the parser consumes tokens and produces an AST
      defined in `parser::ast`. Neither layer may reach into
      `storage`.
  - id: SR3
    statement: >
      The executor must be structured around a Volcano-style iterator
      abstraction: each physical operator implements a
      `PhysicalOperator` trait with `open`, `next`, `close`, and a
      `schema()` accessor. Operators compose as a tree; `next` returns
      one row at a time (or a small batch).
  - id: SR4
    statement: >
      At least the following physical operators must exist as distinct
      types: `SeqScan`, `IndexScan`, `Filter`, `Project`, `NestedLoopJoin`,
      `Sort`, `Limit`, `Insert`, `Update`, `Delete`.
  - id: SR5
    statement: >
      Storage is page-based. Page size is 4096 bytes and is defined as
      a single compile-time constant in `storage`. Pages are identified
      by a typed `PageId(u64)`.
  - id: SR6
    statement: >
      The WAL must be append-only and record-oriented (one record per
      logical operation), with a CRC32 per record for detection of
      torn writes. WAL format and replay code share one module
      (`storage::wal`).
  - id: SR7
    statement: >
      Secondary indexes live in `storage::btree` and are keyed by
      `(index_key, row_id)`. The logical planner emits an `IndexScan`
      when the `WHERE` clause contains exactly one
      equality/range predicate on an indexed column.
  - id: SR8
    statement: >
      The transaction manager must expose `begin`, `commit`, and
      `rollback` operations. Concurrent transactions are NOT a
      requirement; a simple single-writer model is acceptable, but the
      transaction boundary (WAL framing, commit point) must be
      explicit and tested.
  - id: SR9
    statement: >
      Errors are returned through a single crate-wide `MiniDbError`
      enum defined in the `error` module. Lower-level errors (IO,
      parse, type-check, runtime) are variants of it. `unwrap`/`expect`
      must not appear in non-test code outside of obviously-infallible
      paths.
  - id: SR10
    statement: >
      Each module must carry unit tests under `#[cfg(test)]`. The
      crate must additionally carry integration tests in `tests/` that
      exercise end-to-end SQL via the binary.
  - id: SR11
    statement: >
      The project must include a `README.md` with a build/install
      section, a SQL-subset reference, example sessions, and a short
      architectural overview.
  - id: SR12
    statement: >
      Logging must go through the `log` crate (or an equivalent
      trait-based facade). `println!`/`eprintln!` is allowed only at
      the top-level CLI boundary (program output and error banners).

edit_requirements: []

constraints:
  language: Rust
  frontend_allowed: false
  terminal_allowed: true
  network_allowed: true
  external_crates_allowed: true
  target_os: linux
  rust_toolchain: stable
  external_hosted_services_allowed: false
  build_time_budget_s: 5400

deliverables:
  - cargo_project
  - readme

validation:
  notes:
    - "cargo build --release must succeed at validation time."
    - "Scale target: several thousand lines of Rust across the listed modules."

intentions:
  functional:
    - id: I-FR1
      source: FR1
      kind: functional
      statement: The binary ships with an interactive REPL for SQL.
    - id: I-FR2
      source: FR2
      kind: functional
      statement: >
        The binary supports single-statement batch evaluation via an
        --eval flag with distinct exit codes for usage, SQL, and IO
        errors.
    - id: I-FR4
      source: FR4
      kind: functional
      statement: >
        The SQL surface includes DDL, DML, index management, and
        transaction control statements.
    - id: I-FR12
      source: FR12
      kind: functional
      statement: >
        Secondary B-tree indexes exist and can be chosen by the
        executor for equality or range predicates.
    - id: I-FR13
      source: FR13
      kind: functional
      statement: >
        Explicit transactions are supported via BEGIN/COMMIT/ROLLBACK
        and statements outside a BEGIN are auto-committed.
    - id: I-FR14
      source: FR14
      kind: functional
      statement: >
        Crash recovery replays the WAL on startup to restore committed
        transactions and discards uncommitted prefixes.
    - id: I-FR15
      source: FR15
      kind: functional
      statement: >
        SELECT output can be formatted as a human-readable table, CSV
        (RFC 4180), or JSON arrays of row-objects.
    - id: I-FR16
      source: FR16
      kind: functional
      statement: >
        Per-statement summary lines go to stderr, keeping stdout as a
        clean data stream.
  structural:
    - id: I-SR2
      source: SR2
      kind: structural
      statement: >
        SQL parsing is decoupled from execution; the parser's AST is a
        distinct data structure that does not touch the storage layer.
    - id: I-SR3
      source: SR3
      kind: structural
      statement: >
        Execution uses a Volcano-style iterator model with
        open/next/close on a PhysicalOperator trait.
    - id: I-SR4
      source: SR4
      kind: structural
      statement: >
        Physical operators include at minimum SeqScan, IndexScan,
        Filter, Project, NestedLoopJoin, Sort, Limit, Insert, Update,
        Delete.
    - id: I-SR5
      source: SR5
      kind: structural
      statement: >
        Storage is organised around 4096-byte pages with typed PageIds.
    - id: I-SR6
      source: SR6
      kind: structural
      statement: >
        The WAL is an append-only record-oriented log with per-record
        CRC32 for torn-write detection.
    - id: I-SR7
      source: SR7
      kind: structural
      statement: >
        Secondary indexes are B-trees keyed by (index_key, row_id) and
        live in a dedicated storage sub-module.
    - id: I-SR9
      source: SR9
      kind: structural
      statement: >
        Errors flow through a single crate-wide error enum rather than
        a mix of panics and unrelated error types.
    - id: I-SR12
      source: SR12
      kind: structural
      statement: >
        Diagnostic logging goes through the `log` crate facade. Direct
        `println!`/`eprintln!` is restricted to the CLI top-level
        boundary; library modules do not write to stdout/stderr
        themselves.

tags: [rust, database, sql, wal, btree, transactional]
notes: >
  The benchmark thesis is recoverability of intent. The Volcano
  iterator choice (SR3) and the single crate-wide error enum (SR9) are
  particularly load-bearing — they are easy to accidentally blur.
