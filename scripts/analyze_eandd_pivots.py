"""Exploratory pivot analysis for the E&D final logical panel.

This script reads the same final logical panel as ``analyze_eandd_final``
and creates robustness views that separate builder quality, finder quality,
family affinity, effort, and question-prior effects.
"""

# ruff: noqa: E402

from __future__ import annotations

import csv
import json
import math
import sys
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

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
    _answer_rows,
    _load_control_finds,
    _load_final_builds,
    _load_final_finds,
    _mean,
    _stability_tables,
    _split_csv,
    _usage_total_tokens,
)


@dataclass(frozen=True)
class View:
    role: str
    view_id: str
    description: str
    rows: list[dict[str, Any]]


def main() -> None:
    p = paths()
    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)
    out_dir = p.reports_dir / "eandd_final" / "pivots"
    out_dir.mkdir(parents=True, exist_ok=True)

    builds = _load_final_builds(p.runs_dir, task_ids, agent_ids)
    finds = _load_final_finds(p.runs_dir, builds, agent_ids)
    controls = _load_control_finds(p.runs_dir, task_ids, agent_ids)

    formal_rows = _answer_rows(finds)
    control_rows = _answer_rows(controls)
    _stability_summary, question_stability = _stability_tables(finds, controls)
    question_summary = _question_summary(formal_rows, control_rows, question_stability)

    baseline_builders = _summarize_role(formal_rows, "builder_id")
    baseline_finders = _summarize_role(formal_rows, "finder_id")
    top4_builders = _top_ids(baseline_builders, 4)
    top6_builders = _top_ids(baseline_builders, 6)
    top4_finders = _top_ids(baseline_finders, 4)
    top6_finders = _top_ids(baseline_finders, 6)
    positive_lift_finders = _positive_lift_finders(formal_rows, control_rows)
    implementation_signal_questions = {
        key for key, row in question_summary.items() if row["formal_minus_question_only_stability"] >= 0.05
    }
    low_prior_questions = {
        key for key, row in question_summary.items() if row["question_only_stable_cell_rate"] < 0.90
    }
    hard_formal_questions = {key for key, row in question_summary.items() if row["formal_accuracy"] <= 0.90}

    views = _build_views(
        formal_rows,
        top4_builders=top4_builders,
        top6_builders=top6_builders,
        top4_finders=top4_finders,
        top6_finders=top6_finders,
        positive_lift_finders=positive_lift_finders,
        implementation_signal_questions=implementation_signal_questions,
        low_prior_questions=low_prior_questions,
        hard_formal_questions=hard_formal_questions,
    )
    agent_rows = _agent_view_rows(views)
    effort_rows = _effort_rows(agent_rows)
    agent_token_rows = _agent_token_diagnostic_rows(agent_rows, builds, finds)
    affinity_rows = _affinity_rows(formal_rows)
    family_rows = _family_pair_rows(formal_rows)
    task_agent_rows = _task_agent_rows(formal_rows)

    _write_csv(out_dir / "agent_views.csv", agent_rows)
    _write_csv(out_dir / "effort_comparisons.csv", effort_rows)
    _write_csv(out_dir / "agent_token_diagnostics.csv", agent_token_rows)
    _write_csv(out_dir / "affinity_by_agent.csv", affinity_rows)
    _write_csv(out_dir / "family_pair_matrix.csv", family_rows)
    _write_csv(out_dir / "task_agent_views.csv", task_agent_rows)
    _write_csv(out_dir / "question_signal.csv", list(question_summary.values()))
    manifest = {
        "generated_at": _source_snapshot_at(builds, finds, controls),
        "analysis_script": "scripts/analyze_eandd_pivots.py",
        "input_panel": f"{_display_path(p.reports_dir / 'eandd_final')} compile-pass logical panel regenerated from raw JSON",
        "top4_builders": top4_builders,
        "top6_builders": top6_builders,
        "top4_finders": top4_finders,
        "top6_finders": top6_finders,
        "positive_lift_finders": positive_lift_finders,
        "implementation_signal_questions": sorted([f"{t}:{q}" for t, q in implementation_signal_questions]),
        "low_prior_questions": sorted([f"{t}:{q}" for t, q in low_prior_questions]),
        "hard_formal_questions": sorted([f"{t}:{q}" for t, q in hard_formal_questions]),
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(
        out_dir / "pivot_insights.md",
        manifest=manifest,
        agent_rows=agent_rows,
        effort_rows=effort_rows,
        agent_token_rows=agent_token_rows,
        affinity_rows=affinity_rows,
        family_rows=family_rows,
        question_summary=question_summary,
    )
    print(f"wrote {out_dir / 'pivot_insights.md'}")


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def _source_snapshot_at(builds, finds, controls) -> str:
    timestamps = [
        cell.run.manifest.created_at
        for cell in (*builds, *finds, *controls)
        if cell.run.manifest.created_at
    ]
    if timestamps:
        return max(timestamps)
    return datetime.fromtimestamp(0, timezone.utc).isoformat()


def _question_summary(
    formal_rows: list[dict[str, Any]],
    control_rows: list[dict[str, Any]],
    question_stability: list[dict[str, Any]],
) -> dict[tuple[str, str], dict[str, Any]]:
    formal: dict[tuple[str, str], list[int]] = defaultdict(list)
    control: dict[tuple[str, str, str], list[int]] = defaultdict(list)
    for row in formal_rows:
        formal[(row["task_id"], row["question_id"])].append(row["is_correct"])
    for row in control_rows:
        control[(row["condition"], row["task_id"], row["question_id"])].append(row["is_correct"])
    stability = {
        (row["condition"], row["task_id"], row["question_id"]): row
        for row in question_stability
    }
    out = {}
    for key, values in formal.items():
        task_id, question_id = key
        formal_acc = _mean(values)
        qonly = _mean(control[("question_only", task_id, question_id)])
        spec = _mean(control[("spec_only", task_id, question_id)])
        formal_stability = stability[("with_codebase", task_id, question_id)]["stable_cell_rate"]
        qonly_stability = stability[("question_only", task_id, question_id)]["stable_cell_rate"]
        spec_stability = stability[("spec_only", task_id, question_id)]["stable_cell_rate"]
        out[key] = {
            "task_id": task_id,
            "question_id": question_id,
            "formal_accuracy": formal_acc,
            "spec_only_accuracy": spec,
            "question_only_accuracy": qonly,
            "formal_stable_cell_rate": formal_stability,
            "spec_only_stable_cell_rate": spec_stability,
            "question_only_stable_cell_rate": qonly_stability,
            "formal_minus_question_only_stability": formal_stability - qonly_stability,
            "formal_minus_spec_only_stability": formal_stability - spec_stability,
            "formal_minus_question_only": formal_acc - qonly,
            "formal_minus_spec_only": formal_acc - spec,
            "formal_question_rows": len(values),
        }
    return out


def _summarize_role(rows: list[dict[str, Any]], role_key: str) -> list[dict[str, Any]]:
    grouped: dict[str, list[int]] = defaultdict(list)
    for row in rows:
        grouped[row[role_key]].append(row["is_correct"])
    return sorted(
        [
            {
                "agent_id": agent_id,
                "accuracy": _mean(values),
                "question_rows": len(values),
            }
            for agent_id, values in grouped.items()
        ],
        key=lambda row: row["accuracy"],
        reverse=True,
    )


def _top_ids(rows: list[dict[str, Any]], n: int) -> list[str]:
    return [row["agent_id"] for row in rows[:n]]


def _positive_lift_finders(
    formal_rows: list[dict[str, Any]],
    control_rows: list[dict[str, Any]],
) -> list[str]:
    formal = _summarize_role(formal_rows, "finder_id")
    qonly = _control_accuracy_by_finder(control_rows, "question_only")
    ids = [
        row["agent_id"]
        for row in formal
        if row["accuracy"] - qonly.get(row["agent_id"], math.nan) > 0
    ]
    return ids


def _control_accuracy_by_finder(
    control_rows: list[dict[str, Any]],
    condition: str,
) -> dict[str, float]:
    grouped: dict[str, list[int]] = defaultdict(list)
    for row in control_rows:
        if row["condition"] == condition:
            grouped[row["finder_id"]].append(row["is_correct"])
    return {finder_id: _mean(values) for finder_id, values in grouped.items()}


def _build_views(
    rows: list[dict[str, Any]],
    *,
    top4_builders: list[str],
    top6_builders: list[str],
    top4_finders: list[str],
    top6_finders: list[str],
    positive_lift_finders: list[str],
    implementation_signal_questions: set[tuple[str, str]],
    low_prior_questions: set[tuple[str, str]],
    hard_formal_questions: set[tuple[str, str]],
) -> list[View]:
    return [
        View("builder", "all_finders", "All finders judging each builder", rows),
        View(
            "builder",
            "top4_formal_finders",
            "Only the top four formal-accuracy finders judge builders",
            [row for row in rows if row["finder_id"] in top4_finders],
        ),
        View(
            "builder",
            "top6_formal_finders",
            "Only the top six formal-accuracy finders judge builders",
            [row for row in rows if row["finder_id"] in top6_finders],
        ),
        View(
            "builder",
            "positive_lift_finders",
            "Only finders whose formal accuracy exceeds question-only control",
            [row for row in rows if row["finder_id"] in positive_lift_finders],
        ),
        View(
            "builder",
            "cross_family_finders",
            "Only cross-family finder judgments",
            [row for row in rows if _agent_family(row["builder_id"]) != _agent_family(row["finder_id"])],
        ),
        View(
            "builder",
            "cross_base_finders",
            "Only judgments where builder and finder are not the same base model",
            [row for row in rows if _agent_base(row["builder_id"]) != _agent_base(row["finder_id"])],
        ),
        View(
            "builder",
            "implementation_signal_questions",
            "Questions where formal three-trial agreement exceeds question-only by >= 0.05",
            [
                row
                for row in rows
                if (row["task_id"], row["question_id"]) in implementation_signal_questions
            ],
        ),
        View(
            "builder",
            "low_prior_questions",
            "Questions with question-only three-trial agreement < 0.90",
            [row for row in rows if (row["task_id"], row["question_id"]) in low_prior_questions],
        ),
        View(
            "builder",
            "hard_formal_questions",
            "Questions with formal accuracy <= 0.90",
            [row for row in rows if (row["task_id"], row["question_id"]) in hard_formal_questions],
        ),
        View("finder", "all_builders", "All builders judged by each finder", rows),
        View(
            "finder",
            "top4_builders",
            "Finder quality on top four builders",
            [row for row in rows if row["builder_id"] in top4_builders],
        ),
        View(
            "finder",
            "top6_builders",
            "Finder quality on top six builders",
            [row for row in rows if row["builder_id"] in top6_builders],
        ),
        View(
            "finder",
            "cross_family_builders",
            "Only cross-family builder inputs",
            [row for row in rows if _agent_family(row["builder_id"]) != _agent_family(row["finder_id"])],
        ),
        View(
            "finder",
            "cross_base_builders",
            "Only inputs where builder and finder are not the same base model",
            [row for row in rows if _agent_base(row["builder_id"]) != _agent_base(row["finder_id"])],
        ),
        View(
            "finder",
            "implementation_signal_questions",
            "Questions where formal three-trial agreement exceeds question-only by >= 0.05",
            [
                row
                for row in rows
                if (row["task_id"], row["question_id"]) in implementation_signal_questions
            ],
        ),
        View(
            "finder",
            "low_prior_questions",
            "Questions with question-only three-trial agreement < 0.90",
            [row for row in rows if (row["task_id"], row["question_id"]) in low_prior_questions],
        ),
        View(
            "finder",
            "hard_formal_questions",
            "Questions with formal accuracy <= 0.90",
            [row for row in rows if (row["task_id"], row["question_id"]) in hard_formal_questions],
        ),
    ]


def _agent_view_rows(views: list[View]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    role_key = {"builder": "builder_id", "finder": "finder_id"}
    for view in views:
        grouped: dict[str, list[int]] = defaultdict(list)
        for row in view.rows:
            grouped[row[role_key[view.role]]].append(row["is_correct"])
        ranked = sorted(grouped.items(), key=lambda item: _mean(item[1]), reverse=True)
        for rank, (agent_id, values) in enumerate(ranked, start=1):
            out.append(
                {
                    "role": view.role,
                    "view_id": view.view_id,
                    "description": view.description,
                    "rank": rank,
                    "agent_id": agent_id,
                    "family": _agent_family(agent_id),
                    "base_model": _agent_base(agent_id),
                    "effort": _agent_effort(agent_id),
                    "accuracy": _mean(values),
                    "question_rows": len(values),
                }
            )
    return out


def _effort_rows(agent_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, str], dict[str, dict[str, Any]]] = defaultdict(dict)
    for row in agent_rows:
        grouped[(row["role"], row["view_id"], row["base_model"])][row["effort"]] = row
    out = []
    for (role, view_id, base_model), variants in sorted(grouped.items()):
        if "high" not in variants or "low" not in variants:
            continue
        high = variants["high"]
        low = variants["low"]
        out.append(
            {
                "role": role,
                "view_id": view_id,
                "base_model": base_model,
                "family": high["family"],
                "high_accuracy": high["accuracy"],
                "low_accuracy": low["accuracy"],
                "high_minus_low": high["accuracy"] - low["accuracy"],
                "supports_high_effort": int(high["accuracy"] > low["accuracy"]),
            }
        )
    return out


def _agent_token_diagnostic_rows(
    agent_rows: list[dict[str, Any]],
    builds,
    finds,
) -> list[dict[str, Any]]:
    """Diagnostic accuracy/token frontier, not the paper efficiency metric.

    The paper's builder-efficiency estimand is within-finder relative
    inspection effort over builders. These token rows are retained only as
    operational diagnostics for run cost and model-use planning.
    """

    build_tokens: dict[str, list[int]] = defaultdict(list)
    find_tokens: dict[str, list[int]] = defaultdict(list)
    for cell in builds:
        build_tokens[cell.builder_id].append(_usage_total_tokens(cell.run))
    for cell in finds:
        find_tokens[cell.finder_id].append(_usage_total_tokens(cell.run))

    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in agent_rows:
        token_values = build_tokens[row["agent_id"]] if row["role"] == "builder" else find_tokens[row["agent_id"]]
        positive_tokens = [value for value in token_values if value > 0]
        mean_tokens = _mean(positive_tokens)
        enriched = {
            **row,
            "mean_tokens": mean_tokens,
            "accuracy_per_million_tokens": row["accuracy"] / (mean_tokens / 1_000_000)
            if mean_tokens > 0
            else math.nan,
        }
        grouped[(row["role"], row["view_id"])].append(enriched)

    out: list[dict[str, Any]] = []
    for (role, view_id), rows in grouped.items():
        frontier = {id(row) for row in _pareto_frontier(rows)}
        best_accuracy = max(row["accuracy"] for row in rows)
        least_cost_within = {
            threshold: _least_cost_within(rows, best_accuracy, threshold)
            for threshold in (0.005, 0.01, 0.02)
        }
        for row in rows:
            out.append(
                {
                    **row,
                    "pareto_frontier": int(id(row) in frontier),
                    "within_0_005_least_cost": int(row is least_cost_within[0.005]),
                    "within_0_010_least_cost": int(row is least_cost_within[0.01]),
                    "within_0_020_least_cost": int(row is least_cost_within[0.02]),
                }
            )
    return sorted(out, key=lambda row: (row["role"], row["view_id"], row["rank"]))


def _pareto_frontier(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    frontier = []
    for row in rows:
        dominated = False
        for other in rows:
            if other is row:
                continue
            if (
                other["accuracy"] >= row["accuracy"]
                and other["mean_tokens"] <= row["mean_tokens"]
                and (
                    other["accuracy"] > row["accuracy"]
                    or other["mean_tokens"] < row["mean_tokens"]
                )
            ):
                dominated = True
                break
        if not dominated:
            frontier.append(row)
    return frontier


def _least_cost_within(
    rows: list[dict[str, Any]],
    best_accuracy: float,
    threshold: float,
) -> dict[str, Any]:
    candidates = [row for row in rows if row["accuracy"] >= best_accuracy - threshold]
    return min(candidates, key=lambda row: row["mean_tokens"])


def _affinity_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for role, role_key, other_key in (
        ("builder", "builder_id", "finder_id"),
        ("finder", "finder_id", "builder_id"),
    ):
        grouped: dict[str, dict[str, list[int]]] = defaultdict(lambda: defaultdict(list))
        for row in rows:
            agent_id = row[role_key]
            same_family = _agent_family(row[role_key]) == _agent_family(row[other_key])
            same_base = _agent_base(row[role_key]) == _agent_base(row[other_key])
            grouped[agent_id]["same_family" if same_family else "different_family"].append(
                row["is_correct"]
            )
            grouped[agent_id]["same_base" if same_base else "different_base"].append(row["is_correct"])
        for agent_id, values in grouped.items():
            same_family_acc = _mean(values["same_family"])
            different_family_acc = _mean(values["different_family"])
            same_base_acc = _mean(values["same_base"])
            different_base_acc = _mean(values["different_base"])
            out.append(
                {
                    "role": role,
                    "agent_id": agent_id,
                    "family": _agent_family(agent_id),
                    "base_model": _agent_base(agent_id),
                    "effort": _agent_effort(agent_id),
                    "same_family_accuracy": same_family_acc,
                    "different_family_accuracy": different_family_acc,
                    "same_family_minus_different_family": same_family_acc - different_family_acc,
                    "same_base_accuracy": same_base_acc,
                    "different_base_accuracy": different_base_acc,
                    "same_base_minus_different_base": same_base_acc - different_base_acc,
                }
            )
    return sorted(out, key=lambda row: (row["role"], row["agent_id"]))


def _family_pair_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], list[int]] = defaultdict(list)
    for row in rows:
        grouped[(_agent_family(row["builder_id"]), _agent_family(row["finder_id"]))].append(
            row["is_correct"]
        )
    return [
        {
            "builder_family": key[0],
            "finder_family": key[1],
            "accuracy": _mean(values),
            "question_rows": len(values),
        }
        for key, values in sorted(grouped.items())
    ]


def _task_agent_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for role, role_key in (("builder", "builder_id"), ("finder", "finder_id")):
        grouped: dict[tuple[str, str], list[int]] = defaultdict(list)
        for row in rows:
            grouped[(row["task_id"], row[role_key])].append(row["is_correct"])
        for (task_id, agent_id), values in sorted(grouped.items()):
            out.append(
                {
                    "role": role,
                    "task_id": task_id,
                    "agent_id": agent_id,
                    "family": _agent_family(agent_id),
                    "base_model": _agent_base(agent_id),
                    "effort": _agent_effort(agent_id),
                    "accuracy": _mean(values),
                    "question_rows": len(values),
                }
            )
    return out


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        path.write_text("", encoding="utf-8")
        return
    fieldnames = list(rows[0].keys())
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def _view_rows(agent_rows: list[dict[str, Any]], role: str, view_id: str) -> list[dict[str, Any]]:
    return [row for row in agent_rows if row["role"] == role and row["view_id"] == view_id]


def _fmt(value: float, digits: int = 3) -> str:
    if value is None or math.isnan(value):
        return ""
    return f"{value:.{digits}f}"


def _write_markdown(
    path: Path,
    *,
    manifest: dict[str, Any],
    agent_rows: list[dict[str, Any]],
    effort_rows: list[dict[str, Any]],
    agent_token_rows: list[dict[str, Any]],
    affinity_rows: list[dict[str, Any]],
    family_rows: list[dict[str, Any]],
    question_summary: dict[tuple[str, str], dict[str, Any]],
) -> None:
    lines = [
        "# E&D Final Pivot Analysis",
        "",
        f"Generated: {manifest['generated_at']}",
        "",
        "## View Sets",
        "",
        f"- Top 4 builders: {', '.join(manifest['top4_builders'])}",
        f"- Top 6 builders: {', '.join(manifest['top6_builders'])}",
        f"- Top 4 finders: {', '.join(manifest['top4_finders'])}",
        f"- Top 6 finders: {', '.join(manifest['top6_finders'])}",
        f"- Positive-lift finders: {', '.join(manifest['positive_lift_finders'])}",
        f"- Implementation-signal questions: {len(manifest['implementation_signal_questions'])}",
        f"- Low-prior questions: {len(manifest['low_prior_questions'])}",
        f"- Hard formal questions: {len(manifest['hard_formal_questions'])}",
        "",
        "## Builder Robustness",
        "",
    ]
    for view_id in (
        "all_finders",
        "top4_formal_finders",
        "top6_formal_finders",
        "positive_lift_finders",
        "cross_family_finders",
        "implementation_signal_questions",
    ):
        lines.extend(_agent_ordering_block(agent_rows, "builder", view_id, 6))

    lines.extend(["", "## Finder Robustness", ""])
    for view_id in (
        "all_builders",
        "top4_builders",
        "top6_builders",
        "cross_family_builders",
        "implementation_signal_questions",
        "low_prior_questions",
    ):
        lines.extend(_agent_ordering_block(agent_rows, "finder", view_id, 6))

    lines.extend(
        [
            "",
            "## Effort Checks",
            "",
            "| role | view | base model | high-low |",
            "| --- | --- | --- | ---: |",
        ]
    )
    for row in effort_rows:
        if row["view_id"] not in {
            "all_finders",
            "top4_formal_finders",
            "cross_family_finders",
            "all_builders",
            "top4_builders",
            "cross_family_builders",
            "implementation_signal_questions",
        }:
            continue
        lines.append(
            f"| {row['role']} | {row['view_id']} | {row['base_model']} | {_fmt(row['high_minus_low'])} |"
        )

    lines.extend(
        [
            "",
            "## Accuracy-Token Diagnostics",
            "",
            "Diagnostic only; not the paper's within-finder builder-efficiency metric.",
            "",
            "| role | view | frontier agent | accuracy | mean tokens | acc / M tok |",
            "| --- | --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in agent_token_rows:
        if not row["pareto_frontier"]:
            continue
        if row["view_id"] not in {
            "all_finders",
            "top4_formal_finders",
            "implementation_signal_questions",
            "all_builders",
            "top4_builders",
            "low_prior_questions",
        }:
            continue
        lines.append(
            f"| {row['role']} | {row['view_id']} | {row['agent_id']} | {_fmt(row['accuracy'])} | {row['mean_tokens']:.0f} | {_fmt(row['accuracy_per_million_tokens'])} |"
        )

    lines.extend(
        [
            "",
            "## Low-Cost Near-Ceiling Points",
            "",
            "| role | view | band | agent | accuracy | mean tokens |",
            "| --- | --- | --- | --- | ---: | ---: |",
        ]
    )
    for row in agent_token_rows:
        if row["view_id"] not in {
            "all_finders",
            "top4_formal_finders",
            "implementation_signal_questions",
            "all_builders",
            "top4_builders",
            "low_prior_questions",
        }:
            continue
        for column, label in (
            ("within_0_005_least_cost", "within 0.5pp"),
            ("within_0_010_least_cost", "within 1.0pp"),
            ("within_0_020_least_cost", "within 2.0pp"),
        ):
            if row[column]:
                lines.append(
                    f"| {row['role']} | {row['view_id']} | {label} | {row['agent_id']} | {_fmt(row['accuracy'])} | {row['mean_tokens']:.0f} |"
                )

    lines.extend(
        [
            "",
            "## Family Pair Matrix",
            "",
            "| builder family | finder family | accuracy | rows |",
            "| --- | --- | ---: | ---: |",
        ]
    )
    for row in family_rows:
        lines.append(
            f"| {row['builder_family']} | {row['finder_family']} | {_fmt(row['accuracy'])} | {row['question_rows']} |"
        )

    top_affinity = sorted(
        affinity_rows,
        key=lambda row: row["same_family_minus_different_family"],
        reverse=True,
    )[:8]
    lines.extend(
        [
            "",
            "## Strongest Same-Family Affinity",
            "",
            "| role | agent | same-family | different-family | delta |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in top_affinity:
        lines.append(
            f"| {row['role']} | {row['agent_id']} | {_fmt(row['same_family_accuracy'])} | {_fmt(row['different_family_accuracy'])} | {_fmt(row['same_family_minus_different_family'])} |"
        )

    question_rows = sorted(
        question_summary.values(),
        key=lambda row: row["formal_minus_question_only_stability"],
        reverse=True,
    )
    lines.extend(
        [
            "",
            "## Implementation-Signal Questions",
            "",
            "| task | question | formal 3-trial agreement | question-only 3-trial agreement | agreement lift | formal acc. | question-only acc. |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in question_rows[:12]:
        lines.append(
            f"| {row['task_id']} | {row['question_id']} | {_fmt(row['formal_stable_cell_rate'])} | {_fmt(row['question_only_stable_cell_rate'])} | {_fmt(row['formal_minus_question_only_stability'])} | {_fmt(row['formal_accuracy'])} | {_fmt(row['question_only_accuracy'])} |"
        )

    lines.extend(
        [
            "",
            "## Files",
            "",
            "- `agent_views.csv`",
            "- `effort_comparisons.csv`",
            "- `agent_token_diagnostics.csv`",
            "- `affinity_by_agent.csv`",
            "- `family_pair_matrix.csv`",
            "- `task_agent_views.csv`",
            "- `question_signal.csv`",
            "- `manifest.json`",
        ]
    )
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def _agent_ordering_block(
    agent_rows: list[dict[str, Any]],
    role: str,
    view_id: str,
    top_n: int,
) -> list[str]:
    rows = _view_rows(agent_rows, role, view_id)[:top_n]
    lines = [
        f"### {role}: {view_id}",
        "",
        "| order | agent | accuracy | rows |",
        "| ---: | --- | ---: | ---: |",
    ]
    for row in rows:
        lines.append(
            f"| {row['rank']} | {row['agent_id']} | {_fmt(row['accuracy'])} | {row['question_rows']} |"
        )
    lines.append("")
    return lines


if __name__ == "__main__":
    main()
