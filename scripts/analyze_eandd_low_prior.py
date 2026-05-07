"""Low-prior question subset sensitivity analysis for the E&D final panel.

The low-prior subset is defined exactly as in the pivot analysis: questions
whose question-only three-trial agreement is below 90%. The analysis reuses the
paper's within-finder effort machinery, but rescoring each find run on only the
low-prior questions for its task. Effort metrics remain run-level because the
proxy records inspection cost for the whole find run, not per question.
"""

# ruff: noqa: E402

from __future__ import annotations

import json
import math
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import pandas as pd

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
    _load_control_finds,
    _load_final_builds,
    _load_final_finds,
    _mean,
    _split_csv,
    _stability_tables,
)
from scripts.analyze_eandd_within_finder_efficiency import (
    METRICS,
    _add_builder_score_bootstrap_cis,
    _builder_score_rows,
    _cell_means,
    _cell_output_rows,
    _geomean,
    _rank_stability_rows,
    _recovery_rows,
    _write_csv,
)

LOW_PRIOR_STABILITY_THRESHOLD = 0.90

AGENT_ORDER = [
    "codex_gpt5_5_high",
    "codex_gpt5_4_mini_high",
    "claude_code_opus_4_7_high",
    "claude_code_sonnet_4_6_high",
    "claude_code_mimo_v2_5_pro_high",
    "claude_code_mimo_v2_5_high",
    "codex_gpt5_5_low",
    "codex_gpt5_4_mini_low",
    "claude_code_opus_4_7_low",
    "claude_code_sonnet_4_6_low",
    "claude_code_mimo_v2_5_pro_low",
    "claude_code_mimo_v2_5_low",
]


def main() -> None:
    p = paths()
    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)
    out_dir = p.reports_dir / "eandd_final" / "low_prior"
    within_dir = out_dir / "within_finder_efficiency"
    plots_dir = out_dir / "plots"
    within_dir.mkdir(parents=True, exist_ok=True)
    plots_dir.mkdir(parents=True, exist_ok=True)

    builds = _load_final_builds(p.runs_dir, task_ids, agent_ids)
    finds = _load_final_finds(p.runs_dir, builds, agent_ids)
    controls = _load_control_finds(p.runs_dir, task_ids, agent_ids)
    _stability_summary, question_stability = _stability_tables(finds, controls)
    low_prior_questions = _low_prior_questions(question_stability)

    run_rows = _subset_run_rows(finds, low_prior_questions)
    panels = _panels(agent_ids)

    cell_rows: list[dict[str, Any]] = []
    builder_score_rows: list[dict[str, Any]] = []
    recovery_rows: list[dict[str, Any]] = []
    rank_rows: list[dict[str, Any]] = []
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
    _write_csv(within_dir / "within_finder_cells.csv", cell_rows)
    _write_csv(within_dir / "builder_scores.csv", builder_score_rows)
    _write_csv(within_dir / "recovery_scores.csv", recovery_rows)
    _write_csv(within_dir / "rank_stability.csv", rank_rows)

    residual_rows = _affinity_residual_rows(finds, low_prior_questions)
    family_rows = _family_residual_rows(residual_rows)
    _write_csv(plots_dir / "recovery_affinity_residual_12x12.csv", residual_rows)
    family_rows.to_csv(plots_dir / "recovery_affinity_residual_family.csv")

    manifest = {
        "generated_at": _source_snapshot_at(builds, finds, controls),
        "analysis_script": "scripts/analyze_eandd_low_prior.py",
        "input_panel": f"{_display_path(p.runs_dir)} compile-pass final logical panel",
        "question_subset": {
            "subset_id": "low_prior_questions",
            "definition": "question-only three-trial agreement < 0.90",
            "threshold": LOW_PRIOR_STABILITY_THRESHOLD,
            "questions": sorted([f"{task_id}:{question_id}" for task_id, question_id in low_prior_questions]),
            "question_count": len(low_prior_questions),
            "questions_per_task": dict(
                sorted(
                    (
                        task_id,
                        sum(1 for question_task, _question_id in low_prior_questions if question_task == task_id),
                    )
                    for task_id in task_ids
                )
            ),
        },
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
            "subset_scoring": "accuracy and perfect-run gates are computed only over low-prior questions for the task",
            "effort_attribution": "inspection effort is recorded per find run, so the full run-level effort is assigned to the subset",
            "normalization": "within each (finder, task), divide each builder cell by the minimum builder cost in the same panel",
            "conditional_gate": "perfect_run: average only find runs whose low-prior subset accuracy is 1.0",
        },
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(
        out_dir / "low_prior_insights.md",
        manifest=manifest,
        builder_score_rows=builder_score_rows,
        recovery_rows=recovery_rows,
        rank_rows=rank_rows,
        family_rows=family_rows,
    )
    print(f"wrote {out_dir / 'low_prior_insights.md'}")


def _panels(agent_ids: list[str]):
    from scripts.analyze_eandd_within_finder_efficiency import _panels as _source_panels

    return _source_panels(agent_ids)


def _low_prior_questions(question_stability: list[dict[str, Any]]) -> set[tuple[str, str]]:
    questions = {
        (row["task_id"], row["question_id"])
        for row in question_stability
        if row["condition"] == "question_only"
        and float(row["stable_cell_rate"]) < LOW_PRIOR_STABILITY_THRESHOLD
    }
    if not questions:
        raise RuntimeError("low-prior question subset is empty")
    return questions


def _subset_run_rows(finds, low_prior_questions: set[tuple[str, str]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for cell in finds:
        selected_answers = [
            answer
            for answer in cell.run.answers
            if (cell.task_id, answer.question_id) in low_prior_questions
        ]
        if not selected_answers:
            continue
        eff = cell.run.usage.efficiency
        if eff is None:
            raise RuntimeError(f"{cell.run.find_run_id} has no efficiency metrics")
        accuracy = sum(answer.is_correct for answer in selected_answers) / len(selected_answers)
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
                "question_count": len(selected_answers),
            }
        )
    return rows


def _affinity_residual_rows(finds, low_prior_questions: set[tuple[str, str]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, str], list[dict[str, float]]] = defaultdict(list)
    for cell in finds:
        eff = cell.run.usage.efficiency
        if eff is None:
            raise RuntimeError(f"{cell.run.find_run_id} has no efficiency metrics")
        selected_answers = [
            answer
            for answer in cell.run.answers
            if (cell.task_id, answer.question_id) in low_prior_questions
        ]
        if not selected_answers:
            continue
        accuracy = sum(answer.is_correct for answer in selected_answers) / len(selected_answers)
        grouped[(cell.task_id, cell.builder_id, cell.finder_id)].append(
            {
                "accuracy": accuracy,
                "retrieval_bytes": float(eff.novel_retrieval_bytes),
                "question_count": float(len(selected_answers)),
            }
        )

    task_cell_scores: list[dict[str, Any]] = []
    for (task_id, builder_id, finder_id), group in grouped.items():
        effort = _mean([row["retrieval_bytes"] for row in group])
        if effort <= 0:
            continue
        mean_accuracy = _mean([row["accuracy"] for row in group])
        question_count = int(round(_mean([row["question_count"] for row in group])))
        success_signal = mean_accuracy**question_count
        task_cell_scores.append(
            {
                "task_id": task_id,
                "builder_id": builder_id,
                "finder_id": finder_id,
                "raw_recovery": success_signal / effort,
                "mean_accuracy": mean_accuracy,
                "success_signal": success_signal,
                "mean_retrieval_bytes": effort,
            }
        )

    best_by_task_finder_block: dict[tuple[str, str, str], float] = defaultdict(float)
    for row in task_cell_scores:
        key = (row["task_id"], row["finder_id"], _agent_effort(row["builder_id"]))
        best_by_task_finder_block[key] = max(best_by_task_finder_block[key], row["raw_recovery"])

    by_builder_finder: dict[tuple[str, str], list[dict[str, float]]] = defaultdict(list)
    for row in task_cell_scores:
        normalization_group = (row["task_id"], row["finder_id"], _agent_effort(row["builder_id"]))
        best = best_by_task_finder_block[normalization_group]
        if best <= 0:
            continue
        by_builder_finder[(row["builder_id"], row["finder_id"])].append(
            {
                "normalized_score": max(row["raw_recovery"] / best, 1e-9),
                "mean_accuracy": row["mean_accuracy"],
                "success_signal": row["success_signal"],
                "mean_retrieval_bytes": row["mean_retrieval_bytes"],
            }
        )

    rows = []
    for builder_id in AGENT_ORDER:
        for finder_id in AGENT_ORDER:
            cells = by_builder_finder[(builder_id, finder_id)]
            rows.append(
                {
                    "builder_id": builder_id,
                    "finder_id": finder_id,
                    "score": _geomean([cell["normalized_score"] for cell in cells]) if cells else math.nan,
                    "normalization_scope": "within_builder_effort_block_per_task_finder",
                    "task_cells": len(cells),
                    "mean_accuracy": _mean([cell["mean_accuracy"] for cell in cells]) if cells else math.nan,
                    "mean_success_signal": _mean([cell["success_signal"] for cell in cells])
                    if cells
                    else math.nan,
                    "mean_retrieval_bytes": _mean([cell["mean_retrieval_bytes"] for cell in cells])
                    if cells
                    else math.nan,
                }
            )

    df = pd.DataFrame(rows)
    df["builder_family"] = df["builder_id"].map(_family_label)
    df["finder_family"] = df["finder_id"].map(_family_label)
    df["same_family"] = df["builder_family"] == df["finder_family"]
    df["same_base_model"] = df["builder_id"].map(_base_model) == df["finder_id"].map(_base_model)
    df["builder_mean"] = df.groupby("builder_id")["score"].transform("mean")
    df["finder_mean"] = df.groupby("finder_id")["score"].transform("mean")
    df["grand_mean"] = df["score"].mean()
    df["affinity_residual"] = df["score"] - df["builder_mean"] - df["finder_mean"] + df["grand_mean"]
    return df.to_dict("records")


def _family_residual_rows(rows: list[dict[str, Any]]) -> pd.DataFrame:
    order = ["OpenAI/Codex", "Anthropic/Claude", "MiMo"]
    df = pd.DataFrame(rows)
    family = df.pivot_table(
        index="builder_family",
        columns="finder_family",
        values="affinity_residual",
        aggfunc="mean",
    )
    return family.loc[order, order]


def _family_label(agent_id: str) -> str:
    if agent_id.startswith("codex_"):
        return "OpenAI/Codex"
    if agent_id.startswith("claude_code_mimo_"):
        return "MiMo"
    if agent_id.startswith("claude_code_"):
        return "Anthropic/Claude"
    raise ValueError(f"unknown agent family for {agent_id!r}")


def _base_model(agent_id: str) -> str:
    if agent_id.endswith("_high"):
        return agent_id[:-5]
    if agent_id.endswith("_low"):
        return agent_id[:-4]
    return agent_id


def _source_snapshot_at(builds, finds, controls) -> str:
    timestamps = [
        cell.run.manifest.created_at
        for cell in (*builds, *finds, *controls)
        if cell.run.manifest.created_at
    ]
    if timestamps:
        return max(timestamps)
    return datetime.fromtimestamp(0, timezone.utc).isoformat()


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


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
    family_rows: pd.DataFrame,
) -> None:
    lines = [
        "# Low-Prior Subset Sensitivity",
        "",
        f"Generated: {manifest['generated_at']}",
        "",
        "Questions: " + ", ".join(manifest["question_subset"]["questions"]),
        "",
        "Accuracy and all-correct gates are computed only on the low-prior questions.",
        "Effort metrics remain full find-run costs because the trace has no per-question cost attribution.",
        "",
        "## Builder R_b",
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
                "| rank | builder | R_total_bytes | n | mean subset acc |",
                "| ---: | --- | ---: | ---: | ---: |",
            ]
        )
        for row in rows:
            lines.append(
                f"| {row['rank']} | {row['builder_id']} | {_fmt(row['score'])} | {row['n_cells']}/{row['max_cells']} | {_fmt(row['mean_cell_accuracy'])} |"
            )
        lines.append("")

    lines.extend(["## Recovery Diagnostic", ""])
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
                "| rank | builder | recovery | n | mean subset acc | mean retrieval bytes |",
                "| ---: | --- | ---: | ---: | ---: | ---: |",
            ]
        )
        for row in rows:
            lines.append(
                f"| {row['rank']} | {row['builder_id']} | {_fmt(row['score'])} | {row['n_cells']}/{row['max_cells']} | {_fmt(row['mean_cell_accuracy'])} | {row['mean_cell_effort']:.0f} |"
            )
        lines.append("")

    lines.extend(
        [
            "## Rank Stability",
            "",
            "| panel | task | slice | median tau | positive fraction |",
            "| --- | --- | --- | ---: | ---: |",
        ]
    )
    for row in rank_rows:
        if (
            row["gate"] == "perfect_run"
            and row["metric"] == "total_bytes"
            and row["panel_id"] in {"high_effort", "low_effort"}
        ):
            lines.append(
                f"| {row['panel_id']} | {row['task_id']} | {row['slice_id']} | {_fmt(row['median_tau'])} | {_fmt(row['positive_fraction'])} |"
            )

    lines.extend(
        [
            "",
            "## Family Affinity Residual",
            "",
            _family_markdown_table(family_rows),
            "",
        ]
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _family_markdown_table(family_rows: pd.DataFrame) -> str:
    lines = [
        "| builder family | OpenAI/Codex finder | Anthropic/Claude finder | MiMo finder |",
        "| --- | ---: | ---: | ---: |",
    ]
    for builder_family, row in family_rows.iterrows():
        lines.append(
            f"| {builder_family} | {row['OpenAI/Codex']:.3f} | {row['Anthropic/Claude']:.3f} | {row['MiMo']:.3f} |"
        )
    return "\n".join(lines)


if __name__ == "__main__":
    main()
