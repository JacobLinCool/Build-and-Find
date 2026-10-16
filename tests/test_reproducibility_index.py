from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REPORTS = ROOT / "runs_release" / "reports"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


index = load_script_module("write_reproducibility_index", "scripts/write_reproducibility_index.py")


def test_option_free_claim_reads_summary_counts(tmp_path: Path) -> None:
    summary = {
        "option_free": {
            "all": {
                "artifact_open": {"correct": 3, "total": 4},
                "question_only_open": {"correct": 1, "total": 4},
                "difference_pp": "+50.0",
            }
        },
        "multiple_choice": {
            "artifact_present": {"correct": 4, "total": 4},
            "question_only": {"correct": 2, "total": 4},
        },
    }
    path = tmp_path / "summary.json"
    path.write_text(json.dumps(summary), encoding="utf-8")
    text = index._option_free_claim(path)
    assert "correct in 3/4 responses with artifact access and 1/4" in text
    assert "(+50.0 pp)" in text
    assert "correct in 4/4 and 2/4 answer rows" in text
    assert index._option_free_claim(tmp_path / "missing.json").endswith("n/a")


def test_runtime_tests_claim_reads_summary_counts(tmp_path: Path) -> None:
    path = tmp_path / "summary.json"
    path.write_text(
        json.dumps({"suites": {"passed": 2, "run": 3}, "tests": {"passed": 9, "total": 10}}),
        encoding="utf-8",
    )
    assert "in full in 2 of 3 compile-pass builds, and 9 of 10 tests" in (
        index._runtime_tests_claim(path)
    )
    assert index._runtime_tests_claim(tmp_path / "missing.json").endswith("n/a")


def test_reproducibility_index_lists_option_free_runtime_and_per_task_sources() -> None:
    text = (REPORTS / "reproducibility_index.md").read_text(encoding="utf-8")
    assert text.startswith("# Reproducibility Index\n")
    for heading in ("### Per-task R_b", "### Option-free check", "### Builder-written test suites"):
        assert heading in text
    assert "38/48 responses with artifact access and 13/48" in text
    assert "40 of 41 compile-pass builds, and 2060 of 2061 tests" in text
