#!/usr/bin/env python3
"""Summarize the builder-written test suites run on the released build artifacts.

``reports/runtime_tests/artifact_tests.csv`` holds one row per released build
artifact, written by ``scripts/run_artifact_tests.py``: ``cargo build --release``,
then ``cargo test``. This script checks the rows against ``tables/build_runs.csv``
(the same build runs, tasks, and release-mode compile-probe outcomes) and writes
build, suite, and test counts overall, per builder, and per task. The suites are
written by the builders, so they indicate self-consistency of each artifact, not
conformance to the task specification.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from bnf.config import Paths, paths


ROOT = Path(__file__).resolve().parents[1]
ANALYSIS_ID = "runtime_tests_v1"
OUTPUT_PARTS = ("runtime_tests",)
INPUT_NAME = "artifact_tests.csv"
OUTPUT_FILES = ("summary.json", "report.md")
FIELDS = (
    "build_run_id",
    "task_id",
    "cargo_present",
    "build_rc",
    "build_ok",
    "build_seconds",
    "test_rc",
    "test_ok",
    "test_seconds",
    "test_binaries",
    "tests_passed",
    "tests_failed",
    "tests_ignored",
    "note",
)
BUILD_RUN_PREFIX = "build-"
NO_ROOT_CARGO_TOML = "no_root_cargo_toml"
RELEASE_BUILD_FAILED = "release_build_failed"
NOT_COMPILED_REASONS = (NO_ROOT_CARGO_TOML, RELEASE_BUILD_FAILED)
FAILED_TEST_RE = re.compile(r"\btest (?P<name>\S+) \.\.\. FAILED\b")
CSV_BOOLS = {"True": True, "False": False}
TABLE_BOOLS = {"true": True, "false": False}


@dataclass(frozen=True)
class ArtifactTest:
    build_run_id: str
    task_id: str
    builder_id: str
    build_trial_id: str
    cargo_present: bool
    build_ok: bool
    test_ok: bool
    tests_passed: int
    tests_failed: int
    tests_ignored: int
    note: str

    @property
    def tests_total(self) -> int:
        return self.tests_passed + self.tests_failed

    @property
    def not_compiled_reason(self) -> str | None:
        if self.build_ok:
            return None
        return RELEASE_BUILD_FAILED if self.cargo_present else NO_ROOT_CARGO_TOML

    @property
    def failing_tests(self) -> tuple[str, ...]:
        return tuple(match.group("name") for match in FAILED_TEST_RE.finditer(self.note))


@dataclass(frozen=True)
class Counts:
    builds: int
    compiled: int
    suites_passed: int
    suites_without_tests: int
    tests_passed: int
    tests_failed: int
    tests_ignored: int

    @classmethod
    def of(cls, rows: Iterable[ArtifactTest]) -> Counts:
        rows = list(rows)
        compiled = [row for row in rows if row.build_ok]
        return cls(
            builds=len(rows),
            compiled=len(compiled),
            suites_passed=sum(row.test_ok for row in compiled),
            suites_without_tests=sum(row.tests_total == 0 for row in compiled),
            tests_passed=sum(row.tests_passed for row in compiled),
            tests_failed=sum(row.tests_failed for row in compiled),
            tests_ignored=sum(row.tests_ignored for row in compiled),
        )

    @property
    def suites_failed(self) -> int:
        return self.compiled - self.suites_passed

    @property
    def tests_total(self) -> int:
        return self.tests_passed + self.tests_failed


@dataclass(frozen=True)
class Analysis:
    inputs: dict[str, Path]
    rows: list[ArtifactTest]

    @property
    def overall(self) -> Counts:
        return Counts.of(self.rows)

    def by(self, key: str) -> dict[str, Counts]:
        values = sorted({getattr(row, key) for row in self.rows})
        return {
            value: Counts.of(row for row in self.rows if getattr(row, key) == value)
            for value in values
        }

    @property
    def failed_suites(self) -> list[ArtifactTest]:
        return [row for row in self.rows if row.build_ok and not row.test_ok]

    def not_compiled(self) -> dict[str, int]:
        return {
            reason: sum(row.not_compiled_reason == reason for row in self.rows)
            for reason in NOT_COMPILED_REASONS
        }


def main() -> None:
    args = _parse_args()
    p = paths()
    out_dir = (args.output_dir or p.reports_dir.joinpath(*OUTPUT_PARTS)).resolve()
    input_path = (args.input or p.reports_dir.joinpath(*OUTPUT_PARTS, INPUT_NAME)).resolve()
    analysis = analyze(p, input_path=input_path)
    write_outputs(analysis, out_dir)
    print(f"wrote {_display_path(out_dir)}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--input",
        type=Path,
        default=None,
        help="Per-artifact test results (default: reports/runtime_tests/artifact_tests.csv).",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Output directory (default: reports/runtime_tests).",
    )
    return parser.parse_args()


# --------------------------------------------------------------------------- inputs


def analysis_inputs(p: Paths, *, input_path: Path) -> dict[str, Path]:
    return {
        "artifact_tests": input_path,
        "build_runs": p.tables_dir / "build_runs.csv",
    }


def analyze(p: Paths, *, input_path: Path) -> Analysis:
    inputs = analysis_inputs(p, input_path=input_path)
    missing = [_display_path(path) for path in inputs.values() if not path.exists()]
    if missing:
        raise FileNotFoundError(f"missing inputs: {', '.join(missing)}")
    rows = parse_rows(
        _read_csv(inputs["artifact_tests"], expected_fields=FIELDS),
        _read_csv(inputs["build_runs"]),
    )
    return Analysis(inputs=inputs, rows=rows)


def parse_rows(
    test_rows: list[dict[str, str]], build_rows: list[dict[str, str]]
) -> list[ArtifactTest]:
    """Validate the per-artifact rows against the released build runs and parse them."""
    builds = {
        row["build_run_id"]: row
        for row in build_rows
        if row["build_run_id"].startswith(BUILD_RUN_PREFIX)
    }
    seen: set[str] = set()
    for row in test_rows:
        if row["build_run_id"] in seen:
            raise ValueError(f"{row['build_run_id']}: listed twice")
        seen.add(row["build_run_id"])
    missing = sorted(set(builds) - seen)
    extra = sorted(seen - set(builds))
    if missing or extra:
        raise ValueError(
            "artifact tests do not cover the released build runs: "
            f"missing {missing[:3]}, extra {extra[:3]}"
        )
    parsed = [_parse_row(row, builds[row["build_run_id"]]) for row in test_rows]
    return sorted(parsed, key=lambda row: row.build_run_id)


def _parse_row(row: dict[str, str], build: dict[str, str]) -> ArtifactTest:
    build_run_id = row["build_run_id"]
    if row["task_id"] != build["task_id"]:
        raise ValueError(f"{build_run_id}: task {row['task_id']!r} != {build['task_id']!r}")
    parsed = ArtifactTest(
        build_run_id=build_run_id,
        task_id=row["task_id"],
        builder_id=build["builder_id"],
        build_trial_id=build["build_trial_id"],
        cargo_present=_bool(row, "cargo_present", CSV_BOOLS),
        build_ok=_bool(row, "build_ok", CSV_BOOLS),
        test_ok=_bool(row, "test_ok", CSV_BOOLS),
        tests_passed=_count(row, "tests_passed"),
        tests_failed=_count(row, "tests_failed"),
        tests_ignored=_count(row, "tests_ignored"),
        note=row["note"],
    )
    if parsed.build_ok != _bool(build, "compile_pass", TABLE_BOOLS):
        raise ValueError(f"{build_run_id}: build_ok does not match compile_pass in build_runs.csv")
    if parsed.build_ok and not parsed.cargo_present:
        raise ValueError(f"{build_run_id}: compiled without a root Cargo.toml")
    if not parsed.build_ok and (
        parsed.test_ok or parsed.tests_passed or parsed.tests_failed or parsed.tests_ignored
    ):
        raise ValueError(f"{build_run_id}: test results recorded for a build that did not compile")
    if parsed.test_ok and parsed.tests_failed:
        raise ValueError(f"{build_run_id}: suite passed with failing tests")
    if parsed.tests_failed and not parsed.failing_tests:
        raise ValueError(f"{build_run_id}: failing tests are not named in the note")
    return parsed


def _bool(row: dict[str, str], key: str, values: dict[str, bool]) -> bool:
    try:
        return values[row[key]]
    except KeyError:
        raise ValueError(f"{row['build_run_id']}: {key} is {row[key]!r}") from None


def _count(row: dict[str, str], key: str) -> int:
    value = int(row[key])
    if value < 0:
        raise ValueError(f"{row['build_run_id']}: negative {key}")
    return value


def _read_csv(path: Path, *, expected_fields: tuple[str, ...] | None = None) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if expected_fields is not None and tuple(reader.fieldnames or ()) != expected_fields:
            raise ValueError(f"{path.name}: expected columns {', '.join(expected_fields)}")
        return list(reader)


# --------------------------------------------------------------------------- outputs


def write_outputs(analysis: Analysis, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.json").write_text(
        json.dumps(_summary(analysis), indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (out_dir / "report.md").write_text(_report(analysis), encoding="utf-8")


def _count_fields(counts: Counts) -> dict[str, int]:
    return {
        "builds": counts.builds,
        "compiled": counts.compiled,
        "suites_passed": counts.suites_passed,
        "suites_failed": counts.suites_failed,
        "tests_passed": counts.tests_passed,
        "tests_failed": counts.tests_failed,
        "tests_total": counts.tests_total,
    }


def _failed_suite_fields(row: ArtifactTest) -> dict[str, Any]:
    return {
        "build_run_id": row.build_run_id,
        "builder_id": row.builder_id,
        "task_id": row.task_id,
        "build_trial_id": row.build_trial_id,
        "failing_tests": list(row.failing_tests),
        "tests_passed": row.tests_passed,
        "tests_failed": row.tests_failed,
        "tests_total": row.tests_total,
    }


def _summary(analysis: Analysis) -> dict[str, Any]:
    overall = analysis.overall
    return {
        "analysis_id": ANALYSIS_ID,
        "analysis_script": "scripts/summarize_runtime_tests.py",
        "inputs": {name: _display_path(path) for name, path in analysis.inputs.items()},
        "input_hashes": {
            _display_path(path): _sha256(path) for path in analysis.inputs.values()
        },
        "procedure": (
            "cargo build --release, then cargo test, in each released build artifact; "
            "artifacts without a root Cargo.toml are not built"
        ),
        "suite_origin": (
            "builder-written test suites; they indicate self-consistency, not specification "
            "conformance"
        ),
        "builds": {
            "total": overall.builds,
            "compiled": overall.compiled,
            "not_compiled": analysis.not_compiled(),
        },
        "suites": {
            "run": overall.compiled,
            "passed": overall.suites_passed,
            "failed": overall.suites_failed,
            "without_tests": overall.suites_without_tests,
        },
        "tests": {
            "passed": overall.tests_passed,
            "failed": overall.tests_failed,
            "total": overall.tests_total,
            "ignored": overall.tests_ignored,
        },
        "failed_suites": [_failed_suite_fields(row) for row in analysis.failed_suites],
        "builders": [
            {"builder_id": builder_id, **_count_fields(counts)}
            for builder_id, counts in analysis.by("builder_id").items()
        ],
        "tasks": [
            {"task_id": task_id, **_count_fields(counts)}
            for task_id, counts in analysis.by("task_id").items()
        ],
        "outputs": list(OUTPUT_FILES),
    }


def _report(analysis: Analysis) -> str:
    overall = analysis.overall
    not_compiled = analysis.not_compiled()
    lines = [
        "# Builder-written test suites on the released build artifacts",
        "",
        "Generated by `scripts/summarize_runtime_tests.py`; do not edit by hand.",
        "",
        f"`{INPUT_NAME}` records `cargo build --release` followed by `cargo test` in each of "
        f"the {overall.builds} released build artifacts (see `README.md`). The test suites "
        "are written by the builders; they indicate self-consistency, not specification "
        "conformance. Test counts cover the compile-pass builds; ignored tests are counted "
        "separately.",
        "",
        "## Overall",
        "",
        "| quantity | value |",
        "| --- | ---: |",
        f"| released build artifacts | {overall.builds} |",
        f"| compiled (`cargo build --release` succeeded) | {overall.compiled} |",
        f"| not compiled: no root `Cargo.toml` | {not_compiled[NO_ROOT_CARGO_TOML]} |",
        f"| not compiled: release build failed | {not_compiled[RELEASE_BUILD_FAILED]} |",
        "| compile-pass builds whose full suite passed | "
        f"{overall.suites_passed} of {overall.compiled} |",
        f"| compile-pass builds without tests | {overall.suites_without_tests} |",
        f"| tests passed | {overall.tests_passed:,} of {overall.tests_total:,} |",
        f"| tests ignored | {overall.tests_ignored} |",
        "",
        "## Failing suites",
        "",
    ]
    if analysis.failed_suites:
        lines.extend(
            [
                "| build run | builder | task | trial | failing tests | tests passed |",
                "| --- | --- | --- | ---: | --- | ---: |",
            ]
        )
        for row in analysis.failed_suites:
            names = ", ".join(f"`{name}`" for name in row.failing_tests) or "(not named)"
            lines.append(
                f"| `{row.build_run_id}` | `{row.builder_id}` | `{row.task_id}` | "
                f"{row.build_trial_id} | {names} | {row.tests_passed} of {row.tests_total} |"
            )
    else:
        lines.append("Every compile-pass build passed its full suite.")
    for title, key, header in (
        ("Per builder", "builder_id", "builder"),
        ("Per task", "task_id", "task"),
    ):
        lines.extend(
            [
                "",
                f"## {title}",
                "",
                f"| {header} | builds | compile-pass | suites passed | tests passed / total |",
                "| --- | ---: | ---: | ---: | ---: |",
            ]
        )
        for value, counts in analysis.by(key).items():
            lines.append(
                f"| `{value}` | {counts.builds} | {counts.compiled} | {counts.suites_passed} | "
                f"{counts.tests_passed:,} / {counts.tests_total:,} |"
            )
    lines.append("")
    return "\n".join(lines)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


if __name__ == "__main__":
    main()
