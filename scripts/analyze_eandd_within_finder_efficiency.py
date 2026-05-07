"""Paper-correct within-finder efficiency analysis for the E&D final panel.

The paper's builder-efficiency estimand is not build-token cost and not a
finder's global token cost. It is the effort that the same finder spends when
inspecting different builders' artifacts:

    r_m(b, f, t) = c_m(b, f, t) / min_b' c_m(b', f, t)

where c_m is the mean inspection effort for builder b, finder f, task t. The
primary paper gate averages only perfect find runs; cells without any perfect
run are missing and reported as coverage. The builder headline R_b is the
geometric mean of the contributing within-(finder, task) ratios. This script
regenerates that analysis from the frozen final logical panel selected by
``scripts/analyze_eandd_final.py``.
"""

# ruff: noqa: E402

from __future__ import annotations

import csv
import json
import math
import random
import statistics
import sys
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Literal

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.config import paths
from scripts.analyze_eandd_final import (
    DEFAULT_AGENTS,
    DEFAULT_TASKS,
    _agent_base,
    _agent_effort,
    _agent_family,
    _load_final_builds,
    _load_final_finds,
    _mean,
    _split_csv,
)

Gate = Literal["all_runs", "perfect_run", "max_penalty"]

METRICS = (
    "retrieval_bytes",
    "tool_call_bytes",
    "total_bytes",
    "tool_call_count",
    "turn_count",
    "wall_clock_seconds",
)
BOOTSTRAP_REPS = 2000
BOOTSTRAP_SEED = 7


@dataclass(frozen=True)
class Panel:
    panel_id: str
    description: str
    builder_ids: set[str]
    finder_ids: set[str]
    primary: bool


def main() -> None:
    p = paths()
    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)
    out_dir = p.reports_dir / "eandd_final" / "within_finder_efficiency"
    out_dir.mkdir(parents=True, exist_ok=True)

    builds = _load_final_builds(p.runs_dir, task_ids, agent_ids)
    finds = _load_final_finds(p.runs_dir, builds, agent_ids)
    run_rows = _run_rows(finds)
    panels = _panels(agent_ids)

    cell_rows = []
    builder_score_rows = []
    recovery_rows = []
    rank_rows = []
    for panel in panels:
        panel_rows = [
            row
            for row in run_rows
            if row["builder_id"] in panel.builder_ids and row["finder_id"] in panel.finder_ids
        ]
        for gate in ("all_runs", "perfect_run", "max_penalty"):
            for metric in METRICS:
                means = _cell_means(panel_rows, metric=metric, gate=gate)
                cell_rows.extend(_cell_output_rows(panel.panel_id, gate, metric, means))
                builder_score_rows.extend(_builder_score_rows(panel, gate, metric, means))
                rank_rows.extend(_rank_stability_rows(panel, gate, metric, means))
        for recovery_variant in (
            "soft_full_recovery",
            "perfect_run_rate",
            "answer_accuracy",
        ):
            recovery_rows.extend(_recovery_rows(panel, panel_rows, "retrieval_bytes", recovery_variant))
            recovery_rows.extend(_recovery_rows(panel, panel_rows, "total_bytes", recovery_variant))

    _add_builder_score_bootstrap_cis(builder_score_rows, panels, run_rows)

    _write_csv(out_dir / "within_finder_cells.csv", cell_rows)
    _write_csv(out_dir / "builder_scores.csv", builder_score_rows)
    _write_csv(out_dir / "recovery_scores.csv", recovery_rows)
    _write_csv(out_dir / "rank_stability.csv", rank_rows)
    manifest = {
        "generated_at": _source_snapshot_at(builds, finds),
        "analysis_script": "scripts/analyze_eandd_within_finder_efficiency.py",
        "input_panel": f"{_display_path(p.runs_dir)} compile-pass final logical panel",
        "run_rows": len(run_rows),
        "panels": [
            {
                "panel_id": panel.panel_id,
                "description": panel.description,
                "builders": sorted(panel.builder_ids),
                "finders": sorted(panel.finder_ids),
                "primary": panel.primary,
            }
            for panel in panels
        ],
        "metric_contract": {
            "headline": "total_bytes = novel_retrieval_bytes + novel_tool_call_bytes",
            "normalization": "within each (finder, task), divide each builder cell by the minimum builder cost in the same panel",
            "conditional_gate": "perfect_run: average only find runs whose task accuracy is 1.0; a (builder, finder, task) cell contributes if at least one run is perfect",
            "secondary_gates": {
                "all_runs": "average every run, regardless of accuracy",
                "max_penalty": "use perfect-run mean when available; otherwise assign the max all-run mean cost observed in the same (finder, task)",
            },
            "preferred_recovery": "soft_full_recovery = accuracy ** question_count / inspection bytes, normalized by the maximum builder value within each (finder, task), then geomean across cells",
            "recovery_diagnostics": {
                "perfect_run_rate": "observed perfect-task run rate divided by inspection bytes; strict but high variance with six runs per cell",
                "answer_accuracy": "legacy answer-level accuracy divided by inspection bytes; retained as a diagnostic because it can over-reward low-cost partial recovery",
            },
        },
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(
        out_dir / "within_finder_efficiency_insights.md",
        manifest=manifest,
        builder_score_rows=builder_score_rows,
        recovery_rows=recovery_rows,
        rank_rows=rank_rows,
    )
    print(f"wrote {out_dir / 'within_finder_efficiency_insights.md'}")


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def _source_snapshot_at(builds, finds) -> str:
    timestamps = [
        cell.run.manifest.created_at
        for cell in (*builds, *finds)
        if cell.run.manifest.created_at
    ]
    if timestamps:
        return max(timestamps)
    return datetime.fromtimestamp(0, timezone.utc).isoformat()


def _run_rows(finds) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for cell in finds:
        eff = cell.run.usage.efficiency
        if eff is None:
            raise RuntimeError(f"{cell.run.find_run_id} has no efficiency metrics")
        accuracy = sum(answer.is_correct for answer in cell.run.answers) / len(cell.run.answers)
        rows.append(
            {
                "task_id": cell.task_id,
                "builder_id": cell.builder_id,
                "builder_family": _agent_family(cell.builder_id),
                "builder_base_model": _agent_base(cell.builder_id),
                "builder_effort": _agent_effort(cell.builder_id),
                "builder_trial_id": cell.builder_trial_id,
                "finder_id": cell.finder_id,
                "finder_family": _agent_family(cell.finder_id),
                "finder_base_model": _agent_base(cell.finder_id),
                "finder_effort": _agent_effort(cell.finder_id),
                "finder_trial_id": cell.finder_trial_id,
                "accuracy": accuracy,
                "perfect_run": int(accuracy == 1.0),
                "retrieval_bytes": eff.novel_retrieval_bytes,
                "tool_call_bytes": eff.novel_tool_call_bytes,
                "total_bytes": eff.novel_retrieval_bytes + eff.novel_tool_call_bytes,
                "tool_call_count": eff.novel_tool_call_count,
                "turn_count": eff.turn_count,
                "wall_clock_seconds": eff.wall_clock_seconds,
                "question_count": len(cell.run.answers),
            }
        )
    return rows


def _panels(agent_ids: list[str]) -> list[Panel]:
    high = {agent_id for agent_id in agent_ids if _agent_effort(agent_id) == "high"}
    low = {agent_id for agent_id in agent_ids if _agent_effort(agent_id) == "low"}
    all_ids = set(agent_ids)
    return [
        Panel(
            panel_id="high_effort",
            description="Primary high-effort panel: high builders inspected by high finders",
            builder_ids=high,
            finder_ids=high,
            primary=True,
        ),
        Panel(
            panel_id="low_effort",
            description="Primary low-effort panel: low builders inspected by low finders",
            builder_ids=low,
            finder_ids=low,
            primary=True,
        ),
        Panel(
            panel_id="all_effort_diagnostic",
            description="Diagnostic only: all builders and all finders pooled across effort regimes",
            builder_ids=all_ids,
            finder_ids=all_ids,
            primary=False,
        ),
    ]


def _cell_means(
    rows: list[dict[str, Any]],
    *,
    metric: str,
    gate: Gate,
) -> dict[tuple[str, str, str], dict[str, Any]]:
    all_grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    perfect_grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        key = (row["task_id"], row["builder_id"], row["finder_id"])
        all_grouped[key].append(row)
        if row["perfect_run"]:
            perfect_grouped[key].append(row)

    all_means = {
        key: _metric_mean(group, metric)
        for key, group in all_grouped.items()
        if _metric_mean(group, metric) > 0
    }
    max_by_ft: dict[tuple[str, str], float] = defaultdict(float)
    for (task_id, _builder_id, finder_id), value in all_means.items():
        max_by_ft[(task_id, finder_id)] = max(max_by_ft[(task_id, finder_id)], value)

    out: dict[tuple[str, str, str], dict[str, Any]] = {}
    for key, group in all_grouped.items():
        task_id, _builder_id, finder_id = key
        perfect_group = perfect_grouped.get(key, [])
        if gate == "all_runs":
            contributing = group
            cost = _metric_mean(contributing, metric)
            imputed = 0
        elif gate == "perfect_run":
            contributing = perfect_group
            if not contributing:
                continue
            cost = _metric_mean(contributing, metric)
            imputed = 0
        elif gate == "max_penalty":
            if perfect_group:
                contributing = perfect_group
                cost = _metric_mean(contributing, metric)
                imputed = 0
            else:
                contributing = []
                cost = max_by_ft[(task_id, finder_id)]
                imputed = 1
        else:
            raise ValueError(gate)

        if cost <= 0:
            continue
        out[key] = {
            "task_id": task_id,
            "builder_id": key[1],
            "finder_id": finder_id,
            "cost": cost,
            "metric": metric,
            "gate": gate,
            "imputed_penalty": imputed,
            "n_runs_total": len(group),
            "n_runs_contributing": len(contributing),
            "n_perfect_runs": len(perfect_group),
            "mean_accuracy_all_runs": _mean([row["accuracy"] for row in group]),
            "mean_accuracy_contributing": _mean([row["accuracy"] for row in contributing])
            if contributing
            else math.nan,
        }
    return out


def _metric_mean(rows: list[dict[str, Any]], metric: str) -> float:
    values = [float(row[metric]) for row in rows if row[metric] is not None]
    return _mean(values)


def _cell_output_rows(
    panel_id: str,
    gate: Gate,
    metric: str,
    means: dict[tuple[str, str, str], dict[str, Any]],
) -> list[dict[str, Any]]:
    min_by_ft: dict[tuple[str, str], float] = {}
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in means.values():
        grouped[(row["task_id"], row["finder_id"])].append(row)
    for ft, rows in grouped.items():
        min_by_ft[ft] = min(row["cost"] for row in rows)

    out = []
    for row in means.values():
        min_cost = min_by_ft[(row["task_id"], row["finder_id"])]
        out.append(
            {
                "panel_id": panel_id,
                "gate": gate,
                "metric": metric,
                "task_id": row["task_id"],
                "builder_id": row["builder_id"],
                "finder_id": row["finder_id"],
                "builder_family": _agent_family(row["builder_id"]),
                "finder_family": _agent_family(row["finder_id"]),
                "same_family": int(_agent_family(row["builder_id"]) == _agent_family(row["finder_id"])),
                "same_base_model": int(_agent_base(row["builder_id"]) == _agent_base(row["finder_id"])),
                "cost": row["cost"],
                "min_cost_for_finder_task": min_cost,
                "within_finder_ratio": row["cost"] / min_cost,
                "n_runs_total": row["n_runs_total"],
                "n_runs_contributing": row["n_runs_contributing"],
                "n_perfect_runs": row["n_perfect_runs"],
                "mean_accuracy_all_runs": row["mean_accuracy_all_runs"],
                "imputed_penalty": row["imputed_penalty"],
            }
        )
    return sorted(out, key=lambda r: (r["panel_id"], r["gate"], r["metric"], r["task_id"], r["finder_id"], r["builder_id"]))


def _builder_score_rows(
    panel: Panel,
    gate: Gate,
    metric: str,
    means: dict[tuple[str, str, str], dict[str, Any]],
) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in means.values():
        grouped[(row["task_id"], row["finder_id"])].append(row)

    ratios: dict[str, list[float]] = defaultdict(list)
    accuracies: dict[str, list[float]] = defaultdict(list)
    penalties: dict[str, int] = defaultdict(int)
    for rows in grouped.values():
        min_cost = min(row["cost"] for row in rows)
        if min_cost <= 0:
            continue
        for row in rows:
            ratios[row["builder_id"]].append(row["cost"] / min_cost)
            accuracies[row["builder_id"]].append(row["mean_accuracy_all_runs"])
            penalties[row["builder_id"]] += int(row["imputed_penalty"])

    max_cells = len(panel.finder_ids) * len({row["task_id"] for row in means.values()})
    scored = []
    for builder_id, values in ratios.items():
        score = _geomean(values)
        scored.append(
            {
                "panel_id": panel.panel_id,
                "primary_panel": int(panel.primary),
                "gate": gate,
                "metric": metric,
                "builder_id": builder_id,
                "family": _agent_family(builder_id),
                "base_model": _agent_base(builder_id),
                "effort": _agent_effort(builder_id),
                "score": score,
                "n_cells": len(values),
                "max_cells": max_cells,
                "cell_coverage": len(values) / max_cells if max_cells else math.nan,
                "n_penalty_cells": penalties[builder_id],
                "mean_cell_accuracy": _mean(accuracies[builder_id]),
                "ci95_low": math.nan,
                "ci95_high": math.nan,
            }
        )

    scored.sort(key=lambda row: (row["score"], -row["n_cells"], row["builder_id"]))
    for rank, row in enumerate(scored, start=1):
        row["rank"] = rank
    return scored


def _add_builder_score_bootstrap_cis(
    builder_score_rows: list[dict[str, Any]],
    panels: list[Panel],
    run_rows: list[dict[str, Any]],
) -> None:
    """Attach clustered bootstrap CIs for the primary total-byte R_b rows."""

    by_key = {
        (row["panel_id"], row["gate"], row["metric"], row["builder_id"]): row
        for row in builder_score_rows
    }
    for panel in panels:
        if not panel.primary:
            continue
        panel_rows = [
            row
            for row in run_rows
            if row["builder_id"] in panel.builder_ids and row["finder_id"] in panel.finder_ids
        ]
        intervals = _bootstrap_builder_scores(
            panel,
            panel_rows,
            gate="perfect_run",
            metric="total_bytes",
        )
        for builder_id, (lo, hi) in intervals.items():
            row = by_key.get((panel.panel_id, "perfect_run", "total_bytes", builder_id))
            if row is None:
                continue
            row["ci95_low"] = lo
            row["ci95_high"] = hi


def _bootstrap_builder_scores(
    panel: Panel,
    rows: list[dict[str, Any]],
    *,
    gate: Gate,
    metric: str,
) -> dict[str, tuple[float, float]]:
    rng = random.Random(BOOTSTRAP_SEED)
    clusters: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        clusters[(row["finder_id"], row["task_id"])].append(row)
    cluster_keys = sorted(clusters)
    values: dict[str, list[float]] = defaultdict(list)
    for _ in range(BOOTSTRAP_REPS):
        sampled: list[dict[str, Any]] = []
        for _cluster in cluster_keys:
            sampled.extend(clusters[rng.choice(cluster_keys)])
        means = _cell_means(sampled, metric=metric, gate=gate)
        for score_row in _builder_score_rows(panel, gate, metric, means):
            values[score_row["builder_id"]].append(score_row["score"])
    return {
        builder_id: (_percentile(samples, 0.025), _percentile(samples, 0.975))
        for builder_id, samples in values.items()
        if samples
    }


def _percentile(values: list[float], q: float) -> float:
    ordered = sorted(values)
    if not ordered:
        return math.nan
    index = min(len(ordered) - 1, max(0, int(q * len(ordered))))
    return ordered[index]


def _recovery_rows(
    panel: Panel,
    rows: list[dict[str, Any]],
    effort_metric: str,
    recovery_variant: str,
) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[(row["task_id"], row["builder_id"], row["finder_id"])].append(row)

    cell_scores = []
    for (task_id, builder_id, finder_id), group in grouped.items():
        effort = _metric_mean(group, effort_metric)
        if effort <= 0:
            continue
        mean_accuracy = _mean([row["accuracy"] for row in group])
        question_count = int(round(_mean([row["question_count"] for row in group])))
        perfect_run_rate = _mean([row["perfect_run"] for row in group])
        if recovery_variant == "soft_full_recovery":
            success_signal = mean_accuracy**question_count
        elif recovery_variant == "perfect_run_rate":
            success_signal = perfect_run_rate
        elif recovery_variant == "answer_accuracy":
            success_signal = mean_accuracy
        else:
            raise ValueError(recovery_variant)
        cell_scores.append(
            {
                "task_id": task_id,
                "builder_id": builder_id,
                "finder_id": finder_id,
                "recovery": success_signal / effort,
                "success_signal": success_signal,
                "mean_accuracy": mean_accuracy,
                "perfect_run_rate": perfect_run_rate,
                "question_count": question_count,
                "mean_effort": effort,
            }
        )

    max_by_ft: dict[tuple[str, str], float] = defaultdict(float)
    for row in cell_scores:
        ft = (row["task_id"], row["finder_id"])
        max_by_ft[ft] = max(max_by_ft[ft], row["recovery"])

    normalized: dict[str, list[float]] = defaultdict(list)
    accuracies: dict[str, list[float]] = defaultdict(list)
    efforts: dict[str, list[float]] = defaultdict(list)
    success_signals: dict[str, list[float]] = defaultdict(list)
    perfect_rates: dict[str, list[float]] = defaultdict(list)
    zero_cells: dict[str, int] = defaultdict(int)
    for row in cell_scores:
        best = max_by_ft[(row["task_id"], row["finder_id"])]
        if best <= 0:
            continue
        builder_id = row["builder_id"]
        normalized_score = row["recovery"] / best
        if normalized_score <= 0:
            zero_cells[builder_id] += 1
            normalized_score = 1e-9
        normalized[builder_id].append(normalized_score)
        accuracies[builder_id].append(row["mean_accuracy"])
        efforts[builder_id].append(row["mean_effort"])
        success_signals[builder_id].append(row["success_signal"])
        perfect_rates[builder_id].append(row["perfect_run_rate"])

    out = []
    max_cells = len(panel.finder_ids) * len({row["task_id"] for row in rows})
    for builder_id, values in normalized.items():
        out.append(
            {
                "panel_id": panel.panel_id,
                "primary_panel": int(panel.primary),
                "effort_metric": effort_metric,
                "recovery_variant": recovery_variant,
                "primary_recovery": int(recovery_variant == "soft_full_recovery"),
                "builder_id": builder_id,
                "family": _agent_family(builder_id),
                "base_model": _agent_base(builder_id),
                "effort": _agent_effort(builder_id),
                "score": _geomean(values),
                "n_cells": len(values),
                "max_cells": max_cells,
                "cell_coverage": len(values) / max_cells if max_cells else math.nan,
                "mean_cell_accuracy": _mean(accuracies[builder_id]),
                "mean_success_signal": _mean(success_signals[builder_id]),
                "mean_perfect_run_rate": _mean(perfect_rates[builder_id]),
                "mean_cell_effort": _mean(efforts[builder_id]),
                "n_zero_recovery_cells": zero_cells[builder_id],
            }
        )
    out.sort(key=lambda row: (-row["score"], row["builder_id"]))
    for rank, row in enumerate(out, start=1):
        row["rank"] = rank
    return out


def _rank_stability_rows(
    panel: Panel,
    gate: Gate,
    metric: str,
    means: dict[tuple[str, str, str], dict[str, Any]],
) -> list[dict[str, Any]]:
    ranks: dict[tuple[str, str], dict[str, int]] = {}
    costs: dict[tuple[str, str], dict[str, float]] = defaultdict(dict)
    for (task_id, builder_id, finder_id), row in means.items():
        costs[(task_id, finder_id)][builder_id] = row["cost"]
    for tf, builder_costs in costs.items():
        ordered = sorted(builder_costs.items(), key=lambda item: (item[1], item[0]))
        ranks[tf] = {builder_id: rank for rank, (builder_id, _cost) in enumerate(ordered, start=1)}

    rows = []
    task_ids = sorted({task_id for task_id, _finder_id in ranks})
    for task_id in task_ids:
        finder_ids = sorted(finder_id for t, finder_id in ranks if t == task_id)
        pair_rows = []
        for i, finder_a in enumerate(finder_ids):
            for finder_b in finder_ids[i + 1 :]:
                tau = _kendall_tau_a(ranks[(task_id, finder_a)], ranks[(task_id, finder_b)])
                if math.isnan(tau):
                    continue
                pair_rows.append(
                    {
                        "finder_a": finder_a,
                        "finder_b": finder_b,
                        "tau": tau,
                        "same_family": _agent_family(finder_a) == _agent_family(finder_b),
                    }
                )
        for slice_id, selected in (
            ("all_pairs", pair_rows),
            ("same_family_pairs", [row for row in pair_rows if row["same_family"]]),
            ("cross_family_pairs", [row for row in pair_rows if not row["same_family"]]),
        ):
            rows.append(
                {
                    "panel_id": panel.panel_id,
                    "primary_panel": int(panel.primary),
                    "gate": gate,
                    "metric": metric,
                    "task_id": task_id,
                    "slice_id": slice_id,
                    "n_pairs": len(selected),
                    "median_tau": statistics.median([row["tau"] for row in selected])
                    if selected
                    else math.nan,
                    "mean_tau": _mean([row["tau"] for row in selected]) if selected else math.nan,
                    "positive_fraction": _mean([int(row["tau"] > 0) for row in selected])
                    if selected
                    else math.nan,
                }
            )
    return rows


def _kendall_tau_a(a_ranks: dict[str, int], b_ranks: dict[str, int]) -> float:
    common = sorted(set(a_ranks) & set(b_ranks))
    if len(common) < 2:
        return math.nan
    concordant = 0
    discordant = 0
    for i, x in enumerate(common):
        for y in common[i + 1 :]:
            da = a_ranks[x] - a_ranks[y]
            db = b_ranks[x] - b_ranks[y]
            if da * db > 0:
                concordant += 1
            elif da * db < 0:
                discordant += 1
    denom = concordant + discordant
    return (concordant - discordant) / denom if denom else math.nan


def _geomean(values: list[float]) -> float:
    positive = [value for value in values if value > 0]
    if not positive:
        return math.nan
    return math.exp(sum(math.log(value) for value in positive) / len(positive))


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        path.write_text("", encoding="utf-8")
        return
    fieldnames = list(rows[0].keys())
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def _fmt(value: float, digits: int = 3) -> str:
    if value is None or math.isnan(value):
        return ""
    return f"{value:.{digits}f}"


def _write_markdown(
    path: Path,
    *,
    manifest: dict[str, Any],
    builder_score_rows: list[dict[str, Any]],
    recovery_rows: list[dict[str, Any]],
    rank_rows: list[dict[str, Any]],
) -> None:
    lines = [
        "# Within-Finder Efficiency Analysis",
        "",
        f"Generated: {manifest['generated_at']}",
        "",
        "This report uses the paper estimand: builder efficiency is the same finder-task",
        "relative inspection effort over builders. It does not use build tokens or",
        "finder-global token consumption as the builder efficiency metric.",
        "",
        "## Primary Builder R_b",
        "",
        "Lower is better. `n` counts contributing `(finder, task)` cells; max is 12 in each primary panel.",
        "",
    ]
    for panel_id in ("high_effort", "low_effort"):
        rows = [
            row
            for row in builder_score_rows
            if row["panel_id"] == panel_id
            and row["gate"] == "perfect_run"
            and row["metric"] == "total_bytes"
        ]
        lines.extend(
            [
                f"### {panel_id}",
                "",
                "| rank | builder | R_total_bytes | n | coverage | mean cell acc |",
                "| ---: | --- | ---: | ---: | ---: | ---: |",
            ]
        )
        for row in rows:
            lines.append(
                f"| {row['rank']} | {row['builder_id']} | {_fmt(row['score'])} | {row['n_cells']}/{row['max_cells']} | {_fmt(row['cell_coverage'])} | {_fmt(row['mean_cell_accuracy'])} |"
            )
        lines.append("")

    lines.extend(
        [
            "## Metric Cross-Checks",
            "",
            "All-correct-cell R_b under component metrics.",
            "",
            "| panel | metric | rank 1 | score | rank 2 | score |",
            "| --- | --- | --- | ---: | --- | ---: |",
        ]
    )
    for panel_id in ("high_effort", "low_effort"):
        for metric in METRICS:
            rows = [
                row
                for row in builder_score_rows
                if row["panel_id"] == panel_id and row["gate"] == "perfect_run" and row["metric"] == metric
            ]
            rows = sorted(rows, key=lambda row: row["rank"])
            if not rows:
                continue
            second = rows[1] if len(rows) > 1 else None
            lines.append(
                f"| {panel_id} | {metric} | {rows[0]['builder_id']} | {_fmt(rows[0]['score'])} | {second['builder_id'] if second else ''} | {_fmt(second['score']) if second else ''} |"
            )

    lines.extend(
        [
            "",
            "## Full-Task Recovery Diagnostic",
            "",
            "Higher is better. Preferred recovery uses `accuracy^question_count / effort`,",
            "normalized within each `(finder, task)` by the maximum builder value. This is a",
            "diagnostic approximation rather than a calibrated probability; it penalizes",
            "low per-question accuracy at the task level instead of linearly trading",
            "partial answer accuracy against retrieval cost.",
            "",
        ]
    )
    for panel_id in ("high_effort", "low_effort"):
        rows = [
            row
            for row in recovery_rows
            if row["panel_id"] == panel_id
            and row["effort_metric"] == "retrieval_bytes"
            and row["recovery_variant"] == "soft_full_recovery"
        ]
        lines.extend(
            [
                f"### {panel_id}",
                "",
                "| rank | builder | recovery | n | mean acc | mean full-recovery signal | mean retrieval bytes |",
                "| ---: | --- | ---: | ---: | ---: | ---: | ---: |",
            ]
        )
        for row in rows:
            lines.append(
                f"| {row['rank']} | {row['builder_id']} | {_fmt(row['score'])} | {row['n_cells']}/{row['max_cells']} | {_fmt(row['mean_cell_accuracy'])} | {_fmt(row['mean_success_signal'])} | {row['mean_cell_effort']:.0f} |"
            )
        lines.append("")

    lines.extend(
        [
            "## Recovery Cross-Checks",
            "",
            "| panel | variant | effort | rank 1 | score | rank 2 | score |",
            "| --- | --- | --- | --- | ---: | --- | ---: |",
        ]
    )
    for panel_id in ("high_effort", "low_effort"):
        for variant in ("soft_full_recovery", "perfect_run_rate", "answer_accuracy"):
            variant_label = {
                "soft_full_recovery": "full_task_recovery",
                "perfect_run_rate": "all_correct_rate",
                "answer_accuracy": "answer_accuracy",
            }[variant]
            for effort_metric in ("retrieval_bytes", "total_bytes"):
                rows = [
                    row
                    for row in recovery_rows
                    if row["panel_id"] == panel_id
                    and row["effort_metric"] == effort_metric
                    and row["recovery_variant"] == variant
                ]
                rows = sorted(rows, key=lambda row: row["rank"])
                if not rows:
                    continue
                second = rows[1] if len(rows) > 1 else None
                lines.append(
                    f"| {panel_id} | {variant_label} | {effort_metric} | {rows[0]['builder_id']} | {_fmt(rows[0]['score'])} | {second['builder_id'] if second else ''} | {_fmt(second['score']) if second else ''} |"
                )

    lines.extend(
        [
            "## Rank Stability",
            "",
            "Kendall tau across finder pairs on builder cost order, using total bytes on all-correct cells.",
            "",
            "| panel | task | slice | n pairs | median tau | positive fraction |",
            "| --- | --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in rank_rows:
        if row["panel_id"] not in {"high_effort", "low_effort"}:
            continue
        if row["gate"] != "perfect_run" or row["metric"] != "total_bytes":
            continue
        lines.append(
            f"| {row['panel_id']} | {row['task_id']} | {row['slice_id']} | {row['n_pairs']} | {_fmt(row['median_tau'])} | {_fmt(row['positive_fraction'])} |"
        )

    lines.extend(
        [
            "",
            "## Files",
            "",
            "- `within_finder_cells.csv`",
            "- `builder_scores.csv`",
            "- `recovery_scores.csv`",
            "- `rank_stability.csv`",
            "- `manifest.json`",
        ]
    )
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
