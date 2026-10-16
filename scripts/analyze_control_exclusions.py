#!/usr/bin/env python3
"""Question-only control exclusions and the calibration values that depend on them.

The question-only control excludes runs whose transcripts show reads outside
their workspace. The excluded runs are listed in
``reports/eandd_final/control_exclusions/excluded_question_only_runs.csv``.
This script removes them from the control and recomputes every value that
depends on the question-only control: the calibration rows (Tables 1, 4 and 14),
the repeatability rows (Tables 2 and 15), the per-finder calibration (Table 9),
the frozen low-prior subset, the answer-bearing-only sensitivity, and a rerun of
the core-finder selector.

Differences from the question-only control use matched strata: when a control
run is excluded, the artifact-conditioned and spec-only rows of the same finder,
task, and finder trial leave the contrast as well. Intervals replay the released
question-cluster bootstrap of ``analyze_eandd_calibration_uncertainty.py``
(2000 resamples, RNG seeded with ``20260602:<contrast>[:<task>]``) on exact
fractions, so every printed value is rounded half up on the exact rational. The
script first replays the released ``bootstrap_lift.csv`` and stops if it does not
reproduce exactly.

Implemented-gold columns use the v1.1 labels: the released v1.0 labels with
``label_corrections_v1_1.csv`` applied in memory. The released label file is
never modified.
"""

# ruff: noqa: E402

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import random
import sys
from collections import Counter, defaultdict
from collections.abc import Callable, Hashable, Iterable
from dataclasses import dataclass, replace
from decimal import Decimal
from fractions import Fraction
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.analysis.tables import ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID, PAPER_FINAL_PANEL_ID
from bnf.config import Paths, paths
from scripts.analyze_artifact_conformance_impact import (
    _apply_label_corrections,
    _artifact_question_key,
)
from scripts.analyze_eandd_calibration_uncertainty import BOOTSTRAP_SAMPLES, BOOTSTRAP_SEED
from scripts.select_eandd_core_finders import (
    _audit_gate_failures,
    _choose_audit_finders,
    _core_gate_failures,
    _finder_rank_stability,
    _load_affinity,
    _load_finder_summary,
    _sort_selection_rows,
)
from scripts.select_eandd_core_finders import _write_csv as _write_selection_csv


ANALYSIS_ID = "control_exclusions_v1"
OUTPUT_PARTS = ("eandd_final", "control_exclusions")
EXCLUSIONS_NAME = "excluded_question_only_runs.csv"
EXCLUSION_FIELDS = (
    "find_run_id",
    "finder_id",
    "task_id",
    "finder_trial_id",
    "exposure",
    "evidence",
)
EXPOSURES = ("answer_bearing", "seed_project", "other_artifact")
ANSWER_BEARING = "answer_bearing"
QUESTION_ONLY_PANEL_ID = "question_only"
SPEC_ONLY_PANEL_ID = "spec_only"
TASK_IDS = ("scratch_minidb", "scratch_nanoweb")
ALL_TASKS = "all"
IMPLEMENTED_GOLD = "implemented_gold"
RELIABLE_AGREEMENT = Fraction(9, 10)
LOW_PRIOR_AGREEMENT = Fraction(9, 10)
SLICE_CONTRASTS = {
    "spec_only": "spec_only_minus_question_only",
    "artifact_present": "artifact_present_minus_question_only",
    "compile_pass": "compile_pass_minus_question_only",
    "compile_failed": "compile_failed_minus_question_only",
}
LOW_PRIOR_CONTRAST_ID = "low_prior_compile_pass_minus_question_only"
QUESTION_SETS = ("all", "low_prior", "implementation_signal")
SELECTION_FIELDS = [
    "selection",
    "finder_id",
    "family",
    "base_model",
    "effort",
    "formal_accuracy",
    "formal_run_ci95_low",
    "formal_run_ci95_high",
    "formal_ci_width",
    "question_only_accuracy",
    "formal_minus_question_only",
    "low_prior_formal_accuracy",
    "low_prior_question_only_accuracy",
    "low_prior_lift",
    "implementation_signal_formal_accuracy",
    "implementation_signal_question_only_accuracy",
    "implementation_signal_lift",
    "spec_only_accuracy",
    "mean_find_tokens",
    "median_find_tokens",
    "rank_stability_mean_tau",
    "rank_stability_median_tau",
    "same_family_minus_different_family",
    "same_base_minus_different_base",
    "selection_reason",
]

Cluster = tuple[str, str]
Stratum = tuple[str, str, str]
LabelKey = tuple[str, str, str]


@dataclass(frozen=True)
class Answer:
    find_run_id: str
    build_run_id: str
    task_id: str
    question_id: str
    finder_id: str
    finder_trial_id: str
    correct: bool
    panels: frozenset[str]

    @property
    def cluster(self) -> Cluster:
        return (self.task_id, self.question_id)

    @property
    def stratum(self) -> Stratum:
        return (self.finder_id, self.task_id, self.finder_trial_id)


@dataclass(frozen=True)
class ExcludedRun:
    find_run_id: str
    finder_id: str
    task_id: str
    finder_trial_id: str
    exposure: str
    evidence: str

    @property
    def stratum(self) -> Stratum:
        return (self.finder_id, self.task_id, self.finder_trial_id)


@dataclass(frozen=True)
class Rate:
    correct: int
    total: int

    @classmethod
    def of(cls, rows: Iterable[Answer]) -> Rate:
        correct = total = 0
        for row in rows:
            correct += int(row.correct)
            total += 1
        return cls(correct, total)

    @property
    def value(self) -> Fraction:
        if not self.total:
            raise ZeroDivisionError("rate over zero rows")
        return Fraction(self.correct, self.total)


@dataclass(frozen=True)
class Contrast:
    contrast_id: str
    task_id: str
    clusters: int
    treatment_rows: int
    control_rows: int
    difference: Fraction
    ci_low: Fraction
    ci_high: Fraction

    @property
    def stream(self) -> str:
        return contrast_stream(self.contrast_id, self.task_id)

    @property
    def printed(self) -> str:
        return (
            f"{signed_points(self.difference)} "
            f"[{signed_points(self.ci_low)}, {signed_points(self.ci_high)}]"
        )


@dataclass(frozen=True)
class Arms:
    question_only: list[Answer]
    spec_only: list[Answer]
    artifact_present: list[Answer]
    compile_pass: list[Answer]
    compile_failed: list[Answer]


@dataclass(frozen=True)
class CalibrationRow:
    table: str
    slice_id: str
    task_id: str
    builds: int | None
    find_runs: int
    all_rows: Rate
    delta: Contrast | None
    implemented_gold: Rate | None
    perfect: Rate | None


@dataclass(frozen=True)
class RepeatabilityRow:
    table: str
    slice_id: str
    task_id: str
    builds: int | None
    find_runs: int
    answer_rows: int
    stable_cells: int
    total_cells: int
    min_attempts: int
    max_attempts: int
    reliable_questions: int
    questions: int


@dataclass(frozen=True)
class LowPriorQuestion:
    task_id: str
    question_id: str
    released: tuple[int, int]
    corrected: tuple[int, int]

    @property
    def still_low_prior(self) -> bool:
        return Fraction(*self.corrected) < LOW_PRIOR_AGREEMENT


@dataclass(frozen=True)
class LowPrior:
    question_only: Rate
    recovery_all_rows: Rate
    delta: Contrast
    implemented_gold: Rate
    questions: list[LowPriorQuestion]


@dataclass(frozen=True)
class FinderCalibration:
    finder_id: str
    formal_all_rows: Rate
    question_only_runs: int
    formal_matched: dict[str, Rate]
    question_only: dict[str, Rate]
    mean_find_tokens: float

    def delta(self, question_set: str) -> Fraction:
        return self.formal_matched[question_set].value - self.question_only[question_set].value


@dataclass(frozen=True)
class Selection:
    rows: list[dict[str, Any]]
    core_finders: list[str]
    audit_finders: list[str]
    released_core_finders: list[str]
    released_audit_finders: list[str]

    @property
    def unchanged(self) -> bool:
        return (
            self.core_finders == self.released_core_finders
            and self.audit_finders == self.released_audit_finders
        )


@dataclass(frozen=True)
class PrintedValue:
    value_id: str
    location: str
    printed: str
    exact: str
    half_tie: bool = False


@dataclass(frozen=True)
class Analysis:
    inputs: dict[str, Path]
    generated_at: str | None
    exclusions: list[ExcludedRun]
    question_only: dict[str, Rate]
    question_only_runs: dict[str, int]
    released_check: list[dict[str, Any]]
    calibration: list[CalibrationRow]
    repeatability: list[RepeatabilityRow]
    low_prior: LowPrior
    finders: list[FinderCalibration]
    selection: Selection
    contrasts: list[Contrast]
    label_sources: dict[str, int]
    printed: list[PrintedValue]


def main() -> None:
    args = _parse_args()
    p = paths()
    out_dir = (args.output_dir or p.reports_dir.joinpath(*OUTPUT_PARTS)).resolve()
    exclusions_path = (
        args.exclusions or p.reports_dir.joinpath(*OUTPUT_PARTS, EXCLUSIONS_NAME)
    ).resolve()
    analysis = analyze(p, exclusions_path=exclusions_path)
    write_outputs(analysis, out_dir)
    print(f"wrote {_display_path(out_dir)}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--exclusions",
        type=Path,
        default=None,
        help="Excluded question-only runs (default: the released list under the reports root).",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Output directory (default: reports/eandd_final/control_exclusions).",
    )
    return parser.parse_args()


# --------------------------------------------------------------------------- inputs


def analysis_inputs(p: Paths, *, exclusions_path: Path) -> dict[str, Path]:
    report_dir = p.reports_dir / "eandd_final"
    labels_dir = p.reports_dir / "artifact_conformance_consensus" / "final"
    return {
        "question_answers": p.tables_dir / "question_answers.csv",
        "table_manifest": p.tables_dir / "MANIFEST.json",
        "exclusions": exclusions_path,
        "final_labels": labels_dir / "artifact_conformance_final.csv",
        "label_corrections": labels_dir / "label_corrections_v1_1.csv",
        "low_prior_manifest": report_dir / "low_prior" / "manifest.json",
        "pivot_manifest": report_dir / "pivots" / "manifest.json",
        "released_bootstrap": report_dir / "validity_stress" / "bootstrap_lift.csv",
        "finder_summary": report_dir / "finder_summary.csv",
        "affinity": report_dir / "pivots" / "affinity_by_agent.csv",
        "within_finder_cells": report_dir / "within_finder_efficiency" / "within_finder_cells.csv",
        "released_selection": report_dir / "core_finder_selection" / "core_finders.json",
    }


def load_answers(path: Path) -> list[Answer]:
    return [
        Answer(
            find_run_id=row["find_run_id"],
            build_run_id=row["build_run_id"],
            task_id=row["task_id"],
            question_id=row["question_id"],
            finder_id=row["finder_id"],
            finder_trial_id=row["finder_trial_id"],
            correct=_as_bool(row["is_correct"]),
            panels=frozenset(row["panel_ids"].split("|")),
        )
        for row in _read_csv(path)
    ]


def split_arms(answers: list[Answer]) -> Arms:
    compile_pass = [row for row in answers if PAPER_FINAL_PANEL_ID in row.panels]
    compile_pass_runs = {row.find_run_id for row in compile_pass}
    artifact_present = [
        row for row in answers if ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID in row.panels
    ]
    return Arms(
        question_only=[row for row in answers if QUESTION_ONLY_PANEL_ID in row.panels],
        spec_only=[row for row in answers if SPEC_ONLY_PANEL_ID in row.panels],
        artifact_present=artifact_present,
        compile_pass=compile_pass,
        compile_failed=[
            row for row in artifact_present if row.find_run_id not in compile_pass_runs
        ],
    )


def load_exclusions(path: Path) -> list[ExcludedRun]:
    """Read and validate the excluded question-only run list."""
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if tuple(reader.fieldnames or ()) != EXCLUSION_FIELDS:
            raise ValueError(f"{path.name}: expected columns {', '.join(EXCLUSION_FIELDS)}")
        runs = [
            ExcludedRun(**{key: row[key].strip() for key in EXCLUSION_FIELDS}) for row in reader
        ]
    seen: set[str] = set()
    for run in runs:
        if run.exposure not in EXPOSURES:
            raise ValueError(f"{run.find_run_id}: unknown exposure {run.exposure!r}")
        if not run.evidence:
            raise ValueError(f"{run.find_run_id}: missing evidence")
        if run.find_run_id in seen:
            raise ValueError(f"{run.find_run_id}: listed twice")
        seen.add(run.find_run_id)
    return runs


def validate_exclusions(exclusions: list[ExcludedRun], question_only: list[Answer]) -> None:
    """Require every excluded run to be a question-only run with matching metadata."""
    runs = {row.find_run_id: row for row in question_only}
    for run in exclusions:
        row = runs.get(run.find_run_id)
        if row is None:
            raise ValueError(f"{run.find_run_id}: not a question-only control run")
        if row.stratum != run.stratum:
            raise ValueError(
                f"{run.find_run_id}: finder/task/trial {run.stratum} does not match {row.stratum}"
            )


def load_labels(
    final_path: Path, corrections_path: Path
) -> tuple[dict[LabelKey, str], Counter[str]]:
    """Return v1.1 final labels and label-source counts (v1.0 plus listed corrections).

    The overlay is the one ``analyze_artifact_conformance_impact.py`` applies, so
    unknown, repeated, stale, or invalid corrections raise here as well.
    """
    rows = _apply_label_corrections(_read_csv(final_path), _read_csv(corrections_path))
    labels = {_artifact_question_key(row): row["final_label"] for row in rows}
    return labels, Counter(row["label_source"] for row in rows)


def load_question_set(values: Iterable[str]) -> frozenset[Cluster]:
    clusters = set()
    for value in values:
        task_id, question_id = value.split(":", 1)
        clusters.add((task_id, question_id))
    return frozenset(clusters)


# --------------------------------------------------------------------------- helpers


def excluded_strata(exclusions: Iterable[ExcludedRun]) -> set[Stratum]:
    return {run.stratum for run in exclusions}


def drop_runs(rows: Iterable[Answer], run_ids: set[str]) -> list[Answer]:
    return [row for row in rows if row.find_run_id not in run_ids]


def drop_strata(rows: Iterable[Answer], strata: set[Stratum]) -> list[Answer]:
    """Matched strata: drop rows whose finder x task x finder trial lost its control run."""
    return [row for row in rows if row.stratum not in strata]


def restrict(
    rows: Iterable[Answer],
    *,
    task_id: str | None = None,
    questions: frozenset[Cluster] | None = None,
) -> list[Answer]:
    return [
        row
        for row in rows
        if (task_id is None or row.task_id == task_id)
        and (questions is None or row.cluster in questions)
    ]


def implemented_gold_rows(rows: Iterable[Answer], labels: dict[LabelKey, str]) -> list[Answer]:
    out = []
    for row in rows:
        label = labels.get((row.task_id, row.build_run_id, row.question_id))
        if label is None:
            raise RuntimeError(f"missing conformance label for {row.find_run_id}:{row.question_id}")
        if label == IMPLEMENTED_GOLD:
            out.append(row)
    return out


def perfect_runs(rows: Iterable[Answer]) -> Rate:
    """Perfect-run rate over runs with at least one scored row."""
    by_run: dict[str, bool] = {}
    for row in rows:
        by_run[row.find_run_id] = by_run.get(row.find_run_id, True) and row.correct
    return Rate(sum(by_run.values()), len(by_run))


def count_runs(rows: Iterable[Answer]) -> int:
    return len({row.find_run_id for row in rows})


def count_builds(rows: Iterable[Answer]) -> int:
    return len({row.build_run_id for row in rows})


def cluster_counts(rows: Iterable[Answer]) -> dict[Cluster, tuple[int, int]]:
    grouped: dict[Cluster, list[int]] = defaultdict(lambda: [0, 0])
    for row in rows:
        counts = grouped[row.cluster]
        counts[0] += int(row.correct)
        counts[1] += 1
    return {cluster: (counts[0], counts[1]) for cluster, counts in grouped.items()}


def pooled_difference(
    treatment: dict[Cluster, tuple[int, int]],
    control: dict[Cluster, tuple[int, int]],
    clusters: list[Cluster],
) -> Fraction:
    treatment_correct = treatment_total = control_correct = control_total = 0
    for cluster in clusters:
        treatment_correct += treatment[cluster][0]
        treatment_total += treatment[cluster][1]
        control_correct += control[cluster][0]
        control_total += control[cluster][1]
    return Fraction(treatment_correct, treatment_total) - Fraction(control_correct, control_total)


def contrast_stream(contrast_id: str, task_id: str = ALL_TASKS) -> str:
    return contrast_id if task_id == ALL_TASKS else f"{contrast_id}:{task_id}"


def bootstrap_contrast(
    contrast_id: str,
    treatment: Iterable[Answer],
    control: Iterable[Answer],
    *,
    task_id: str = ALL_TASKS,
    samples: int = BOOTSTRAP_SAMPLES,
    seed: int = BOOTSTRAP_SEED,
) -> Contrast:
    """Question-cluster percentile bootstrap of the released script, on exact fractions."""
    stream = contrast_stream(contrast_id, task_id)
    treatment_counts = cluster_counts(treatment)
    control_counts = cluster_counts(control)
    clusters = sorted(set(treatment_counts) & set(control_counts))
    if not clusters:
        raise RuntimeError(f"{stream}: no shared clusters")
    rng = random.Random(f"{seed}:{stream}")
    replicates = []
    for _ in range(samples):
        draw = [clusters[rng.randrange(len(clusters))] for _ in clusters]
        replicates.append(pooled_difference(treatment_counts, control_counts, draw))
    replicates.sort()
    return Contrast(
        contrast_id=contrast_id,
        task_id=task_id,
        clusters=len(clusters),
        treatment_rows=sum(treatment_counts[cluster][1] for cluster in clusters),
        control_rows=sum(control_counts[cluster][1] for cluster in clusters),
        difference=pooled_difference(treatment_counts, control_counts, clusters),
        ci_low=replicates[int(0.025 * (samples - 1))],
        ci_high=replicates[int(0.975 * (samples - 1))],
    )


def question_agreement(
    rows: Iterable[Answer],
    cell_key: Callable[[Answer], Hashable],
) -> dict[Cluster, tuple[int, int, int, int]]:
    """Per question: (stable cells, cells, min attempts, max attempts).

    A cell is stable when every attempt it contains is correct. Complete cells
    hold three attempts; a cell that lost an excluded run keeps its remaining
    attempts.
    """
    cells: dict[tuple[Cluster, Hashable], list[bool]] = defaultdict(list)
    for row in rows:
        cells[(row.cluster, cell_key(row))].append(row.correct)
    out: dict[Cluster, list[int]] = {}
    for (cluster, _key), attempts in cells.items():
        stats = out.setdefault(cluster, [0, 0, len(attempts), len(attempts)])
        stats[0] += int(all(attempts))
        stats[1] += 1
        stats[2] = min(stats[2], len(attempts))
        stats[3] = max(stats[3], len(attempts))
    return {cluster: (s[0], s[1], s[2], s[3]) for cluster, s in out.items()}


def round_half_up(value: Fraction, places: int = 1) -> Decimal:
    """Round an exact rational to ``places`` decimals, halves away from zero."""
    scaled = abs(value) * 10**places
    rounded = math.floor(scaled + Fraction(1, 2))
    return Decimal(rounded if value >= 0 else -rounded).scaleb(-places)


def is_half_tie(value: Fraction, places: int = 1) -> bool:
    scaled = abs(value) * 10**places
    return scaled - math.floor(scaled) == Fraction(1, 2)


def percent(value: Fraction) -> str:
    return f"{round_half_up(100 * value):.1f}"


def signed_points(value: Fraction) -> str:
    return f"{round_half_up(100 * value):+.1f}"


# --------------------------------------------------------------------------- analysis


def analyze(p: Paths, *, exclusions_path: Path) -> Analysis:
    inputs = analysis_inputs(p, exclusions_path=exclusions_path)
    missing = [_display_path(path) for path in inputs.values() if not path.exists()]
    if missing:
        raise FileNotFoundError(f"missing inputs: {', '.join(missing)}")

    arms = split_arms(load_answers(inputs["question_answers"]))
    exclusions = load_exclusions(inputs["exclusions"])
    validate_exclusions(exclusions, arms.question_only)
    labels, label_sources = load_labels(inputs["final_labels"], inputs["label_corrections"])
    low_prior_set = load_question_set(
        _read_json(inputs["low_prior_manifest"])["question_subset"]["questions"]
    )
    question_sets: dict[str, frozenset[Cluster] | None] = {
        "all": None,
        "low_prior": low_prior_set,
        "implementation_signal": load_question_set(
            _read_json(inputs["pivot_manifest"])["implementation_signal_questions"]
        ),
    }

    released_check = check_released_bootstrap(arms, low_prior_set, inputs["released_bootstrap"])

    excluded_ids = {run.find_run_id for run in exclusions}
    answer_bearing_ids = {run.find_run_id for run in exclusions if run.exposure == ANSWER_BEARING}
    strata = excluded_strata(exclusions)
    control = drop_runs(arms.question_only, excluded_ids)
    answer_bearing_only = drop_runs(arms.question_only, answer_bearing_ids)
    question_only = {
        "released": Rate.of(arms.question_only),
        "corrected": Rate.of(control),
        "answer_bearing_only": Rate.of(answer_bearing_only),
        **{f"corrected:{task}": Rate.of(restrict(control, task_id=task)) for task in TASK_IDS},
    }
    question_only_runs = {
        "released": count_runs(arms.question_only),
        "corrected": count_runs(control),
        "answer_bearing_only": count_runs(answer_bearing_only),
    }

    contrasts: dict[str, Contrast] = {}
    calibration = calibration_rows(arms, control, strata, labels, contrasts)
    repeatability = repeatability_rows(arms, control, labels)
    low_prior = low_prior_summary(arms, control, strata, labels, low_prior_set, contrasts)
    finders = finder_calibration(arms, control, strata, question_sets, inputs["finder_summary"])
    selection = rerun_selector(inputs, finders)

    analysis = Analysis(
        inputs=inputs,
        generated_at=_read_json(inputs["table_manifest"]).get("generated_at"),
        exclusions=exclusions,
        question_only=question_only,
        question_only_runs=question_only_runs,
        released_check=released_check,
        calibration=calibration,
        repeatability=repeatability,
        low_prior=low_prior,
        finders=finders,
        selection=selection,
        contrasts=list(contrasts.values()),
        label_sources=dict(sorted(label_sources.items())),
        printed=[],
    )
    return replace(analysis, printed=printed_values(analysis))


def check_released_bootstrap(
    arms: Arms,
    low_prior: frozenset[Cluster],
    released_path: Path,
) -> list[dict[str, Any]]:
    """Replay the released bootstrap_lift.csv with this script's bootstrap; stop on mismatch."""
    replay = {
        "artifact_present_minus_question_only": (arms.artifact_present, arms.question_only),
        "compile_pass_minus_question_only": (arms.compile_pass, arms.question_only),
        LOW_PRIOR_CONTRAST_ID: (
            restrict(arms.compile_pass, questions=low_prior),
            restrict(arms.question_only, questions=low_prior),
        ),
    }
    rows = []
    for released in _read_csv(released_path):
        treatment, control = replay[released["contrast_id"]]
        contrast = bootstrap_contrast(released["contrast_id"], treatment, control)
        replayed = {
            "contrast_id": released["contrast_id"],
            "cluster_unit": "task_question",
            "clusters": str(contrast.clusters),
            "treatment_answer_rows": str(contrast.treatment_rows),
            "control_answer_rows": str(contrast.control_rows),
            "difference": f"{float(contrast.difference):.6f}",
            "ci_low": f"{float(contrast.ci_low):.6f}",
            "ci_high": f"{float(contrast.ci_high):.6f}",
            "bootstrap_samples": str(BOOTSTRAP_SAMPLES),
            "bootstrap_seed": str(BOOTSTRAP_SEED),
        }
        if replayed != released:
            raise RuntimeError(
                f"released bootstrap not reproduced for {released['contrast_id']}: "
                f"{replayed} != {released}"
            )
        rows.append({**replayed, "reproduced": True})
    if {row["contrast_id"] for row in rows} != set(replay):
        raise RuntimeError("released bootstrap_lift.csv does not list the expected contrasts")
    return rows


def calibration_rows(
    arms: Arms,
    control: list[Answer],
    strata: set[Stratum],
    labels: dict[LabelKey, str],
    contrasts: dict[str, Contrast],
) -> list[CalibrationRow]:
    """Rows of Table 1 (all tasks) and Table 14 (per task), in paper order."""
    slices = {
        "spec_only": arms.spec_only,
        "artifact_present": arms.artifact_present,
        "compile_pass": arms.compile_pass,
        "compile_failed": arms.compile_failed,
    }
    layouts = (
        ("table1", tuple(slices), (ALL_TASKS,)),
        ("table12", ("spec_only", "artifact_present", "compile_pass"), TASK_IDS),
    )
    rows = []
    for table, slice_ids, task_ids in layouts:
        for task_id in task_ids:
            control_rows = restrict(control, task_id=_task_filter(task_id))
            rows.append(
                CalibrationRow(
                    table=table,
                    slice_id="question_only",
                    task_id=task_id,
                    builds=None,
                    find_runs=count_runs(control_rows),
                    all_rows=Rate.of(control_rows),
                    delta=None,
                    implemented_gold=None,
                    perfect=None,
                )
            )
        for slice_id in slice_ids:
            for task_id in task_ids:
                control_rows = restrict(control, task_id=_task_filter(task_id))
                treatment = restrict(slices[slice_id], task_id=_task_filter(task_id))
                contrast = bootstrap_contrast(
                    SLICE_CONTRASTS[slice_id],
                    drop_strata(treatment, strata),
                    control_rows,
                    task_id=task_id,
                )
                contrasts[contrast.stream] = contrast
                scored = (
                    None if slice_id == "spec_only" else implemented_gold_rows(treatment, labels)
                )
                rows.append(
                    CalibrationRow(
                        table=table,
                        slice_id=slice_id,
                        task_id=task_id,
                        builds=None if slice_id == "spec_only" else count_builds(treatment),
                        find_runs=count_runs(treatment),
                        all_rows=Rate.of(treatment),
                        delta=contrast,
                        implemented_gold=None if scored is None else Rate.of(scored),
                        perfect=None if scored is None else perfect_runs(scored),
                    )
                )
    return rows


def _task_filter(task_id: str) -> str | None:
    return None if task_id == ALL_TASKS else task_id


def repeatability_rows(
    arms: Arms,
    control: list[Answer],
    labels: dict[LabelKey, str],
) -> list[RepeatabilityRow]:
    """Rows of Table 2 (all tasks) and Table 15 (formal rows per task)."""

    def by_build_and_finder(row: Answer) -> Hashable:
        return (row.build_run_id, row.finder_id)

    def by_finder(row: Answer) -> Hashable:
        return row.finder_id

    formal = ("formal", implemented_gold_rows(arms.compile_pass, labels), by_build_and_finder)
    layouts = (
        (
            "table2",
            (ALL_TASKS,),
            [
                formal,
                ("spec_only", arms.spec_only, by_finder),
                ("question_only", control, by_finder),
            ],
        ),
        ("table13", TASK_IDS, [formal]),
    )
    rows = []
    for table, task_ids, slices in layouts:
        for task_id in task_ids:
            for slice_id, slice_rows, cell_key in slices:
                scoped = restrict(slice_rows, task_id=_task_filter(task_id))
                agreement = question_agreement(scoped, cell_key)
                rows.append(
                    RepeatabilityRow(
                        table=table,
                        slice_id=slice_id,
                        task_id=task_id,
                        builds=count_builds(scoped) if slice_id == "formal" else None,
                        find_runs=count_runs(scoped),
                        answer_rows=len(scoped),
                        stable_cells=sum(stats[0] for stats in agreement.values()),
                        total_cells=sum(stats[1] for stats in agreement.values()),
                        min_attempts=min(stats[2] for stats in agreement.values()),
                        max_attempts=max(stats[3] for stats in agreement.values()),
                        reliable_questions=sum(
                            Fraction(stats[0], stats[1]) >= RELIABLE_AGREEMENT
                            for stats in agreement.values()
                        ),
                        questions=len(agreement),
                    )
                )
    return rows


def low_prior_summary(
    arms: Arms,
    control: list[Answer],
    strata: set[Stratum],
    labels: dict[LabelKey, str],
    low_prior: frozenset[Cluster],
    contrasts: dict[str, Contrast],
) -> LowPrior:
    """Frozen low-prior subset under the corrected control.

    The subset stays the ten questions selected on the released controls; the
    membership table reports which of them still fall below 90% agreement.
    """

    def by_finder(row: Answer) -> str:
        return row.finder_id

    released = question_agreement(arms.question_only, by_finder)
    corrected = question_agreement(control, by_finder)
    rederived = {
        cluster
        for cluster, stats in released.items()
        if Fraction(stats[0], stats[1]) < LOW_PRIOR_AGREEMENT
    }
    if rederived != low_prior:
        raise RuntimeError("frozen low-prior subset does not match the released controls")
    contrast = bootstrap_contrast(
        LOW_PRIOR_CONTRAST_ID,
        drop_strata(restrict(arms.compile_pass, questions=low_prior), strata),
        restrict(control, questions=low_prior),
    )
    contrasts[contrast.stream] = contrast
    return LowPrior(
        question_only=Rate.of(restrict(control, questions=low_prior)),
        recovery_all_rows=Rate.of(restrict(arms.compile_pass, questions=low_prior)),
        delta=contrast,
        implemented_gold=Rate.of(
            restrict(implemented_gold_rows(arms.compile_pass, labels), questions=low_prior)
        ),
        questions=[
            LowPriorQuestion(
                task_id=cluster[0],
                question_id=cluster[1],
                released=released[cluster][:2],
                corrected=corrected[cluster][:2],
            )
            for cluster in sorted(low_prior)
        ],
    )


def finder_calibration(
    arms: Arms,
    control: list[Answer],
    strata: set[Stratum],
    question_sets: dict[str, frozenset[Cluster] | None],
    finder_summary_path: Path,
) -> list[FinderCalibration]:
    """Table 9: all-row formal recovery and matched differences per finder."""
    mean_tokens = {
        row["finder_id"]: float(row["mean_find_tokens"]) for row in _read_csv(finder_summary_path)
    }
    rows = []
    for finder_id in sorted({row.finder_id for row in arms.compile_pass}):
        formal = [row for row in arms.compile_pass if row.finder_id == finder_id]
        formal_matched = drop_strata(formal, strata)
        finder_control = [row for row in control if row.finder_id == finder_id]
        rows.append(
            FinderCalibration(
                finder_id=finder_id,
                formal_all_rows=Rate.of(formal),
                question_only_runs=count_runs(finder_control),
                formal_matched={
                    name: Rate.of(restrict(formal_matched, questions=questions))
                    for name, questions in question_sets.items()
                },
                question_only={
                    name: Rate.of(restrict(finder_control, questions=questions))
                    for name, questions in question_sets.items()
                },
                mean_find_tokens=mean_tokens[finder_id],
            )
        )
    return sorted(rows, key=lambda row: (-row.formal_all_rows.value, row.finder_id))


def rerun_selector(inputs: dict[str, Path], finders: list[FinderCalibration]) -> Selection:
    """Rerun the core-finder gates with Table 9's corrected control inputs.

    Formal accuracy, its interval, tokens, affinity, and rank stability come from
    the released selector inputs. The question-only accuracy and the overall,
    low-prior, and implementation-signal lifts are replaced by the corrected
    control and the matched differences of Table 9.
    """
    summary = _load_finder_summary(inputs["finder_summary"])
    affinity = _load_affinity(inputs["affinity"])
    rank_stability = _finder_rank_stability(inputs["within_finder_cells"])
    by_finder = {finder.finder_id: finder for finder in finders}
    if set(summary) != set(by_finder):
        raise RuntimeError("finder_summary.csv and the compile-pass panel list different finders")

    rows = []
    for finder_id, base in sorted(summary.items()):
        finder = by_finder[finder_id]
        if abs(float(finder.formal_all_rows.value) - base["formal_accuracy"]) > 5e-7:
            raise RuntimeError(f"{finder_id}: formal accuracy differs from finder_summary.csv")
        row = {
            **base,
            "question_only_accuracy": float(finder.question_only["all"].value),
            "formal_minus_question_only": float(finder.delta("all")),
            "low_prior_formal_accuracy": float(finder.formal_matched["low_prior"].value),
            "low_prior_question_only_accuracy": float(finder.question_only["low_prior"].value),
            "low_prior_lift": float(finder.delta("low_prior")),
            "implementation_signal_formal_accuracy": float(
                finder.formal_matched["implementation_signal"].value
            ),
            "implementation_signal_question_only_accuracy": float(
                finder.question_only["implementation_signal"].value
            ),
            "implementation_signal_lift": float(finder.delta("implementation_signal")),
            **affinity[finder_id],
            **rank_stability[finder_id],
        }
        row["formal_ci_width"] = row["formal_run_ci95_high"] - row["formal_run_ci95_low"]
        row["core_gate_failures"] = _core_gate_failures(row)
        row["core_selected"] = not row["core_gate_failures"]
        row["audit_gate_failures"] = _audit_gate_failures(row)
        row["audit_candidate"] = (not row["core_selected"]) and not row["audit_gate_failures"]
        rows.append(row)

    core_finders = [
        row["finder_id"]
        for row in sorted(
            (row for row in rows if row["core_selected"]),
            key=lambda row: (-row["formal_accuracy"], row["mean_find_tokens"], row["finder_id"]),
        )
    ]
    audit_finders = _choose_audit_finders(rows)
    for row in rows:
        if row["finder_id"] in core_finders:
            row["selection"] = "core_default"
            row["selection_reason"] = "passes all default core gates"
        elif row["finder_id"] in audit_finders:
            row["selection"] = "prior_stress_audit"
            row["selection_reason"] = "strong finder-specific prior lift; calibration audits only"
        else:
            row["selection"] = "not_selected"
            row["selection_reason"] = "; ".join(row["core_gate_failures"])
    released = _read_json(inputs["released_selection"])
    return Selection(
        rows=_sort_selection_rows(rows),
        core_finders=core_finders,
        audit_finders=audit_finders,
        released_core_finders=list(released["default_core_finders"]),
        released_audit_finders=list(released["prior_stress_audit_finders"]),
    )


# --------------------------------------------------------------------------- printed values


def printed_values(analysis: Analysis) -> list[PrintedValue]:
    """Every control-dependent value printed in the camera-ready, with its location."""
    values: list[PrintedValue] = []

    def text(value_id: str, location: str, printed: str, exact: str = "") -> None:
        values.append(PrintedValue(value_id, location, printed, exact))

    def rate(value_id: str, location: str, value: Rate) -> None:
        values.append(
            PrintedValue(
                value_id,
                location,
                percent(value.value),
                f"{value.correct}/{value.total} = {_exact_percent(value.value)}%",
                is_half_tie(100 * value.value),
            )
        )

    def contrast(value_id: str, location: str, value: Contrast) -> None:
        endpoints = (value.difference, value.ci_low, value.ci_high)
        values.append(
            PrintedValue(
                value_id,
                location,
                value.printed,
                f"{_exact_points(value.difference)} [{_exact_points(value.ci_low)}, "
                f"{_exact_points(value.ci_high)}] pp; {value.clusters} clusters; "
                f"stream {value.stream}",
                any(is_half_tie(100 * endpoint) for endpoint in endpoints),
            )
        )

    def difference(value_id: str, location: str, value: Fraction) -> None:
        values.append(
            PrintedValue(
                value_id,
                location,
                signed_points(value),
                f"{_exact_points(value)} pp",
                is_half_tie(100 * value),
            )
        )

    exclusions = analysis.exclusions
    exposure_counts = Counter(run.exposure for run in exclusions)
    qo = analysis.question_only
    released_runs = analysis.question_only_runs["released"]
    table1 = {row.slice_id: row for row in analysis.calibration if row.table == "table1"}
    table12 = {
        (row.slice_id, row.task_id): row for row in analysis.calibration if row.table == "table12"
    }
    rep = {(row.table, row.slice_id, row.task_id): row for row in analysis.repeatability}
    low_prior = analysis.low_prior
    compile_pass = table1["compile_pass"]

    location = "Sec. 4.1 (sec:setup), exclusion sentence"
    text("setup.excluded_runs", location, str(len(exclusions)), f"of {released_runs} runs")
    text("setup.question_only_runs", location, str(released_runs))
    rate("setup.question_only_released", location, qo["released"])
    rate("setup.question_only_corrected", location, qo["corrected"])

    location = "Sec. 4.2 (sec:prior_stress), first paragraph"
    rate("prior_stress.question_only", location, qo["corrected"])
    contrast("prior_stress.spec_only_delta", location, table1["spec_only"].delta)
    rate("prior_stress.compile_pass_all_rows", location, compile_pass.all_rows)
    contrast("prior_stress.compile_pass_delta", location, compile_pass.delta)

    table_locations = {
        "table1": "Table 1 (tab:prior-stress)",
        "table12": "Table 14 (tab:prior-stress-by-task)",
    }
    for row in analysis.calibration:
        location = table_locations[row.table]
        prefix = f"{row.table}.{row.slice_id}"
        if row.task_id != ALL_TASKS:
            prefix += f".{row.task_id}"
        if row.builds is not None:
            text(f"{prefix}.builds", location, str(row.builds))
        text(f"{prefix}.find_runs", location, str(row.find_runs))
        rate(f"{prefix}.accuracy", location, row.all_rows)
        if row.delta is not None:
            contrast(f"{prefix}.delta", location, row.delta)
        if row.implemented_gold is not None and row.perfect is not None:
            rate(f"{prefix}.impl_gold_accuracy", location, row.implemented_gold)
            rate(f"{prefix}.impl_gold_perfect", location, row.perfect)
    location = "Table 1 (tab:prior-stress), caption"
    text("table1.caption.excluded_runs", location, str(len(exclusions)))
    unscored = table1["artifact_present"].find_runs - table1["artifact_present"].perfect.total
    text(
        "table1.caption.unscored_runs",
        location,
        str(unscored),
        f"artifact-present {table1['artifact_present'].perfect.total} of "
        f"{table1['artifact_present'].find_runs} runs scored; compile-failed "
        f"{table1['compile_failed'].perfect.total} of {table1['compile_failed'].find_runs}",
    )

    location = "Table 2 (tab:scope)"
    for slice_id in ("formal", "spec_only", "question_only"):
        row = rep[("table2", slice_id, ALL_TASKS)]
        text(
            f"table2.{slice_id}.reliable",
            location,
            f"{row.reliable_questions} / {row.questions}",
        )
    row = rep[("table2", "question_only", ALL_TASKS)]
    text("table2.question_only.find_runs", location, str(row.find_runs))
    text("table2.question_only.answer_rows", location, str(row.answer_rows))
    rate(
        "table2.question_only.agreement",
        location,
        Rate(row.stable_cells, row.total_cells),
    )
    location = "Table 15 (tab:scope-by-task)"
    for task_id in TASK_IDS:
        row = rep[("table13", "formal", task_id)]
        text(
            f"table13.formal.{task_id}.reliable",
            location,
            f"{row.reliable_questions} / {row.questions}",
        )

    location = "Sec. 4.8 (sec:low_prior_main)"
    still = sum(question.still_low_prior for question in low_prior.questions)
    text(
        "low_prior.still_low_prior",
        location,
        f"{still} of {len(low_prior.questions)}",
        "frozen questions still below 90% agreement on the remaining attempts",
    )
    rate("low_prior.question_only", location, low_prior.question_only)
    rate("low_prior.recovery_all_rows", location, low_prior.recovery_all_rows)
    contrast("low_prior.delta", location, low_prior.delta)
    rate("low_prior.impl_gold_recovery", location, low_prior.implemented_gold)

    location = "Table 4 (tab:low-prior-evidence-summary)"
    rate("table4.all.question_only", location, qo["corrected"])
    rate("table4.all.recovery", location, compile_pass.all_rows)
    contrast("table4.all.delta", location, compile_pass.delta)
    rate("table4.all.impl_gold_recovery", location, compile_pass.implemented_gold)
    rate("table4.low_prior.question_only", location, low_prior.question_only)
    rate("table4.low_prior.recovery", location, low_prior.recovery_all_rows)
    contrast("table4.low_prior.delta", location, low_prior.delta)
    rate("table4.low_prior.impl_gold_recovery", location, low_prior.implemented_gold)
    text("table4.caption.excluded_runs", location, str(len(exclusions)))

    rate("validity.question_only", "Sec. 6 (sec:validity-scope)", qo["corrected"])

    location = "App. C (app:details), Question-only control exclusions"
    text("app_c.excluded_runs", location, f"{len(exclusions)} of {released_runs}")
    for exposure in EXPOSURES:
        text(f"app_c.{exposure}_runs", location, str(exposure_counts[exposure]))
    text("app_c.remaining_runs", location, str(table1["question_only"].find_runs))
    text("app_c.remaining_rows", location, str(qo["corrected"].total))
    rate("app_c.question_only_corrected", location, qo["corrected"])
    rate("app_c.question_only_answer_bearing_only", location, qo["answer_bearing_only"])

    location = "Table 9 (tab:finder_calibration)"
    for finder in analysis.finders:
        rate(f"table8.{finder.finder_id}.formal", location, finder.formal_all_rows)
        for question_set in QUESTION_SETS:
            difference(
                f"table8.{finder.finder_id}.delta_{question_set}",
                location,
                finder.delta(question_set),
            )
    location = "App. E (app:finder_calibration), selector paragraph"
    top = max(analysis.finders, key=lambda finder: finder.delta("all"))
    difference(f"app_e.largest_question_only_lift.{top.finder_id}", location, top.delta("all"))
    text(
        "app_e.selection_unchanged",
        location,
        "unchanged" if analysis.selection.unchanged else "CHANGED",
        f"core {', '.join(analysis.selection.core_finders)}; "
        f"audit {', '.join(analysis.selection.audit_finders)}",
    )

    location = "App. F (app:metric_details), Uncertainty"
    text("app_f.clusters_overall", location, str(compile_pass.delta.clusters))
    text("app_f.clusters_low_prior", location, str(low_prior.delta.clusters))
    text(
        "app_f.clusters_per_task",
        location,
        str(table12[("compile_pass", TASK_IDS[0])].delta.clusters),
    )
    text("app_f.bootstrap_resamples", location, f"{BOOTSTRAP_SAMPLES:,}")

    rate("app_n.question_only", "App. N (app:benchcard), Known biases", qo["corrected"])
    return values


# --------------------------------------------------------------------------- outputs


def write_outputs(analysis: Analysis, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    _write_csv(out_dir / "printed_values.csv", [_printed_row(value) for value in analysis.printed])
    _write_csv(
        out_dir / "contrasts.csv", [_contrast_row(contrast) for contrast in analysis.contrasts]
    )
    _write_csv(
        out_dir / "calibration_rows.csv", [_calibration_row(row) for row in analysis.calibration]
    )
    _write_csv(
        out_dir / "repeatability.csv", [_repeatability_row(row) for row in analysis.repeatability]
    )
    _write_csv(
        out_dir / "low_prior_questions.csv",
        [_low_prior_row(question) for question in analysis.low_prior.questions],
    )
    _write_csv(out_dir / "finder_calibration.csv", [_finder_row(row) for row in analysis.finders])
    _write_selection_csv(
        out_dir / "finder_selection.csv", SELECTION_FIELDS, analysis.selection.rows
    )
    (out_dir / "summary.json").write_text(
        json.dumps(_summary(analysis), indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (out_dir / "report.md").write_text(_report(analysis), encoding="utf-8")


def _printed_row(value: PrintedValue) -> dict[str, Any]:
    return {
        "value_id": value.value_id,
        "paper_location": value.location,
        "printed": value.printed,
        "exact": value.exact,
        "half_tie": int(value.half_tie),
    }


def _contrast_row(contrast: Contrast) -> dict[str, Any]:
    return {
        "contrast_id": contrast.contrast_id,
        "task_id": contrast.task_id,
        "rng_stream": f"{BOOTSTRAP_SEED}:{contrast.stream}",
        "strata": "matched",
        "control": "question_only_after_exclusions",
        "cluster_unit": "task_question",
        "clusters": contrast.clusters,
        "treatment_answer_rows": contrast.treatment_rows,
        "control_answer_rows": contrast.control_rows,
        "difference": f"{float(contrast.difference):.6f}",
        "ci_low": f"{float(contrast.ci_low):.6f}",
        "ci_high": f"{float(contrast.ci_high):.6f}",
        "printed_pp": contrast.printed,
        "bootstrap_samples": BOOTSTRAP_SAMPLES,
        "bootstrap_seed": BOOTSTRAP_SEED,
    }


def _calibration_row(row: CalibrationRow) -> dict[str, Any]:
    return {
        "table": row.table,
        "slice_id": row.slice_id,
        "task_id": row.task_id,
        "builds": "" if row.builds is None else row.builds,
        "find_runs": row.find_runs,
        **_rate_fields("all_rows", row.all_rows),
        "delta_contrast": "" if row.delta is None else row.delta.stream,
        "delta_pp": "" if row.delta is None else row.delta.printed,
        **_rate_fields("impl_gold", row.implemented_gold),
        **_rate_fields("impl_gold_perfect_runs", row.perfect),
    }


def _repeatability_row(row: RepeatabilityRow) -> dict[str, Any]:
    return {
        "table": row.table,
        "slice_id": row.slice_id,
        "task_id": row.task_id,
        "builds": "" if row.builds is None else row.builds,
        "find_runs": row.find_runs,
        "answer_rows": row.answer_rows,
        "stable_cells": row.stable_cells,
        "total_cells": row.total_cells,
        "agreement_pct": percent(Fraction(row.stable_cells, row.total_cells)),
        "attempts_per_cell": f"{row.min_attempts}-{row.max_attempts}",
        "reliable_questions": row.reliable_questions,
        "questions": row.questions,
    }


def _low_prior_row(question: LowPriorQuestion) -> dict[str, Any]:
    return {
        "task_id": question.task_id,
        "question_id": question.question_id,
        "released_stable_cells": question.released[0],
        "released_cells": question.released[1],
        "corrected_stable_cells": question.corrected[0],
        "corrected_cells": question.corrected[1],
        "corrected_agreement_pct": percent(Fraction(*question.corrected)),
        "still_low_prior": int(question.still_low_prior),
    }


def _finder_row(finder: FinderCalibration) -> dict[str, Any]:
    row: dict[str, Any] = {
        "finder_id": finder.finder_id,
        **_rate_fields("formal_all_rows", finder.formal_all_rows),
        "question_only_runs": finder.question_only_runs,
    }
    for question_set in QUESTION_SETS:
        row.update(
            _rate_fields(f"formal_matched_{question_set}", finder.formal_matched[question_set])
        )
        row.update(
            _rate_fields(f"question_only_{question_set}", finder.question_only[question_set])
        )
        row[f"delta_{question_set}_pp"] = signed_points(finder.delta(question_set))
    row["mean_tokens_k"] = round_half_up(Fraction(finder.mean_find_tokens) / 1000, 0)
    return row


def _rate_fields(prefix: str, rate: Rate | None) -> dict[str, Any]:
    if rate is None:
        return {f"{prefix}_correct": "", f"{prefix}_total": "", f"{prefix}_pct": ""}
    return {
        f"{prefix}_correct": rate.correct,
        f"{prefix}_total": rate.total,
        f"{prefix}_pct": percent(rate.value),
    }


def _summary(analysis: Analysis) -> dict[str, Any]:
    exposure_counts = Counter(run.exposure for run in analysis.exclusions)
    selection = analysis.selection
    return {
        "analysis_id": ANALYSIS_ID,
        "analysis_script": "scripts/analyze_control_exclusions.py",
        "generated_at": analysis.generated_at,
        "inputs": {name: _display_path(path) for name, path in analysis.inputs.items()},
        "input_hashes": {_display_path(path): _sha256(path) for path in analysis.inputs.values()},
        "exclusion_rule": (
            "question-only control runs whose transcripts show reads outside their workspace"
        ),
        "excluded_runs": len(analysis.exclusions),
        "excluded_runs_by_exposure": {
            exposure: exposure_counts[exposure] for exposure in EXPOSURES
        },
        "matched_strata": "finder_id x task_id x finder_trial_id of each excluded run",
        "bootstrap": {
            "samples": BOOTSTRAP_SAMPLES,
            "seed": BOOTSTRAP_SEED,
            "rng_stream": "<seed>:<contrast_id>[:<task_id>]",
            "cluster_unit": "task_id + question_id",
            "interval": "percentile 95%, indices int(0.025*(B-1)) and int(0.975*(B-1))",
            "rounding": "round half up on the exact rational",
        },
        "released_bootstrap_reproduced": all(row["reproduced"] for row in analysis.released_check),
        "released_bootstrap_check": analysis.released_check,
        "question_only_accuracy": {
            name: {"correct": rate.correct, "total": rate.total, "pct": percent(rate.value)}
            for name, rate in analysis.question_only.items()
        },
        "question_only_runs": analysis.question_only_runs,
        "label_sources_v1_1": analysis.label_sources,
        "low_prior": {
            "frozen_questions": len(analysis.low_prior.questions),
            "still_low_prior": sum(q.still_low_prior for q in analysis.low_prior.questions),
            "question_only_pct": percent(analysis.low_prior.question_only.value),
            "recovery_all_rows_pct": percent(analysis.low_prior.recovery_all_rows.value),
            "delta_pp": analysis.low_prior.delta.printed,
            "impl_gold_recovery_pct": percent(analysis.low_prior.implemented_gold.value),
        },
        "core_finder_selection": {
            "default_core_finders": selection.core_finders,
            "prior_stress_audit_finders": selection.audit_finders,
            "released_default_core_finders": selection.released_core_finders,
            "released_prior_stress_audit_finders": selection.released_audit_finders,
            "selection_unchanged": selection.unchanged,
            "inputs_replaced": [
                "question_only_accuracy",
                "formal_minus_question_only",
                "low_prior_formal_accuracy",
                "low_prior_question_only_accuracy",
                "low_prior_lift",
                "implementation_signal_formal_accuracy",
                "implementation_signal_question_only_accuracy",
                "implementation_signal_lift",
            ],
        },
        "outputs": [
            "printed_values.csv",
            "contrasts.csv",
            "calibration_rows.csv",
            "repeatability.csv",
            "low_prior_questions.csv",
            "finder_calibration.csv",
            "finder_selection.csv",
            "summary.json",
            "report.md",
        ],
    }


def _report(analysis: Analysis) -> str:
    exposure_counts = Counter(run.exposure for run in analysis.exclusions)
    qo = analysis.question_only
    lines = [
        "# Question-only control exclusions",
        "",
        "Generated by `scripts/analyze_control_exclusions.py`; do not edit by hand.",
        "",
        "The question-only control excludes runs whose transcripts show reads outside their",
        f"workspace: {len(analysis.exclusions)} of {analysis.question_only_runs['released']} runs "
        f"({exposure_counts['answer_bearing']} answer-bearing, "
        f"{exposure_counts['seed_project']} seed-project, "
        f"{exposure_counts['other_artifact']} other-artifact; see "
        f"`{EXCLUSIONS_NAME}` and `README.md`).",
        f"Question-only accuracy is {percent(qo['released'].value)}% on all runs, "
        f"{percent(qo['corrected'].value)}% after the exclusions, and "
        f"{percent(qo['answer_bearing_only'].value)}% when only the answer-bearing runs "
        "are excluded.",
        "",
        "Differences from the question-only control use matched finder x task x trial strata "
        f"and the released question-cluster bootstrap ({BOOTSTRAP_SAMPLES} resamples, RNG "
        f"`{BOOTSTRAP_SEED}:<contrast>[:<task>]`). Printed values are rounded half up on the "
        "exact rational. Implemented-gold columns use the v1.0 labels with "
        "`label_corrections_v1_1.csv` applied.",
        "",
        "## Checks",
        "",
        "- Released `bootstrap_lift.csv` reproduced exactly: "
        + ("yes" if all(row["reproduced"] for row in analysis.released_check) else "NO"),
        "- Frozen low-prior subset equals the released-control rule (agreement < 90%): yes",
        "- Core-finder selection unchanged: "
        + ("yes" if analysis.selection.unchanged else "NO")
        + f" (core {', '.join(f'`{x}`' for x in analysis.selection.core_finders)}; "
        f"audit {', '.join(f'`{x}`' for x in analysis.selection.audit_finders)})",
        "- Label sources after v1.1 corrections: "
        + ", ".join(f"{name} {count}" for name, count in analysis.label_sources.items()),
        "",
        "## Printed values",
        "",
        "Exact values are percentages or percentage points; `tie` marks an exact half at the",
        "printed precision.",
    ]
    current = None
    for value in analysis.printed:
        if value.location != current:
            current = value.location
            lines.extend(
                ["", f"### {current}", "", "| value | printed | exact |", "| --- | ---: | --- |"]
            )
        exact = value.exact + (" (tie)" if value.half_tie else "")
        lines.append(f"| `{value.value_id}` | {value.printed} | {exact} |")
    lines.extend(
        [
            "",
            "## Core-finder selector rerun",
            "",
            "Formal accuracy, its interval, tokens, affinity, and rank stability are the released",
            "selector inputs. The question-only accuracy and the overall, low-prior, and",
            "implementation-signal lifts are the corrected control and the matched differences",
            "of Table 9, so a lift can differ from formal minus question-only accuracy.",
            "",
            "| finder | selection | q-only lift | low-prior lift | impl.-signal lift |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
    )
    finders = {finder.finder_id: finder for finder in analysis.finders}
    for row in analysis.selection.rows:
        finder = finders[row["finder_id"]]
        lifts = " | ".join(signed_points(finder.delta(name)) for name in QUESTION_SETS)
        lines.append(f"| `{finder.finder_id}` | {row['selection']} | {lifts} |")
    lines.extend(
        [
            "",
            "## Frozen low-prior questions",
            "",
            "| question | released stable cells | corrected stable cells | still below 90% |",
            "| --- | ---: | ---: | :---: |",
        ]
    )
    for question in analysis.low_prior.questions:
        lines.append(
            f"| `{question.task_id}:{question.question_id}` | "
            f"{question.released[0]}/{question.released[1]} | "
            f"{question.corrected[0]}/{question.corrected[1]} | "
            f"{'yes' if question.still_low_prior else 'no'} |"
        )
    lines.extend(
        [
            "",
            "## Excluded runs",
            "",
            "| find run | exposure | evidence |",
            "| --- | --- | --- |",
        ]
    )
    for run in analysis.exclusions:
        lines.append(f"| `{run.find_run_id}` | {run.exposure} | {run.evidence} |")
    lines.append("")
    return "\n".join(lines)


# --------------------------------------------------------------------------- io


def _exact_percent(value: Fraction) -> str:
    return f"{float(100 * value):.4f}"


def _exact_points(value: Fraction) -> str:
    return f"{float(100 * value):+.4f}"


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def _read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def _as_bool(value: str) -> bool:
    return value.strip().lower() in {"1", "true", "yes"}


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


if __name__ == "__main__":
    main()
