"""Render paper comparison figures with the same seaborn style as the matrix plots.

The figures are disposable analysis outputs. They read only derived report CSVs
under the configured reports root and write manuscript-ready PDF/PNG/SVG files
under the directory named by BNF_FIGURES_DIR (relative paths resolve against the
repository root). Without it, they go to paper/neurips2026_eandd/figures when the
manuscript sources are present, and to figures/ under the reports root otherwise.
"""

from __future__ import annotations

import os
from pathlib import Path

import matplotlib as mpl
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns
from adjustText import adjust_text
from matplotlib.colors import LinearSegmentedColormap
from matplotlib.lines import Line2D

from bnf.config import paths

ROOT = Path(__file__).resolve().parent.parent
REPORTS = paths().reports_dir / "eandd_final"
IMPACT_REPORTS = paths().reports_dir / "artifact_conformance_consensus" / "impact"
FIGURES_DIR_ENV = "BNF_FIGURES_DIR"
PAPER_DIR = ROOT / "paper" / "neurips2026_eandd"


def _figures_dir() -> Path:
    configured = os.environ.get(FIGURES_DIR_ENV)
    if not configured:
        return PAPER_DIR / "figures" if PAPER_DIR.is_dir() else paths().reports_dir / "figures"
    path = Path(configured)
    return path if path.is_absolute() else ROOT / path


OUT = _figures_dir()

TEXT = "#111827"
MUTED = "#6b7280"
GRID = "#e5e7eb"
TOTAL_BYTE_RB_LABEL = r"Total-byte $R_b$"
TOOL_CALL_COUNT_RB_LABEL = r"Tool-call-count $R_b$"
KENDALL_TAU_LABEL = r"Median Kendall $\tau$"
COLORS = {
    "codex": "#2563eb",
    "claude": "#c2410c",
    "mimo": "#047857",
}

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

LABELS = {
    "claude_code_opus_4_7_high": "Opus 4.7",
    "claude_code_sonnet_4_6_high": "Sonnet 4.6",
    "codex_gpt5_4_mini_high": "GPT-5.4-mini",
    "codex_gpt5_5_high": "GPT-5.5",
    "claude_code_opus_4_7_low": "Opus 4.7-low",
    "claude_code_sonnet_4_6_low": "Sonnet 4.6-low",
    "codex_gpt5_4_mini_low": "GPT-5.4-mini-low",
    "codex_gpt5_5_low": "GPT-5.5-low",
    "claude_code_mimo_v2_5_pro_high": "MiMo 2.5 Pro",
    "claude_code_mimo_v2_5_pro_low": "MiMo 2.5 Pro-low",
    "claude_code_mimo_v2_5_high": "MiMo 2.5",
    "claude_code_mimo_v2_5_low": "MiMo 2.5-low",
}

SHORT_LABELS = {
    "claude_code_opus_4_7_high": "Opus 4.7",
    "claude_code_sonnet_4_6_high": "Sonnet 4.6",
    "codex_gpt5_4_mini_high": "GPT-5.4-mini",
    "codex_gpt5_5_high": "GPT-5.5",
    "claude_code_opus_4_7_low": "Opus 4.7",
    "claude_code_sonnet_4_6_low": "Sonnet 4.6",
    "codex_gpt5_4_mini_low": "GPT-5.4-mini",
    "codex_gpt5_5_low": "GPT-5.5",
    "claude_code_mimo_v2_5_pro_high": "MiMo 2.5 Pro",
    "claude_code_mimo_v2_5_pro_low": "MiMo 2.5 Pro",
    "claude_code_mimo_v2_5_high": "MiMo 2.5",
    "claude_code_mimo_v2_5_low": "MiMo 2.5",
}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    _set_theme()
    _plot_builder_recovery()
    _plot_robustness_slices()
    _plot_builder_rb_forest()
    _write_task_recovery_gate_table()
    _plot_rank_stability_heatmap()
    _plot_token_pareto()
    _plot_effort_frontier()
    _plot_affinity_residual()
    _plot_low_prior_builder_rb_forest()
    _write_low_prior_task_recovery_gate_table()
    _plot_low_prior_rank_stability_heatmap()
    _plot_low_prior_affinity_residual()
    _write_readme()


def _set_theme() -> None:
    mpl.rcParams["pdf.fonttype"] = 42
    mpl.rcParams["ps.fonttype"] = 42
    mpl.rcParams["svg.fonttype"] = "none"
    sns.set_theme(
        style="whitegrid",
        context="paper",
        rc={
            "axes.edgecolor": "#d1d5db",
            "axes.labelcolor": TEXT,
            "axes.titlecolor": TEXT,
            "figure.facecolor": "white",
            "grid.color": GRID,
            "grid.linewidth": 0.8,
            "xtick.color": "#4b5563",
            "ytick.color": "#4b5563",
            "font.family": "DejaVu Sans",
            "mathtext.fontset": "dejavusans",
            "pdf.fonttype": 42,
            "ps.fonttype": 42,
            "svg.fonttype": "none",
        },
    )


def _read_csv(relative: str) -> pd.DataFrame:
    return pd.read_csv(REPORTS / relative)


def _read_impact_csv(relative: str) -> pd.DataFrame:
    return pd.read_csv(IMPACT_REPORTS / relative)


def _label(agent_id: str) -> str:
    return LABELS.get(agent_id, agent_id)


def _short_label(agent_id: str) -> str:
    return SHORT_LABELS.get(agent_id, _label(agent_id))


def _colors(df: pd.DataFrame) -> list[str]:
    return [COLORS.get(family, MUTED) for family in df["family"]]


def _legend(ax) -> None:
    handles = [
        Line2D([0], [0], marker="o", linestyle="", markersize=5.5, color=color, label=label)
        for label, color in (("GPT", COLORS["codex"]), ("Claude", COLORS["claude"]), ("MiMo", COLORS["mimo"]))
    ]
    ax.legend(handles=handles, loc="lower right", frameon=False, ncol=3, fontsize=7.5)


def _figure_legend(fig: plt.Figure, *, y: float = 0.02) -> None:
    handles = [
        Line2D([0], [0], marker="o", linestyle="", markersize=5.5, color=color, label=label)
        for label, color in (("GPT", COLORS["codex"]), ("Claude", COLORS["claude"]), ("MiMo", COLORS["mimo"]))
    ]
    fig.legend(handles=handles, loc="lower center", frameon=False, ncol=3, fontsize=7.5, bbox_to_anchor=(0.5, y))


def _save(fig: plt.Figure, stem: str) -> None:
    for suffix in ("pdf", "png", "svg"):
        path = OUT / f"{stem}.{suffix}"
        fig.savefig(path, bbox_inches="tight", pad_inches=0.04, facecolor="white")
        if suffix == "svg":
            _strip_trailing_whitespace(path)
    plt.close(fig)


def _strip_trailing_whitespace(path: Path) -> None:
    text = path.read_text()
    cleaned = "\n".join(line.rstrip() for line in text.splitlines())
    if text.endswith("\n"):
        cleaned += "\n"
    path.write_text(cleaned)


def _format_pct_axis(ax, *, xmin: float, xmax: float) -> None:
    ax.set_xlim(xmin, xmax)
    ax.xaxis.set_major_formatter(lambda value, _pos: f"{value:.0f}%")
    ax.grid(axis="x", color=GRID)
    ax.grid(axis="y", visible=False)
    sns.despine(ax=ax, left=True, bottom=False)


def _plot_builder_recovery() -> None:
    df = _read_impact_csv("conditioned_builder_summary.csv").copy()
    df["label"] = df["builder_id"].map(lambda agent_id: f"{_short_label(agent_id)}-{_effort(agent_id)}")
    df["conditional_recovery_pct"] = df["downstream_accuracy"] * 100.0
    df["implementation_coverage_pct"] = df["artifact_present_implemented_gold_rate"] * 100.0
    df["adjusted_recovery_pct"] = df["implementation_adjusted_downstream_recovery"] * 100.0
    ordered = [agent_id for agent_id in reversed(AGENT_ORDER) if agent_id in set(df["builder_id"])]
    df = df.set_index("builder_id").loc[ordered].reset_index()
    x_min = 5.0 * np.floor((float(df["adjusted_recovery_pct"].min()) - 2.0) / 5.0)
    x_max = 100.8
    label_gap = 0.07 * (x_max - x_min)
    label_offset = 0.01 * (x_max - x_min)

    y = np.arange(len(df))
    fig, ax = plt.subplots(figsize=(7.1, 3.85))
    for idx, row in df.iterrows():
        color = COLORS.get(row["family"], MUTED)
        ax.hlines(
            idx,
            row["adjusted_recovery_pct"],
            row["implementation_coverage_pct"],
            color="#cbd5e1",
            linewidth=1.4,
            zorder=1,
        )
        ax.scatter(
            row["implementation_coverage_pct"],
            idx,
            s=28,
            facecolors="white",
            edgecolors=color,
            linewidths=1.4,
            zorder=3,
        )
        ax.scatter(row["adjusted_recovery_pct"], idx, s=30, color=color, zorder=4)
        close_to_coverage = row["implementation_coverage_pct"] - row["adjusted_recovery_pct"] < label_gap
        label_x = (
            row["adjusted_recovery_pct"] - label_offset
            if close_to_coverage
            else row["adjusted_recovery_pct"] + label_offset
        )
        label_ha = "right" if close_to_coverage else "left"
        ax.text(
            label_x,
            idx,
            f"{row['adjusted_recovery_pct']:.1f}%",
            va="center",
            ha=label_ha,
            fontsize=7.4,
            color=TEXT,
            clip_on=False,
        )
    ax.set_yticks(y)
    ax.set_yticklabels(df["label"])
    ax.set_xlabel("Implementation-aware recovery and implementation coverage")
    ax.set_ylabel("")
    _format_pct_axis(ax, xmin=x_min, xmax=x_max)
    metric_handles = [
        Line2D([0], [0], marker="o", linestyle="", markersize=5.5, color=TEXT, label="Recovery × coverage"),
        Line2D(
            [0],
            [0],
            marker="o",
            linestyle="",
            markersize=5.5,
            markerfacecolor="white",
            markeredgecolor=TEXT,
            label="Implementation coverage",
        ),
    ]
    family_handles = [
        Line2D([0], [0], marker="o", linestyle="", markersize=5.5, color=color, label=label)
        for label, color in (("GPT", COLORS["codex"]), ("Claude", COLORS["claude"]), ("MiMo", COLORS["mimo"]))
    ]
    fig.legend(
        handles=metric_handles + family_handles,
        loc="lower center",
        frameon=False,
        ncol=5,
        fontsize=7.2,
        bbox_to_anchor=(0.5, 0.002),
    )
    fig.tight_layout(rect=[0.0, 0.12, 1.0, 1.0])
    _save(fig, "fig_builder_recovery")


def _effort(agent_id: str) -> str:
    if agent_id.endswith("_high"):
        return "high"
    if agent_id.endswith("_low"):
        return "low"
    return "unknown"


def _plot_robustness_slices() -> None:
    df = _read_impact_csv("conditioned_agent_views.csv").copy()
    df["label"] = df["agent_id"].map(_label)
    df["accuracy_pct"] = df["accuracy"] * 100.0
    slices = [
        ("All rows", "all_finders", "all_builders"),
        ("Calibrated", "top4_formal_finders", "top4_builders"),
        ("Cross-family", "cross_family_finders", "cross_family_builders"),
        ("Impl. signal", "implementation_signal_questions", "implementation_signal_questions"),
        ("Low-prior", "low_prior_questions", "low_prior_questions"),
    ]
    columns = [
        ("builder", "Builder", 1),
        ("builder", "Builder", 2),
        ("builder", "Builder", 3),
        ("finder", "Finder", 1),
        ("finder", "Finder", 2),
        ("finder", "Finder", 3),
    ]

    values: list[list[float]] = []
    annotations: list[list[str]] = []
    for slice_label, builder_view, finder_view in slices:
        row_values = []
        row_annotations = []
        for role, _view_label, rank in columns:
            view_id = builder_view if role == "builder" else finder_view
            match = df[(df["role"] == role) & (df["view_id"] == view_id) & (df["rank"] == rank)]
            if len(match) != 1:
                raise ValueError(f"expected one row for {role=} {view_id=} {rank=}, found {len(match)}")
            record = match.iloc[0]
            row_values.append(record["accuracy_pct"])
            row_annotations.append(f"{_heatmap_label(record['label'])}\n{record['accuracy_pct']:.1f}%")
        values.append(row_values)
        annotations.append(row_annotations)

    matrix = pd.DataFrame(
        values,
        index=[slice_label for slice_label, _builder_view, _finder_view in slices],
        columns=["#1", "#2", "#3", "#1", "#2", "#3"],
    )

    fig, ax = plt.subplots(figsize=(7.4, 2.75))
    cmap = LinearSegmentedColormap.from_list("recovery_blue", ["#f8fafc", "#dbeafe", "#93c5fd"])
    sns.heatmap(
        matrix,
        ax=ax,
        cmap=cmap,
        vmin=95,
        vmax=100,
        linewidths=1.0,
        linecolor="white",
        cbar_kws={"label": "Recovery", "shrink": 0.78, "pad": 0.025},
    )
    for row_i, annotation_row in enumerate(annotations):
        for col_i, text in enumerate(annotation_row):
            ax.text(
                col_i + 0.5,
                row_i + 0.5,
                text,
                ha="center",
                va="center",
                fontsize=6.15,
                fontweight="bold",
                color=TEXT,
            )
    ax.axvline(3, color="white", linewidth=2.4)
    ax.text(1.5 / 6, 1.06, "Builder view", ha="center", va="bottom", transform=ax.transAxes, fontsize=8.5, fontweight="bold")
    ax.text(4.5 / 6, 1.06, "Finder view", ha="center", va="bottom", transform=ax.transAxes, fontsize=8.5, fontweight="bold")
    ax.set_xlabel("")
    ax.set_ylabel("")
    ax.tick_params(axis="x", labelrotation=0, labelsize=7.3, length=0)
    ax.tick_params(axis="y", labelrotation=0, labelsize=7.3, length=0)
    fig.tight_layout(rect=[0.0, 0.0, 1.0, 0.98])
    _save(fig, "fig_robustness_slices")


def _heatmap_label(label: str) -> str:
    replacements = {
        "GPT-5.4-mini-low": "GPT-5.4\nmini-low",
        "GPT-5.4-mini": "GPT-5.4\nmini",
        "MiMo 2.5 Pro-low": "MiMo 2.5\nPro-low",
        "MiMo 2.5 Pro": "MiMo 2.5\nPro",
        "Sonnet 4.6-low": "Sonnet 4.6\nlow",
        "Opus 4.7-low": "Opus 4.7\nlow",
        "GPT-5.5-low": "GPT-5.5\nlow",
    }
    return replacements.get(label, label)


def _plot_builder_rb_forest() -> None:
    _plot_builder_rb_forest_from(
        "conditioned_builder_scores.csv",
        "fig_builder_rb_forest",
        impact=True,
    )


def _plot_low_prior_builder_rb_forest() -> None:
    _plot_builder_rb_forest_from(
        "low_prior_conditioned_builder_scores.csv",
        "fig_low_prior_builder_rb_forest",
        impact=True,
    )


def _plot_builder_rb_forest_from(source: str, stem: str, *, impact: bool = False) -> None:
    df = _read_impact_csv(source) if impact else _read_csv(source)
    df = df[
        (df["primary_panel"] == 1)
        & (df["gate"] == "perfect_run")
        & (df["metric"] == "total_bytes")
        & (df["panel_id"].isin(["high_effort", "low_effort"]))
    ].copy()
    df["label"] = df["builder_id"].map(_short_label)
    finite_high_all = df["ci95_high"].replace([np.inf, -np.inf], np.nan).dropna()
    x_max = max(2.18, float(finite_high_all.max()) + 0.22 if len(finite_high_all) else 2.18)
    coverage_x = x_max - 0.03

    fig, axes = plt.subplots(ncols=2, figsize=(7.4, 3.2), sharex=True)

    for ax, panel_id, title in zip(axes, ["high_effort", "low_effort"], ["High effort", "Low effort"], strict=True):
        work = df[df["panel_id"] == panel_id].sort_values("score", ascending=False).reset_index(drop=True)
        y = np.arange(len(work))
        for idx, row in work.iterrows():
            color = COLORS[row["family"]]
            ax.hlines(idx, row["ci95_low"], row["ci95_high"], color=color, linewidth=1.8)
            ax.plot(row["score"], idx, marker="o", markersize=4.8, markerfacecolor="white", markeredgecolor=color, markeredgewidth=1.4)
            ax.text(coverage_x, idx, f"{int(row['n_cells'])}/12", ha="right", va="center", fontsize=6.8, color=MUTED)
        ax.set_yticks(y, work["label"])
        ax.set_title(title, fontsize=8.5, fontweight="bold")
        ax.set_xlim(0.95, x_max)
        ax.text(coverage_x, len(work) - 0.28, "cells", ha="right", va="bottom", fontsize=6.5, color=MUTED)
        ax.set_xlabel(TOTAL_BYTE_RB_LABEL)
        ax.grid(axis="x", color=GRID)
        ax.grid(axis="y", visible=False)
        ax.tick_params(axis="both", labelsize=7.3)
        sns.despine(ax=ax, left=True, bottom=False)

    _figure_legend(fig, y=-0.005)
    fig.tight_layout(rect=[0.0, 0.045, 1.0, 1.0])
    _save(fig, stem)


def _write_task_recovery_gate_table() -> None:
    _write_task_recovery_gate_table_from("conditioned_run_rows.csv", "tab_task_recovery_gate.tex")


def _write_low_prior_task_recovery_gate_table() -> None:
    _write_task_recovery_gate_table_from(
        "low_prior_conditioned_run_rows.csv",
        "tab_low_prior_task_recovery_gate.tex",
    )


def _write_task_recovery_gate_table_from(source: str, stem: str) -> None:
    df = _read_impact_csv(source)
    task_order = ["scratch_minidb", "scratch_nanoweb"]
    task_labels = {
        "scratch_minidb": r"\texttt{minidb}",
        "scratch_nanoweb": r"\texttt{nanoweb}",
    }
    effort_agent_ids = {
        "high": [agent_id for agent_id in AGENT_ORDER if agent_id.endswith("_high")],
        "low": [agent_id for agent_id in AGENT_ORDER if agent_id.endswith("_low")],
    }

    lines = [
        r"\begin{minipage}[t]{0.49\textwidth}",
        r"\centering",
        r"\textbf{High effort}\\[0.25em]",
        r"\scriptsize",
        r"\setlength{\tabcolsep}{2.7pt}",
        rf"\begin{{tabular}}{{@{{}}l{'c' * len(task_order)}@{{}}}}",
        r"\toprule",
        "Builder & " + " & ".join(task_labels[task_id] for task_id in task_order) + r" \\",
        r"\midrule",
    ]
    lines.extend(_task_recovery_table_rows(df, effort_agent_ids["high"], "high", task_order))
    lines.extend(
        [
            r"\bottomrule",
            r"\end{tabular}",
            r"\end{minipage}\hfill",
            r"\begin{minipage}[t]{0.49\textwidth}",
            r"\centering",
            r"\textbf{Low effort}\\[0.25em]",
            r"\scriptsize",
            r"\setlength{\tabcolsep}{2.7pt}",
            rf"\begin{{tabular}}{{@{{}}l{'c' * len(task_order)}@{{}}}}",
            r"\toprule",
            "Builder & " + " & ".join(task_labels[task_id] for task_id in task_order) + r" \\",
            r"\midrule",
        ]
    )
    lines.extend(_task_recovery_table_rows(df, effort_agent_ids["low"], "low", task_order))
    lines.extend(
        [
            r"\bottomrule",
            r"\end{tabular}",
            r"\end{minipage}",
            "",
        ]
    )
    (OUT / stem).write_text("\n".join(lines), encoding="utf-8")


def _task_recovery_table_rows(
    df: pd.DataFrame,
    agent_ids: list[str],
    effort: str,
    task_order: list[str],
) -> list[str]:
    panel = df[(df["builder_effort"] == effort) & (df["finder_effort"] == effort)].copy()
    task_max_cells = {
        task_id: panel[panel["task_id"] == task_id]["finder_id"].nunique()
        for task_id in task_order
    }
    missing_max = [task_id for task_id, max_cells in task_max_cells.items() if max_cells <= 0]
    if missing_max:
        raise ValueError(f"no finder cells for {effort=} tasks={missing_max}")

    rows: list[str] = []
    for agent_id in agent_ids:
        cells = []
        for task_id in task_order:
            task = panel[(panel["builder_id"] == agent_id) & (panel["task_id"] == task_id)]
            by_finder = task.groupby("finder_id")["accuracy"].mean()
            cells.append(_format_task_recovery_cell(by_finder, task_max_cells[task_id]))
        rows.append(_short_label(agent_id) + " & " + " & ".join(cells) + r" \\")
    return rows


def _format_task_recovery_cell(by_finder: pd.Series, max_cells: int) -> str:
    n_cells = int(by_finder.count())
    if n_cells == 0:
        return rf"-- (0/{max_cells})"
    return rf"{float(by_finder.mean()) * 100:.1f}\% ({n_cells}/{max_cells})"


def _repel_scatter_labels(
    ax,
    texts: list[plt.Text],
    x: list[float],
    y: list[float],
    *,
    target_x: list[float] | None = None,
    target_y: list[float] | None = None,
) -> None:
    if not texts:
        return
    targets_x = x if target_x is None else target_x
    targets_y = y if target_y is None else target_y
    adjust_text(
        texts,
        x=x,
        y=y,
        target_x=targets_x,
        target_y=targets_y,
        ax=ax,
        avoid_self=True,
        prevent_crossings=True,
        ensure_inside_axes=True,
        expand_axes=False,
        force_text=(0.16, 0.26),
        force_static=(0.20, 0.30),
        force_pull=(0.01, 0.02),
        force_explode=(0.16, 0.24),
        expand=(1.08, 1.18),
        max_move=(12, 10),
        min_arrow_len=4,
        arrowprops={
            "arrowstyle": "-",
            "color": "#64748b",
            "linewidth": 0.55,
            "shrinkA": 1.5,
            "shrinkB": 4.5,
        },
        iter_lim=500,
    )


def _plot_rank_stability_heatmap() -> None:
    _plot_rank_stability_heatmap_from(
        "conditioned_rank_stability.csv",
        "fig_rank_stability_heatmap",
        impact=True,
    )


def _plot_low_prior_rank_stability_heatmap() -> None:
    _plot_rank_stability_heatmap_from(
        "low_prior_conditioned_rank_stability.csv",
        "fig_low_prior_rank_stability_heatmap",
        impact=True,
    )


def _plot_rank_stability_heatmap_from(source: str, stem: str, *, impact: bool = False) -> None:
    df = _read_impact_csv(source) if impact else _read_csv(source)
    df = df[
        (df["primary_panel"] == 1)
        & (df["gate"] == "perfect_run")
        & (df["metric"] == "total_bytes")
        & (df["panel_id"].isin(["high_effort", "low_effort"]))
    ].copy()
    row_order = [
        ("high_effort", "scratch_minidb", "High / minidb"),
        ("high_effort", "scratch_nanoweb", "High / nanoweb"),
        ("low_effort", "scratch_minidb", "Low / minidb"),
        ("low_effort", "scratch_nanoweb", "Low / nanoweb"),
    ]
    col_order = [
        ("all_pairs", "All"),
        ("same_family_pairs", "Same-family"),
        ("cross_family_pairs", "Cross-family"),
    ]
    lookup = df.set_index(["panel_id", "task_id", "slice_id"])
    values = pd.DataFrame(
        [
            [lookup.loc[(panel, task, slice_id), "median_tau"] for slice_id, _ in col_order]
            for panel, task, _ in row_order
        ],
        index=[label for _, _, label in row_order],
        columns=[label for _, label in col_order],
    )
    annot = pd.DataFrame(
        [
            [
                f"{lookup.loc[(panel, task, slice_id), 'median_tau']:.2f}\n"
                f"{lookup.loc[(panel, task, slice_id), 'positive_fraction'] * 100:.1f}%+"
                for slice_id, _ in col_order
            ]
            for panel, task, _ in row_order
        ],
        index=values.index,
        columns=values.columns,
    )

    fig, ax = plt.subplots(figsize=(5.7, 3.05))
    sns.heatmap(
        values,
        ax=ax,
        cmap=sns.light_palette(COLORS["codex"], as_cmap=True),
        vmin=0,
        vmax=1,
        linewidths=1.0,
        linecolor="white",
        annot=annot,
        fmt="",
        annot_kws={"fontsize": 8, "fontweight": "bold"},
        cbar_kws={"label": KENDALL_TAU_LABEL, "shrink": 0.8, "pad": 0.035},
    )
    ax.set_xlabel("")
    ax.set_ylabel("")
    ax.tick_params(axis="x", labelrotation=0, labelsize=7.6)
    ax.tick_params(axis="y", labelrotation=0, labelsize=7.6)
    fig.tight_layout()
    _save(fig, stem)


def _plot_token_pareto() -> None:
    df = _read_csv("pivots/agent_token_diagnostics.csv")
    df = df[
        ((df["role"] == "builder") & (df["view_id"] == "all_finders"))
        | ((df["role"] == "finder") & (df["view_id"] == "all_builders"))
    ].copy()
    df["label"] = df["agent_id"].map(_label)
    df["mean_tokens_m"] = df["mean_tokens"] / 1_000_000.0
    df["accuracy_pct"] = df["accuracy"] * 100.0

    fig, axes = plt.subplots(ncols=2, figsize=(7.4, 3.25))

    for ax, role, title, xlim, ylim in (
        (axes[0], "builder", "Builders", (0, 16), (88, 100)),
        (axes[1], "finder", "Finders", (0, 1.0), (92, 100)),
    ):
        work = df[df["role"] == role]
        texts = []
        point_x = []
        point_y = []
        label_x = []
        label_y = []
        for _, row in work.iterrows():
            alpha = 0.95 if row["pareto_frontier"] else 0.45
            size = 32 if row["pareto_frontier"] else 22
            ax.scatter(row["mean_tokens_m"], row["accuracy_pct"], s=size, color=COLORS[row["family"]], alpha=alpha, edgecolor="white", linewidth=0.4)
            point_x.append(row["mean_tokens_m"])
            point_y.append(row["accuracy_pct"])
            if row["pareto_frontier"]:
                texts.append(
                    ax.text(
                        row["mean_tokens_m"],
                        row["accuracy_pct"],
                        row["label"],
                        fontsize=6.5,
                        color=TEXT,
                    )
                )
                label_x.append(row["mean_tokens_m"])
                label_y.append(row["accuracy_pct"])
        ax.set_title(title, fontsize=8.5, fontweight="bold")
        ax.set_xlim(*xlim)
        ax.set_ylim(*ylim)
        ax.set_xlabel("Mean vendor tokens per run (M)")
        ax.yaxis.set_major_formatter(lambda value, _pos: f"{value:.0f}%")
        ax.grid(True, color=GRID)
        ax.tick_params(axis="both", labelsize=7.3)
        sns.despine(ax=ax)
        _repel_scatter_labels(ax, texts, point_x, point_y, target_x=label_x, target_y=label_y)
    axes[0].set_ylabel("Recovery accuracy")
    axes[1].set_ylabel("")
    _figure_legend(fig, y=-0.005)
    fig.tight_layout(rect=[0.0, 0.045, 1.0, 1.0])
    _save(fig, "fig_token_pareto")


def _plot_effort_frontier() -> None:
    df = _read_impact_csv("conditioned_builder_scores.csv")
    df = df[
        (df["primary_panel"] == 1)
        & (df["gate"] == "perfect_run")
        & (df["metric"].isin(["total_bytes", "tool_call_count"]))
        & (df["panel_id"].isin(["high_effort", "low_effort"]))
    ].copy()
    wide = df.pivot_table(
        index=["panel_id", "builder_id", "family", "n_cells"],
        columns="metric",
        values="score",
        aggfunc="first",
    ).reset_index()
    wide["label"] = wide["builder_id"].map(_label)

    fig, axes = plt.subplots(ncols=2, figsize=(7.4, 3.25), sharey=True)
    for ax, panel_id, title in zip(axes, ["high_effort", "low_effort"], ["High effort", "Low effort"], strict=True):
        work = wide[wide["panel_id"] == panel_id]
        texts = []
        point_x = []
        point_y = []
        for _, row in work.iterrows():
            ax.scatter(row["total_bytes"], row["tool_call_count"], s=28, color=COLORS[row["family"]], edgecolor="white", linewidth=0.5)
            suffix = "" if int(row["n_cells"]) == 12 else f"\nn={int(row['n_cells'])}"
            texts.append(
                ax.text(
                    row["total_bytes"],
                    row["tool_call_count"],
                    f"{_short_label(row['builder_id'])}{suffix}",
                    fontsize=6.5,
                    color=TEXT,
                )
            )
            point_x.append(row["total_bytes"])
            point_y.append(row["tool_call_count"])
        ax.axvline(1.25, color="#cbd5e1", linestyle="--", linewidth=0.9)
        ax.axhline(1.25, color="#cbd5e1", linestyle="--", linewidth=0.9)
        ax.set_title(title, fontsize=8.5, fontweight="bold")
        ax.set_xlim(0.95, 2.12)
        ax.set_ylim(0.95, 1.67)
        ax.set_xlabel(TOTAL_BYTE_RB_LABEL)
        ax.grid(True, color=GRID)
        ax.tick_params(axis="both", labelsize=7.3)
        sns.despine(ax=ax)
        _repel_scatter_labels(ax, texts, point_x, point_y)
    axes[0].set_ylabel(TOOL_CALL_COUNT_RB_LABEL)
    axes[1].set_ylabel("")
    _figure_legend(fig, y=-0.005)
    fig.tight_layout(rect=[0.0, 0.045, 1.0, 1.0])
    _save(fig, "fig_effort_frontier")


def _plot_affinity_residual() -> None:
    _plot_affinity_residual_from(
        "conditioned_affinity_residual_12x12.csv",
        "recovery_affinity_residual_12x12_paper",
        impact=True,
    )


def _plot_low_prior_affinity_residual() -> None:
    _plot_affinity_residual_from(
        "low_prior_conditioned_affinity_residual_12x12.csv",
        "fig_low_prior_affinity_residual_12x12",
        impact=True,
    )


def _plot_affinity_residual_from(source: str, stem: str, *, impact: bool = False) -> None:
    df = _read_impact_csv(source) if impact else _read_csv(source)
    work = df.assign(
        builder_label=df["builder_id"].map(_label),
        finder_label=df["finder_id"].map(_label),
    )
    label_order = [_label(agent_id) for agent_id in AGENT_ORDER]
    matrix = work.pivot(
        index="builder_label",
        columns="finder_label",
        values="affinity_residual",
    ).loc[label_order, label_order]
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
        cbar_kws={"label": "Affinity residual", "shrink": 0.68, "pad": 0.035},
    )
    ax.axvline(6, color="#111827", linewidth=1.9)
    ax.axhline(6, color="#111827", linewidth=1.9)
    ax.set_xlabel("Finder", labelpad=8, fontsize=8.5)
    ax.set_ylabel("Builder", labelpad=26, fontsize=8.5)
    ax.tick_params(axis="x", labelrotation=45, labelsize=5.8, pad=2)
    ax.tick_params(axis="y", labelrotation=0, labelsize=5.9, pad=2)
    for tick in ax.get_xticklabels():
        tick.set_horizontalalignment("right")
        tick.set_rotation_mode("anchor")
    for spine in ax.spines.values():
        spine.set_visible(False)
    ax.collections[0].colorbar.ax.tick_params(labelsize=7)
    ax.collections[0].colorbar.ax.yaxis.label.set_size(7.5)
    fig.tight_layout()
    _save(fig, stem)


def _write_readme() -> None:
    reports_path = _display_path(REPORTS)
    impact_path = _display_path(IMPACT_REPORTS)
    md = f"""# Paper figures

Figures are rendered from derived CSVs under `{reports_path}` and audited
scoring outputs under `{impact_path}`.
Regenerate with:

```bash
make paper-figures
```

The script writes PDF, PNG, and SVG versions of the ordering, metric-comparison,
slice-comparison, rank-stability, low-prior appendix, affinity, and
token-diagnostic figures used by the NeurIPS E&D manuscript.
"""
    (OUT / "README.md").write_text(md, encoding="utf-8")


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


if __name__ == "__main__":
    main()
