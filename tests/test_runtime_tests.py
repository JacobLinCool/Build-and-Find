from __future__ import annotations

import csv
import importlib.util
import json
import re
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / "runs_release"
RELEASE_OUTPUT_DIR = RELEASE / "reports" / "runtime_tests"
FAILING_BUILD = "build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


rt = load_script_module("summarize_runtime_tests", "scripts/summarize_runtime_tests.py")
runner = load_script_module("run_artifact_tests", "scripts/run_artifact_tests.py")


def build_row(build_run_id: str, **overrides: str) -> dict[str, str]:
    row = {
        "build_run_id": build_run_id,
        "task_id": "scratch_minidb",
        "builder_id": "codex_gpt5_5_high",
        "build_trial_id": "0",
        "compile_pass": "true",
    }
    row.update(overrides)
    return row


def result_row(build_run_id: str, **overrides: str) -> dict[str, str]:
    row = {
        "build_run_id": build_run_id,
        "task_id": "scratch_minidb",
        "cargo_present": "True",
        "build_rc": "0",
        "build_ok": "True",
        "build_seconds": "1.0",
        "test_rc": "0",
        "test_ok": "True",
        "test_seconds": "1.0",
        "test_binaries": "2",
        "tests_passed": "10",
        "tests_failed": "0",
        "tests_ignored": "0",
        "note": "",
    }
    row.update(overrides)
    return row


NOT_BUILT = {
    "cargo_present": "False",
    "build_rc": "",
    "build_ok": "False",
    "build_seconds": "",
    "test_rc": "",
    "test_ok": "False",
    "test_seconds": "",
    "test_binaries": "0",
    "tests_passed": "0",
    "note": "no root Cargo.toml (excluded by release-mode compile probe)",
}
FAILED_SUITE = {
    "test_rc": "101",
    "test_ok": "False",
    "tests_passed": "7",
    "tests_failed": "2",
    "note": "test alpha ... FAILED; test beta::gamma ... FAILED",
}


def synthetic_rows() -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    builds = [
        build_row("build-a"),
        build_row("build-b", builder_id="codex_gpt5_5_low", task_id="scratch_nanoweb"),
        build_row("build-c", builder_id="codex_gpt5_5_low", compile_pass="false"),
        build_row("control-build-spec_only-scratch_minidb-x", compile_pass="false"),
    ]
    tests = [
        result_row("build-c", **NOT_BUILT),
        result_row("build-a", tests_ignored="1"),
        result_row("build-b", task_id="scratch_nanoweb", **FAILED_SUITE),
    ]
    return tests, builds


def test_script_fields_and_default_path_match_the_runner() -> None:
    assert rt.FIELDS == runner.FIELDNAMES
    assert runner.DEFAULT_OUT == "/".join(
        ("runs_release", "reports", *rt.OUTPUT_PARTS, rt.INPUT_NAME)
    )


def test_parse_rows_counts_builds_suites_and_tests() -> None:
    tests, builds = synthetic_rows()
    analysis = rt.Analysis(inputs={}, rows=rt.parse_rows(tests, builds))

    assert [row.build_run_id for row in analysis.rows] == ["build-a", "build-b", "build-c"]
    assert analysis.overall == rt.Counts(
        builds=3,
        compiled=2,
        suites_passed=1,
        suites_without_tests=0,
        tests_passed=17,
        tests_failed=2,
        tests_ignored=1,
    )
    assert analysis.overall.tests_total == 19
    assert analysis.overall.suites_failed == 1
    assert analysis.not_compiled() == {"no_root_cargo_toml": 1, "release_build_failed": 0}
    assert [row.build_run_id for row in analysis.failed_suites] == ["build-b"]
    assert analysis.failed_suites[0].failing_tests == ("alpha", "beta::gamma")

    by_builder = analysis.by("builder_id")
    assert list(by_builder) == ["codex_gpt5_5_high", "codex_gpt5_5_low"]
    assert by_builder["codex_gpt5_5_low"].builds == 2
    assert by_builder["codex_gpt5_5_low"].compiled == 1
    assert by_builder["codex_gpt5_5_low"].suites_passed == 0
    high = by_builder["codex_gpt5_5_high"]
    assert (high.tests_passed, high.tests_total) == (10, 10)


def test_release_build_failure_is_its_own_reason() -> None:
    not_tested = {
        key: NOT_BUILT[key] for key in ("test_rc", "test_ok", "test_seconds", "tests_passed")
    }
    tests = [result_row("build-a", build_rc="101", build_ok="False", **not_tested)]
    rows = rt.parse_rows(tests, [build_row("build-a", compile_pass="false")])
    assert rows[0].not_compiled_reason == rt.RELEASE_BUILD_FAILED


def test_suite_without_tests_is_counted() -> None:
    tests = [result_row("build-a", test_binaries="0", tests_passed="0")]
    analysis = rt.Analysis(inputs={}, rows=rt.parse_rows(tests, [build_row("build-a")]))
    assert analysis.overall.suites_without_tests == 1


@pytest.mark.parametrize(
    ("tests", "builds", "message"),
    [
        ([result_row("build-a"), result_row("build-a")], [build_row("build-a")], "listed twice"),
        ([result_row("build-a")], [build_row("build-a"), build_row("build-b")], "missing"),
        ([result_row("build-a"), result_row("build-b")], [build_row("build-a")], "extra"),
        (
            [result_row("build-a", task_id="scratch_nanoweb")],
            [build_row("build-a")],
            "task",
        ),
        ([result_row("build-a")], [build_row("build-a", compile_pass="false")], "compile_pass"),
        ([result_row("build-a", build_ok="true")], [build_row("build-a")], "build_ok is"),
        ([result_row("build-a", cargo_present="False")], [build_row("build-a")], "Cargo.toml"),
        (
            [result_row("build-a", **{**NOT_BUILT, "tests_passed": "3"})],
            [build_row("build-a", compile_pass="false")],
            "did not compile",
        ),
        ([result_row("build-a", tests_failed="1")], [build_row("build-a")], "suite passed"),
        (
            [result_row("build-a", **{**FAILED_SUITE, "note": "test run failed rc=101"})],
            [build_row("build-a")],
            "not named",
        ),
        ([result_row("build-a", tests_passed="-1")], [build_row("build-a")], "negative"),
    ],
)
def test_parse_rows_rejects_inconsistent_rows(
    tests: list[dict[str, str]], builds: list[dict[str, str]], message: str
) -> None:
    with pytest.raises(ValueError, match=message):
        rt.parse_rows(tests, builds)


def test_read_csv_requires_the_runner_columns(tmp_path: Path) -> None:
    path = tmp_path / rt.INPUT_NAME
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rt.FIELDS[:-1]), lineterminator="\n")
        writer.writeheader()
    with pytest.raises(ValueError, match="expected columns"):
        rt._read_csv(path, expected_fields=rt.FIELDS)


def test_report_states_when_every_suite_passes() -> None:
    rows = rt.parse_rows([result_row("build-a")], [build_row("build-a")])
    report = rt._report(rt.Analysis(inputs={}, rows=rows))
    assert "Every compile-pass build passed its full suite." in report
    assert "| compile-pass builds whose full suite passed | 1 of 1 |" in report


# --------------------------------------------------------------------------- release


@pytest.fixture(scope="module")
def analysis():
    return rt.analyze(
        rt.Paths(root=ROOT, runs_root=RELEASE),
        input_path=RELEASE_OUTPUT_DIR / rt.INPUT_NAME,
    )


def test_release_counts(analysis) -> None:
    overall = analysis.overall
    assert (overall.builds, overall.compiled) == (48, 41)
    assert analysis.not_compiled() == {"no_root_cargo_toml": 7, "release_build_failed": 0}
    assert (overall.suites_passed, overall.suites_failed, overall.suites_without_tests) == (
        40,
        1,
        0,
    )
    assert (overall.tests_passed, overall.tests_failed, overall.tests_total) == (2060, 1, 2061)
    assert overall.tests_ignored == 0
    [failed] = analysis.failed_suites
    assert failed.build_run_id == FAILING_BUILD
    assert failed.failing_tests == ("cors_sets_origin",)
    assert (failed.tests_passed, failed.tests_total) == (25, 26)


def test_release_per_builder_counts(analysis) -> None:
    expected = {
        "claude_code_mimo_v2_5_high": (4, 4, 4, 285, 285),
        "claude_code_mimo_v2_5_low": (4, 3, 3, 154, 154),
        "claude_code_mimo_v2_5_pro_high": (4, 2, 2, 185, 185),
        "claude_code_mimo_v2_5_pro_low": (4, 1, 1, 101, 101),
        "claude_code_opus_4_7_high": (4, 4, 4, 263, 263),
        "claude_code_opus_4_7_low": (4, 4, 4, 162, 162),
        "claude_code_sonnet_4_6_high": (4, 4, 4, 357, 357),
        "claude_code_sonnet_4_6_low": (4, 4, 4, 251, 251),
        "codex_gpt5_4_mini_high": (4, 4, 4, 77, 77),
        "codex_gpt5_4_mini_low": (4, 3, 3, 41, 41),
        "codex_gpt5_5_high": (4, 4, 4, 107, 107),
        "codex_gpt5_5_low": (4, 4, 3, 77, 78),
    }
    actual = {
        builder_id: (c.builds, c.compiled, c.suites_passed, c.tests_passed, c.tests_total)
        for builder_id, c in analysis.by("builder_id").items()
    }
    assert actual == expected


def test_release_csv_is_lf_and_has_no_host_paths() -> None:
    data = (RELEASE_OUTPUT_DIR / rt.INPUT_NAME).read_bytes()
    assert b"\r" not in data
    text = data.decode("utf-8")
    assert not re.search(r"(?:^|[\s,(\"'])(?:/|~)", text)
    assert str(Path.home()) not in text


def test_committed_outputs_are_current(analysis, tmp_path: Path) -> None:
    rt.write_outputs(analysis, tmp_path)
    for name in rt.OUTPUT_FILES:
        generated = (tmp_path / name).read_text(encoding="utf-8")
        committed = (RELEASE_OUTPUT_DIR / name).read_text(encoding="utf-8")
        assert str(ROOT) not in generated
        if name == "summary.json":
            # Input hashes depend on the checkout's line endings; compare everything else.
            generated_json, committed_json = json.loads(generated), json.loads(committed)
            generated_json.pop("input_hashes")
            committed_json.pop("input_hashes")
            assert generated_json == committed_json
        else:
            assert generated == committed, name
