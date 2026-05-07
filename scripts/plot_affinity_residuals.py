"""Render affinity residual plots for the final E&D recovery matrix."""

# ruff: noqa: E402

from __future__ import annotations

import sys
import math
import statistics
from collections import defaultdict
from pathlib import Path
from typing import Any

import matplotlib as mpl
import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.config import paths
from scripts.analyze_eandd_final import (
    DEFAULT_AGENTS,
    DEFAULT_TASKS,
    _load_final_builds,
    _load_final_finds,
    _split_csv,
)

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
AGENT_LABELS = {
    "codex_gpt5_5_high": "GPT-5.5",
    "codex_gpt5_4_mini_high": "GPT-5.4 Mini",
    "claude_code_opus_4_7_high": "Opus 4.7",
    "claude_code_sonnet_4_6_high": "Sonnet 4.6",
    "claude_code_mimo_v2_5_pro_high": "MiMo 2.5 Pro",
    "claude_code_mimo_v2_5_high": "MiMo 2.5",
    "codex_gpt5_5_low": "GPT-5.5 (low)",
    "codex_gpt5_4_mini_low": "GPT-5.4 Mini (low)",
    "claude_code_opus_4_7_low": "Opus 4.7 (low)",
    "claude_code_sonnet_4_6_low": "Sonnet 4.6 (low)",
    "claude_code_mimo_v2_5_pro_low": "MiMo 2.5 Pro (low)",
    "claude_code_mimo_v2_5_low": "MiMo 2.5 (low)",
}
PAPER_LABELS = {
    "codex_gpt5_5_high": "GPT-5.5",
    "codex_gpt5_4_mini_high": "GPT-5.4 Mini",
    "claude_code_opus_4_7_high": "Claude Opus 4.7",
    "claude_code_sonnet_4_6_high": "Claude Sonnet 4.6",
    "claude_code_mimo_v2_5_pro_high": "MiMo 2.5 Pro",
    "claude_code_mimo_v2_5_high": "MiMo 2.5",
    "codex_gpt5_5_low": "GPT-5.5 (low)",
    "codex_gpt5_4_mini_low": "GPT-5.4 Mini (low)",
    "claude_code_opus_4_7_low": "Claude Opus 4.7 (low)",
    "claude_code_sonnet_4_6_low": "Claude Sonnet 4.6 (low)",
    "claude_code_mimo_v2_5_pro_low": "MiMo 2.5 Pro (low)",
    "claude_code_mimo_v2_5_low": "MiMo 2.5 (low)",
}


def main() -> None:
    out_dir = _plots_dir()
    out_dir.mkdir(parents=True, exist_ok=True)
    df = _residual_rows(pd.DataFrame(recovery_matrix_rows()))
    family = _family_residuals(df)

    residual_csv = out_dir / "recovery_affinity_residual_12x12.csv"
    family_csv = out_dir / "recovery_affinity_residual_family.csv"
    df.to_csv(residual_csv, index=False)
    family.to_csv(family_csv)

    _set_theme()
    _plot_residual_matrix(df)
    _plot_residual_matrix_paper(df)
    _plot_family_matrix(family)

    print(f"wrote {residual_csv}")
    print(f"wrote {family_csv}")


def _plots_dir() -> Path:
    return paths().reports_dir / "eandd_final" / "plots"


def recovery_matrix_rows() -> list[dict[str, Any]]:
    p = paths()
    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)
    unknown = sorted(set(AGENT_ORDER) - set(agent_ids))
    if unknown:
        raise RuntimeError(f"plot order contains unknown agents: {unknown}")

    builds = _load_final_builds(p.runs_dir, task_ids, agent_ids)
    finds = _load_final_finds(p.runs_dir, builds, agent_ids)

    grouped: dict[tuple[str, str, str], list[dict[str, float]]] = defaultdict(list)
    for cell in finds:
        eff = cell.run.usage.efficiency
        if eff is None:
            raise RuntimeError(f"{cell.run.find_run_id} has no efficiency metrics")
        accuracy = sum(answer.is_correct for answer in cell.run.answers) / len(cell.run.answers)
        grouped[(cell.task_id, cell.builder_id, cell.finder_id)].append(
            {
                "accuracy": accuracy,
                "retrieval_bytes": float(eff.novel_retrieval_bytes),
                "question_count": float(len(cell.run.answers)),
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

    out = []
    for builder_id in AGENT_ORDER:
        for finder_id in AGENT_ORDER:
            cells = by_builder_finder[(builder_id, finder_id)]
            if not cells:
                score = math.nan
                mean_accuracy = math.nan
                mean_success_signal = math.nan
                mean_retrieval_bytes = math.nan
            else:
                score = _geomean([cell["normalized_score"] for cell in cells])
                mean_accuracy = _mean([cell["mean_accuracy"] for cell in cells])
                mean_success_signal = _mean([cell["success_signal"] for cell in cells])
                mean_retrieval_bytes = _mean([cell["mean_retrieval_bytes"] for cell in cells])
            out.append(
                {
                    "builder_id": builder_id,
                    "builder_label": AGENT_LABELS[builder_id],
                    "finder_id": finder_id,
                    "finder_label": AGENT_LABELS[finder_id],
                    "score": score,
                    "normalization_scope": "within_builder_effort_block_per_task_finder",
                    "task_cells": len(cells),
                    "mean_accuracy": mean_accuracy,
                    "mean_success_signal": mean_success_signal,
                    "mean_retrieval_bytes": mean_retrieval_bytes,
                }
            )
    return out


def _residual_rows(df: pd.DataFrame) -> pd.DataFrame:
    df = df.copy()
    df["builder_family"] = df["builder_id"].map(_family)
    df["finder_family"] = df["finder_id"].map(_family)
    df["same_family"] = df["builder_family"] == df["finder_family"]
    df["same_base_model"] = df["builder_id"].map(_base_model) == df["finder_id"].map(_base_model)

    row_mean = df.groupby("builder_id")["score"].transform("mean")
    col_mean = df.groupby("finder_id")["score"].transform("mean")
    grand_mean = df["score"].mean()
    df["builder_mean"] = row_mean
    df["finder_mean"] = col_mean
    df["grand_mean"] = grand_mean
    df["affinity_residual"] = df["score"] - row_mean - col_mean + grand_mean
    return df


def _family_residuals(df: pd.DataFrame) -> pd.DataFrame:
    order = ["OpenAI/Codex", "Anthropic/Claude", "MiMo"]
    family = df.pivot_table(
        index="builder_family",
        columns="finder_family",
        values="affinity_residual",
        aggfunc="mean",
    )
    return family.loc[order, order]


def _plot_residual_matrix(df: pd.DataFrame) -> None:
    label_order = [AGENT_LABELS[agent_id] for agent_id in AGENT_ORDER]
    matrix = df.pivot(index="builder_label", columns="finder_label", values="affinity_residual")
    matrix = matrix.loc[label_order, label_order]
    limit = max(abs(float(matrix.min().min())), abs(float(matrix.max().max())))

    fig, ax = plt.subplots(figsize=(12.6, 10.8))
    sns.heatmap(
        matrix,
        ax=ax,
        cmap="vlag",
        center=0.0,
        vmin=-limit,
        vmax=limit,
        square=True,
        linewidths=0.7,
        linecolor="white",
        annot=True,
        fmt="+.2f",
        annot_kws={"fontsize": 7.5, "fontweight": "semibold"},
        cbar_kws={"label": "Affinity residual", "shrink": 0.72, "pad": 0.035},
    )
    _format_12x12_axes(fig, ax)
    fig.text(
        0.19,
        0.965,
        "Builder x Finder Affinity Residuals",
        ha="left",
        va="top",
        fontsize=16,
        fontweight="bold",
        color="#111827",
    )
    fig.text(
        0.19,
        0.932,
        "Residual = cell score - builder mean - finder mean + grand mean; positive cells indicate pair-specific recoverability beyond marginal strength.",
        ha="left",
        va="top",
        fontsize=9.5,
        color="#4b5563",
    )
    fig.savefig(
        _plots_dir() / "recovery_affinity_residual_12x12.png",
        bbox_inches="tight",
        pad_inches=0.2,
        facecolor="white",
    )
    fig.savefig(
        _plots_dir() / "recovery_affinity_residual_12x12.pdf",
        bbox_inches="tight",
        pad_inches=0.2,
        facecolor="white",
    )
    plt.close(fig)


def _plot_residual_matrix_paper(df: pd.DataFrame) -> None:
    label_order = [PAPER_LABELS[agent_id] for agent_id in AGENT_ORDER]
    work = df.assign(
        builder_paper_label=df["builder_id"].map(PAPER_LABELS),
        finder_paper_label=df["finder_id"].map(PAPER_LABELS),
    )
    matrix = work.pivot(
        index="builder_paper_label",
        columns="finder_paper_label",
        values="affinity_residual",
    )
    matrix = matrix.loc[label_order, label_order]
    limit = max(abs(float(matrix.min().min())), abs(float(matrix.max().max())))

    fig, ax = plt.subplots(figsize=(8.7, 7.5))
    sns.heatmap(
        matrix,
        ax=ax,
        cmap="vlag",
        center=0.0,
        vmin=-limit,
        vmax=limit,
        square=True,
        linewidths=0.35,
        linecolor="white",
        annot=True,
        fmt="+.2f",
        annot_kws={"fontsize": 5.1, "fontweight": "semibold"},
        cbar_kws={
            "label": "Affinity residual",
            "shrink": 0.68,
            "pad": 0.035,
            "ticks": [-0.5, -0.25, 0.0, 0.25, 0.5],
        },
    )
    ax.axvline(6, color="#111827", linewidth=1.9)
    ax.axhline(6, color="#111827", linewidth=1.9)
    ax.set_xlabel("Finder", labelpad=38, fontsize=8.5)
    ax.set_ylabel("Builder", labelpad=26, fontsize=8.5)
    ax.xaxis.tick_top()
    ax.xaxis.set_label_position("top")
    ax.tick_params(axis="x", labelrotation=50, labelsize=5.8, pad=2)
    ax.tick_params(axis="y", labelrotation=0, labelsize=5.9, pad=2)
    for tick in ax.get_xticklabels():
        tick.set_horizontalalignment("left")
        tick.set_rotation_mode("anchor")
    for spine in ax.spines.values():
        spine.set_visible(False)
    ax.collections[0].colorbar.ax.tick_params(labelsize=7)
    ax.collections[0].colorbar.ax.yaxis.label.set_size(7.5)
    fig.subplots_adjust(left=0.22, right=0.92, bottom=0.08, top=0.7)
    fig.savefig(
        _plots_dir() / "recovery_affinity_residual_12x12_paper.png",
        bbox_inches="tight",
        pad_inches=0.05,
        facecolor="white",
    )
    fig.savefig(
        _plots_dir() / "recovery_affinity_residual_12x12_paper.pdf",
        bbox_inches="tight",
        pad_inches=0.05,
        facecolor="white",
    )
    plt.close(fig)


def _plot_family_matrix(family: pd.DataFrame) -> None:
    limit = max(abs(float(family.min().min())), abs(float(family.max().max())))
    fig, ax = plt.subplots(figsize=(5.4, 4.2))
    sns.heatmap(
        family,
        ax=ax,
        cmap="vlag",
        center=0.0,
        vmin=-limit,
        vmax=limit,
        square=True,
        linewidths=1.0,
        linecolor="white",
        annot=True,
        fmt="+.3f",
        annot_kws={"fontsize": 10, "fontweight": "semibold"},
        cbar_kws={"label": "Mean residual", "shrink": 0.8, "pad": 0.04},
    )
    ax.set_xlabel("Finder family", labelpad=8)
    ax.set_ylabel("Builder family", labelpad=8)
    ax.set_title("Family-Level Affinity Residual", fontsize=12, fontweight="bold", pad=12)
    ax.tick_params(axis="x", labelrotation=25)
    ax.tick_params(axis="y", labelrotation=0)
    fig.tight_layout()
    fig.savefig(
        _plots_dir() / "recovery_affinity_residual_family.png",
        bbox_inches="tight",
        pad_inches=0.18,
        facecolor="white",
    )
    fig.savefig(
        _plots_dir() / "recovery_affinity_residual_family.pdf",
        bbox_inches="tight",
        pad_inches=0.18,
        facecolor="white",
    )
    plt.close(fig)


def _format_12x12_axes(fig, ax) -> None:
    ax.axvline(6, color="#111827", linewidth=2.8)
    ax.axhline(6, color="#111827", linewidth=2.8)
    ax.set_xlabel("Finder", labelpad=40)
    ax.set_ylabel("Builder", labelpad=28)
    ax.xaxis.tick_top()
    ax.xaxis.set_label_position("top")
    ax.tick_params(axis="x", labelrotation=55, labelsize=9, pad=2)
    ax.tick_params(axis="y", labelrotation=0, labelsize=9, pad=2)
    ax.text(0.25, 1.13, "High effort finders", transform=ax.transAxes, ha="center", va="bottom", fontsize=10, fontweight="bold")
    ax.text(0.75, 1.13, "Low effort finders", transform=ax.transAxes, ha="center", va="bottom", fontsize=10, fontweight="bold")
    ax.text(-0.17, 0.75, "High effort builders", transform=ax.transAxes, ha="center", va="center", rotation=90, fontsize=10, fontweight="bold")
    ax.text(-0.17, 0.25, "Low effort builders", transform=ax.transAxes, ha="center", va="center", rotation=90, fontsize=10, fontweight="bold")
    fig.subplots_adjust(left=0.19, right=0.9, bottom=0.09, top=0.74)


def _set_theme() -> None:
    mpl.rcParams["pdf.fonttype"] = 42
    mpl.rcParams["ps.fonttype"] = 42
    sns.set_theme(
        context="paper",
        style="white",
        font="Arial",
        rc={
            "figure.dpi": 160,
            "savefig.dpi": 300,
            "pdf.fonttype": 42,
            "ps.fonttype": 42,
            "axes.titleweight": "bold",
            "axes.labelweight": "bold",
        },
    )


def _family(agent_id: str) -> str:
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


def _agent_effort(agent_id: str) -> str:
    if agent_id.endswith("_high"):
        return "high"
    if agent_id.endswith("_low"):
        return "low"
    raise ValueError(f"cannot infer effort from agent id {agent_id!r}")


def _mean(values: list[float]) -> float:
    return statistics.fmean(values) if values else math.nan


def _geomean(values: list[float]) -> float:
    positive = [value for value in values if value > 0]
    if not positive:
        return math.nan
    return math.exp(sum(math.log(value) for value in positive) / len(positive))


if __name__ == "__main__":
    main()
