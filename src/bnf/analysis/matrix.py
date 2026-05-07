"""Builder×Finder aggregation."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable

import numpy as np

from bnf.schema.run import BuildRun, FindRun


@dataclass
class Observation:
    task_id: str
    builder_id: str
    build_run_id: str
    finder_id: str
    find_run_id: str
    question_id: str
    is_correct: int  # 0 or 1
    answered: int  # 0 or 1


def iter_observations(
    build_runs: Iterable[BuildRun], find_runs: Iterable[FindRun]
) -> list[Observation]:
    build_by_id = {br.build_run_id: br for br in build_runs}
    out: list[Observation] = []
    for fr in find_runs:
        br = build_by_id.get(fr.build_run_id)
        if br is None:
            continue
        for ans in fr.answers:
            out.append(
                Observation(
                    task_id=fr.task_id,
                    builder_id=br.manifest.agent.id,
                    build_run_id=br.build_run_id,
                    finder_id=fr.manifest.agent.id,
                    find_run_id=fr.find_run_id,
                    question_id=ans.question_id,
                    is_correct=int(ans.is_correct),
                    answered=int(ans.selected_option is not None),
                )
            )
    return out


def build_builder_finder_matrix(
    build_runs: Iterable[BuildRun],
    find_runs: Iterable[FindRun],
    *,
    metric: str = "accuracy",
) -> dict:
    """Return a dict with rows=builders, cols=finders, values=metric.

    Question-level metrics:
    - ``accuracy`` (correct / total)
    - ``answered`` rate

    Run-level metrics:
    - ``retrieval_payload_bytes``
    - ``tool_calls``
    - ``vendor_total_tokens``
    """

    obs = iter_observations(build_runs, find_runs)
    builders = sorted({o.builder_id for o in obs})
    finders = sorted({o.finder_id for o in obs})
    matrix: dict[str, dict[str, float]] = {b: {f: float("nan") for f in finders} for b in builders}
    counts: dict[tuple[str, str], int] = {}
    sums: dict[tuple[str, str], float] = {}

    if metric in {"accuracy", "answered"}:
        for o in obs:
            key = (o.builder_id, o.finder_id)
            counts[key] = counts.get(key, 0) + 1
            val = o.is_correct if metric == "accuracy" else o.answered
            sums[key] = sums.get(key, 0.0) + val
    else:
        build_by_id = {br.build_run_id: br for br in build_runs}
        for fr in find_runs:
            br = build_by_id.get(fr.build_run_id)
            if br is None:
                continue
            value = _find_run_metric(fr, metric)
            if value != value:
                continue
            key = (br.manifest.agent.id, fr.manifest.agent.id)
            counts[key] = counts.get(key, 0) + 1
            sums[key] = sums.get(key, 0.0) + value

    for (b, f), n in counts.items():
        if n > 0:
            matrix[b][f] = sums[(b, f)] / n
    return {
        "metric": metric,
        "builders": builders,
        "finders": finders,
        "values": matrix,
        "n": {f"{b}|{f}": counts[(b, f)] for (b, f) in counts},
    }


#: Metric keys that read from the proxy-sourced Efficiency block
#: (paper §17.3). These are the cross-vendor-comparable axes.
EFFICIENCY_METRICS = (
    "novel_retrieval_bytes",
    "novel_tool_call_count",
    "novel_tool_result_count",
    "wall_clock_seconds",
    "generation_tok_per_s",
)

#: Legacy metric keys kept for backward compatibility with old run
#: records captured before the proxy existed.
LEGACY_METRICS = (
    "retrieval_payload_bytes",
    "tool_calls",
    "vendor_total_tokens",
)


def _find_run_metric(find_run: FindRun, metric: str) -> float:
    eff = find_run.usage.efficiency
    if metric in EFFICIENCY_METRICS:
        if eff is None:
            return float("nan")
        value = getattr(eff, metric)
        return float("nan") if value is None else float(value)
    if metric == "retrieval_payload_bytes":
        return float(find_run.usage.retrieval_payload_bytes)
    if metric == "tool_calls":
        return float(find_run.usage.tool_calls)
    if metric == "vendor_total_tokens":
        if find_run.usage.vendor_usage is None:
            return float("nan")
        return float(find_run.usage.vendor_usage.total_tokens)
    raise ValueError(f"unsupported metric {metric!r}")


def bootstrap_ci(
    samples: list[int],
    *,
    alpha: float = 0.05,
    n_resamples: int = 2000,
    rng_seed: int = 0,
) -> tuple[float, float]:
    """Percentile bootstrap CI for the mean of 0/1 indicators."""

    if not samples:
        return (float("nan"), float("nan"))
    rng = np.random.default_rng(rng_seed)
    arr = np.asarray(samples, dtype=float)
    resamples = rng.choice(arr, size=(n_resamples, arr.size), replace=True)
    means = resamples.mean(axis=1)
    low = float(np.quantile(means, alpha / 2))
    high = float(np.quantile(means, 1 - alpha / 2))
    return (low, high)
