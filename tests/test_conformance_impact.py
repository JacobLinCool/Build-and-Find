from __future__ import annotations

import importlib.util
import math
import sys
from collections import Counter
from pathlib import Path
from typing import Any

import pytest


ROOT = Path(__file__).resolve().parents[1]
FINAL_DIR = ROOT / "runs_release" / "reports" / "artifact_conformance_consensus" / "final"
IMPACT_DIR = ROOT / "runs_release" / "reports" / "artifact_conformance_consensus" / "impact"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


impact = load_script_module(
    "analyze_artifact_conformance_impact",
    "scripts/analyze_artifact_conformance_impact.py",
)


class _AlwaysFirst:
    """RNG stand-in whose ``choice`` always returns the first key."""

    def choice(self, seq: list[Any]) -> Any:
        return seq[0]


def _run_row(finder_id: str, builder_id: str, total_bytes: float) -> dict[str, Any]:
    return {
        "task_id": "scratch_minidb",
        "builder_id": builder_id,
        "finder_id": finder_id,
        "accuracy": 1.0,
        "original_accuracy": 1.0,
        "perfect_run": 1,
        "question_count": 15,
        "total_bytes": total_bytes,
    }


def _label_row(question_id: str, final_label: str) -> dict[str, str]:
    return {
        "task_id": "scratch_nanoweb",
        "artifact_id": "build-a",
        "question_id": question_id,
        "final_label": final_label,
        "label_source": "consensus_prelabel",
    }


def _correction(question_id: str, old_label: str) -> dict[str, str]:
    return {
        "task_id": "scratch_nanoweb",
        "artifact_id": "build-a",
        "question_id": question_id,
        "old_label": old_label,
        "new_label": "absent_or_ambiguous",
        "label_source": "rule_empty_artifact",
    }


def test_bootstrap_cluster_drawn_twice_contributes_two_cells() -> None:
    rows = [
        _run_row("finder_a", "builder_x", 100.0),
        _run_row("finder_a", "builder_y", 150.0),
        _run_row("finder_b", "builder_x", 300.0),
        _run_row("finder_b", "builder_y", 200.0),
    ]
    clusters: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for row in rows:
        clusters.setdefault((row["finder_id"], row["task_id"]), []).append(row)
    panel = impact.Panel(
        panel_id="high_effort",
        description="test",
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
        effort="high",
        primary=True,
    )

    sampled = impact._bootstrap_resample(clusters, _AlwaysFirst())
    means = impact._cell_means(sampled, metric="total_bytes", gate="perfect_run")
    scores = {
        row["builder_id"]: row
        for row in impact._builder_score_rows(panel, "perfect_run", "total_bytes", means)
    }

    assert {row["finder_id"] for row in sampled} == {"finder_a#0", "finder_a#1"}
    assert scores["builder_x"]["n_cells"] == 2
    assert scores["builder_y"]["n_cells"] == 2
    assert scores["builder_y"]["score"] == pytest.approx(1.5)
    assert {row["finder_id"] for row in rows} == {"finder_a", "finder_b"}


def _task_run_row(
    task_id: str,
    finder_id: str,
    builder_id: str,
    total_bytes: float,
    *,
    perfect_run: int = 1,
) -> dict[str, Any]:
    return {
        **_run_row(finder_id, builder_id, total_bytes),
        "task_id": task_id,
        "builder_effort": "high",
        "finder_effort": "high",
        "accuracy": float(perfect_run),
        "perfect_run": perfect_run,
    }


def test_builder_scores_by_task_use_only_each_tasks_cells() -> None:
    rows = [
        _task_run_row("scratch_minidb", "finder_a", "builder_x", 100.0),
        _task_run_row("scratch_minidb", "finder_a", "builder_y", 200.0),
        _task_run_row("scratch_minidb", "finder_a", "builder_z", 400.0),
        _task_run_row("scratch_minidb", "finder_b", "builder_x", 100.0),
        _task_run_row("scratch_minidb", "finder_b", "builder_y", 300.0),
        _task_run_row("scratch_minidb", "finder_b", "builder_z", 50.0, perfect_run=0),
        _task_run_row("scratch_nanoweb", "finder_a", "builder_x", 300.0),
        _task_run_row("scratch_nanoweb", "finder_a", "builder_y", 100.0),
        _task_run_row("scratch_nanoweb", "finder_b", "builder_x", 150.0),
        _task_run_row("scratch_nanoweb", "finder_b", "builder_y", 100.0),
    ]
    panel = impact.Panel(
        panel_id="high_effort",
        description="test",
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
        effort="high",
        primary=True,
    )
    pooled = impact._builder_score_rows(
        panel,
        "perfect_run",
        "total_bytes",
        impact._cell_means(rows, metric="total_bytes", gate="perfect_run"),
    )

    by_task = {
        (row["task_id"], row["builder_id"]): row
        for row in impact._builder_score_by_task_rows([panel], rows, pooled_score_rows=pooled)
    }

    assert sorted(by_task) == [
        ("scratch_minidb", "builder_x"),
        ("scratch_minidb", "builder_y"),
        ("scratch_minidb", "builder_z"),
        ("scratch_nanoweb", "builder_x"),
        ("scratch_nanoweb", "builder_y"),
    ]
    minidb_y = by_task[("scratch_minidb", "builder_y")]
    assert minidb_y["score"] == pytest.approx(6**0.5)
    assert minidb_y["rank"] == 2
    assert (minidb_y["ci95_low"], minidb_y["ci95_high"]) == pytest.approx((2.0, 3.0))
    minidb_z = by_task[("scratch_minidb", "builder_z")]
    assert minidb_z["score"] == pytest.approx(4.0)
    assert (minidb_z["n_cells"], minidb_z["max_cells"]) == (1, 2)
    assert (minidb_z["ci95_low"], minidb_z["ci95_high"]) == pytest.approx((4.0, 4.0))
    nanoweb_x = by_task[("scratch_nanoweb", "builder_x")]
    assert nanoweb_x["score"] == pytest.approx(4.5**0.5)
    assert (nanoweb_x["rank"], nanoweb_x["pooled_rank"]) == (2, 1)
    assert by_task[("scratch_nanoweb", "builder_y")]["rank"] == 1
    assert {row["finder_id"] for row in rows} == {"finder_a", "finder_b"}


def _release_impact_rows(name: str) -> list[dict[str, str]]:
    path = IMPACT_DIR / name
    if not path.exists():
        pytest.skip(f"{name} is not available")
    return impact._read_csv(path)


def test_release_builder_scores_by_task_are_current(tmp_path: Path) -> None:
    run_rows = [
        {
            **row,
            "accuracy": float(row["accuracy"]),
            "original_accuracy": float(row["original_accuracy"]),
            "perfect_run": int(row["perfect_run"]),
            "question_count": int(row["question_count"]),
            "total_bytes": float(row["total_bytes"]),
        }
        for row in _release_impact_rows("conditioned_run_rows.csv")
    ]
    pooled = _release_impact_rows("conditioned_builder_scores.csv")
    expected = impact._builder_score_by_task_rows(
        impact._panels(),
        run_rows,
        pooled_score_rows=pooled,
    )

    impact._write_csv(tmp_path / "by_task.csv", expected)

    assert (tmp_path / "by_task.csv").read_bytes() == (
        IMPACT_DIR / "conditioned_builder_scores_by_task.csv"
    ).read_bytes()


def test_release_builder_scores_by_task_partition_the_pooled_scores() -> None:
    pooled = {
        (row["panel_id"], row["builder_id"]): row
        for row in _release_impact_rows("conditioned_builder_scores.csv")
        if row["gate"] == "perfect_run" and row["metric"] == "total_bytes"
    }
    by_task = _release_impact_rows("conditioned_builder_scores_by_task.csv")
    log_sums: Counter[tuple[str, str]] = Counter()
    cells: Counter[tuple[str, str]] = Counter()
    blocks: dict[tuple[str, str], list[dict[str, str]]] = {}
    for row in by_task:
        key = (row["panel_id"], row["builder_id"])
        log_sums[key] += int(row["n_cells"]) * math.log(float(row["score"]))
        cells[key] += int(row["n_cells"])
        blocks.setdefault((row["panel_id"], row["task_id"]), []).append(row)
        assert row["pooled_rank"] == pooled[key]["rank"]
        assert int(row["n_cells"]) <= int(row["max_cells"])
        assert float(row["ci95_low"]) <= float(row["ci95_high"])

    assert sorted(blocks) == [
        (panel_id, task_id)
        for panel_id in ("high_effort", "low_effort")
        for task_id in sorted(impact.SCRATCH_TASKS)
    ]
    for rows in blocks.values():
        assert [int(row["rank"]) for row in rows] == list(range(1, len(rows) + 1))
        scores = [float(row["score"]) for row in rows]
        assert scores == sorted(scores)
    assert set(cells) == set(pooled)
    for key, row in pooled.items():
        assert cells[key] == int(row["n_cells"])
        assert math.exp(log_sums[key] / cells[key]) == pytest.approx(float(row["score"]))


def test_label_corrections_replace_label_and_source_only_for_their_keys() -> None:
    released = [
        _label_row("q_nw_01", "implemented_gold"),
        _label_row("q_nw_02", "implemented_gold"),
    ]

    corrected = impact._apply_label_corrections(
        released,
        [_correction("q_nw_01", "implemented_gold")],
    )

    assert corrected[0]["final_label"] == "absent_or_ambiguous"
    assert corrected[0]["label_source"] == "rule_empty_artifact"
    assert corrected[1] == released[1]
    assert released[0]["final_label"] == "implemented_gold"


def test_label_corrections_fail_on_unknown_key_or_stale_old_label() -> None:
    released = [_label_row("q_nw_01", "implemented_gold")]

    with pytest.raises(RuntimeError, match="no released label row"):
        impact._apply_label_corrections(released, [_correction("q_nw_99", "implemented_gold")])
    with pytest.raises(RuntimeError, match="does not match released label"):
        impact._apply_label_corrections(released, [_correction("q_nw_01", "implemented_non_gold")])
    with pytest.raises(RuntimeError, match="duplicate label correction"):
        impact._apply_label_corrections(
            released,
            [_correction("q_nw_01", "implemented_gold")] * 2,
        )


def test_release_label_corrections_apply_to_released_labels() -> None:
    corrections = impact._read_csv(FINAL_DIR / impact.LABEL_CORRECTIONS_FILENAME)
    labels = impact._apply_label_corrections(
        impact._read_csv(FINAL_DIR / "artifact_conformance_final.csv"),
        corrections,
    )

    assert len(corrections) == 15
    assert Counter(row["label_source"] for row in labels) == {
        "consensus_prelabel": 681,
        "manual_review": 24,
        "rule_empty_artifact": 15,
    }
    assert Counter(row["final_label"] for row in labels)["implemented_gold"] == 672


merge = load_script_module(
    "merge_artifact_conformance_annotations",
    "scripts/merge_artifact_conformance_annotations.py",
)


def test_final_readme_documents_v1_1_corrections_and_overlay_counts() -> None:
    released = [
        _label_row("q_nw_01", "implemented_gold"),
        {**_label_row("q_nw_02", "implemented_gold"), "label_source": "manual_review"},
    ]

    lines = merge._correction_markdown(released, [_correction("q_nw_01", "implemented_gold")])

    assert "## v1.1 Label Corrections" in lines
    assert any(
        line.startswith("| `rule_empty_artifact` | the build artifact contains no source files")
        and line.endswith("| 1 | 1 |")
        for line in lines
    )
    assert "| `absent_or_ambiguous` | 1 | 0.500000 |" in lines
    assert "| `rule_empty_artifact` | 1 |" in lines
    assert "| `manual_review` | 1 |" in lines
    assert released[0]["final_label"] == "implemented_gold"


def test_release_final_readme_matches_v1_1_overlay() -> None:
    readme = (FINAL_DIR / "README.md").read_text(encoding="utf-8")

    assert f"`{impact.LABEL_CORRECTIONS_FILENAME}`" in readme
    assert "| `consensus_prelabel` | 696 |" in readme
    assert "| `consensus_prelabel` | 681 |" in readme
    assert "| `rule_empty_artifact` | 15 |" in readme
    assert "| `implemented_gold` | 672 | 0.933333 |" in readme
