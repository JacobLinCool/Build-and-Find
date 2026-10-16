#!/usr/bin/env python3
"""Primary-panel sensitivity to runs whose transcripts reach outside their workspace.

``reports/eandd_final/transcript_audit/primary_panel_exposure.csv`` lists the
primary-panel (compile-pass) find runs whose transcripts show reads, file
listings, or git commands that reach outside their workspace. This script
recomputes primary-panel accuracy over all answer rows and over implemented-gold
rows, and the R_b builder scores and ranks of both effort panels, once on all
runs and once without the listed runs. The paper's analyses keep every run; this
is a sensitivity analysis only.

Metric definitions are delegated to ``analyze_artifact_conformance_impact.py``
(implemented-gold rows, conditioned runs, perfect-run cell means of total bytes,
and the geometric-mean R_b score). Implemented-gold rows use the v1.1 labels: the
released v1.0 labels with ``label_corrections_v1_1.csv`` applied in memory. The
released label file is never modified. Percentages are rounded half up on the
exact rational.
"""

# ruff: noqa: E402

from __future__ import annotations

import argparse
import csv
import json
import math
import sys
from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
from decimal import Decimal
from fractions import Fraction
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
for import_path in (ROOT / "scripts", ROOT / "src", ROOT):
    if str(import_path) not in sys.path:
        sys.path.insert(0, str(import_path))

import analyze_artifact_conformance_impact as impact
from bnf.config import Paths, paths


ANALYSIS_ID = "transcript_exposure_v1"
OUTPUT_PARTS = ("eandd_final", "transcript_audit")
EXPOSURE_NAME = "primary_panel_exposure.csv"
EXPOSURE_FIELDS = ("find_run_id", "finder_id", "task_id", "category", "note")
CATEGORIES = (
    "answer_files_in_search_output",
    "host_config_read",
    "git_status_listing",
    "workspace_listing",
    "git_checkout_attempt",
)
ALL_RUNS = "all_runs"
EXCLUDING_LISTED = "excluding_listed_runs"
RB_GATE, RB_METRIC = impact.PRIMARY_FIGURE2_KEY
ACCURACY_PLACES = 3
MAX_CHANGE_PLACES = 2

LabelKey = tuple[str, str, str]


@dataclass(frozen=True)
class ExposedRun:
    find_run_id: str
    finder_id: str
    task_id: str
    category: str
    note: str


@dataclass(frozen=True)
class Rate:
    correct: int
    total: int

    @classmethod
    def of(cls, outcomes: Iterable[bool]) -> Rate:
        correct = total = 0
        for outcome in outcomes:
            correct += int(outcome)
            total += 1
        return cls(correct, total)

    @property
    def value(self) -> Fraction:
        if not self.total:
            raise ZeroDivisionError("rate over zero rows")
        return Fraction(self.correct, self.total)


@dataclass(frozen=True)
class BuilderRank:
    builder_id: str
    rank: int
    score: float
    n_cells: int


@dataclass(frozen=True)
class Variant:
    variant_id: str
    excluded_runs: int
    find_runs: int
    all_rows: Rate
    implemented_gold: Rate
    rb: dict[str, list[BuilderRank]]


@dataclass(frozen=True)
class Analysis:
    inputs: dict[str, Path]
    generated_at: str | None
    exposures: list[ExposedRun]
    all_runs: Variant
    excluding: Variant
    label_sources: dict[str, int]

    def accuracy_change(self, scope: str) -> Fraction:
        return getattr(self.excluding, scope).value - getattr(self.all_runs, scope).value

    @property
    def max_abs_change(self) -> Fraction:
        return max(abs(self.accuracy_change(scope)) for scope in ("all_rows", "implemented_gold"))

    def ranks_unchanged(self, panel_id: str) -> bool:
        def ranks(variant: Variant) -> dict[str, int]:
            return {row.builder_id: row.rank for row in variant.rb[panel_id]}

        return ranks(self.all_runs) == ranks(self.excluding)


def main() -> None:
    args = _parse_args()
    p = paths()
    out_dir = (args.output_dir or p.reports_dir.joinpath(*OUTPUT_PARTS)).resolve()
    exposure_path = (
        args.exposure or p.reports_dir.joinpath(*OUTPUT_PARTS, EXPOSURE_NAME)
    ).resolve()
    analysis = analyze(p, exposure_path=exposure_path)
    write_outputs(analysis, out_dir)
    print(f"wrote {impact._display_path(out_dir)}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--exposure",
        type=Path,
        default=None,
        help="Listed primary-panel runs (default: the released list under the reports root).",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Output directory (default: reports/eandd_final/transcript_audit).",
    )
    return parser.parse_args()


# --------------------------------------------------------------------------- inputs


def analysis_inputs(p: Paths, *, exposure_path: Path) -> dict[str, Path]:
    labels_dir = p.reports_dir / "artifact_conformance_consensus" / "final"
    return {
        "find_runs": p.tables_dir / "find_runs.csv",
        "question_answers": p.tables_dir / "question_answers.csv",
        "table_manifest": p.tables_dir / "MANIFEST.json",
        "exposure": exposure_path,
        "final_labels": labels_dir / "artifact_conformance_final.csv",
        "label_corrections": labels_dir / impact.LABEL_CORRECTIONS_FILENAME,
    }


def load_exposures(path: Path) -> list[ExposedRun]:
    """Read and validate the listed primary-panel runs."""
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if tuple(reader.fieldnames or ()) != EXPOSURE_FIELDS:
            raise ValueError(f"{path.name}: expected columns {', '.join(EXPOSURE_FIELDS)}")
        runs = [ExposedRun(**{key: row[key].strip() for key in EXPOSURE_FIELDS}) for row in reader]
    seen: set[str] = set()
    for run in runs:
        if run.category not in CATEGORIES:
            raise ValueError(f"{run.find_run_id}: unknown category {run.category!r}")
        if not run.note:
            raise ValueError(f"{run.find_run_id}: missing note")
        if run.find_run_id in seen:
            raise ValueError(f"{run.find_run_id}: listed twice")
        seen.add(run.find_run_id)
    return runs


def validate_exposures(exposures: list[ExposedRun], find_rows: list[dict[str, str]]) -> None:
    """Require every listed run to be a compile-pass primary-panel run with matching metadata."""
    runs = {row["find_run_id"]: row for row in find_rows}
    for run in exposures:
        row = runs.get(run.find_run_id)
        if row is None or not impact._in_panel(row, impact.PAPER_FINAL_PANEL_ID):
            raise ValueError(f"{run.find_run_id}: not a primary-panel find run")
        if not impact._as_bool(row["compile_pass"]):
            raise ValueError(f"{run.find_run_id}: artifact did not compile")
        if (row["finder_id"], row["task_id"]) != (run.finder_id, run.task_id):
            raise ValueError(
                f"{run.find_run_id}: finder/task {(run.finder_id, run.task_id)} does not match "
                f"{(row['finder_id'], row['task_id'])}"
            )


def load_labels(
    final_path: Path, corrections_path: Path
) -> tuple[dict[LabelKey, dict[str, str]], Counter[str]]:
    """Return v1.1 labels keyed by artifact question, and their label-source counts."""
    rows = impact._apply_label_corrections(
        impact._read_csv(final_path), impact._read_csv(corrections_path)
    )
    label_by_key = {
        impact._artifact_question_key(row): row
        for row in rows
        if row["task_id"] in impact.SCRATCH_TASKS
    }
    return label_by_key, Counter(row["label_source"] for row in rows)


# --------------------------------------------------------------------------- analysis


def builder_ranks(runs: list[dict[str, Any]], panel: impact.Panel) -> list[BuilderRank]:
    """R_b of one effort panel: perfect-run cell means of total bytes, as in Figure 2."""
    means = impact._cell_means(impact._panel_run_rows(runs, panel), metric=RB_METRIC, gate=RB_GATE)
    return [
        BuilderRank(
            builder_id=row["builder_id"],
            rank=row["rank"],
            score=row["score"],
            n_cells=row["n_cells"],
        )
        for row in impact._builder_score_rows(panel, RB_GATE, RB_METRIC, means)
    ]


def primary_variant(
    variant_id: str,
    *,
    find_rows: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
    label_by_key: dict[LabelKey, dict[str, str]],
    excluded: set[str],
) -> Variant:
    finds = [row for row in find_rows if row["find_run_id"] not in excluded]
    answers = [row for row in answer_rows if row["find_run_id"] not in excluded]
    primary = [row for row in answers if impact._in_panel(row, impact.PAPER_FINAL_PANEL_ID)]
    implemented_gold = impact._conditioned_answer_rows(
        answer_rows=answers,
        label_by_key=label_by_key,
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
    )
    runs = impact._conditioned_run_rows(
        find_rows=finds,
        answer_rows=answers,
        label_by_key=label_by_key,
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
    )
    return Variant(
        variant_id=variant_id,
        excluded_runs=len(excluded),
        find_runs=len({row["find_run_id"] for row in primary}),
        all_rows=Rate.of(impact._as_bool(row["is_correct"]) for row in primary),
        implemented_gold=Rate.of(bool(row["is_correct"]) for row in implemented_gold),
        rb={panel.panel_id: builder_ranks(runs, panel) for panel in impact._panels()},
    )


def analyze(p: Paths, *, exposure_path: Path) -> Analysis:
    inputs = analysis_inputs(p, exposure_path=exposure_path)
    missing = [impact._display_path(path) for path in inputs.values() if not path.exists()]
    if missing:
        raise FileNotFoundError(f"missing inputs: {', '.join(missing)}")

    find_rows = impact._read_csv(inputs["find_runs"])
    answer_rows = impact._read_csv(inputs["question_answers"])
    exposures = load_exposures(inputs["exposure"])
    validate_exposures(exposures, find_rows)
    label_by_key, label_sources = load_labels(inputs["final_labels"], inputs["label_corrections"])
    table_manifest = json.loads(inputs["table_manifest"].read_text(encoding="utf-8"))

    def variant(variant_id: str, excluded: set[str]) -> Variant:
        return primary_variant(
            variant_id,
            find_rows=find_rows,
            answer_rows=answer_rows,
            label_by_key=label_by_key,
            excluded=excluded,
        )

    return Analysis(
        inputs=inputs,
        generated_at=table_manifest.get("generated_at"),
        exposures=exposures,
        all_runs=variant(ALL_RUNS, set()),
        excluding=variant(EXCLUDING_LISTED, {run.find_run_id for run in exposures}),
        label_sources=dict(sorted(label_sources.items())),
    )


# --------------------------------------------------------------------------- formatting


def round_half_up(value: Fraction, places: int) -> Decimal:
    """Round an exact rational to ``places`` decimals, halves away from zero."""
    scaled = abs(value) * 10**places
    rounded = math.floor(scaled + Fraction(1, 2))
    return Decimal(rounded if value >= 0 else -rounded).scaleb(-places)


def percent(value: Fraction, places: int = ACCURACY_PLACES) -> str:
    return f"{round_half_up(100 * value, places):.{places}f}"


def signed_points(value: Fraction, places: int = ACCURACY_PLACES) -> str:
    return f"{round_half_up(100 * value, places):+.{places}f}"


# --------------------------------------------------------------------------- outputs


def write_outputs(analysis: Analysis, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.json").write_text(
        json.dumps(_summary(analysis), indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (out_dir / "report.md").write_text(_report(analysis), encoding="utf-8")


def _rate_fields(rate: Rate) -> dict[str, Any]:
    return {"correct": rate.correct, "total": rate.total, "pct": percent(rate.value)}


def _rb_rows(analysis: Analysis, panel_id: str) -> list[dict[str, Any]]:
    excluding = {row.builder_id: row for row in analysis.excluding.rb[panel_id]}
    rows = []
    for row in analysis.all_runs.rb[panel_id]:
        other = excluding[row.builder_id]
        rows.append(
            {
                "builder_id": row.builder_id,
                f"rank_{ALL_RUNS}": row.rank,
                f"rank_{EXCLUDING_LISTED}": other.rank,
                f"score_{ALL_RUNS}": round(row.score, 6),
                f"score_{EXCLUDING_LISTED}": round(other.score, 6),
                f"n_cells_{ALL_RUNS}": row.n_cells,
                f"n_cells_{EXCLUDING_LISTED}": other.n_cells,
            }
        )
    return rows


def _summary(analysis: Analysis) -> dict[str, Any]:
    categories = Counter(run.category for run in analysis.exposures)
    panel_ids = list(analysis.all_runs.rb)
    accuracy = {}
    for scope in ("all_rows", "implemented_gold"):
        accuracy[scope] = {
            ALL_RUNS: _rate_fields(getattr(analysis.all_runs, scope)),
            EXCLUDING_LISTED: _rate_fields(getattr(analysis.excluding, scope)),
            "change_pp": signed_points(analysis.accuracy_change(scope)),
        }
    return {
        "analysis_id": ANALYSIS_ID,
        "analysis_script": "scripts/analyze_transcript_exposure.py",
        "generated_at": analysis.generated_at,
        "inputs": {name: impact._display_path(path) for name, path in analysis.inputs.items()},
        "input_hashes": {
            impact._display_path(path): impact._sha256(path) for path in analysis.inputs.values()
        },
        "exclusion_rule": (
            "primary-panel runs whose transcripts show reads, file listings, or git commands "
            "that reach outside their workspace"
        ),
        "listed_runs": len(analysis.exposures),
        "listed_runs_by_category": {category: categories[category] for category in CATEGORIES},
        "primary_find_runs": {
            ALL_RUNS: analysis.all_runs.find_runs,
            EXCLUDING_LISTED: analysis.excluding.find_runs,
        },
        "labels": "v1.0 final labels with label_corrections_v1_1.csv applied",
        "label_sources_v1_1": analysis.label_sources,
        "rounding": "round half up on the exact rational",
        "accuracy": accuracy,
        "max_abs_accuracy_change_pp": percent(analysis.max_abs_change, MAX_CHANGE_PLACES),
        "max_abs_accuracy_change_pp_exact": f"{float(100 * analysis.max_abs_change):.4f}",
        "rb": {
            "gate": RB_GATE,
            "metric": RB_METRIC,
            "panels": {
                panel_id: {
                    "ranks_unchanged": analysis.ranks_unchanged(panel_id),
                    "builders": _rb_rows(analysis, panel_id),
                }
                for panel_id in panel_ids
            },
        },
        "rb_ranks_unchanged": all(analysis.ranks_unchanged(panel_id) for panel_id in panel_ids),
        "outputs": ["summary.json", "report.md"],
    }


def _report(analysis: Analysis) -> str:
    categories = Counter(run.category for run in analysis.exposures)
    n_listed = len(analysis.exposures)
    lines = [
        "# Primary-panel transcript exposure",
        "",
        "Generated by `scripts/analyze_transcript_exposure.py`; do not edit by hand.",
        "",
        f"`{EXPOSURE_NAME}` lists {n_listed} of the {analysis.all_runs.find_runs} primary-panel "
        "find runs whose transcripts show reads, file listings, or git commands that reach "
        "outside their workspace (see `README.md`):",
        "",
        "| category | runs |",
        "| --- | ---: |",
        *(f"| `{category}` | {categories[category]} |" for category in CATEGORIES),
        "",
        "Excluding these runs is a sensitivity analysis; every other analysis keeps them.",
        "Implemented-gold rows use the v1.0 labels with `label_corrections_v1_1.csv` applied.",
        "Percentages are rounded half up on the exact rational.",
        "",
        "## Primary-panel accuracy",
        "",
        f"| rows | all runs | excluding the {n_listed} listed runs | change (pp) |",
        "| --- | ---: | ---: | ---: |",
    ]
    for scope, label in (("all_rows", "all answer rows"), ("implemented_gold", "implemented-gold")):
        before = getattr(analysis.all_runs, scope)
        after = getattr(analysis.excluding, scope)
        lines.append(
            f"| {label} | {percent(before.value)}% ({before.correct}/{before.total}) | "
            f"{percent(after.value)}% ({after.correct}/{after.total}) | "
            f"{signed_points(analysis.accuracy_change(scope))} |"
        )
    lines.extend(
        [
            "",
            "Largest absolute change: "
            f"{percent(analysis.max_abs_change, MAX_CHANGE_PLACES)} pp "
            f"(exact {float(100 * analysis.max_abs_change):.4f} pp).",
            "",
            "## R_b ranks",
            "",
            f"R_b uses the `{RB_GATE}` gate and `{RB_METRIC}`, as in Figure 2. Every rank "
            + (
                "is unchanged in both panels."
                if all(analysis.ranks_unchanged(panel_id) for panel_id in analysis.all_runs.rb)
                else "is NOT unchanged; see the tables below."
            ),
        ]
    )
    for panel_id in analysis.all_runs.rb:
        lines.extend(
            [
                "",
                f"### `{panel_id}` (ranks unchanged: "
                f"{'yes' if analysis.ranks_unchanged(panel_id) else 'NO'})",
                "",
                "| builder | rank, all runs | rank, excluding | R_b, all runs | R_b, excluding "
                "| cells, all runs | cells, excluding |",
                "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
            ]
        )
        for row in _rb_rows(analysis, panel_id):
            lines.append(
                f"| `{row['builder_id']}` | {row[f'rank_{ALL_RUNS}']} | "
                f"{row[f'rank_{EXCLUDING_LISTED}']} | {row[f'score_{ALL_RUNS}']:.3f} | "
                f"{row[f'score_{EXCLUDING_LISTED}']:.3f} | {row[f'n_cells_{ALL_RUNS}']} | "
                f"{row[f'n_cells_{EXCLUDING_LISTED}']} |"
            )
    lines.append("")
    return "\n".join(lines)


if __name__ == "__main__":
    main()
