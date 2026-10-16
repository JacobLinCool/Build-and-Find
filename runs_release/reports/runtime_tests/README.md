# Runtime tests of the released build artifacts

`artifact_tests.csv` records one run of each released build artifact's own test
suite. `scripts/run_artifact_tests.py` ran `cargo build --release` and then
`cargo test` in each of the 48 released build artifacts under
`runs_release/artifacts/` (24 per task) on 2026-07-24. The release build runs
first because several integration tests locate the binary under
`target/release/`.

The test suites are builder-written: the builder prompt asks for tests that pass
with `cargo test` (`src/bnf/prompts/builder.py`), and each builder wrote its own.
They indicate self-consistency, not specification conformance.

## Results

| quantity | value |
| --- | ---: |
| released build artifacts | 48 |
| compiled (`cargo build --release` succeeded) | 41 |
| compile-pass builds whose full suite passed | 40 of 41 |
| tests passed | 2,060 of 2,061 |

The seven builds that did not compile have no root `Cargo.toml`; they are the
seven builds that the release-mode compile probe excludes (`compile_pass` is
false in `runs_release/tables/build_runs.csv`). The one failing test is
`cors_sets_origin` in `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480`
(25 of 26 tests pass). `report.md` gives the per-builder and per-task counts.

## Columns of `artifact_tests.csv`

- `build_run_id`, `task_id`: the build run as recorded in
  `runs_release/tables/build_runs.csv`.
- `cargo_present`: whether the artifact has a root `Cargo.toml`. Artifacts
  without one are not built; their build and test columns are empty or zero.
- `build_rc`, `build_ok`, `build_seconds`: exit code, success, and wall-clock
  seconds of `cargo build --release`.
- `test_rc`, `test_ok`, `test_seconds`: the same for `cargo test`. `test_ok` is
  true when every test binary passed.
- `test_binaries`: the number of `test result:` lines in the `cargo test` output
  (unit-test, integration-test, and doc-test binaries).
- `tests_passed`, `tests_failed`, `tests_ignored`: sums over those lines.
- `note`: why an artifact was not built, or the failing tests (up to five) named
  in the `cargo test` output.

Booleans are written as `True` and `False`.

## Run conditions

Each command had a 1200-second timeout. Builds of the same task shared one
`CARGO_TARGET_DIR` so that dependencies compiled once per task. Every
compile-pass artifact ships a `Cargo.lock` that pins its external dependencies,
all of them crates.io packages. Eight compile-pass artifacts use Rust edition 2024,
which needs Rust 1.85 or later. The CSV does not record the host or the Rust
toolchain version.

## Rerunning the tests

```bash
uv run python scripts/run_artifact_tests.py
```

The script needs `cargo` and network access to crates.io (or a populated cargo
registry cache). It writes this directory's `artifact_tests.csv` by default
(`--out` changes the path) and rewrites it after every artifact. Other options:
`--artifacts-root` (default `runs_release/artifacts`), `--target-root` (shared
target directory, default `/tmp/bnf-artifact-tests-target`, with one
subdirectory per task), and `--only <substring>` (only artifacts whose build run
ID contains the substring). The timing columns differ from run to run. The
script now passes `--locked` to both cargo commands, so a rerun fails instead of
rewriting a released `Cargo.lock`; the recorded run used the commands without the
flag and left every released lock file unchanged.

## Regenerating the summary

```bash
BNF_RUNS_ROOT=runs_release uv run python scripts/summarize_runtime_tests.py
```

`make paper-analysis` runs the same command. The script checks
`artifact_tests.csv` against `runs_release/tables/build_runs.csv` (the same 48
build runs and tasks, and `build_ok` equal to `compile_pass`) and writes:

- `summary.json`: inputs, input hashes, build, suite, and test counts, the
  failing suites with their failing tests, and per-builder and per-task counts.
- `report.md`: the same values as tables.
