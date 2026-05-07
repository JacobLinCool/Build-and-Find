"""Tests for the accuracy × efficiency analysis (paper §17.6, §19.4).

Cover:

* ``iter_run_points`` pulls the right efficiency axes off records with
  and without a proxy-sourced Efficiency block.
* ``pairwise_dominance_finders`` classifies outcomes correctly and
  skips runs lacking efficiency data.
* ``scatter_rows`` round-trips through CSV-friendly dicts.
"""

from __future__ import annotations

from bnf.analysis import (
    iter_run_points,
    pairwise_dominance_finders,
    scatter_rows,
)
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    Efficiency,
    FindRun,
    FinderAnswer,
    RunManifest,
    UsageMetrics,
    ValidationResult,
)


def _manifest(run_id: str, agent_id: str, *, trial_id: int = 0) -> RunManifest:
    return RunManifest(
        run_id=run_id,
        spec_hash="h",
        prompt_hash="p",
        agent=AgentInfo(id=agent_id, adapter="claude_code", model_id="m"),
        seed=0,
        trial_id=trial_id,
    )


def _build_run(run_id: str, task: str, agent_id: str) -> BuildRun:
    return BuildRun(
        build_run_id=run_id,
        task_id=task,
        manifest=_manifest(run_id, agent_id),
        artifact_path="/tmp/nope",
        usage=UsageMetrics(),
        validation=ValidationResult(compile_pass=True),
        status="ok",
    )


def _find_run(
    run_id: str,
    build_run_id: str,
    task: str,
    finder: str,
    *,
    accuracy: float,
    efficiency: Efficiency | None = None,
    trial_id: int = 0,
) -> FindRun:
    total = 4
    correct = int(round(accuracy * total))
    answers: list[FinderAnswer] = []
    for i in range(total):
        answers.append(
            FinderAnswer(
                question_id=f"q{i}",
                presented_options={"A": "a", "B": "b", "C": "c", "D": "d"},
                presented_correct_key="A",
                selected_option="A" if i < correct else "B",
                is_correct=i < correct,
                evidence_files=[],
                evidence_symbols=[],
            )
        )
    usage = UsageMetrics(efficiency=efficiency)
    return FindRun(
        find_run_id=run_id,
        build_run_id=build_run_id,
        task_id=task,
        manifest=_manifest(run_id, finder, trial_id=trial_id),
        answers=answers,
        usage=usage,
        status="ok",
    )


def test_iter_run_points_pulls_efficiency_axes() -> None:
    br = _build_run("b1", "task-x", "builder-a")
    fr = _find_run(
        "f1",
        build_run_id="b1",
        task="task-x",
        finder="finder-a",
        accuracy=0.75,
        efficiency=Efficiency(
            novel_retrieval_bytes=10_000,
            novel_tool_call_count=3,
            novel_tool_result_count=3,
            wall_clock_seconds=12.5,
            generation_tok_per_s=42.0,
        ),
    )
    (point,) = iter_run_points([br], [fr])
    assert point.accuracy == 0.75
    assert point.effort == 10_000.0
    assert point.efficiency["novel_tool_call_count"] == 3.0
    assert point.efficiency["wall_clock_seconds"] == 12.5
    assert point.efficiency["generation_tok_per_s"] == 42.0


def test_iter_run_points_handles_missing_efficiency() -> None:
    br = _build_run("b1", "task-x", "builder-a")
    fr = _find_run("f1", build_run_id="b1", task="task-x", finder="finder-a", accuracy=0.5)
    (point,) = iter_run_points([br], [fr])
    # effort and per-axis fields are NaN, not crashes.
    import math

    assert math.isnan(point.effort)
    assert math.isnan(point.efficiency["novel_retrieval_bytes"])


def test_pairwise_dominance_wins_ties_losses_incomparable() -> None:
    """Cover all four outcome labels with four matched tasks, two
    finders."""

    builds = []
    finds = []
    for i, task in enumerate(("t1", "t2", "t3", "t4")):
        br = _build_run(f"b{i}", task, "builder-a")
        builds.append(br)

    # Task t1: A strictly dominates (better accuracy + cheaper).
    finds.append(
        _find_run(
            "fa1",
            "b0",
            "t1",
            "A",
            accuracy=0.9,
            efficiency=Efficiency(
                novel_retrieval_bytes=1_000,
                wall_clock_seconds=1.0,
                novel_tool_call_count=1,
                novel_tool_result_count=1,
            ),
        )
    )
    finds.append(
        _find_run(
            "fb1",
            "b0",
            "t1",
            "B",
            accuracy=0.5,
            efficiency=Efficiency(
                novel_retrieval_bytes=10_000,
                wall_clock_seconds=5.0,
                novel_tool_call_count=5,
                novel_tool_result_count=5,
            ),
        )
    )
    # Task t2: exact tie.
    tie_eff = Efficiency(
        novel_retrieval_bytes=100,
        wall_clock_seconds=1.0,
        novel_tool_call_count=1,
        novel_tool_result_count=1,
    )
    finds.append(_find_run("fa2", "b1", "t2", "A", accuracy=0.5, efficiency=tie_eff))
    finds.append(_find_run("fb2", "b1", "t2", "B", accuracy=0.5, efficiency=tie_eff))
    # Task t3: B dominates.
    finds.append(
        _find_run(
            "fa3",
            "b2",
            "t3",
            "A",
            accuracy=0.4,
            efficiency=Efficiency(
                novel_retrieval_bytes=9_000,
                wall_clock_seconds=4.0,
                novel_tool_call_count=4,
                novel_tool_result_count=4,
            ),
        )
    )
    finds.append(
        _find_run(
            "fb3",
            "b2",
            "t3",
            "B",
            accuracy=0.9,
            efficiency=Efficiency(
                novel_retrieval_bytes=1_000,
                wall_clock_seconds=1.0,
                novel_tool_call_count=1,
                novel_tool_result_count=1,
            ),
        )
    )
    # Task t4: incomparable — A cheaper but less accurate.
    finds.append(
        _find_run(
            "fa4",
            "b3",
            "t4",
            "A",
            accuracy=0.4,
            efficiency=Efficiency(
                novel_retrieval_bytes=500,
                wall_clock_seconds=1.0,
                novel_tool_call_count=1,
                novel_tool_result_count=1,
            ),
        )
    )
    finds.append(
        _find_run(
            "fb4",
            "b3",
            "t4",
            "B",
            accuracy=0.9,
            efficiency=Efficiency(
                novel_retrieval_bytes=5_000,
                wall_clock_seconds=3.0,
                novel_tool_call_count=2,
                novel_tool_result_count=2,
            ),
        )
    )

    cells = pairwise_dominance_finders(builds, finds)
    ab = cells[("A", "B")]
    ba = cells[("B", "A")]

    assert ab.matched_tasks == 4
    assert ab.wins == 1  # t1
    assert ab.ties == 1  # t2
    assert ab.losses == 1  # t3
    assert ab.incomparable == 1  # t4
    assert abs(ab.dominance_rate - 0.25) < 1e-9

    # Mirror image: B wins on t3, ties on t2, loses on t1, incomparable on t4.
    assert ba.wins == 1
    assert ba.ties == 1
    assert ba.losses == 1
    assert ba.incomparable == 1


def test_pairwise_dominance_skips_runs_missing_efficiency() -> None:
    """Legacy records without an Efficiency block must not pollute
    the dominance matrix."""

    builds = [_build_run("b0", "t1", "builder-a")]
    finds = [
        # A has efficiency
        _find_run(
            "fa",
            "b0",
            "t1",
            "A",
            accuracy=0.9,
            efficiency=Efficiency(
                novel_retrieval_bytes=1_000,
                wall_clock_seconds=1.0,
                novel_tool_call_count=1,
                novel_tool_result_count=1,
            ),
        ),
        # B doesn't
        _find_run("fb", "b0", "t1", "B", accuracy=0.5),
    ]
    cells = pairwise_dominance_finders(builds, finds)
    assert cells[("A", "B")].matched_tasks == 0
    assert cells[("B", "A")].matched_tasks == 0


def test_scatter_rows_exports_all_axes() -> None:
    br = _build_run("b0", "t1", "builder-a")
    fr = _find_run(
        "fa",
        "b0",
        "t1",
        "A",
        accuracy=0.75,
        efficiency=Efficiency(
            novel_retrieval_bytes=2_048,
            novel_tool_call_count=3,
            novel_tool_result_count=3,
            wall_clock_seconds=5.0,
            generation_tok_per_s=50.0,
        ),
    )
    rows = scatter_rows([br], [fr])
    assert len(rows) == 1
    row = rows[0]
    assert row["accuracy"] == 0.75
    assert row["novel_retrieval_bytes"] == 2_048
    assert row["generation_tok_per_s"] == 50.0
    assert row["task_id"] == "t1"
    assert row["finder_id"] == "A"
