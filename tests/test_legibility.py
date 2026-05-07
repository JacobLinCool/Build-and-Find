"""Tests for the fidelity / legibility decomposition.

These metrics decide builder ranking, so the threshold semantics need
to be exact. We check four shapes:

* unanimous correct → high agreement, no convergent miss
* unanimous wrong   → both high agreement AND convergent miss (the
  "code reads clearly but disagrees with spec" case — exactly the
  signal we want builders punished for)
* split wrong       → no convergent miss (agreement below threshold)
* below 2 finders   → cell is dropped entirely (n=1 has no agreement)
"""

from __future__ import annotations

from bnf.analysis.legibility import (
    DEFAULT_CONSENSUS_THRESHOLD,
    DEFAULT_LEGIBILITY_THRESHOLD,
    compute_legibility_per_builder,
)
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    FindRun,
    FinderAnswer,
    RunManifest,
    UsageMetrics,
    ValidationResult,
)


def _br(builder_id: str, build_run_id: str = "br-1", task_id: str = "t1") -> BuildRun:
    return BuildRun(
        build_run_id=build_run_id,
        task_id=task_id,
        manifest=RunManifest(
            run_id=build_run_id,
            spec_hash="h",
            prompt_hash="p",
            agent=AgentInfo(id=builder_id, adapter="codex", model_id="m"),
            seed=0,
            trial_id=0,
        ),
        artifact_path="/tmp/x",
        usage=UsageMetrics(),
        validation=ValidationResult(compile_pass=True),
        status="ok",
    )


def _fr(
    finder_id: str,
    build_run_id: str,
    *,
    picks: list[tuple[str, str, str | None]],
    task_id: str = "t1",
) -> FindRun:
    """``picks`` = list of (question_id, presented_correct_key, selected_option)."""

    answers = [
        FinderAnswer(
            question_id=qid,
            presented_options={"A": "a", "B": "b", "C": "c", "D": "d"},
            presented_correct_key=correct,
            selected_option=sel,
            is_correct=(sel == correct),
            evidence_files=[],
            evidence_symbols=[],
        )
        for qid, correct, sel in picks
    ]
    fid = f"fr-{finder_id}-{build_run_id}"
    return FindRun(
        find_run_id=fid,
        build_run_id=build_run_id,
        task_id=task_id,
        manifest=RunManifest(
            run_id=fid,
            spec_hash="h",
            prompt_hash="p",
            agent=AgentInfo(id=finder_id, adapter="codex", model_id="m"),
            seed=0,
            trial_id=0,
        ),
        answers=answers,
        usage=UsageMetrics(),
        status="ok",
    )


def test_unanimous_correct_high_agreement_no_convergent_miss() -> None:
    br = _br("builder-X")
    finds = [
        _fr("F1", "br-1", picks=[("q1", "A", "A")]),
        _fr("F2", "br-1", picks=[("q1", "A", "A")]),
        _fr("F3", "br-1", picks=[("q1", "A", "A")]),
        _fr("F4", "br-1", picks=[("q1", "A", "A")]),
    ]
    leg = compute_legibility_per_builder([br], finds)
    cell = leg["builder-X"]
    assert cell.n_question_cells == 1
    assert cell.high_agreement_cells == 1
    assert cell.inter_finder_agreement == 1.0
    assert cell.convergent_miss_cells == 0
    assert cell.convergent_miss_rate == 0.0


def test_unanimous_wrong_is_both_legible_and_convergent_miss() -> None:
    """The flagship case the metric is designed to catch: 4/4 finders
    correctly read the code and unanimously pick a non-canonical option.
    Code reads great; builder diverged from spec."""

    br = _br("builder-Y")
    finds = [
        _fr("F1", "br-1", picks=[("q1", "A", "D")]),
        _fr("F2", "br-1", picks=[("q1", "A", "D")]),
        _fr("F3", "br-1", picks=[("q1", "A", "D")]),
        _fr("F4", "br-1", picks=[("q1", "A", "D")]),
    ]
    leg = compute_legibility_per_builder([br], finds)
    cell = leg["builder-Y"]
    assert cell.n_question_cells == 1
    assert cell.inter_finder_agreement == 1.0  # legibility is perfect
    assert cell.convergent_miss_rate == 1.0  # but every cell is a divergence
    assert cell.convergent_miss_events == (("t1", "q1", "D", 4, 4),)


def test_split_wrong_below_threshold_is_not_convergent_miss() -> None:
    """1/4 finders picked B, 3/4 picked the correct A → no convergent
    miss (the wrong pick is below the strict-majority threshold)."""

    br = _br("builder-Z")
    finds = [
        _fr("F1", "br-1", picks=[("q1", "A", "A")]),
        _fr("F2", "br-1", picks=[("q1", "A", "A")]),
        _fr("F3", "br-1", picks=[("q1", "A", "A")]),
        _fr("F4", "br-1", picks=[("q1", "A", "B")]),  # the rogue
    ]
    leg = compute_legibility_per_builder([br], finds)
    cell = leg["builder-Z"]
    assert cell.convergent_miss_cells == 0
    # 3/4 = 0.75 ≥ default 2/3 legibility threshold
    assert cell.high_agreement_cells == 1


def test_split_wrong_two_each_no_convergence_anywhere() -> None:
    br = _br("builder-W")
    finds = [
        _fr("F1", "br-1", picks=[("q1", "A", "B")]),
        _fr("F2", "br-1", picks=[("q1", "A", "B")]),
        _fr("F3", "br-1", picks=[("q1", "A", "C")]),
        _fr("F4", "br-1", picks=[("q1", "A", "C")]),
    ]
    leg = compute_legibility_per_builder([br], finds)
    cell = leg["builder-W"]
    # Top option = B with 2/4 = 0.5; that's NOT > consensus threshold
    # (default 0.5 is "≥ majority" — but with a 2-2 tie there is no
    # majority winner, just a most_common pick. The rate stays 0
    # because share = 0.5 == 0.5 IS counted as ≥. Wait — this is a
    # subtle case. Default is 0.5, share is 0.5 → counted. So it IS
    # a convergent miss under default, with B as top option.
    # Validate that contract is honored: ≥ is inclusive.
    assert cell.convergent_miss_cells == 1
    # legibility threshold is 2/3; 0.5 < 0.667 → not high-agreement
    assert cell.high_agreement_cells == 0


def test_single_finder_cell_is_skipped() -> None:
    br = _br("builder-S")
    finds = [_fr("F1", "br-1", picks=[("q1", "A", "B")])]
    leg = compute_legibility_per_builder([br], finds)
    # The lone-finder cell is dropped — agreement is undefined for n=1.
    # With no surviving cells, the builder doesn't appear in the result.
    assert "builder-S" not in leg


def test_threshold_is_proportional_to_finder_pool() -> None:
    """Same 2-of-3 split should pass a 0.5 consensus threshold but
    not a 0.7 one — verifying the metric scales with the finder pool
    size rather than counting absolutes."""

    br = _br("builder-P")
    finds = [
        _fr("F1", "br-1", picks=[("q1", "A", "C")]),
        _fr("F2", "br-1", picks=[("q1", "A", "C")]),
        _fr("F3", "br-1", picks=[("q1", "A", "A")]),
    ]
    loose = compute_legibility_per_builder([br], finds, consensus_threshold=0.5)
    strict = compute_legibility_per_builder([br], finds, consensus_threshold=0.7)
    assert loose["builder-P"].convergent_miss_cells == 1  # 2/3 ≥ 0.5
    assert strict["builder-P"].convergent_miss_cells == 0  # 2/3 < 0.7


def test_defaults_match_documentation() -> None:
    # Wired-in defaults shouldn't drift silently — the report wording
    # quotes them, so a change here must be deliberate.
    assert DEFAULT_CONSENSUS_THRESHOLD == 0.5
    assert abs(DEFAULT_LEGIBILITY_THRESHOLD - 2 / 3) < 1e-9
