"""Accuracy × efficiency analysis (paper §17.6, §19.4).

Two primary views:

* **Scatter**: one data point per ``(codebase, finder, trial)`` with
  ``accuracy`` on one axis and ``novel_retrieval_bytes`` (or any other
  efficiency axis) on the other. Export for plotting.
* **Pairwise dominance**: for each ordered pair ``(A, B)`` of finders,
  the fraction of matched tasks on which A's run dominates B's — i.e.
  A's accuracy is at least B's and every efficiency axis is at most
  B's. See :func:`pairwise_dominance_finders`.

Both views deliberately keep accuracy and efficiency as separate
dimensions; they are never collapsed into a single scalar (paper
§17.6).
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Iterable, Optional

from bnf.schema.run import BuildRun, FindRun

#: Default efficiency axes used by dominance comparisons (paper §17.3).
#: ``generation_tok_per_s`` is deliberately excluded: higher is better
#: for that field, whereas the others are "lower is better". Mixing
#: directions in a single dominance predicate is nonsensical, so tok/s
#: is reported separately.
DEFAULT_EFFICIENCY_AXES = (
    "novel_retrieval_bytes",
    "novel_tool_call_bytes",
    "novel_tool_call_count",
    "novel_tool_result_count",
    "wall_clock_seconds",
)


@dataclass
class RunPoint:
    """One ``(codebase, finder, trial)`` tuple in the accuracy-vs-
    efficiency plane."""

    task_id: str
    builder_id: str
    build_run_id: str
    finder_id: str
    find_run_id: str
    trial_id: int
    #: 0..1, from :attr:`FindRun.accuracy`.
    accuracy: float
    #: The chosen efficiency axis, NaN if the run has no proxy data.
    effort: float
    #: All efficiency axes at once for dominance analysis.
    efficiency: dict[str, float] = field(default_factory=dict)


def iter_run_points(
    build_runs: Iterable[BuildRun],
    find_runs: Iterable[FindRun],
    *,
    effort_metric: str = "novel_retrieval_bytes",
) -> list[RunPoint]:
    """Flatten ``(build_run, find_run)`` pairs into a scatter-friendly
    list of :class:`RunPoint`.

    Skips find_runs whose matching build_run cannot be located."""

    build_by_id = {br.build_run_id: br for br in build_runs}
    out: list[RunPoint] = []
    for fr in find_runs:
        br = build_by_id.get(fr.build_run_id)
        if br is None:
            continue
        eff_axes = _efficiency_axes_for_run(fr)
        out.append(
            RunPoint(
                task_id=fr.task_id,
                builder_id=br.manifest.agent.id,
                build_run_id=br.build_run_id,
                finder_id=fr.manifest.agent.id,
                find_run_id=fr.find_run_id,
                trial_id=fr.manifest.trial_id,
                accuracy=fr.accuracy,
                effort=eff_axes.get(effort_metric, float("nan")),
                efficiency=eff_axes,
            )
        )
    return out


def _efficiency_axes_for_run(fr: FindRun) -> dict[str, float]:
    """Pull the comparable efficiency axes off a FindRun.

    Returns all of :data:`DEFAULT_EFFICIENCY_AXES` plus
    ``generation_tok_per_s``. Missing values become ``NaN`` so callers
    can decide whether to drop them.
    """

    eff = fr.usage.efficiency
    if eff is None:
        return {axis: float("nan") for axis in DEFAULT_EFFICIENCY_AXES + ("generation_tok_per_s",)}
    out = {
        "novel_retrieval_bytes": float(eff.novel_retrieval_bytes),
        "novel_tool_call_bytes": float(eff.novel_tool_call_bytes),
        "novel_tool_call_count": float(eff.novel_tool_call_count),
        "novel_tool_result_count": float(eff.novel_tool_result_count),
        "wall_clock_seconds": float(eff.wall_clock_seconds),
    }
    out["generation_tok_per_s"] = (
        float(eff.generation_tok_per_s) if eff.generation_tok_per_s is not None else float("nan")
    )
    return out


# ----------------------------------------------------------------------
# Pairwise dominance
# ----------------------------------------------------------------------


@dataclass
class DominanceCell:
    """One entry in the pairwise dominance matrix."""

    row: str  # challenger (A)
    col: str  # baseline (B)
    wins: int = 0  # A strictly dominates B on this matched task
    ties: int = 0  # A equals B on accuracy and all effort axes
    losses: int = 0  # B strictly dominates A
    incomparable: int = 0  # neither dominates (wins on some, loses on others)
    matched_tasks: int = 0

    @property
    def dominance_rate(self) -> float:
        """Fraction of matched tasks on which the row agent dominates
        the column agent, treating ties as neither win nor loss."""

        if self.matched_tasks == 0:
            return float("nan")
        return self.wins / self.matched_tasks


def pairwise_dominance_finders(
    build_runs: Iterable[BuildRun],
    find_runs: Iterable[FindRun],
    *,
    axes: tuple[str, ...] = DEFAULT_EFFICIENCY_AXES,
) -> dict[tuple[str, str], DominanceCell]:
    """Return a mapping ``(row_finder, col_finder) → DominanceCell``.

    Matching rule: two FindRuns belong to the same task comparison iff
    they share the same ``(task_id, build_run_id, trial_id)`` — i.e. the
    same codebase artefact and trial, differing only in finder.

    Row A dominates column B on a matched task iff:

    * ``accuracy_A ≥ accuracy_B`` and
    * for every axis in ``axes`` (all "lower is better"),
      ``axis_A ≤ axis_B`` and
    * at least one comparison is strict (otherwise it counts as a tie).

    Runs without proxy efficiency (legacy records) are skipped for the
    dominance comparison.
    """

    find_runs = list(find_runs)
    points = iter_run_points(build_runs, find_runs, effort_metric=axes[0])
    # Group by matched key so we can compare all finders on the same
    # codebase instance.
    grouped: dict[tuple[str, str, int], dict[str, RunPoint]] = {}
    for pt in points:
        key = (pt.task_id, pt.build_run_id, pt.trial_id)
        grouped.setdefault(key, {})[pt.finder_id] = pt

    finders = sorted({pt.finder_id for pt in points})
    cells: dict[tuple[str, str], DominanceCell] = {
        (a, b): DominanceCell(row=a, col=b) for a in finders for b in finders if a != b
    }

    for per_task in grouped.values():
        for a in finders:
            for b in finders:
                if a == b or a not in per_task or b not in per_task:
                    continue
                outcome = _dominance_outcome(per_task[a], per_task[b], axes)
                if outcome is None:
                    # At least one side is missing efficiency data —
                    # skip so legacy records don't pollute the matrix.
                    continue
                cell = cells[(a, b)]
                cell.matched_tasks += 1
                setattr(cell, outcome, getattr(cell, outcome) + 1)
    return cells


def _dominance_outcome(a: RunPoint, b: RunPoint, axes: tuple[str, ...]) -> Optional[str]:
    """Return ``"wins"``, ``"ties"``, ``"losses"``, or ``"incomparable"``
    comparing A against B on the matched task. Returns ``None`` if
    either run lacks efficiency data for any axis.
    """

    if math.isnan(a.accuracy) or math.isnan(b.accuracy):
        return None
    for axis in axes:
        if math.isnan(a.efficiency.get(axis, float("nan"))):
            return None
        if math.isnan(b.efficiency.get(axis, float("nan"))):
            return None

    a_better_anywhere = a.accuracy > b.accuracy
    b_better_anywhere = a.accuracy < b.accuracy
    for axis in axes:
        # Lower is better for effort axes.
        av = a.efficiency[axis]
        bv = b.efficiency[axis]
        if av < bv:
            a_better_anywhere = True
        elif av > bv:
            b_better_anywhere = True

    if a_better_anywhere and not b_better_anywhere:
        return "wins"
    if b_better_anywhere and not a_better_anywhere:
        return "losses"
    if a_better_anywhere and b_better_anywhere:
        return "incomparable"
    return "ties"


# ----------------------------------------------------------------------
# Scatter export
# ----------------------------------------------------------------------


def scatter_rows(
    build_runs: Iterable[BuildRun],
    find_runs: Iterable[FindRun],
) -> list[dict[str, object]]:
    """Flat rows suitable for CSV / JSON export to a plotting tool.

    One row per ``(codebase, finder, trial)``. Paper §17.6's primary
    figure is a scatter of these points, with ``novel_retrieval_bytes``
    on one axis and ``accuracy`` on the other.
    """

    rows: list[dict[str, object]] = []
    for pt in iter_run_points(build_runs, find_runs):
        row: dict[str, object] = {
            "task_id": pt.task_id,
            "builder_id": pt.builder_id,
            "build_run_id": pt.build_run_id,
            "finder_id": pt.finder_id,
            "find_run_id": pt.find_run_id,
            "trial_id": pt.trial_id,
            "accuracy": pt.accuracy,
        }
        for axis, val in pt.efficiency.items():
            row[axis] = None if math.isnan(val) else val
        rows.append(row)
    return rows
