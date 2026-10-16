from __future__ import annotations

import csv
import importlib.util
import json
import re
import sys
from collections import Counter
from fractions import Fraction
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / "runs_release"
RELEASE_OUTPUT_DIR = RELEASE / "reports" / "eandd_final" / "transcript_audit"
OUTPUT_FILES = ("summary.json", "report.md")


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


te = load_script_module("analyze_transcript_exposure", "scripts/analyze_transcript_exposure.py")
impact = te.impact

PRIMARY = "all_records|paper_final_scratch"
BUILDERS = ("codex_gpt5_5_high", "claude_code_opus_4_7_high")
FINDERS = ("claude_code_sonnet_4_6_high", "codex_gpt5_4_mini_high")


def find_row(builder: str, finder: str, total_bytes: float, **overrides: str) -> dict[str, str]:
    row = {
        "find_run_id": f"find-{builder}-{finder}",
        "build_run_id": f"build-{builder}",
        "task_id": "scratch_minidb",
        "builder_id": builder,
        "builder_family": impact._agent_family(builder),
        "builder_effort": "high",
        "build_trial_id": "0",
        "compile_pass": "true",
        "finder_id": finder,
        "finder_family": impact._agent_family(finder),
        "finder_effort": "high",
        "finder_trial_id": "0",
        "accuracy": "1.0",
        "perfect_run": "true",
        "retrieval_bytes": str(total_bytes),
        "tool_call_bytes": str(total_bytes),
        "total_bytes": str(total_bytes),
        "tool_call_count": "1",
        "turn_count": "1",
        "wall_clock_seconds": "1",
        "panel_ids": PRIMARY,
    }
    row.update(overrides)
    return row


def answer_rows(find: dict[str, str], correct: tuple[bool, ...]) -> list[dict[str, str]]:
    return [
        {
            "question_row_id": f"{find['find_run_id']}:q{index}",
            "find_run_id": find["find_run_id"],
            "build_run_id": find["build_run_id"],
            "task_id": find["task_id"],
            "builder_id": find["builder_id"],
            "finder_id": find["finder_id"],
            "question_id": f"q{index}",
            "is_correct": "true" if ok else "false",
            "panel_ids": find["panel_ids"],
        }
        for index, ok in enumerate(correct, start=1)
    ]


def write_exposure(path: Path, rows: list[dict[str, str]], fields=te.EXPOSURE_FIELDS) -> Path:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(fields))
        writer.writeheader()
        writer.writerows(rows)
    return path


def exposure_row(**overrides: str) -> dict[str, str]:
    row = {
        "find_run_id": f"find-{BUILDERS[0]}-{FINDERS[0]}",
        "finder_id": FINDERS[0],
        "task_id": "scratch_minidb",
        "category": "git_status_listing",
        "note": "Ran git status in its workspace.",
    }
    row.update(overrides)
    return row


def test_rounding_is_half_up_on_exact_rationals() -> None:
    assert te.percent(Fraction(1, 8), 1) == "12.5"
    assert te.percent(Fraction(1, 800), 2) == "0.13"
    assert te.percent(Fraction(21413, 22140)) == "96.716"
    assert te.signed_points(Fraction(-1, 800), 2) == "-0.13"
    assert te.signed_points(Fraction(0), 3) == "+0.000"


def test_load_exposures_validates_columns_categories_and_duplicates(tmp_path: Path) -> None:
    good = write_exposure(tmp_path / "good.csv", [exposure_row()])
    assert te.load_exposures(good)[0].category == "git_status_listing"

    with pytest.raises(ValueError, match="expected columns"):
        te.load_exposures(write_exposure(tmp_path / "cols.csv", [], fields=te.EXPOSURE_FIELDS[:-1]))
    with pytest.raises(ValueError, match="unknown category"):
        te.load_exposures(write_exposure(tmp_path / "cat.csv", [exposure_row(category="other")]))
    with pytest.raises(ValueError, match="missing note"):
        te.load_exposures(write_exposure(tmp_path / "note.csv", [exposure_row(note=" ")]))
    with pytest.raises(ValueError, match="listed twice"):
        te.load_exposures(write_exposure(tmp_path / "dup.csv", [exposure_row()] * 2))


def test_validate_exposures_requires_matching_primary_panel_runs() -> None:
    finds = [
        find_row(BUILDERS[0], FINDERS[0], 100.0),
        find_row(BUILDERS[1], FINDERS[0], 100.0, panel_ids="all_records|control_all"),
        find_row(BUILDERS[0], FINDERS[1], 100.0, compile_pass="false"),
    ]
    te.validate_exposures([te.ExposedRun(**exposure_row())], finds)

    with pytest.raises(ValueError, match="not a primary-panel find run"):
        te.validate_exposures(
            [te.ExposedRun(**exposure_row(find_run_id=finds[1]["find_run_id"]))], finds
        )
    with pytest.raises(ValueError, match="did not compile"):
        te.validate_exposures(
            [
                te.ExposedRun(
                    **exposure_row(find_run_id=finds[2]["find_run_id"], finder_id=FINDERS[1])
                )
            ],
            finds,
        )
    with pytest.raises(ValueError, match="does not match"):
        te.validate_exposures([te.ExposedRun(**exposure_row(task_id="scratch_nanoweb"))], finds)


def test_primary_variant_drops_listed_runs_from_accuracy_and_rb() -> None:
    finds = [
        find_row(BUILDERS[0], FINDERS[0], 100.0),
        find_row(BUILDERS[1], FINDERS[0], 300.0),
        find_row(BUILDERS[0], FINDERS[1], 200.0),
        find_row(BUILDERS[1], FINDERS[1], 100.0),
    ]
    answers = [
        *answer_rows(finds[0], (True, False)),
        *answer_rows(finds[1], (True, True)),
        *answer_rows(finds[2], (True, True)),
        *answer_rows(finds[3], (True, True)),
    ]
    # The second question of the first builder's artifact is not implemented as specified.
    label_by_key = {
        impact._artifact_question_key(row): {
            "final_label": "absent_or_ambiguous"
            if (row["build_run_id"], row["question_id"]) == (f"build-{BUILDERS[0]}", "q2")
            else "implemented_gold"
        }
        for row in answers
    }

    def variant(excluded: set[str]):
        return te.primary_variant(
            "test",
            find_rows=finds,
            answer_rows=answers,
            label_by_key=label_by_key,
            excluded=excluded,
        )

    everything = variant(set())
    assert (everything.find_runs, everything.all_rows) == (4, te.Rate(7, 8))
    assert everything.implemented_gold == te.Rate(6, 6)
    high = {row.builder_id: row for row in everything.rb["high_effort"]}
    assert high[BUILDERS[0]].score == pytest.approx((1.0 * 2.0) ** 0.5)
    assert high[BUILDERS[1]].score == pytest.approx((3.0 * 1.0) ** 0.5)
    assert [row.builder_id for row in everything.rb["high_effort"]] == list(BUILDERS)
    assert everything.rb["low_effort"] == []

    # Dropping the first builder's cheapest run leaves it one cell at twice the minimum.
    dropped = variant({finds[0]["find_run_id"]})
    assert (dropped.excluded_runs, dropped.find_runs) == (1, 3)
    assert (dropped.all_rows, dropped.implemented_gold) == (te.Rate(6, 6), te.Rate(5, 5))
    assert [(row.builder_id, row.rank, row.n_cells) for row in dropped.rb["high_effort"]] == [
        (BUILDERS[1], 1, 2),
        (BUILDERS[0], 2, 1),
    ]

    result = te.Analysis(
        inputs={},
        generated_at=None,
        exposures=[],
        all_runs=everything,
        excluding=dropped,
        label_sources={},
    )
    assert result.accuracy_change("all_rows") == Fraction(1, 8)
    assert result.accuracy_change("implemented_gold") == 0
    assert result.max_abs_change == Fraction(1, 8)
    assert not result.ranks_unchanged("high_effort")
    assert result.ranks_unchanged("low_effort")


# --------------------------------------------------------------------------- release data


@pytest.fixture(scope="module")
def analysis():
    if not (RELEASE / "tables" / "question_answers.csv").exists():
        pytest.skip("runs_release tables are not available")
    return te.analyze(
        te.Paths(root=ROOT, runs_root=RELEASE),
        exposure_path=RELEASE_OUTPUT_DIR / te.EXPOSURE_NAME,
    )


def test_release_list_covers_62_primary_panel_runs(analysis) -> None:
    assert len(analysis.exposures) == 62
    assert Counter(run.category for run in analysis.exposures) == {
        "answer_files_in_search_output": 1,
        "host_config_read": 1,
        "git_status_listing": 48,
        "workspace_listing": 11,
        "git_checkout_attempt": 1,
    }
    assert (analysis.all_runs.find_runs, analysis.excluding.find_runs) == (1476, 1414)
    for run in analysis.exposures:
        assert not re.search(r"(?:^|[\s(`'\"])(?:/|~)", run.note), run.note
        assert "REDACTED" not in run.note


def test_release_accuracy_and_rb_ranks(analysis) -> None:
    assert analysis.all_runs.all_rows == te.Rate(21413, 22140)
    assert analysis.excluding.all_rows == te.Rate(20536, 21210)
    assert analysis.all_runs.implemented_gold == te.Rate(21084, 21312)
    assert analysis.excluding.implemented_gold == te.Rate(20216, 20425)
    printed = [
        te.percent(getattr(variant, scope).value)
        for scope in ("all_rows", "implemented_gold")
        for variant in (analysis.all_runs, analysis.excluding)
    ]
    assert printed == ["96.716", "96.822", "98.930", "98.977"]
    assert analysis.max_abs_change <= Fraction(11, 10000)
    assert te.percent(analysis.max_abs_change, 2) == "0.11"
    assert analysis.label_sources == {
        "consensus_prelabel": 681,
        "manual_review": 24,
        "rule_empty_artifact": 15,
    }
    assert set(analysis.all_runs.rb) == {"high_effort", "low_effort"}
    for panel_id in analysis.all_runs.rb:
        assert analysis.ranks_unchanged(panel_id), panel_id


def test_release_baseline_rb_matches_impact_report(analysis) -> None:
    path = (
        RELEASE
        / "reports"
        / "artifact_conformance_consensus"
        / "impact"
        / "conditioned_builder_scores.csv"
    )
    released = {
        (row["panel_id"], row["builder_id"]): row
        for row in impact._read_csv(path)
        if (row["gate"], row["metric"]) == (te.RB_GATE, te.RB_METRIC)
    }
    baseline = {
        (panel_id, row.builder_id): row
        for panel_id, rows in analysis.all_runs.rb.items()
        for row in rows
    }
    assert set(baseline) == set(released)
    for key, row in baseline.items():
        assert row.rank == int(released[key]["rank"]), key
        assert row.n_cells == int(released[key]["n_cells"]), key
        assert row.score == pytest.approx(float(released[key]["score"]), rel=1e-12), key


def test_committed_outputs_are_current(analysis, tmp_path: Path) -> None:
    te.write_outputs(analysis, tmp_path)
    for name in OUTPUT_FILES:
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
