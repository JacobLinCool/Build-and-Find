"""Builder-side metrics that separate spec fidelity from legibility.

Per-question accuracy conflates two effects: (1) did the *builder*
implement what the spec asked for, and (2) did the *finder* read the
code well enough to recover that decision? This module decomposes those
into two builder-centric rates that are read independently of each other:

* :func:`convergent_miss_rate` — fraction of (codebase, question) pairs
  where a strict majority of finders all picked the **same wrong**
  option. When ≥ ``finder_consensus_threshold`` of finders converge on
  a non-canonical answer, that's strong evidence the *builder* diverged
  from the spec and the code says so legibly. Sole-finder misses
  (likely finder noise) do not count here.

* :func:`inter_finder_agreement` — fraction of (codebase, question)
  pairs where ≥ ``legibility_threshold`` of finders picked the **same**
  option (correct or not). High agreement = the codebase reads
  unambiguously, regardless of spec fidelity. This is the cleanest
  *legibility* signal at the builder's disposal.

Both thresholds are expressed as proportions of the present finder
pool, so the metric scales with however many finders ran rather than
hard-coding "≥ 2 of 4". If only 2 finders ran, ``0.5`` still means
"at least half".
"""

from __future__ import annotations

import math
from collections import Counter, defaultdict
from dataclasses import dataclass
from typing import Iterable

from bnf.schema.run import BuildRun, FindRun

#: Default proportion of finders that must converge on the same wrong
#: option for a (builder, codebase, question) cell to count as a
#: convergent miss. A strict-majority default means a 2-of-3 or
#: 3-of-4 split is enough; 2-of-4 (split convergence) is not.
DEFAULT_CONSENSUS_THRESHOLD = 0.5

#: Default proportion of finders that must pick the same option (any
#: option, correct or not) for a cell to count toward
#: inter-finder agreement. Two-thirds is a stricter bar than majority
#: and approximates the "3 out of 4 readers agree" intuition.
DEFAULT_LEGIBILITY_THRESHOLD = 2 / 3


@dataclass(frozen=True)
class LegibilityCell:
    """Aggregated agreement / divergence stats for one builder."""

    builder_id: str
    n_question_cells: int
    convergent_miss_cells: int
    convergent_miss_rate: float
    high_agreement_cells: int
    inter_finder_agreement: float
    #: Per-(task, question) trace of convergent misses for surfacing in
    #: reports. Each tuple is ``(task_id, question_id, picked_option,
    #: n_picked, n_total)``.
    convergent_miss_events: tuple[tuple[str, str, str, int, int], ...]


def compute_legibility_per_builder(
    build_runs: Iterable[BuildRun],
    find_runs: Iterable[FindRun],
    *,
    consensus_threshold: float = DEFAULT_CONSENSUS_THRESHOLD,
    legibility_threshold: float = DEFAULT_LEGIBILITY_THRESHOLD,
) -> dict[str, LegibilityCell]:
    """Return one :class:`LegibilityCell` per builder id.

    A "question cell" is the unit of aggregation: one specific
    (builder, build_run_id, question_id) triple. We collect all finder
    picks on that cell and decide:

    * convergent miss — the most-picked option is **wrong** AND its
      share of finder votes is ≥ ``consensus_threshold``;
    * high agreement — the most-picked option (any) has share
      ≥ ``legibility_threshold``.

    Cells with fewer than 2 finder observations are skipped — agreement
    is undefined for n=1.
    """

    if not 0 < consensus_threshold <= 1:
        raise ValueError("consensus_threshold must be in (0, 1]")
    if not 0 < legibility_threshold <= 1:
        raise ValueError("legibility_threshold must be in (0, 1]")

    build_by_id = {br.build_run_id: br for br in build_runs}
    # cell_key = (builder_id, build_run_id, task_id, question_id)
    # value = list[(selected_option, presented_correct_key)]
    cells: dict[tuple[str, str, str, str], list[tuple[str | None, str]]] = defaultdict(list)

    for fr in find_runs:
        br = build_by_id.get(fr.build_run_id)
        if br is None:
            continue
        builder_id = br.manifest.agent.id
        for ans in fr.answers:
            cells[(builder_id, fr.build_run_id, fr.task_id, ans.question_id)].append(
                (ans.selected_option, ans.presented_correct_key)
            )

    by_builder: dict[str, dict[str, list]] = defaultdict(
        lambda: {"agreed": 0, "convergent_miss": 0, "n_cells": 0, "events": []}
    )

    for (builder_id, _build_run_id, task_id, qid), picks in cells.items():
        if len(picks) < 2:
            # Need ≥2 finders to talk about agreement at all.
            continue
        bucket = by_builder[builder_id]
        bucket["n_cells"] += 1

        votes = Counter(sel for sel, _correct in picks if sel is not None)
        if not votes:
            continue
        top_option, top_count = votes.most_common(1)[0]
        share = top_count / len(picks)
        correct_key = picks[0][1]  # invariant per cell: same shuffle/correct

        if share >= legibility_threshold:
            bucket["agreed"] += 1
        if top_option != correct_key and share >= consensus_threshold:
            bucket["convergent_miss"] += 1
            bucket["events"].append((task_id, qid, top_option, top_count, len(picks)))

    out: dict[str, LegibilityCell] = {}
    for builder_id, b in by_builder.items():
        n = b["n_cells"]
        out[builder_id] = LegibilityCell(
            builder_id=builder_id,
            n_question_cells=n,
            convergent_miss_cells=b["convergent_miss"],
            convergent_miss_rate=(b["convergent_miss"] / n) if n else math.nan,
            high_agreement_cells=b["agreed"],
            inter_finder_agreement=(b["agreed"] / n) if n else math.nan,
            convergent_miss_events=tuple(sorted(b["events"])),
        )
    return out
