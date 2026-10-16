"""Recompute paper metrics under cumulative source-audited label corrections.

This analysis is an additive rebuttal sensitivity layer.  It reads the released
canonical tables and final artifact-conformance labels plus the frozen rebuttal
spot-audit sample manifest and adjudications.  It applies cumulative correction
scenarios in memory and writes disposable outputs under
``rebuttal/full_conformance_audit/label_sensitivity``.  It never edits
``runs_release``.  It also counts the answers of the post-hoc evidence-citation
audit sample that fall on pairs each scenario relabels, and tabulates the
relabeled pairs by new label and by question.

With ``--author-decisions`` the full-audit flags are combined with the recorded
author decisions instead: baseline, the nine observed errors, the
author-adjudicated set (confirmed relabels plus observed errors; kept flags stay
``implemented_gold``), and a worst case that also relabels every flag without an
author decision.  Those outputs default to ``label_sensitivity_adjudicated``.

With ``--model-assisted-decisions`` (requires ``--author-decisions``) the
model-assisted decisions on the flags without an author decision are also read,
and a ``fully_adjudicated`` scenario (author-adjudicated set plus the
model-assisted relabels; model-kept flags stay ``implemented_gold``) is inserted
before the worst case.  The other scenarios are unchanged.

With ``--label-corrections`` the corrections file (``label_corrections_v1_1.csv``)
is overlaid on the released labels first, so the baseline scenario uses the same
labels as the paper analysis.  Without it the baseline is the released v1.0
labels, which is the input contract of the July full-audit generation.

Metric definitions are intentionally delegated to
``analyze_artifact_conformance_impact.py`` so that conditioned accuracy,
perfect-run cells, and total-byte R_b use the same implementation as the paper
analysis.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import re
import shlex
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, fields
from datetime import date
from pathlib import Path
from typing import Any, Iterable, Sequence

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "scripts"
SRC = ROOT / "src"
for import_path in (SCRIPTS, SRC, ROOT):
    if str(import_path) not in sys.path:
        sys.path.insert(0, str(import_path))

import analyze_artifact_conformance_impact as impact  # noqa: E402


ANALYSIS_ID = "rebuttal_label_sensitivity"
MANIFEST_VERSION = "rebuttal_label_sensitivity_v2"
FULL_AUDIT_DIR = ROOT / "rebuttal" / "full_conformance_audit"
SCENARIOS = (
    "baseline",
    "pilot_two_corrected",
    "high_confidence_corrected",
    "all_observed_corrected",
    "full_audit_flags_corrected",
)
FULL_AUDIT_SCENARIO = "full_audit_flags_corrected"
OBSERVED_SCENARIO = "all_observed_corrected"
AUTHOR_ADJUDICATED_SCENARIO = "author_adjudicated"
AUTHOR_WORST_CASE_SCENARIO = "author_adjudicated_plus_unreviewed_flags"
AUTHOR_SCENARIOS = (
    "baseline",
    OBSERVED_SCENARIO,
    AUTHOR_ADJUDICATED_SCENARIO,
    AUTHOR_WORST_CASE_SCENARIO,
)
FULLY_ADJUDICATED_SCENARIO = "fully_adjudicated"
FULLY_ADJUDICATED_SCENARIOS = (
    "baseline",
    OBSERVED_SCENARIO,
    AUTHOR_ADJUDICATED_SCENARIO,
    FULLY_ADJUDICATED_SCENARIO,
    AUTHOR_WORST_CASE_SCENARIO,
)
SCRATCH_TASKS = tuple(sorted(impact.SCRATCH_TASKS))
SPOT_AUDIT_EXPECTED_ROWS = 24
SPOT_AUDIT_EXPECTED_DISAGREEMENTS = 7
SPOT_AUDIT_EXPECTED_HIGH_CONFIDENCE_DISAGREEMENTS = 6
AUTHOR_DECISION_EXPECTED_ROWS = 43
AUTHOR_DECISION_EXPECTED_KEEPS = 2
AUTHOR_KEEP_DECISION = "keep_implemented_gold"
AUTHOR_DECISION_LABELS = {
    "absent_or_ambiguous": "absent_or_ambiguous",
    "non_gold": "implemented_non_gold",
}
FLAG_BASIS_BY_DISAGREEMENT_BASIS = {
    "both_passes": "both_passes",
    "primary_pass": "primary_only",
    "crosscheck_pass": "crosscheck_only",
}
EVIDENCE_AUDIT_LABELS = ("Supported", "Partially supported", "Unsupported", "Prior-like")
EVIDENCE_AUDIT_SLICES = ("all", "low_prior", "high_prior")


@dataclass(frozen=True)
class Correction:
    task_id: str
    artifact_id: str
    question_id: str
    old_label: str
    new_label: str
    reason: str
    provenance: str
    confidence: str

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.task_id, self.artifact_id, self.question_id)


@dataclass(frozen=True)
class AuthorDecision:
    """One recorded author decision on a full-audit flag (``author_decisions.csv`` row)."""

    task_id: str
    artifact_id: str
    question_id: str
    flag_basis: str
    author_decision: str
    decided_by: str
    decided_on: str
    worksheet_item: str
    rationale: str

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.task_id, self.artifact_id, self.question_id)


AUTHOR_DECISION_FIELDS = tuple(field.name for field in fields(AuthorDecision))


@dataclass(frozen=True)
class ModelAssistedDecision:
    """One model-assisted decision on a flag without an author decision.

    Rows of ``model_assisted_decisions.csv``; ``review_status`` records whether the
    author has reviewed the decision and ``reviewed_on`` the review date (empty
    while the review is pending).  ``decided_by`` keeps the model-assisted source.
    """

    task_id: str
    artifact_id: str
    question_id: str
    flag_basis: str
    decision: str
    actual_option: str
    evidence: str
    rationale: str
    confidence: str
    pass_agreement: str
    decided_by: str
    review_status: str
    decided_on: str
    reviewed_on: str

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.task_id, self.artifact_id, self.question_id)

    @property
    def review(self) -> str:
        if self.reviewed_on:
            return f"{self.review_status} on {self.reviewed_on}"
        return self.review_status


MODEL_DECISION_FIELDS = tuple(field.name for field in fields(ModelAssistedDecision))
MODEL_DECISION_OPTIONS = {"A", "B", "C", "D", "none", "multiple"}
MODEL_DECISION_CONFIDENCES = ("high", "medium", "low")
MODEL_DECISION_PASS_AGREEMENT = {"agree", "arbitrated"}
MODEL_REVIEW_PENDING = "pending_author_review"
MODEL_REVIEW_APPROVED = "author_approved"
MODEL_REVIEW_STATUSES = {MODEL_REVIEW_PENDING, MODEL_REVIEW_APPROVED}


PILOT_CORRECTIONS = (
    Correction(
        task_id="scratch_minidb",
        artifact_id=("build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3"),
        question_id="q_db_05",
        old_label="implemented_gold",
        new_label="implemented_non_gold",
        reason=(
            "Source audit found that the live CLI path bypasses the declared "
            "Volcano operators and materializes rows."
        ),
        provenance="pilot_source_audit",
        confidence="source_confirmed",
    ),
    Correction(
        task_id="scratch_nanoweb",
        artifact_id=("build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601"),
        question_id="q_nw_07",
        old_label="implemented_gold",
        new_label="absent_or_ambiguous",
        reason=(
            "Source audit found a Prometheus endpoint without the required "
            "method/path/status-labeled counters."
        ),
        provenance="pilot_source_audit",
        confidence="source_confirmed",
    ),
)


def main() -> None:
    args = _parse_args()
    runs_root = args.runs_root.resolve()
    author_decisions_path = (
        args.author_decisions.resolve() if args.author_decisions is not None else None
    )
    label_corrections_path = (
        args.label_corrections.resolve() if args.label_corrections is not None else None
    )
    model_decisions_path = (
        args.model_assisted_decisions.resolve()
        if args.model_assisted_decisions is not None
        else None
    )
    if model_decisions_path is not None and author_decisions_path is None:
        raise SystemExit("--model-assisted-decisions requires --author-decisions")
    if args.output_dir is not None:
        output_dir = args.output_dir.resolve()
    elif author_decisions_path is not None:
        output_dir = FULL_AUDIT_DIR / "label_sensitivity_adjudicated"
    else:
        output_dir = FULL_AUDIT_DIR / "label_sensitivity"
    _assert_output_is_additive(runs_root, output_dir)

    inputs = _input_paths(
        runs_root,
        full_audit_disagreements=args.full_audit_disagreements.resolve(),
        author_decisions=author_decisions_path,
        label_corrections=label_corrections_path,
        model_assisted_decisions=model_decisions_path,
    )
    _require_files(inputs.values())
    analysis_command = _analysis_command(
        runs_root=runs_root,
        output_dir=output_dir,
        full_audit_disagreements=inputs["full_audit_disagreements"],
        author_decisions=author_decisions_path,
        label_corrections=label_corrections_path,
        model_assisted_decisions=model_decisions_path,
    )

    find_rows = impact._read_csv(inputs["find_runs"])
    answer_rows = impact._read_csv(inputs["question_answers"])
    baseline_labels = impact._read_csv(inputs["final_labels"])
    if label_corrections_path is not None:
        baseline_labels = impact._apply_label_corrections(
            baseline_labels, impact._read_csv(label_corrections_path)
        )
    low_prior_questions = impact._load_question_set(
        inputs["low_prior_manifest"],
        keys=("question_subset", "questions"),
    )

    spot_corrections = _load_spot_audit_corrections(
        minidb_rows=impact._read_csv(inputs["spot_audit_minidb"]),
        nanoweb_rows=impact._read_csv(inputs["spot_audit_nanoweb"]),
        sample_manifest_rows=impact._read_csv(inputs["spot_audit_sample_manifest"]),
    )
    high_confidence_spot = tuple(
        correction for correction in spot_corrections if correction.confidence == "high"
    )
    observed_corrections = (*PILOT_CORRECTIONS, *spot_corrections)
    full_audit_rows = impact._read_csv(inputs["full_audit_disagreements"])
    full_audit_bundle_validation = _validate_full_audit_bundle(
        inputs=inputs,
        disagreement_rows=full_audit_rows,
    )
    full_audit_corrections = _load_full_audit_corrections(full_audit_rows)
    observed_keys = {correction.key for correction in observed_corrections}
    full_audit_keys = {correction.key for correction in full_audit_corrections}
    missing_regressions = sorted(observed_keys - full_audit_keys)
    if missing_regressions:
        raise RuntimeError(
            f"full audit failed to reproduce prior correction keys: {missing_regressions}"
        )
    scored_label_sources = _scored_pair_label_sources(baseline_labels)
    if sum(scored_label_sources.values()) != full_audit_bundle_validation["adjudication_rows"]:
        raise RuntimeError(
            "scored compile-pass implemented_gold pairs differ from the full-audit population: "
            f"{scored_label_sources}"
        )

    author_decisions: tuple[AuthorDecision, ...] = ()
    author_summary: dict[str, Any] | None = None
    model_decisions: tuple[ModelAssistedDecision, ...] = ()
    model_summary: dict[str, Any] | None = None
    if author_decisions_path is None:
        correction_universe = _merge_correction_universe(
            observed=observed_corrections,
            full_audit=full_audit_corrections,
        )
        scenario_corrections: dict[str, tuple[Correction, ...]] = {
            "baseline": (),
            "pilot_two_corrected": PILOT_CORRECTIONS,
            "high_confidence_corrected": (
                *PILOT_CORRECTIONS,
                *high_confidence_spot,
            ),
            OBSERVED_SCENARIO: observed_corrections,
            FULL_AUDIT_SCENARIO: full_audit_corrections,
        }
        expected_counts = {
            "baseline": 0,
            "pilot_two_corrected": 2,
            "high_confidence_corrected": 8,
            OBSERVED_SCENARIO: 9,
            FULL_AUDIT_SCENARIO: len(full_audit_corrections),
        }
        expected_scenarios = SCENARIOS
    else:
        author_decisions = _load_author_decisions(
            impact._read_csv(inputs["author_decisions"]),
            disagreement_rows=full_audit_rows,
        )
        author_adjudicated, worst_case = _author_adjudicated_corrections(
            observed=observed_corrections,
            full_audit=full_audit_corrections,
            decisions=author_decisions,
        )
        author_summary = _author_adjudication_summary(
            decisions=author_decisions,
            observed=observed_corrections,
            full_audit=full_audit_corrections,
            author_adjudicated=author_adjudicated,
            worst_case=worst_case,
            labels=baseline_labels,
        )
        correction_universe = worst_case
        scenario_corrections = {
            "baseline": (),
            OBSERVED_SCENARIO: observed_corrections,
            AUTHOR_ADJUDICATED_SCENARIO: author_adjudicated,
            AUTHOR_WORST_CASE_SCENARIO: worst_case,
        }
        expected_counts = {
            "baseline": 0,
            OBSERVED_SCENARIO: 9,
            AUTHOR_ADJUDICATED_SCENARIO: author_summary["author_adjudicated_pairs"],
            AUTHOR_WORST_CASE_SCENARIO: author_summary["worst_case_pairs"],
        }
        expected_scenarios = AUTHOR_SCENARIOS
        if model_decisions_path is not None:
            model_decisions = _load_model_assisted_decisions(
                impact._read_csv(inputs["model_assisted_decisions"]),
                disagreement_rows=full_audit_rows,
                author_decisions=author_decisions,
            )
            fully_adjudicated = _fully_adjudicated_corrections(
                observed=observed_corrections,
                author_adjudicated=author_adjudicated,
                full_audit=full_audit_corrections,
                decisions=model_decisions,
            )
            model_summary = _model_assisted_summary(
                decisions=model_decisions,
                observed=observed_corrections,
                full_audit=full_audit_corrections,
                author_adjudicated=author_adjudicated,
                fully_adjudicated=fully_adjudicated,
            )
            # Validation universe: one row per (key, label).  The fully adjudicated
            # rows carry the model-assisted provenance; worst-case rows are added only
            # where the worst case uses a different label (or a model-kept pair).
            fully_pairs = {(row.key, row.new_label) for row in fully_adjudicated}
            correction_universe = tuple(
                sorted(
                    (
                        *fully_adjudicated,
                        *(row for row in worst_case if (row.key, row.new_label) not in fully_pairs),
                    ),
                    key=lambda row: (row.key, row.new_label),
                )
            )
            scenario_corrections = {
                "baseline": (),
                OBSERVED_SCENARIO: observed_corrections,
                AUTHOR_ADJUDICATED_SCENARIO: author_adjudicated,
                FULLY_ADJUDICATED_SCENARIO: fully_adjudicated,
                AUTHOR_WORST_CASE_SCENARIO: worst_case,
            }
            expected_counts[FULLY_ADJUDICATED_SCENARIO] = model_summary["fully_adjudicated_pairs"]
            expected_counts = {
                scenario: expected_counts[scenario] for scenario in scenario_corrections
            }
            expected_scenarios = FULLY_ADJUDICATED_SCENARIOS
    scenarios = tuple(scenario_corrections)
    if scenarios != expected_scenarios:
        raise RuntimeError(f"scenario correction definitions drifted: {scenarios}")
    _validate_correction_sets(scenario_corrections, expected_counts=expected_counts)
    correction_evidence = _validate_corrections(
        corrections=correction_universe,
        scenario_corrections=scenario_corrections,
        labels=baseline_labels,
        answer_rows=answer_rows,
        low_prior_questions=low_prior_questions,
    )
    scenario_labels = {
        scenario: (
            baseline_labels
            if not corrections
            else _apply_corrections(
                baseline_labels,
                corrections=corrections,
                label_source=f"rebuttal_{scenario}_overlay",
            )
        )
        for scenario, corrections in scenario_corrections.items()
    }

    scenario_results = {
        scenario: _analyze_scenario(
            scenario=scenario,
            labels=labels,
            find_rows=find_rows,
            answer_rows=answer_rows,
            low_prior_questions=low_prior_questions,
        )
        for scenario, labels in scenario_labels.items()
    }

    _validate_baseline_rb(
        recomputed=scenario_results["baseline"]["rb_rankings"],
        published_path=inputs["published_builder_scores"],
    )
    _validate_scenario_deltas(
        scenario_results=scenario_results,
        scenario_corrections=scenario_corrections,
        correction_evidence=correction_evidence,
    )

    label_rows = _combine(scenario_results, "label_accuracy")
    coverage_rows = _combine(scenario_results, "implementation_coverage")
    coverage_change_rows = _coverage_change_rows(coverage_rows, scenarios=scenarios)
    conditioned_accuracy_rows = _combine(scenario_results, "conditioned_accuracy")
    rb_rows = _combine(scenario_results, "rb_rankings")
    perfect_run_summary_rows = _combine(scenario_results, "perfect_run_summary")
    perfect_cell_summary_rows = _combine(scenario_results, "perfect_cell_summary")

    rb_change_rows = _rb_change_rows(
        scenario_results=scenario_results,
    )
    run_metric_changes = _run_metric_change_rows(
        scenario_results=scenario_results,
    )
    perfect_cell_changes = _perfect_cell_change_rows(
        scenario_results=scenario_results,
    )
    evidence_overlap_rows = _evidence_audit_overlap_rows(
        sample_rows=impact._read_csv(inputs["evidence_audit_sample"]),
        scenario_corrections=scenario_corrections,
    )
    relabeled_rows = _relabeled_pair_rows(
        scenarios=scenarios,
        correction_evidence=correction_evidence,
    )
    question_rows = _question_label_rows(scenario_labels=scenario_labels)

    output_dir.mkdir(parents=True, exist_ok=True)
    output_specs: dict[str, tuple[list[dict[str, Any]], Sequence[str]]] = {
        "corrections.csv": (
            correction_evidence,
            (
                "task_id",
                "artifact_id",
                "question_id",
                "old_label",
                "new_label",
                "reason",
                "provenance",
                "confidence",
                "included_scenarios",
                "compile_pass",
                "builder_id",
                "primary_answer_rows",
                "primary_correct_answer_rows",
                "primary_find_runs",
                "finder_count",
                "trials_per_finder",
                "in_low_prior",
            ),
        ),
        "label_accuracy.csv": (
            label_rows,
            (
                "scenario",
                "scope_id",
                "task_id",
                "final_label",
                "artifact_question_pairs",
                "answer_rows",
                "correct_answer_rows",
                "accuracy",
            ),
        ),
        "implementation_coverage.csv": (
            coverage_rows,
            (
                "scenario",
                "scope_id",
                "grain",
                "entity_id",
                "task_id",
                "builder_id",
                "artifact_count",
                "artifact_question_pairs",
                "implemented_gold",
                "implemented_non_gold",
                "absent_or_ambiguous",
                "implementation_coverage",
            ),
        ),
        "implementation_coverage_changes.csv": (
            coverage_change_rows,
            (
                "scenario",
                "scope_id",
                "grain",
                "entity_id",
                "task_id",
                "builder_id",
                "artifact_count",
                "artifact_question_pairs",
                "baseline_implemented_gold",
                "scenario_implemented_gold",
                "implemented_gold_delta",
                "baseline_implementation_coverage",
                "scenario_implementation_coverage",
                "implementation_coverage_delta",
            ),
        ),
        "conditioned_accuracy.csv": (
            conditioned_accuracy_rows,
            (
                "scenario",
                "slice_id",
                "task_id",
                "artifact_question_pairs",
                "answer_rows",
                "correct_answer_rows",
                "accuracy",
            ),
        ),
        "rb_rankings.csv": (
            rb_rows,
            (
                "scenario",
                "panel_id",
                "builder_id",
                "rank",
                "score",
                "n_cells",
                "max_cells",
                "cell_coverage",
                "perfect_runs_contributing",
                "mean_cell_accuracy",
                "mean_question_count",
            ),
        ),
        "rb_rank_changes.csv": (
            rb_change_rows,
            (
                "scenario",
                "panel_id",
                "builder_id",
                "baseline_rank",
                "scenario_rank",
                "rank_delta",
                "baseline_score",
                "scenario_score",
                "score_delta",
                "baseline_n_cells",
                "scenario_n_cells",
                "baseline_cell_coverage",
                "scenario_cell_coverage",
                "ranking_changed",
            ),
        ),
        "perfect_run_summary.csv": (
            perfect_run_summary_rows,
            (
                "scenario",
                "panel_id",
                "find_runs",
                "perfect_runs",
                "perfect_run_rate",
                "observed_cells",
                "contributing_perfect_cells",
                "perfect_cell_coverage",
            ),
        ),
        "perfect_cell_summary.csv": (
            perfect_cell_summary_rows,
            (
                "scenario",
                "panel_id",
                "task_id",
                "builder_id",
                "finder_id",
                "run_count",
                "perfect_runs",
                "contributes_to_perfect_gate",
            ),
        ),
        "run_metric_changes.csv": (
            run_metric_changes,
            (
                "scenario",
                "find_run_id",
                "panel_id",
                "task_id",
                "build_run_id",
                "builder_id",
                "finder_id",
                "baseline_question_count",
                "scenario_question_count",
                "baseline_accuracy",
                "scenario_accuracy",
                "baseline_perfect_run",
                "scenario_perfect_run",
                "perfect_status_changed",
            ),
        ),
        "perfect_cell_changes.csv": (
            perfect_cell_changes,
            (
                "scenario",
                "panel_id",
                "task_id",
                "builder_id",
                "finder_id",
                "baseline_run_count",
                "scenario_run_count",
                "baseline_perfect_runs",
                "scenario_perfect_runs",
                "perfect_run_delta",
                "baseline_contributes",
                "scenario_contributes",
                "contribution_changed",
            ),
        ),
        "evidence_audit_overlap.csv": (
            evidence_overlap_rows,
            (
                "scenario",
                "slice_id",
                "audit_label",
                "sampled_answers",
                "on_relabeled_pairs",
            ),
        ),
        "relabeled_pairs.csv": (
            relabeled_rows,
            (
                "scenario",
                "new_label",
                "relabeled_pairs",
                "primary_answer_rows",
                "gold_option_rows",
                "gold_option_share",
            ),
        ),
        "question_labels.csv": (
            question_rows,
            (
                "scenario",
                "task_id",
                "question_id",
                "compile_pass_artifacts",
                "baseline_implemented_gold",
                "scenario_implemented_gold",
                "relabeled_pairs",
            ),
        ),
    }
    for filename, (rows, fieldnames) in output_specs.items():
        _write_csv(output_dir / filename, rows, fieldnames)

    report_path = output_dir / "report.md"
    report_path.write_text(
        _render_report(
            scenarios=scenarios,
            analysis_command=analysis_command,
            scored_label_sources=scored_label_sources,
            author_decisions=author_decisions,
            author_summary=author_summary,
            author_decisions_path=inputs.get("author_decisions"),
            label_corrections_path=inputs.get("label_corrections"),
            model_decisions=model_decisions,
            model_summary=model_summary,
            model_decisions_path=inputs.get("model_assisted_decisions"),
            correction_evidence=correction_evidence,
            label_rows=label_rows,
            coverage_rows=coverage_rows,
            coverage_change_rows=coverage_change_rows,
            conditioned_accuracy_rows=conditioned_accuracy_rows,
            rb_rows=rb_rows,
            rb_change_rows=rb_change_rows,
            perfect_run_summary_rows=perfect_run_summary_rows,
            run_metric_changes=run_metric_changes,
            perfect_cell_changes=perfect_cell_changes,
            evidence_overlap_rows=evidence_overlap_rows,
            evidence_sample_path=inputs["evidence_audit_sample"],
            relabeled_rows=relabeled_rows,
            question_rows=question_rows,
        ),
        encoding="utf-8",
    )

    output_paths = [output_dir / filename for filename in output_specs]
    output_paths.append(report_path)
    manifest_inputs = dict(inputs)
    validation: dict[str, Any] = {
        "correction_keys": len({correction.key for correction in correction_universe}),
        "full_audit_flags": len(full_audit_corrections),
        "full_audit_contains_all_prior_corrections": True,
        "full_audit_disagreement_union_matches_adjudication": True,
        "full_audit_core_generation_verified": True,
        "primary_answer_rows_per_key": 36,
        "spot_audit_rows": SPOT_AUDIT_EXPECTED_ROWS,
        "spot_audit_all_rows_evidence_validated": True,
        "spot_audit_sample_manifest_identity_order_matches": True,
        "spot_audit_disagreements": SPOT_AUDIT_EXPECTED_DISAGREEMENTS,
        "spot_audit_high_confidence_disagreements": (
            SPOT_AUDIT_EXPECTED_HIGH_CONFIDENCE_DISAGREEMENTS
        ),
        "scored_pairs_match_full_audit_population": True,
        "baseline_rb_matches_published": True,
        "scenario_deltas_match_correction_evidence": True,
    }
    if author_summary is not None:
        validation.update(
            {
                "author_decision_rows": len(author_decisions),
                "author_decisions_resolve_to_full_audit_flags": True,
                "author_decision_flag_basis_matches_disagreements": True,
                "author_kept_pairs_in_no_scenario": True,
            }
        )
    if model_summary is not None:
        validation.update(
            {
                "model_assisted_decision_rows": len(model_decisions),
                "model_assisted_decisions_cover_flags_without_author_decision": True,
                "model_assisted_decision_flag_basis_matches_disagreements": True,
                "model_kept_pairs_absent_from_fully_adjudicated": True,
            }
        )
    if model_summary is not None:
        scenario_set = "fully_adjudicated"
    elif author_summary is not None:
        scenario_set = "author_adjudicated"
    else:
        scenario_set = "flag_overlay"
    manifest = {
        "manifest_version": MANIFEST_VERSION,
        "analysis_id": ANALYSIS_ID,
        "analysis_command": analysis_command,
        "analysis_script": _display_path(Path(__file__).resolve()),
        "analysis_script_sha256": _sha256(Path(__file__).resolve()),
        "canonical_inputs_are_read_only": True,
        "scenario_set": scenario_set,
        "scenario_order": list(scenarios),
        "scenario_correction_counts": {
            scenario: len(corrections) for scenario, corrections in scenario_corrections.items()
        },
        "scored_pair_label_sources": scored_label_sources,
        "observed_correction_count": len(observed_corrections),
        "full_audit_flag_count": len(full_audit_corrections),
        "correction_universe_count": len(correction_universe),
        "corrections": [
            {
                "task_id": correction.task_id,
                "artifact_id": correction.artifact_id,
                "question_id": correction.question_id,
                "old_label": correction.old_label,
                "new_label": correction.new_label,
                "reason": correction.reason,
                "provenance": correction.provenance,
                "confidence": correction.confidence,
            }
            for correction in correction_universe
        ],
        "input_paths": {
            name: _display_path(path) for name, path in sorted(manifest_inputs.items())
        },
        "input_sha256": {name: _sha256(path) for name, path in sorted(manifest_inputs.items())},
        "full_audit_bundle_validation": full_audit_bundle_validation,
        "metric_contract": {
            "accuracy_condition": "Only implemented_gold pairs contribute.",
            "effort_metric": "total_bytes",
            "effort_gate": "perfect_run",
            "perfect_run_definition": (
                "All conditioned implemented_gold answers in the run are correct."
            ),
            "effort_attribution": (
                "Observed full-run bytes are retained while accuracy and perfect-run "
                "status are recomputed over implemented_gold questions."
            ),
            "rb_normalization": (
                "Within each finder-task cell, divide builder mean bytes over perfect "
                "runs by the minimum contributing builder mean, then take the "
                "geometric mean across contributing cells."
            ),
            "low_prior_definition": "question-only three-trial agreement < 0.90",
        },
        "output_paths": [_display_path(path) for path in output_paths],
        "output_sha256": {_display_path(path): _sha256(path) for path in output_paths},
        "validation": validation,
    }
    if author_summary is not None:
        manifest["author_adjudication"] = author_summary
    if model_summary is not None:
        manifest["model_assisted_adjudication"] = model_summary
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    print(f"wrote {report_path}")
    print(
        f"validated {len(correction_evidence)} correction rows over "
        f"{validation['correction_keys']} keys at "
        "36 primary rows each; scenarios="
        + ", ".join(
            f"{scenario}:{len(corrections)}"
            for scenario, corrections in scenario_corrections.items()
        )
    )


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs-root",
        type=Path,
        default=ROOT / "runs_release",
        help="Released canonical input root (default: runs_release).",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help=(
            "Additive sensitivity output directory (default: label_sensitivity, or "
            "label_sensitivity_adjudicated with --author-decisions)."
        ),
    )
    parser.add_argument(
        "--full-audit-disagreements",
        type=Path,
        default=FULL_AUDIT_DIR / "disagreements.csv",
        help=(
            "Complete union of full-coverage source-audit flags; overlaid as the fifth "
            "scenario, or combined with --author-decisions."
        ),
    )
    parser.add_argument(
        "--author-decisions",
        type=Path,
        default=None,
        help=(
            "Recorded author decisions on full-audit flags (author_decisions.csv). "
            "Switches to the baseline / observed / author-adjudicated / worst-case "
            "scenario set."
        ),
    )
    parser.add_argument(
        "--model-assisted-decisions",
        type=Path,
        default=None,
        help=(
            "Model-assisted decisions on the flags without an author decision "
            "(model_assisted_decisions.csv). Requires --author-decisions and adds the "
            "fully_adjudicated scenario before the worst case."
        ),
    )
    parser.add_argument(
        "--label-corrections",
        type=Path,
        default=None,
        help=(
            "Label corrections overlaid on the released final labels before any scenario "
            "(label_corrections_v1_1.csv). Without it the baseline is the released labels."
        ),
    )
    return parser.parse_args()


def _analysis_command(
    *,
    runs_root: Path,
    output_dir: Path,
    full_audit_disagreements: Path,
    author_decisions: Path | None,
    label_corrections: Path | None = None,
    model_assisted_decisions: Path | None = None,
) -> str:
    """Render the invocation that regenerates these outputs, with repo-relative paths."""

    command = [
        "uv",
        "run",
        "python",
        _display_path(Path(__file__)),
        "--runs-root",
        _display_path(runs_root),
        "--full-audit-disagreements",
        _display_path(full_audit_disagreements),
    ]
    if author_decisions is not None:
        command.extend(["--author-decisions", _display_path(author_decisions)])
    if model_assisted_decisions is not None:
        command.extend(["--model-assisted-decisions", _display_path(model_assisted_decisions)])
    if label_corrections is not None:
        command.extend(["--label-corrections", _display_path(label_corrections)])
    command.extend(["--output-dir", _display_path(output_dir)])
    return shlex.join(command)


def _input_paths(
    runs_root: Path,
    *,
    full_audit_disagreements: Path,
    author_decisions: Path | None = None,
    label_corrections: Path | None = None,
    model_assisted_decisions: Path | None = None,
) -> dict[str, Path]:
    full_audit_dir = full_audit_disagreements.resolve().parent
    inputs = {
        "final_labels": (
            runs_root
            / "reports"
            / "artifact_conformance_consensus"
            / "final"
            / "artifact_conformance_final.csv"
        ),
        "find_runs": runs_root / "tables" / "find_runs.csv",
        "question_answers": runs_root / "tables" / "question_answers.csv",
        "low_prior_manifest": (
            runs_root / "reports" / "eandd_final" / "low_prior" / "manifest.json"
        ),
        "published_builder_scores": (
            runs_root
            / "reports"
            / "artifact_conformance_consensus"
            / "impact"
            / "conditioned_builder_scores.csv"
        ),
        "spot_audit_minidb": (
            ROOT / "rebuttal" / "conformance_spot_audit" / "minidb_adjudication.csv"
        ),
        "spot_audit_nanoweb": (
            ROOT / "rebuttal" / "conformance_spot_audit" / "nanoweb_adjudication.csv"
        ),
        "spot_audit_sample_manifest": (
            ROOT / "rebuttal" / "conformance_spot_audit" / "sample_manifest.csv"
        ),
        "evidence_audit_sample": runs_root / "reports" / "evidence_citation_audit_sample.csv",
        "full_audit_disagreements": full_audit_disagreements,
        "full_audit_minidb_adjudication": full_audit_dir / "minidb_adjudication.csv",
        "full_audit_nanoweb_adjudication": full_audit_dir / "nanoweb_adjudication.csv",
        "full_audit_validation_manifest": full_audit_dir / "validation_manifest.json",
        "full_audit_core_generation": full_audit_dir / "CORE_GENERATION.json",
    }
    if author_decisions is not None:
        inputs["author_decisions"] = author_decisions
    if label_corrections is not None:
        inputs["label_corrections"] = label_corrections
    if model_assisted_decisions is not None:
        inputs["model_assisted_decisions"] = model_assisted_decisions
    return inputs


def _assert_output_is_additive(runs_root: Path, output_dir: Path) -> None:
    if output_dir == runs_root or runs_root in output_dir.parents:
        raise SystemExit(
            f"refusing to write sensitivity outputs inside canonical runs root: {output_dir}"
        )


def _require_files(paths: Iterable[Path]) -> None:
    missing = [path for path in paths if not path.is_file()]
    if missing:
        raise SystemExit("missing required inputs: " + ", ".join(map(str, missing)))


def _validate_full_audit_bundle(
    *,
    inputs: dict[str, Path],
    disagreement_rows: list[dict[str, str]],
) -> dict[str, Any]:
    """Prove the sensitivity overlay is the complete finalized disagreement union."""

    adjudication_rows = [
        *impact._read_csv(inputs["full_audit_minidb_adjudication"]),
        *impact._read_csv(inputs["full_audit_nanoweb_adjudication"]),
    ]
    if len(adjudication_rows) != 592:
        raise RuntimeError(
            f"full-audit adjudication bundle must contain 592 rows, found {len(adjudication_rows)}"
        )
    adjudication_keys = [
        (row["task_id"], row["artifact_id"], row["question_id"]) for row in adjudication_rows
    ]
    if len(adjudication_keys) != len(set(adjudication_keys)):
        raise RuntimeError("full-audit adjudication bundle contains duplicate keys")
    invalid_flags = sorted(
        {
            row.get("flagged_for_author", "")
            for row in adjudication_rows
            if row.get("flagged_for_author", "") not in {"true", "false"}
        }
    )
    if invalid_flags:
        raise RuntimeError(f"invalid full-audit flagged_for_author values: {invalid_flags}")
    expected_flag_keys = {
        (row["task_id"], row["artifact_id"], row["question_id"])
        for row in adjudication_rows
        if row["flagged_for_author"] == "true"
    }
    disagreement_keys = {
        (row["task_id"], row["artifact_id"], row["question_id"]) for row in disagreement_rows
    }
    if len(disagreement_keys) != len(disagreement_rows):
        raise RuntimeError("full-audit disagreements contain duplicate keys")
    if disagreement_keys != expected_flag_keys:
        missing = sorted(expected_flag_keys - disagreement_keys)
        extra = sorted(disagreement_keys - expected_flag_keys)
        raise RuntimeError(
            "full-audit disagreement union differs from canonical flagged rows: "
            f"missing={missing} extra={extra}. Pass the complete flag union and record "
            "author decisions with --author-decisions instead of removing flags."
        )

    validation = json.loads(inputs["full_audit_validation_manifest"].read_text(encoding="utf-8"))
    if validation.get("population_pairs") != 592:
        raise RuntimeError("full-audit validation manifest has wrong population size")
    if validation.get("author_flag_count") != len(disagreement_rows):
        raise RuntimeError("full-audit validation manifest flag count differs from CSV")

    pointer = json.loads(inputs["full_audit_core_generation"].read_text(encoding="utf-8"))
    generation_manifest_path = ROOT / pointer.get("generation_manifest_path", "")
    if (
        not generation_manifest_path.is_file()
        or not generation_manifest_path.resolve().is_relative_to(
            (inputs["full_audit_disagreements"].parent / "generations").resolve()
        )
    ):
        raise RuntimeError("full-audit generation pointer has an invalid manifest path")
    if pointer.get("generation_manifest_sha256") != _sha256(generation_manifest_path):
        raise RuntimeError("full-audit generation pointer hash mismatch")
    generation_manifest = json.loads(generation_manifest_path.read_text(encoding="utf-8"))
    if generation_manifest.get("generation_id") != pointer.get("current_generation_id"):
        raise RuntimeError("full-audit generation id mismatch")
    if generation_manifest.get("generation_kind") != "core_audit":
        raise RuntimeError("full-audit core pointer names a non-core generation")
    generation_hashes = generation_manifest.get("content_sha256", {})
    for name in (
        "full_audit_disagreements",
        "full_audit_minidb_adjudication",
        "full_audit_nanoweb_adjudication",
        "full_audit_validation_manifest",
    ):
        path = inputs[name]
        relative = path.relative_to(inputs["full_audit_disagreements"].parent).as_posix()
        if generation_hashes.get(relative) != _sha256(path):
            raise RuntimeError(f"full-audit root alias differs from current generation: {relative}")
    return {
        "adjudication_rows": len(adjudication_rows),
        "disagreement_rows": len(disagreement_rows),
        "author_flag_count": validation["author_flag_count"],
        "disagreement_union_matches_adjudication": True,
        "core_generation_id_at_analysis": pointer["current_generation_id"],
        "core_generation_manifest_sha256_at_analysis": _sha256(generation_manifest_path),
        "current_core_generation_verified": True,
        "core_content_hashes_verified": True,
    }


def _load_spot_audit_corrections(
    *,
    minidb_rows: list[dict[str, str]],
    nanoweb_rows: list[dict[str, str]],
    sample_manifest_rows: list[dict[str, str]],
) -> tuple[Correction, ...]:
    all_rows = [*minidb_rows, *nanoweb_rows]
    required_fields = {
        "sample_index",
        "task_id",
        "artifact_id",
        "question_id",
        "consensus_label",
        "source_audit_label",
        "agrees_with_consensus",
        "confidence",
        "runtime_reachability_checked",
        "evidence_files",
        "evidence_symbols",
        "rationale",
        "adjudicator",
    }
    manifest_required_fields = {
        "sample_index",
        "task_id",
        "artifact_id",
        "question_id",
    }
    if len(all_rows) != SPOT_AUDIT_EXPECTED_ROWS:
        raise RuntimeError(
            f"expected {SPOT_AUDIT_EXPECTED_ROWS} spot-audit rows, found {len(all_rows)}"
        )
    if len(sample_manifest_rows) != SPOT_AUDIT_EXPECTED_ROWS:
        raise RuntimeError(
            f"expected {SPOT_AUDIT_EXPECTED_ROWS} sample-manifest rows, "
            f"found {len(sample_manifest_rows)}"
        )
    if any(not required_fields.issubset(row) for row in all_rows):
        raise RuntimeError("spot-audit CSV schema is missing required fields")
    if any(not manifest_required_fields.issubset(row) for row in sample_manifest_rows):
        raise RuntimeError("spot-audit sample manifest is missing required fields")

    expected_sample_indices = list(range(1, SPOT_AUDIT_EXPECTED_ROWS + 1))
    audit_sample_indices = [
        _strict_sample_index(row["sample_index"], source="spot-audit CSV") for row in all_rows
    ]
    manifest_sample_indices = [
        _strict_sample_index(row["sample_index"], source="sample manifest")
        for row in sample_manifest_rows
    ]
    if len(set(audit_sample_indices)) != len(audit_sample_indices):
        raise RuntimeError("spot-audit CSVs contain duplicate sample indices")
    if len(set(manifest_sample_indices)) != len(manifest_sample_indices):
        raise RuntimeError("spot-audit sample manifest contains duplicate sample indices")
    if audit_sample_indices != expected_sample_indices:
        raise RuntimeError("spot-audit CSV sample indices/order must be exactly 1 through 24")
    if manifest_sample_indices != expected_sample_indices:
        raise RuntimeError("spot-audit sample-manifest indices/order must be exactly 1 through 24")

    task_counts = Counter(row["task_id"] for row in all_rows)
    if task_counts != Counter({"scratch_minidb": 12, "scratch_nanoweb": 12}):
        raise RuntimeError(f"unexpected spot-audit task counts: {task_counts}")
    audit_identities = [
        (
            row["sample_index"],
            row["task_id"],
            row["artifact_id"],
            row["question_id"],
        )
        for row in all_rows
    ]
    manifest_identities = [
        (
            row["sample_index"],
            row["task_id"],
            row["artifact_id"],
            row["question_id"],
        )
        for row in sample_manifest_rows
    ]
    audit_keys = [identity[1:] for identity in audit_identities]
    manifest_keys = [identity[1:] for identity in manifest_identities]
    if len(set(audit_keys)) != len(audit_keys):
        raise RuntimeError("spot-audit CSVs contain duplicate artifact-question keys")
    if len(set(manifest_keys)) != len(manifest_keys):
        raise RuntimeError("spot-audit sample manifest contains duplicate artifact-question keys")
    if audit_identities != manifest_identities:
        mismatch_index = next(
            index
            for index, (audit_identity, manifest_identity) in enumerate(
                zip(audit_identities, manifest_identities, strict=True),
                start=1,
            )
            if audit_identity != manifest_identity
        )
        raise RuntimeError(
            "spot-audit CSV identity/order differs from sample manifest at "
            f"row {mismatch_index}: audit={audit_identities[mismatch_index - 1]}, "
            f"manifest={manifest_identities[mismatch_index - 1]}"
        )

    corrections: list[Correction] = []
    for row in all_rows:
        sample_index = row["sample_index"]
        consensus_label = row["consensus_label"].strip()
        source_audit_label = row["source_audit_label"].strip()
        if consensus_label != "implemented_gold":
            raise RuntimeError(
                "spot-audit population row is not consensus implemented_gold: "
                f"sample {sample_index}"
            )
        if source_audit_label not in impact.FINAL_LABELS:
            raise RuntimeError(
                f"invalid source_audit_label {source_audit_label!r} for sample {sample_index}"
            )
        confidence = row["confidence"].strip().lower()
        if confidence not in {"high", "medium", "low"}:
            raise RuntimeError(f"invalid confidence {confidence!r} for sample {sample_index}")
        if not _strict_audit_bool(
            row["runtime_reachability_checked"],
            field="runtime_reachability_checked",
            sample_index=sample_index,
        ):
            raise RuntimeError(f"runtime reachability was not checked for sample {sample_index}")
        for evidence_field in (
            "evidence_files",
            "evidence_symbols",
            "rationale",
            "adjudicator",
        ):
            if not row[evidence_field].strip():
                raise RuntimeError(f"spot-audit sample {sample_index} has empty {evidence_field}")
        agrees = _strict_audit_bool(
            row["agrees_with_consensus"],
            field="agrees_with_consensus",
            sample_index=sample_index,
        )
        if agrees:
            if source_audit_label != consensus_label:
                raise RuntimeError(
                    "spot-audit row says it agrees but labels differ: "
                    f"{row['task_id']}:{row['artifact_id']}:{row['question_id']}"
                )
            continue
        if source_audit_label == consensus_label:
            raise RuntimeError(
                "spot-audit row says it disagrees but labels match: "
                f"{row['task_id']}:{row['artifact_id']}:{row['question_id']}"
            )
        corrections.append(
            Correction(
                task_id=row["task_id"],
                artifact_id=row["artifact_id"],
                question_id=row["question_id"],
                old_label=consensus_label,
                new_label=source_audit_label,
                reason=row["rationale"],
                provenance=f"spot_audit_sample_{int(sample_index):02d}",
                confidence=confidence,
            )
        )

    corrections.sort(key=lambda correction: correction.key)
    confidence_counts = Counter(correction.confidence for correction in corrections)
    if len(corrections) != SPOT_AUDIT_EXPECTED_DISAGREEMENTS:
        raise RuntimeError(
            f"expected {SPOT_AUDIT_EXPECTED_DISAGREEMENTS} spot-audit "
            f"disagreements, found {len(corrections)}"
        )
    if confidence_counts["high"] != SPOT_AUDIT_EXPECTED_HIGH_CONFIDENCE_DISAGREEMENTS:
        raise RuntimeError(
            f"unexpected number of high-confidence spot-audit disagreements: {confidence_counts}"
        )
    if confidence_counts["medium"] != 1:
        raise RuntimeError(
            f"expected one medium-confidence disagreement, found {confidence_counts}"
        )
    return tuple(corrections)


def _load_full_audit_corrections(
    rows: list[dict[str, str]],
) -> tuple[Correction, ...]:
    required_fields = {
        "task_id",
        "artifact_id",
        "question_id",
        "consensus_label",
        "primary_verdict",
        "primary_confidence",
        "primary_evidence_files",
        "primary_evidence_symbols",
        "primary_evidence_spans",
        "primary_rationale",
        "primary_execution_engine",
        "primary_model_id",
        "primary_reasoning_effort",
        "primary_prompt_version",
        "primary_prompt_template_sha256",
        "primary_rendered_prompt_sha256",
        "primary_session_id",
        "primary_session_result_sha256",
        "primary_session_result_hash_source",
        "primary_integrity_attestation_sha256",
        "crosscheck_verdict",
        "crosscheck_confidence",
        "crosscheck_evidence_files",
        "crosscheck_evidence_symbols",
        "crosscheck_evidence_spans",
        "crosscheck_rationale",
        "crosscheck_execution_engine",
        "crosscheck_model_id",
        "crosscheck_reasoning_effort",
        "crosscheck_prompt_version",
        "crosscheck_prompt_template_sha256",
        "crosscheck_rendered_prompt_sha256",
        "crosscheck_session_id",
        "crosscheck_session_result_sha256",
        "crosscheck_session_result_hash_source",
        "crosscheck_integrity_attestation_sha256",
        "disagreement_basis",
        "flag_verdicts",
        "proposed_sensitivity_label",
        "author_adjudication_status",
    }
    if not rows:
        raise RuntimeError("full-audit disagreements file is empty")
    if any(not required_fields.issubset(row) for row in rows):
        raise RuntimeError("full-audit disagreements CSV is missing required fields")

    allowed_verdicts = {"confirmed_gold", "non_gold", "absent", "ambiguous"}
    allowed_bases = {"primary_pass", "crosscheck_pass", "both_passes"}
    allowed_labels = {"implemented_non_gold", "absent_or_ambiguous"}
    confidence_rank = {"high": 2, "medium": 1, "low": 0}
    corrections: list[Correction] = []
    seen: set[tuple[str, str, str]] = set()
    for row in rows:
        key = (row["task_id"], row["artifact_id"], row["question_id"])
        if key in seen:
            raise RuntimeError(f"duplicate full-audit disagreement key: {key}")
        seen.add(key)
        if row["consensus_label"] != "implemented_gold":
            raise RuntimeError(f"full-audit flag does not originate from implemented_gold: {key}")
        if row["author_adjudication_status"] != "pending":
            raise RuntimeError(f"full-audit flag is not pending author adjudication: {key}")
        primary = row["primary_verdict"]
        crosscheck = row["crosscheck_verdict"]
        if primary not in allowed_verdicts:
            raise RuntimeError(f"invalid primary verdict for {key}: {primary!r}")
        if crosscheck and crosscheck not in allowed_verdicts:
            raise RuntimeError(f"invalid crosscheck verdict for {key}: {crosscheck!r}")
        if not crosscheck:
            raise RuntimeError(f"flagged full-audit pair lacks crosscheck: {key}")
        flag_verdicts = [
            verdict for verdict in (primary, crosscheck) if verdict != "confirmed_gold"
        ]
        recorded_flags = row["flag_verdicts"].split("|")
        if recorded_flags != flag_verdicts:
            raise RuntimeError(
                f"full-audit flag verdict list mismatch for {key}: "
                f"{recorded_flags} != {flag_verdicts}"
            )
        if not flag_verdicts:
            raise RuntimeError(f"full-audit disagreement has no flag verdict: {key}")
        expected_basis = (
            "both_passes"
            if primary != "confirmed_gold" and crosscheck != "confirmed_gold"
            else "primary_pass"
            if primary != "confirmed_gold"
            else "crosscheck_pass"
        )
        if row["disagreement_basis"] not in allowed_bases:
            raise RuntimeError(
                f"invalid disagreement basis for {key}: {row['disagreement_basis']!r}"
            )
        if row["disagreement_basis"] != expected_basis:
            raise RuntimeError(
                f"disagreement basis mismatch for {key}: "
                f"{row['disagreement_basis']} != {expected_basis}"
            )
        expected_label = (
            "implemented_non_gold" if set(flag_verdicts) == {"non_gold"} else "absent_or_ambiguous"
        )
        proposed = row["proposed_sensitivity_label"]
        if proposed not in allowed_labels or proposed != expected_label:
            raise RuntimeError(
                f"full-audit sensitivity label mismatch for {key}: "
                f"{proposed!r} != {expected_label!r}"
            )
        for prefix, verdict in (("primary", primary), ("crosscheck", crosscheck)):
            for field in (
                "rationale",
                "execution_engine",
                "model_id",
                "reasoning_effort",
                "prompt_version",
                "prompt_template_sha256",
                "rendered_prompt_sha256",
                "session_id",
                "session_result_sha256",
                "session_result_hash_source",
                "integrity_attestation_sha256",
                "confidence",
                "evidence_files",
                "evidence_symbols",
                "evidence_spans",
            ):
                value = row[f"{prefix}_{field}"].strip()
                if not value:
                    raise RuntimeError(f"full-audit flag {key} has empty {prefix}_{field}")
            if row[f"{prefix}_confidence"] not in confidence_rank:
                raise RuntimeError(
                    f"invalid {prefix} confidence for {key}: {row[f'{prefix}_confidence']!r}"
                )
            if row[f"{prefix}_session_result_hash_source"] not in {
                "completion_record_and_parent_verified",
                "post_hoc_parent_attestation",
            }:
                raise RuntimeError(
                    f"invalid {prefix} result-hash source for {key}: "
                    f"{row[f'{prefix}_session_result_hash_source']!r}"
                )
            if (
                re.fullmatch(
                    r"[0-9a-f]{64}",
                    row[f"{prefix}_integrity_attestation_sha256"],
                )
                is None
            ):
                raise RuntimeError(f"invalid {prefix} attestation hash for {key}")
            if verdict == "confirmed_gold" and expected_basis == "both_passes":
                raise RuntimeError(f"inconsistent both-pass flag basis for {key}")

        flag_confidences = [
            row[f"{prefix}_confidence"]
            for prefix, verdict in (("primary", primary), ("crosscheck", crosscheck))
            if verdict != "confirmed_gold"
        ]
        confidence = min(
            flag_confidences,
            key=lambda value: confidence_rank[value],
        )
        reasons = []
        if primary != "confirmed_gold":
            reasons.append(f"Primary ({primary}): {row['primary_rationale']}")
        if crosscheck != "confirmed_gold":
            reasons.append(f"Cross-check ({crosscheck}): {row['crosscheck_rationale']}")
        corrections.append(
            Correction(
                task_id=key[0],
                artifact_id=key[1],
                question_id=key[2],
                old_label="implemented_gold",
                new_label=proposed,
                reason=" ".join(reasons),
                provenance=f"full_conformance_audit_{row['disagreement_basis']}",
                confidence=confidence,
            )
        )
    corrections.sort(key=lambda correction: correction.key)
    return tuple(corrections)


def _merge_correction_universe(
    *,
    observed: Sequence[Correction],
    full_audit: Sequence[Correction],
) -> tuple[Correction, ...]:
    observed_by_key = {correction.key: correction for correction in observed}
    full_by_key = {correction.key: correction for correction in full_audit}
    if len(observed_by_key) != len(observed):
        raise RuntimeError("duplicate key in observed correction set")
    if len(full_by_key) != len(full_audit):
        raise RuntimeError("duplicate key in full-audit correction set")
    for key in sorted(set(observed_by_key) & set(full_by_key)):
        before = observed_by_key[key]
        after = full_by_key[key]
        if before.old_label != after.old_label or before.new_label != after.new_label:
            raise RuntimeError(
                f"full-audit correction target conflicts with prior correction "
                f"for {key}: {before.old_label}->{before.new_label} vs "
                f"{after.old_label}->{after.new_label}"
            )
    merged = dict(observed_by_key)
    for key, after in full_by_key.items():
        before = merged.get(key)
        if before is None:
            merged[key] = after
            continue
        merged[key] = Correction(
            task_id=after.task_id,
            artifact_id=after.artifact_id,
            question_id=after.question_id,
            old_label=after.old_label,
            new_label=after.new_label,
            reason=f"Prior audit: {before.reason} Full audit: {after.reason}",
            provenance=f"{before.provenance}|{after.provenance}",
            confidence=f"{before.confidence}|{after.confidence}",
        )
    return tuple(merged[key] for key in sorted(merged))


def _load_author_decisions(
    rows: list[dict[str, str]],
    *,
    disagreement_rows: list[dict[str, str]],
) -> tuple[AuthorDecision, ...]:
    """Validate recorded author decisions against the full-audit flag union."""

    if not rows:
        raise RuntimeError("author decisions file is empty")
    if any(tuple(row) != AUTHOR_DECISION_FIELDS for row in rows):
        raise RuntimeError(f"author decisions CSV columns must be exactly {AUTHOR_DECISION_FIELDS}")
    flags_by_key = {
        (row["task_id"], row["artifact_id"], row["question_id"]): row for row in disagreement_rows
    }
    allowed_decisions = {AUTHOR_KEEP_DECISION, *AUTHOR_DECISION_LABELS}
    decisions: list[AuthorDecision] = []
    seen_keys: set[tuple[str, str, str]] = set()
    seen_items: set[str] = set()
    for row in rows:
        decision = AuthorDecision(**row)
        key = decision.key
        if key in seen_keys:
            raise RuntimeError(f"duplicate author decision for {key}")
        seen_keys.add(key)
        if re.fullmatch(r"#[1-9][0-9]*", decision.worksheet_item) is None:
            raise RuntimeError(f"invalid worksheet item {decision.worksheet_item!r} for {key}")
        if decision.worksheet_item in seen_items:
            raise RuntimeError(f"duplicate worksheet item {decision.worksheet_item}")
        seen_items.add(decision.worksheet_item)
        flag = flags_by_key.get(key)
        if flag is None:
            raise RuntimeError(f"author decision does not match a full-audit flag: {key}")
        expected_basis = FLAG_BASIS_BY_DISAGREEMENT_BASIS.get(flag["disagreement_basis"])
        if decision.flag_basis != expected_basis:
            raise RuntimeError(
                f"author decision flag basis mismatch for {key}: "
                f"{decision.flag_basis!r} != {expected_basis!r}"
            )
        if decision.author_decision not in allowed_decisions:
            raise RuntimeError(f"invalid author decision {decision.author_decision!r} for {key}")
        if not decision.decided_by.strip() or not decision.rationale.strip():
            raise RuntimeError(f"author decision {key} lacks decided_by or rationale")
        if re.fullmatch(r"\d{4}-\d{2}-\d{2}", decision.decided_on) is None:
            raise RuntimeError(f"invalid decided_on {decision.decided_on!r} for {key}")
        date.fromisoformat(decision.decided_on)
        decisions.append(decision)
    return tuple(decisions)


def _author_adjudicated_corrections(
    *,
    observed: Sequence[Correction],
    full_audit: Sequence[Correction],
    decisions: Sequence[AuthorDecision],
) -> tuple[tuple[Correction, ...], tuple[Correction, ...]]:
    """Return the author-adjudicated correction set and its worst-case extension.

    Confirmed author decisions join the observed source-audit errors with the
    author's label.  Kept flags restore ``implemented_gold`` and appear in no
    scenario.  The worst case also relabels every flag without an author decision
    using its proposed sensitivity label.
    """

    full_by_key = {correction.key: correction for correction in full_audit}
    kept = {
        decision.key for decision in decisions if decision.author_decision == AUTHOR_KEEP_DECISION
    }
    kept_observed = sorted(kept & {correction.key for correction in observed})
    if kept_observed:
        raise RuntimeError(
            f"author kept implemented_gold on observed source-audit errors: {kept_observed}"
        )
    relabels: list[Correction] = []
    for decision in decisions:
        if decision.author_decision == AUTHOR_KEEP_DECISION:
            continue
        flag = full_by_key.get(decision.key)
        if flag is None:
            raise RuntimeError(f"author decision does not match a full-audit flag: {decision.key}")
        relabels.append(
            Correction(
                task_id=flag.task_id,
                artifact_id=flag.artifact_id,
                question_id=flag.question_id,
                old_label=flag.old_label,
                new_label=AUTHOR_DECISION_LABELS[decision.author_decision],
                reason=(
                    f"{flag.reason} Author decision {decision.worksheet_item} "
                    f"({decision.decided_by}, {decision.decided_on}): "
                    f"{decision.author_decision}; rationale: {decision.rationale}."
                ),
                provenance=f"{flag.provenance}|author_adjudication",
                confidence="author_confirmed",
            )
        )
    author_adjudicated = _merge_correction_universe(observed=observed, full_audit=relabels)
    adjudicated_by_key = {correction.key: correction for correction in author_adjudicated}
    relabeled_keys = {correction.key for correction in relabels}
    flag_universe = {
        correction.key: correction
        for correction in _merge_correction_universe(observed=observed, full_audit=full_audit)
    }
    worst_case = tuple(
        adjudicated_by_key[key] if key in relabeled_keys else flag_universe[key]
        for key in sorted(flag_universe)
        if key not in kept
    )
    if not set(adjudicated_by_key) <= {correction.key for correction in worst_case}:
        raise RuntimeError("author-adjudicated corrections are not contained in the worst case")
    return author_adjudicated, worst_case


def _author_adjudication_summary(
    *,
    decisions: Sequence[AuthorDecision],
    observed: Sequence[Correction],
    full_audit: Sequence[Correction],
    author_adjudicated: Sequence[Correction],
    worst_case: Sequence[Correction],
    labels: list[dict[str, str]],
) -> dict[str, Any]:
    """Count how author decisions partition the full-audit flags; checks frozen totals."""

    builder_by_key = {impact._artifact_question_key(row): row["builder_id"] for row in labels}
    decision_counts = Counter(decision.author_decision for decision in decisions)
    kept = [decision for decision in decisions if decision.author_decision == AUTHOR_KEEP_DECISION]
    relabeled_keys = {
        decision.key for decision in decisions if decision.author_decision != AUTHOR_KEEP_DECISION
    }
    reviewed_keys = {decision.key for decision in decisions}
    observed_keys = {correction.key for correction in observed}
    unreviewed_keys = {correction.key for correction in full_audit} - reviewed_keys
    if len(decisions) != AUTHOR_DECISION_EXPECTED_ROWS:
        raise RuntimeError(
            f"expected {AUTHOR_DECISION_EXPECTED_ROWS} author decisions, found {len(decisions)}"
        )
    if len(kept) != AUTHOR_DECISION_EXPECTED_KEEPS:
        raise RuntimeError(
            f"expected {AUTHOR_DECISION_EXPECTED_KEEPS} kept flags, found {len(kept)}"
        )
    if len(author_adjudicated) != len(observed_keys | relabeled_keys):
        raise RuntimeError("author-adjudicated set is not observed errors plus author relabels")
    if len(worst_case) != len(observed_keys | relabeled_keys | unreviewed_keys):
        raise RuntimeError("worst case is not author-adjudicated plus unreviewed flags")
    return {
        "decision_rows": len(decisions),
        "decision_counts": dict(sorted(decision_counts.items())),
        "confirmed_relabels": len(relabeled_keys),
        "kept_implemented_gold": len(kept),
        "kept_pairs": [
            {
                "worksheet_item": decision.worksheet_item,
                "task_id": decision.task_id,
                "artifact_id": decision.artifact_id,
                "question_id": decision.question_id,
            }
            for decision in kept
        ],
        "builder_counts": dict(
            sorted(Counter(builder_by_key[decision.key] for decision in decisions).items())
        ),
        "flag_basis_counts": dict(
            sorted(Counter(decision.flag_basis for decision in decisions).items())
        ),
        "decided_by": sorted({decision.decided_by for decision in decisions}),
        "decided_on": sorted({decision.decided_on for decision in decisions}),
        "full_audit_flags": len(full_audit),
        "reviewed_flags": len(reviewed_keys),
        "unreviewed_flags": len(unreviewed_keys),
        "observed_errors": len(observed_keys),
        "observed_in_author_relabels": len(observed_keys & relabeled_keys),
        "observed_in_unreviewed_flags": len(observed_keys & unreviewed_keys),
        "author_adjudicated_pairs": len(author_adjudicated),
        "worst_case_pairs": len(worst_case),
    }


def _load_model_assisted_decisions(
    rows: list[dict[str, str]],
    *,
    disagreement_rows: list[dict[str, str]],
    author_decisions: Sequence[AuthorDecision],
) -> tuple[ModelAssistedDecision, ...]:
    """Validate model-assisted decisions: exactly the flags without an author decision."""

    if not rows:
        raise RuntimeError("model-assisted decisions file is empty")
    if any(tuple(row) != MODEL_DECISION_FIELDS for row in rows):
        raise RuntimeError(
            f"model-assisted decisions CSV columns must be exactly {MODEL_DECISION_FIELDS}"
        )
    flags_by_key = {
        (row["task_id"], row["artifact_id"], row["question_id"]): row for row in disagreement_rows
    }
    author_keys = {decision.key for decision in author_decisions}
    allowed_decisions = {AUTHOR_KEEP_DECISION, *AUTHOR_DECISION_LABELS}
    decisions: list[ModelAssistedDecision] = []
    seen_keys: set[tuple[str, str, str]] = set()
    for row in rows:
        decision = ModelAssistedDecision(**row)
        key = decision.key
        if key in seen_keys:
            raise RuntimeError(f"duplicate model-assisted decision for {key}")
        seen_keys.add(key)
        flag = flags_by_key.get(key)
        if flag is None:
            raise RuntimeError(f"model-assisted decision does not match a full-audit flag: {key}")
        if key in author_keys:
            raise RuntimeError(f"model-assisted decision overlaps an author decision: {key}")
        expected_basis = FLAG_BASIS_BY_DISAGREEMENT_BASIS.get(flag["disagreement_basis"])
        if decision.flag_basis != expected_basis:
            raise RuntimeError(
                f"model-assisted decision flag basis mismatch for {key}: "
                f"{decision.flag_basis!r} != {expected_basis!r}"
            )
        if decision.decision not in allowed_decisions:
            raise RuntimeError(f"invalid model-assisted decision {decision.decision!r} for {key}")
        if decision.actual_option not in MODEL_DECISION_OPTIONS:
            raise RuntimeError(f"invalid actual_option {decision.actual_option!r} for {key}")
        if decision.confidence not in MODEL_DECISION_CONFIDENCES:
            raise RuntimeError(f"invalid confidence {decision.confidence!r} for {key}")
        if decision.pass_agreement not in MODEL_DECISION_PASS_AGREEMENT:
            raise RuntimeError(f"invalid pass_agreement {decision.pass_agreement!r} for {key}")
        for field_name in ("evidence", "rationale", "decided_by", "review_status"):
            if not getattr(decision, field_name).strip():
                raise RuntimeError(f"model-assisted decision {key} lacks {field_name}")
        if decision.review_status not in MODEL_REVIEW_STATUSES:
            raise RuntimeError(f"invalid review_status {decision.review_status!r} for {key}")
        if re.fullmatch(r"\d{4}-\d{2}-\d{2}", decision.decided_on) is None:
            raise RuntimeError(f"invalid decided_on {decision.decided_on!r} for {key}")
        decided_on = date.fromisoformat(decision.decided_on)
        if decision.review_status == MODEL_REVIEW_PENDING:
            if decision.reviewed_on:
                raise RuntimeError(f"pending model-assisted decision {key} has reviewed_on")
        else:
            if re.fullmatch(r"\d{4}-\d{2}-\d{2}", decision.reviewed_on) is None:
                raise RuntimeError(f"invalid reviewed_on {decision.reviewed_on!r} for {key}")
            if date.fromisoformat(decision.reviewed_on) < decided_on:
                raise RuntimeError(f"reviewed_on precedes decided_on for {key}")
        decisions.append(decision)
    uncovered = sorted(set(flags_by_key) - author_keys - seen_keys)
    if uncovered:
        raise RuntimeError(
            "model-assisted decisions must cover every flag without an author decision; "
            f"missing {len(uncovered)}: {uncovered[:5]}"
        )
    return tuple(decisions)


def _fully_adjudicated_corrections(
    *,
    observed: Sequence[Correction],
    author_adjudicated: Sequence[Correction],
    full_audit: Sequence[Correction],
    decisions: Sequence[ModelAssistedDecision],
) -> tuple[Correction, ...]:
    """Return the author-adjudicated set plus the model-assisted relabels.

    Model-kept flags restore ``implemented_gold`` and are left out, so they appear
    only in the worst case.
    """

    full_by_key = {correction.key: correction for correction in full_audit}
    kept_observed = sorted(
        decision.key
        for decision in decisions
        if decision.decision == AUTHOR_KEEP_DECISION
        and decision.key in {correction.key for correction in observed}
    )
    if kept_observed:
        raise RuntimeError(
            f"model-assisted decision kept implemented_gold on observed source-audit "
            f"errors: {kept_observed}"
        )
    relabels: list[Correction] = []
    for decision in decisions:
        if decision.decision == AUTHOR_KEEP_DECISION:
            continue
        flag = full_by_key.get(decision.key)
        if flag is None:
            raise RuntimeError(
                f"model-assisted decision does not match a full-audit flag: {decision.key}"
            )
        relabels.append(
            Correction(
                task_id=flag.task_id,
                artifact_id=flag.artifact_id,
                question_id=flag.question_id,
                old_label=flag.old_label,
                new_label=AUTHOR_DECISION_LABELS[decision.decision],
                reason=(
                    f"{flag.reason} Model-assisted decision ({decision.decided_by}; "
                    f"{decision.pass_agreement}; {decision.decided_on}; "
                    f"{decision.review}): {decision.decision}, option "
                    f"{decision.actual_option}; rationale: {decision.rationale}"
                ),
                provenance=f"{flag.provenance}|model_assisted_adjudication",
                confidence=f"model_assisted_{decision.confidence}",
            )
        )
    return _merge_correction_universe(observed=author_adjudicated, full_audit=relabels)


def _model_assisted_summary(
    *,
    decisions: Sequence[ModelAssistedDecision],
    observed: Sequence[Correction],
    full_audit: Sequence[Correction],
    author_adjudicated: Sequence[Correction],
    fully_adjudicated: Sequence[Correction],
) -> dict[str, Any]:
    """Count how model-assisted decisions partition the flags left by the author."""

    proposed_by_key = {correction.key: correction.new_label for correction in full_audit}
    kept = [decision for decision in decisions if decision.decision == AUTHOR_KEEP_DECISION]
    relabeled = [decision for decision in decisions if decision.decision != AUTHOR_KEEP_DECISION]
    relabeled_keys = {decision.key for decision in relabeled}
    observed_keys = {correction.key for correction in observed}
    author_keys = {correction.key for correction in author_adjudicated}
    fully_keys = {correction.key for correction in fully_adjudicated}
    if fully_keys != author_keys | relabeled_keys:
        raise RuntimeError("fully adjudicated set is not author-adjudicated plus model relabels")
    if fully_keys & {decision.key for decision in kept}:
        raise RuntimeError("model-kept pairs appear in the fully adjudicated scenario")
    label_differs = [
        {
            "task_id": decision.task_id,
            "artifact_id": decision.artifact_id,
            "question_id": decision.question_id,
            "model_assisted_label": AUTHOR_DECISION_LABELS[decision.decision],
            "proposed_sensitivity_label": proposed_by_key[decision.key],
        }
        for decision in relabeled
        if AUTHOR_DECISION_LABELS[decision.decision] != proposed_by_key[decision.key]
    ]
    return {
        "decision_rows": len(decisions),
        "decision_counts": dict(sorted(Counter(d.decision for d in decisions).items())),
        "confidence_counts": dict(sorted(Counter(d.confidence for d in decisions).items())),
        "pass_agreement_counts": dict(sorted(Counter(d.pass_agreement for d in decisions).items())),
        "review_status": sorted({decision.review_status for decision in decisions}),
        "reviewed_on": sorted({decision.reviewed_on for decision in decisions} - {""}),
        "decided_by": sorted({decision.decided_by for decision in decisions}),
        "decided_on": sorted({decision.decided_on for decision in decisions}),
        "model_relabels": len(relabeled_keys),
        "kept_implemented_gold": len(kept),
        "kept_pairs": [
            {
                "task_id": decision.task_id,
                "artifact_id": decision.artifact_id,
                "question_id": decision.question_id,
                "actual_option": decision.actual_option,
                "confidence": decision.confidence,
                "pass_agreement": decision.pass_agreement,
            }
            for decision in kept
        ],
        "observed_in_model_relabels": len(observed_keys & relabeled_keys),
        "labels_differing_from_proposed_sensitivity_label": label_differs,
        "fully_adjudicated_pairs": len(fully_adjudicated),
    }


def _scored_pair_label_sources(labels: list[dict[str, str]]) -> dict[str, int]:
    """Count released label sources over the scored compile-pass implemented_gold pairs."""

    counts = Counter(
        row["label_source"]
        for row in labels
        if row["task_id"] in impact.SCRATCH_TASKS
        and impact._as_bool(row["compile_pass"])
        and row["final_label"] == "implemented_gold"
    )
    return dict(sorted(counts.items()))


def _strict_sample_index(raw: str, *, source: str) -> int:
    if not raw or not raw.isascii() or not raw.isdecimal():
        raise RuntimeError(f"invalid sample_index {raw!r} in {source}")
    parsed = int(raw)
    if raw != str(parsed):
        raise RuntimeError(f"non-canonical sample_index {raw!r} in {source}; expected {parsed}")
    return parsed


def _strict_audit_bool(
    raw: str,
    *,
    field: str,
    sample_index: str,
) -> bool:
    normalized = raw.strip().lower()
    if normalized == "true":
        return True
    if normalized == "false":
        return False
    raise RuntimeError(f"invalid {field} boolean {raw!r} for spot-audit sample {sample_index}")


def _validate_correction_sets(
    scenario_corrections: dict[str, tuple[Correction, ...]],
    *,
    expected_counts: dict[str, int],
) -> None:
    for scenario, corrections in scenario_corrections.items():
        keys = [correction.key for correction in corrections]
        if len(keys) != len(set(keys)):
            raise RuntimeError(f"duplicate correction key in scenario {scenario}")
    actual_counts = {
        scenario: len(corrections) for scenario, corrections in scenario_corrections.items()
    }
    if actual_counts != expected_counts:
        raise RuntimeError(f"unexpected cumulative scenario correction counts: {actual_counts}")
    if next(iter(scenario_corrections)) != "baseline":
        raise RuntimeError("the first sensitivity scenario must be the baseline")
    key_sets = [
        {correction.key for correction in corrections}
        for corrections in scenario_corrections.values()
    ]
    if any(not before <= after for before, after in zip(key_sets, key_sets[1:])):
        raise RuntimeError("correction scenarios are not cumulative")


def _validate_corrections(
    *,
    corrections: Sequence[Correction],
    scenario_corrections: dict[str, tuple[Correction, ...]],
    labels: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
    low_prior_questions: set[tuple[str, str]],
) -> list[dict[str, Any]]:
    by_key: dict[tuple[str, str, str], list[dict[str, str]]] = defaultdict(list)
    for row in labels:
        by_key[impact._artifact_question_key(row)].append(row)

    evidence: list[dict[str, Any]] = []
    # A key carries one label per scenario; it may carry different labels in
    # different scenarios (fully adjudicated vs. worst case), so the universe and
    # the scenario membership are keyed by (key, new_label).
    scenario_keys = {
        scenario: {(correction.key, correction.new_label) for correction in corrections}
        for scenario, corrections in scenario_corrections.items()
    }
    universe_labels = {(correction.key, correction.new_label) for correction in corrections}
    if len(universe_labels) != len(corrections):
        raise RuntimeError("duplicate (key, label) in the correction universe")
    for scenario, scenario_set in scenario_corrections.items():
        for correction in scenario_set:
            if (correction.key, correction.new_label) not in universe_labels:
                raise RuntimeError(
                    f"{scenario} correction for {correction.key} is missing from or "
                    "conflicts with the correction universe"
                )
    for correction in corrections:
        matches = by_key.get(correction.key, [])
        if len(matches) != 1:
            raise RuntimeError(
                f"expected one final-label row for {correction.key}, found {len(matches)}"
            )
        label = matches[0]
        if label["final_label"] != correction.old_label:
            raise RuntimeError(
                f"{correction.key} expected {correction.old_label}, found {label['final_label']}"
            )
        if not impact._as_bool(label["compile_pass"]):
            raise RuntimeError(f"correction key is not compile-pass: {correction.key}")

        rows = [
            row
            for row in answer_rows
            if impact._artifact_question_key(row) == correction.key
            and impact._in_panel(row, impact.PAPER_FINAL_PANEL_ID)
        ]
        find_run_ids = {row["find_run_id"] for row in rows}
        finder_counts = Counter(row["finder_id"] for row in rows)
        correct_rows = sum(impact._as_bool(row["is_correct"]) for row in rows)
        if len(rows) != 36 or len(find_run_ids) != 36:
            raise RuntimeError(
                f"{correction.key} must map to 36 unique primary answer rows; "
                f"found rows={len(rows)}, find_runs={len(find_run_ids)}"
            )
        if len(finder_counts) != 12 or set(finder_counts.values()) != {3}:
            raise RuntimeError(
                f"{correction.key} expected 12 finders x 3 trials; found {finder_counts}"
            )
        evidence.append(
            {
                "task_id": correction.task_id,
                "artifact_id": correction.artifact_id,
                "question_id": correction.question_id,
                "old_label": correction.old_label,
                "new_label": correction.new_label,
                "reason": correction.reason,
                "provenance": correction.provenance,
                "confidence": correction.confidence,
                "included_scenarios": "|".join(
                    scenario
                    for scenario in scenario_corrections
                    if (correction.key, correction.new_label) in scenario_keys[scenario]
                ),
                "compile_pass": label["compile_pass"],
                "builder_id": label["builder_id"],
                "primary_answer_rows": len(rows),
                "primary_correct_answer_rows": correct_rows,
                "primary_find_runs": len(find_run_ids),
                "finder_count": len(finder_counts),
                "trials_per_finder": min(finder_counts.values()),
                "in_low_prior": int(
                    (correction.task_id, correction.question_id) in low_prior_questions
                ),
            }
        )
    return evidence


def _apply_corrections(
    labels: list[dict[str, str]],
    *,
    corrections: Sequence[Correction],
    label_source: str,
) -> list[dict[str, str]]:
    correction_by_key = {correction.key: correction for correction in corrections}
    corrected: list[dict[str, str]] = []
    applied: Counter[tuple[str, str, str]] = Counter()
    for original in labels:
        row = dict(original)
        key = impact._artifact_question_key(row)
        correction = correction_by_key.get(key)
        if correction is not None:
            row["final_label"] = correction.new_label
            row["label_source"] = label_source
            applied[key] += 1
        corrected.append(row)
    if applied != Counter({correction.key: 1 for correction in corrections}):
        raise RuntimeError(f"correction overlay mismatch: {applied}")
    return corrected


def _analyze_scenario(
    *,
    scenario: str,
    labels: list[dict[str, str]],
    find_rows: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
    low_prior_questions: set[tuple[str, str]],
) -> dict[str, Any]:
    label_by_key = {impact._artifact_question_key(row): row for row in labels}
    if len(label_by_key) != len(labels):
        raise RuntimeError("final label table contains duplicate artifact-question keys")

    conditioned_runs = impact._conditioned_run_rows(
        find_rows=find_rows,
        answer_rows=answer_rows,
        label_by_key=label_by_key,
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
    )
    conditioned_answers = impact._conditioned_answer_rows(
        answer_rows=answer_rows,
        label_by_key=label_by_key,
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
    )
    low_prior_answers = impact._conditioned_answer_rows(
        answer_rows=answer_rows,
        label_by_key=label_by_key,
        source_panel_id=impact.PAPER_FINAL_PANEL_ID,
        question_filter=low_prior_questions,
    )

    rb_rankings, perfect_run_summary, perfect_cell_summary = _effort_outputs(
        scenario=scenario,
        conditioned_runs=conditioned_runs,
    )
    return {
        "label_accuracy": _label_accuracy_rows(
            scenario=scenario,
            labels=labels,
            answer_rows=answer_rows,
        ),
        "implementation_coverage": _coverage_rows(scenario=scenario, labels=labels),
        "conditioned_accuracy": _conditioned_accuracy_rows(
            scenario=scenario,
            slices={
                "overall": conditioned_answers,
                "low_prior": low_prior_answers,
            },
        ),
        "conditioned_runs": conditioned_runs,
        "rb_rankings": rb_rankings,
        "perfect_run_summary": perfect_run_summary,
        "perfect_cell_summary": perfect_cell_summary,
    }


def _label_accuracy_rows(
    *,
    scenario: str,
    labels: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    scopes = (
        (
            "artifact_present_all",
            impact.ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
            [row for row in labels if row["task_id"] in impact.SCRATCH_TASKS],
        ),
        (
            "compile_pass_primary",
            impact.PAPER_FINAL_PANEL_ID,
            [
                row
                for row in labels
                if row["task_id"] in impact.SCRATCH_TASKS and impact._as_bool(row["compile_pass"])
            ],
        ),
    )
    for scope_id, panel_id, scope_labels in scopes:
        label_by_key = {
            impact._artifact_question_key(row): row["final_label"] for row in scope_labels
        }
        scoped_answers = [
            row
            for row in answer_rows
            if impact._in_panel(row, panel_id)
            and impact._artifact_question_key(row) in label_by_key
        ]
        for task_id in (*SCRATCH_TASKS, "all"):
            selected_labels = [
                row for row in scope_labels if task_id == "all" or row["task_id"] == task_id
            ]
            selected_answers = [
                row for row in scoped_answers if task_id == "all" or row["task_id"] == task_id
            ]
            for final_label in impact.FINAL_LABELS:
                pair_count = sum(row["final_label"] == final_label for row in selected_labels)
                label_answers = [
                    row
                    for row in selected_answers
                    if label_by_key[impact._artifact_question_key(row)] == final_label
                ]
                correct = sum(impact._as_bool(row["is_correct"]) for row in label_answers)
                rows.append(
                    {
                        "scenario": scenario,
                        "scope_id": scope_id,
                        "task_id": task_id,
                        "final_label": final_label,
                        "artifact_question_pairs": pair_count,
                        "answer_rows": len(label_answers),
                        "correct_answer_rows": correct,
                        "accuracy": _ratio(correct, len(label_answers)),
                    }
                )
    return rows


def _coverage_rows(
    *,
    scenario: str,
    labels: list[dict[str, str]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    scopes = (
        ("artifact_present_all", labels),
        (
            "compile_pass_primary",
            [row for row in labels if impact._as_bool(row["compile_pass"])],
        ),
    )
    for scope_id, scope_rows in scopes:
        for grain, key_fn in (
            ("artifact", lambda row: row["artifact_id"]),
            ("builder", lambda row: row["builder_id"]),
        ):
            grouped: dict[str, list[dict[str, str]]] = defaultdict(list)
            for row in scope_rows:
                if row["task_id"] in impact.SCRATCH_TASKS:
                    grouped[key_fn(row)].append(row)
            for entity_id, group in sorted(grouped.items()):
                counts = Counter(row["final_label"] for row in group)
                artifact_ids = {row["artifact_id"] for row in group}
                task_ids = {row["task_id"] for row in group}
                builder_ids = {row["builder_id"] for row in group}
                rows.append(
                    {
                        "scenario": scenario,
                        "scope_id": scope_id,
                        "grain": grain,
                        "entity_id": entity_id,
                        "task_id": next(iter(task_ids)) if len(task_ids) == 1 else "all",
                        "builder_id": (
                            next(iter(builder_ids)) if len(builder_ids) == 1 else entity_id
                        ),
                        "artifact_count": len(artifact_ids),
                        "artifact_question_pairs": len(group),
                        "implemented_gold": counts["implemented_gold"],
                        "implemented_non_gold": counts["implemented_non_gold"],
                        "absent_or_ambiguous": counts["absent_or_ambiguous"],
                        "implementation_coverage": _ratio(counts["implemented_gold"], len(group)),
                    }
                )
    return rows


def _coverage_change_rows(
    coverage_rows: list[dict[str, Any]],
    *,
    scenarios: Sequence[str],
) -> list[dict[str, Any]]:
    by_scenario: dict[str, dict[tuple[str, str, str], dict[str, Any]]] = defaultdict(dict)
    for row in coverage_rows:
        key = (row["scope_id"], row["grain"], row["entity_id"])
        if key in by_scenario[row["scenario"]]:
            raise RuntimeError(f"duplicate implementation-coverage row for {row['scenario']}:{key}")
        by_scenario[row["scenario"]][key] = row

    baseline = by_scenario["baseline"]
    rows: list[dict[str, Any]] = []
    for scenario in scenarios[1:]:
        current = by_scenario[scenario]
        if set(current) != set(baseline):
            raise RuntimeError(f"implementation-coverage entity set changed in {scenario}")
        for key in sorted(baseline):
            before = baseline[key]
            after = current[key]
            invariant_fields = (
                "task_id",
                "builder_id",
                "artifact_count",
                "artifact_question_pairs",
            )
            if any(before[field] != after[field] for field in invariant_fields):
                raise RuntimeError(
                    f"implementation-coverage denominator drift for {scenario}:{key}"
                )
            gold_delta = int(after["implemented_gold"]) - int(before["implemented_gold"])
            coverage_delta = float(after["implementation_coverage"]) - float(
                before["implementation_coverage"]
            )
            if gold_delta == 0 and math.isclose(
                coverage_delta,
                0.0,
                rel_tol=0.0,
                abs_tol=1e-15,
            ):
                continue
            rows.append(
                {
                    "scenario": scenario,
                    "scope_id": key[0],
                    "grain": key[1],
                    "entity_id": key[2],
                    "task_id": before["task_id"],
                    "builder_id": before["builder_id"],
                    "artifact_count": before["artifact_count"],
                    "artifact_question_pairs": before["artifact_question_pairs"],
                    "baseline_implemented_gold": before["implemented_gold"],
                    "scenario_implemented_gold": after["implemented_gold"],
                    "implemented_gold_delta": gold_delta,
                    "baseline_implementation_coverage": before["implementation_coverage"],
                    "scenario_implementation_coverage": after["implementation_coverage"],
                    "implementation_coverage_delta": coverage_delta,
                }
            )
    return rows


def _relabeled_pair_rows(
    *,
    scenarios: Sequence[str],
    correction_evidence: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Scored pairs each scenario relabels, by new label, with their primary answer rows.

    ``gold_option_rows`` counts the primary answer rows on those pairs whose
    selected option is the specification gold option.
    """
    rows: list[dict[str, Any]] = []
    for scenario in scenarios[1:]:
        selected = [
            row for row in correction_evidence if scenario in row["included_scenarios"].split("|")
        ]
        groups = {"all": selected}
        for label in sorted({row["new_label"] for row in selected}):
            groups[label] = [row for row in selected if row["new_label"] == label]
        for new_label, group in groups.items():
            answer_rows = sum(int(row["primary_answer_rows"]) for row in group)
            gold_rows = sum(int(row["primary_correct_answer_rows"]) for row in group)
            rows.append(
                {
                    "scenario": scenario,
                    "new_label": new_label,
                    "relabeled_pairs": len(group),
                    "primary_answer_rows": answer_rows,
                    "gold_option_rows": gold_rows,
                    "gold_option_share": _ratio(gold_rows, answer_rows),
                }
            )
    return rows


def _question_label_rows(
    *,
    scenario_labels: dict[str, list[dict[str, str]]],
) -> list[dict[str, Any]]:
    """Compile-pass ``implemented_gold`` pairs per question in each scenario.

    ``relabeled_pairs`` is the drop from the baseline count; every correction
    moves a pair away from ``implemented_gold``.
    """

    def gold_by_question(labels: list[dict[str, str]]) -> dict[tuple[str, str], Counter[str]]:
        grouped: dict[tuple[str, str], Counter[str]] = defaultdict(Counter)
        for row in labels:
            if row["task_id"] in impact.SCRATCH_TASKS and impact._as_bool(row["compile_pass"]):
                grouped[(row["task_id"], row["question_id"])][row["final_label"]] += 1
        return grouped

    baseline = gold_by_question(scenario_labels["baseline"])
    rows: list[dict[str, Any]] = []
    for scenario, labels in scenario_labels.items():
        current = gold_by_question(labels)
        if set(current) != set(baseline):
            raise RuntimeError(f"question set changed in {scenario}")
        for task_id, question_id in sorted(baseline):
            before = baseline[(task_id, question_id)]
            after = current[(task_id, question_id)]
            if sum(before.values()) != sum(after.values()):
                raise RuntimeError(f"compile-pass pair count drift for {scenario}:{question_id}")
            relabeled = before["implemented_gold"] - after["implemented_gold"]
            if relabeled < 0:
                raise RuntimeError(f"{scenario} adds implemented_gold pairs to {question_id}")
            rows.append(
                {
                    "scenario": scenario,
                    "task_id": task_id,
                    "question_id": question_id,
                    "compile_pass_artifacts": sum(before.values()),
                    "baseline_implemented_gold": before["implemented_gold"],
                    "scenario_implemented_gold": after["implemented_gold"],
                    "relabeled_pairs": relabeled,
                }
            )
    return rows


def _headline_scenario(scenarios: Sequence[str]) -> str:
    """The most complete adjudication scenario of the run."""
    for scenario in (FULLY_ADJUDICATED_SCENARIO, AUTHOR_ADJUDICATED_SCENARIO, FULL_AUDIT_SCENARIO):
        if scenario in scenarios:
            return scenario
    return scenarios[-1]


def _conditioned_accuracy_rows(
    *,
    scenario: str,
    slices: dict[str, list[dict[str, Any]]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for slice_id, answer_rows in slices.items():
        for task_id in (*SCRATCH_TASKS, "all"):
            selected = [row for row in answer_rows if task_id == "all" or row["task_id"] == task_id]
            correct = sum(int(row["is_correct"]) for row in selected)
            pairs = {(row["build_run_id"], row["question_id"]) for row in selected}
            rows.append(
                {
                    "scenario": scenario,
                    "slice_id": slice_id,
                    "task_id": task_id,
                    "artifact_question_pairs": len(pairs),
                    "answer_rows": len(selected),
                    "correct_answer_rows": correct,
                    "accuracy": _ratio(correct, len(selected)),
                }
            )
    return rows


def _evidence_audit_overlap_rows(
    *,
    sample_rows: list[dict[str, str]],
    scenario_corrections: dict[str, tuple[Correction, ...]],
) -> list[dict[str, Any]]:
    """Count sampled evidence-audit answers on pairs that each scenario relabels.

    The post-hoc evidence-citation audit labels sampled correct answers under the
    released labels.  An answer falls on a relabeled pair when its task, build,
    and question key is corrected in the scenario.
    """

    answers = [(row["find_run_id"], row["question_id"]) for row in sample_rows]
    if len(set(answers)) != len(answers):
        raise RuntimeError("evidence-audit sample has duplicate find-run answers")
    for row in sample_rows:
        if row["audit_label"] not in EVIDENCE_AUDIT_LABELS:
            raise RuntimeError(f"unknown evidence-audit label: {row['audit_label']!r}")
        if row["prior_status"] not in EVIDENCE_AUDIT_SLICES[1:]:
            raise RuntimeError(f"unknown evidence-audit prior status: {row['prior_status']!r}")
    rows: list[dict[str, Any]] = []
    for scenario, corrections in scenario_corrections.items():
        corrected = {correction.key for correction in corrections}
        for slice_id in EVIDENCE_AUDIT_SLICES:
            for audit_label in EVIDENCE_AUDIT_LABELS:
                selected = [
                    row
                    for row in sample_rows
                    if row["audit_label"] == audit_label
                    and (slice_id == "all" or row["prior_status"] == slice_id)
                ]
                rows.append(
                    {
                        "scenario": scenario,
                        "slice_id": slice_id,
                        "audit_label": audit_label,
                        "sampled_answers": len(selected),
                        "on_relabeled_pairs": sum(
                            (row["task_id"], row["build_run_id"], row["question_id"]) in corrected
                            for row in selected
                        ),
                    }
                )
    return rows


def _effort_outputs(
    *,
    scenario: str,
    conditioned_runs: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    rankings: list[dict[str, Any]] = []
    run_summary: list[dict[str, Any]] = []
    cell_summary: list[dict[str, Any]] = []
    for panel in impact._panels():
        panel_rows = impact._panel_run_rows(conditioned_runs, panel)
        means = impact._cell_means(
            panel_rows,
            metric="total_bytes",
            gate="perfect_run",
        )
        score_rows = impact._builder_score_rows(
            panel,
            "perfect_run",
            "total_bytes",
            means,
        )
        for score in score_rows:
            perfect_runs = sum(
                int(cell["n_perfect_runs"])
                for cell in means.values()
                if cell["builder_id"] == score["builder_id"]
            )
            rankings.append(
                {
                    "scenario": scenario,
                    "panel_id": panel.panel_id,
                    "builder_id": score["builder_id"],
                    "rank": score["rank"],
                    "score": score["score"],
                    "n_cells": score["n_cells"],
                    "max_cells": score["max_cells"],
                    "cell_coverage": score["cell_coverage"],
                    "perfect_runs_contributing": perfect_runs,
                    "mean_cell_accuracy": score["mean_cell_accuracy"],
                    "mean_question_count": score["mean_question_count"],
                }
            )

        cells = _cell_counts(panel_rows)
        perfect_cells = sum(row["perfect_runs"] > 0 for row in cells.values())
        run_summary.append(
            {
                "scenario": scenario,
                "panel_id": panel.panel_id,
                "find_runs": len(panel_rows),
                "perfect_runs": sum(int(row["perfect_run"]) for row in panel_rows),
                "perfect_run_rate": _ratio(
                    sum(int(row["perfect_run"]) for row in panel_rows),
                    len(panel_rows),
                ),
                "observed_cells": len(cells),
                "contributing_perfect_cells": perfect_cells,
                "perfect_cell_coverage": _ratio(perfect_cells, len(cells)),
            }
        )
        for (task_id, builder_id, finder_id), cell in sorted(cells.items()):
            cell_summary.append(
                {
                    "scenario": scenario,
                    "panel_id": panel.panel_id,
                    "task_id": task_id,
                    "builder_id": builder_id,
                    "finder_id": finder_id,
                    "run_count": cell["run_count"],
                    "perfect_runs": cell["perfect_runs"],
                    "contributes_to_perfect_gate": int(cell["perfect_runs"] > 0),
                }
            )
    return rankings, run_summary, cell_summary


def _cell_counts(
    panel_rows: list[dict[str, Any]],
) -> dict[tuple[str, str, str], dict[str, int]]:
    out: dict[tuple[str, str, str], dict[str, int]] = defaultdict(
        lambda: {"run_count": 0, "perfect_runs": 0}
    )
    for row in panel_rows:
        key = (row["task_id"], row["builder_id"], row["finder_id"])
        out[key]["run_count"] += 1
        out[key]["perfect_runs"] += int(row["perfect_run"])
    return dict(out)


def _validate_baseline_rb(
    *,
    recomputed: list[dict[str, Any]],
    published_path: Path,
) -> None:
    published = [
        row
        for row in impact._read_csv(published_path)
        if row["panel_id"] in {"high_effort", "low_effort"}
        and row["gate"] == "perfect_run"
        and row["metric"] == "total_bytes"
    ]
    published_by_key = {(row["panel_id"], row["builder_id"]): row for row in published}
    recomputed_by_key = {(row["panel_id"], row["builder_id"]): row for row in recomputed}
    if set(published_by_key) != set(recomputed_by_key):
        raise RuntimeError("recomputed baseline R_b builder set differs from published output")
    for key, expected in published_by_key.items():
        actual = recomputed_by_key[key]
        if not math.isclose(
            float(actual["score"]),
            float(expected["score"]),
            rel_tol=0.0,
            abs_tol=1e-12,
        ):
            raise RuntimeError(
                f"baseline R_b score drift for {key}: {actual['score']} != {expected['score']}"
            )
        for field in ("rank", "n_cells", "max_cells"):
            if int(actual[field]) != int(expected[field]):
                raise RuntimeError(
                    f"baseline R_b {field} drift for {key}: {actual[field]} != {expected[field]}"
                )


def _validate_scenario_deltas(
    *,
    scenario_results: dict[str, dict[str, Any]],
    scenario_corrections: dict[str, tuple[Correction, ...]],
    correction_evidence: list[dict[str, Any]],
) -> None:
    evidence_by_key = {
        ((row["task_id"], row["artifact_id"], row["question_id"]), row["new_label"]): row
        for row in correction_evidence
    }
    if len(evidence_by_key) != len(correction_evidence):
        raise RuntimeError("duplicate correction evidence key")

    baseline = scenario_results["baseline"]
    for scenario in tuple(scenario_corrections)[1:]:
        corrections = scenario_corrections[scenario]
        missing_evidence = [
            correction.key
            for correction in corrections
            if (correction.key, correction.new_label) not in evidence_by_key
        ]
        if missing_evidence:
            raise RuntimeError(
                f"{scenario} corrections lack validation evidence: {missing_evidence}"
            )
        for slice_id in ("overall", "low_prior"):
            before = _one(
                baseline["conditioned_accuracy"],
                slice_id=slice_id,
                task_id="all",
            )
            after = _one(
                scenario_results[scenario]["conditioned_accuracy"],
                slice_id=slice_id,
                task_id="all",
            )
            included = [
                evidence_by_key[(correction.key, correction.new_label)]
                for correction in corrections
                if slice_id == "overall"
                or int(evidence_by_key[(correction.key, correction.new_label)]["in_low_prior"])
            ]
            expected_pair_delta = -len(included)
            expected_answer_delta = -sum(int(row["primary_answer_rows"]) for row in included)
            expected_correct_delta = -sum(
                int(row["primary_correct_answer_rows"]) for row in included
            )
            observed = (
                int(after["artifact_question_pairs"]) - int(before["artifact_question_pairs"]),
                int(after["answer_rows"]) - int(before["answer_rows"]),
                int(after["correct_answer_rows"]) - int(before["correct_answer_rows"]),
            )
            expected = (
                expected_pair_delta,
                expected_answer_delta,
                expected_correct_delta,
            )
            if observed != expected:
                raise RuntimeError(
                    f"{scenario}:{slice_id} delta mismatch: "
                    f"observed={observed}, expected={expected}"
                )


def _rb_change_rows(
    *,
    scenario_results: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    baseline = scenario_results["baseline"]["rb_rankings"]
    baseline_by_key = {(row["panel_id"], row["builder_id"]): row for row in baseline}
    rows: list[dict[str, Any]] = []
    for scenario in tuple(scenario_results)[1:]:
        current_by_key = {
            (row["panel_id"], row["builder_id"]): row
            for row in scenario_results[scenario]["rb_rankings"]
        }
        if set(baseline_by_key) != set(current_by_key):
            raise RuntimeError(f"R_b builder set changed in {scenario}")
        for key in sorted(baseline_by_key):
            before = baseline_by_key[key]
            after = current_by_key[key]
            rows.append(
                {
                    "scenario": scenario,
                    "panel_id": key[0],
                    "builder_id": key[1],
                    "baseline_rank": before["rank"],
                    "scenario_rank": after["rank"],
                    "rank_delta": int(after["rank"]) - int(before["rank"]),
                    "baseline_score": before["score"],
                    "scenario_score": after["score"],
                    "score_delta": float(after["score"]) - float(before["score"]),
                    "baseline_n_cells": before["n_cells"],
                    "scenario_n_cells": after["n_cells"],
                    "baseline_cell_coverage": before["cell_coverage"],
                    "scenario_cell_coverage": after["cell_coverage"],
                    "ranking_changed": int(before["rank"] != after["rank"]),
                }
            )
    return rows


def _run_metric_change_rows(
    *,
    scenario_results: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    baseline_runs = scenario_results["baseline"]["conditioned_runs"]
    baseline_by_id = {row["find_run_id"]: row for row in baseline_runs}
    rows: list[dict[str, Any]] = []
    for scenario in tuple(scenario_results)[1:]:
        current_by_id = {
            row["find_run_id"]: row for row in scenario_results[scenario]["conditioned_runs"]
        }
        if set(baseline_by_id) != set(current_by_id):
            raise RuntimeError(f"conditioned find-run set changed in {scenario}")
        for find_run_id in sorted(baseline_by_id):
            before = baseline_by_id[find_run_id]
            after = current_by_id[find_run_id]
            if (
                before["question_count"] == after["question_count"]
                and math.isclose(
                    float(before["accuracy"]),
                    float(after["accuracy"]),
                    rel_tol=0.0,
                    abs_tol=1e-15,
                )
                and before["perfect_run"] == after["perfect_run"]
            ):
                continue
            rows.append(
                {
                    "scenario": scenario,
                    "find_run_id": find_run_id,
                    "panel_id": _primary_panel_for_run(before),
                    "task_id": before["task_id"],
                    "build_run_id": before["build_run_id"],
                    "builder_id": before["builder_id"],
                    "finder_id": before["finder_id"],
                    "baseline_question_count": before["question_count"],
                    "scenario_question_count": after["question_count"],
                    "baseline_accuracy": before["accuracy"],
                    "scenario_accuracy": after["accuracy"],
                    "baseline_perfect_run": before["perfect_run"],
                    "scenario_perfect_run": after["perfect_run"],
                    "perfect_status_changed": int(before["perfect_run"] != after["perfect_run"]),
                }
            )
    return rows


def _perfect_cell_change_rows(
    *,
    scenario_results: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    baseline_runs = scenario_results["baseline"]["conditioned_runs"]
    rows: list[dict[str, Any]] = []
    for scenario in tuple(scenario_results)[1:]:
        current_runs = scenario_results[scenario]["conditioned_runs"]
        for panel in impact._panels():
            before = _cell_counts(impact._panel_run_rows(baseline_runs, panel))
            after = _cell_counts(impact._panel_run_rows(current_runs, panel))
            if set(before) != set(after):
                raise RuntimeError(f"observed cell set changed in {scenario}:{panel.panel_id}")
            for key in sorted(before):
                baseline_cell = before[key]
                scenario_cell = after[key]
                if baseline_cell == scenario_cell:
                    continue
                baseline_contributes = int(baseline_cell["perfect_runs"] > 0)
                scenario_contributes = int(scenario_cell["perfect_runs"] > 0)
                rows.append(
                    {
                        "scenario": scenario,
                        "panel_id": panel.panel_id,
                        "task_id": key[0],
                        "builder_id": key[1],
                        "finder_id": key[2],
                        "baseline_run_count": baseline_cell["run_count"],
                        "scenario_run_count": scenario_cell["run_count"],
                        "baseline_perfect_runs": baseline_cell["perfect_runs"],
                        "scenario_perfect_runs": scenario_cell["perfect_runs"],
                        "perfect_run_delta": (
                            scenario_cell["perfect_runs"] - baseline_cell["perfect_runs"]
                        ),
                        "baseline_contributes": baseline_contributes,
                        "scenario_contributes": scenario_contributes,
                        "contribution_changed": int(baseline_contributes != scenario_contributes),
                    }
                )
    return rows


def _primary_panel_for_run(row: dict[str, Any]) -> str:
    if row["builder_effort"] == row["finder_effort"] == "high":
        return "high_effort"
    if row["builder_effort"] == row["finder_effort"] == "low":
        return "low_effort"
    return "cross_effort_diagnostic"


def _scenario_text(
    *,
    author_summary: dict[str, Any] | None,
    author_decisions_path: Path | None,
    label_corrections_path: Path | None,
    scored_pairs: int,
    model_summary: dict[str, Any] | None = None,
    model_decisions_path: Path | None = None,
) -> tuple[dict[str, str], list[str], list[str]]:
    """Return scenario definitions, explanatory notes, and scope lines for the report."""

    baseline = "released consensus labels"
    if label_corrections_path is not None:
        baseline += f" with `{_display_path(label_corrections_path)}` applied"
    if author_summary is None or author_decisions_path is None:
        definitions = {
            "baseline": baseline,
            "pilot_two_corrected": "2 pilot source-audit corrections",
            "high_confidence_corrected": "pilot 2 + 6 high-confidence spot-audit disagreements",
            OBSERVED_SCENARIO: "pilot 2 + all 7 spot-audit disagreements (includes 1 medium)",
            FULL_AUDIT_SCENARIO: (
                "every pair flagged by either full-coverage audit pass, with its proposed "
                "label (no author decisions applied)"
            ),
        }
        notes = [
            "The high-confidence scenario excludes the medium-confidence MiniDB",
            "`q_db_02` disagreement; the all-observed scenario includes it.",
            "The full-audit scenario relabels every flag as a sensitivity-only overlay;",
            "it applies no author decisions and does not mutate the released labels.",
        ]
        scope = [
            "This sensitivity establishes the effect of the two pilot corrections and",
            "the seven disagreements observed in the 24-pair stratified spot audit.",
            "It does not estimate the prevalence of disagreements outside that sample.",
        ]
        return definitions, notes, scope

    summary = author_summary
    decision_counts = summary["decision_counts"]
    builders = ", ".join(
        f"`{builder}` ({count})" for builder, count in summary["builder_counts"].items()
    )
    kept_items = ", ".join(pair["worksheet_item"] for pair in summary["kept_pairs"])
    definitions = {
        "baseline": baseline,
        OBSERVED_SCENARIO: (
            f"{summary['observed_errors']} observed source-audit errors: "
            f"{len(PILOT_CORRECTIONS)} pilot corrections + "
            f"{SPOT_AUDIT_EXPECTED_DISAGREEMENTS} spot-audit disagreements"
        ),
        AUTHOR_ADJUDICATED_SCENARIO: (
            f"observed errors + {summary['confirmed_relabels']} author-confirmed flags "
            f"({summary['observed_in_author_relabels']} already observed) = "
            f"{summary['author_adjudicated_pairs']} distinct pairs; the "
            f"{summary['kept_implemented_gold']} author-kept flags stay implemented_gold"
        ),
        AUTHOR_WORST_CASE_SCENARIO: (
            f"worst case: author-adjudicated pairs + all {summary['unreviewed_flags']} "
            "flags without an author decision, each relabeled with its proposed label "
            f"({summary['observed_in_unreviewed_flags']} already observed) = "
            f"{summary['worst_case_pairs']} distinct pairs"
        ),
    }
    notes = [
        f"The full-coverage audit flagged {summary['full_audit_flags']} of the "
        f"{scored_pairs} scored compile-pass pairs.",
        f"`{_display_path(author_decisions_path)}` records author decisions on the "
        f"{summary['reviewed_flags']} flags for {builders}:",
        f"{summary['confirmed_relabels']} confirmed "
        f"({decision_counts.get('absent_or_ambiguous', 0)} absent_or_ambiguous, "
        f"{decision_counts.get('non_gold', 0)} implemented_non_gold) and "
        f"{summary['kept_implemented_gold']} kept as implemented_gold ({kept_items}).",
        f"The other {summary['unreviewed_flags']} flags have no author decision;",
        "the worst-case scenario relabels all of them as an extreme case, not as an",
        "adjudication. No scenario modifies the released labels.",
    ]
    scope = [
        "This sensitivity establishes the effect of the observed source-audit errors",
        "and the author-confirmed flags, and of additionally relabeling every flag",
        "without an author decision. Author review covered only the flags on the",
        "builders listed above, so this analysis does not estimate how many",
        "unreviewed flags are label errors.",
    ]
    if model_summary is None or model_decisions_path is None:
        return definitions, notes, scope

    model = model_summary
    model_counts = model["decision_counts"]
    review = _model_review_text(model)
    definitions[FULLY_ADJUDICATED_SCENARIO] = (
        f"author-adjudicated pairs + {model['model_relabels']} model-assisted relabels "
        f"of the flags without an author decision ({model['observed_in_model_relabels']} "
        f"already observed) = {model['fully_adjudicated_pairs']} distinct pairs; the "
        f"{model['kept_implemented_gold']} model-kept flags stay implemented_gold. "
        f"The model-assisted decisions {review}"
    )
    differing = model["labels_differing_from_proposed_sensitivity_label"]
    reviewed_on = f", reviewed on {', '.join(model['reviewed_on'])}" if model["reviewed_on"] else ""
    notes = [
        *notes,
        "",
        f"`{_display_path(model_decisions_path)}` records model-assisted decisions on those "
        f"{model['decision_rows']} flags",
        f"({', '.join(model['decided_by'])}; review status: "
        f"{', '.join(model['review_status'])}{reviewed_on}): "
        f"{model_counts.get('absent_or_ambiguous', 0)} absent_or_ambiguous, "
        f"{model_counts.get('non_gold', 0)} implemented_non_gold, "
        f"{model['kept_implemented_gold']} kept as implemented_gold.",
        "The fully_adjudicated scenario applies them on top of the author-adjudicated set.",
        f"{len(differing)} model-assisted labels differ from the proposed sensitivity label",
        "that the worst case uses for the same pair; both labels exclude the pair from",
        "the implemented_gold set.",
    ]
    scope = [
        "This sensitivity establishes the effect of the observed source-audit errors",
        "and the author-confirmed flags, of the model-assisted decisions on the other",
        "flags, and of additionally relabeling every flag without an author decision.",
        "Author decisions cover only the flags on the builders listed above.",
        "The model-assisted decisions used by the fully_adjudicated scenario",
        f"{review};",
        "`decided_by` records them as model-assisted adjudication.",
    ]
    return definitions, notes, scope


def _relabeled_pair_lines(
    scenarios: Sequence[str],
    relabeled_rows: list[dict[str, Any]],
    question_rows: list[dict[str, Any]],
) -> list[str]:
    new_labels = ("absent_or_ambiguous", "implemented_non_gold")
    lines = [
        "",
        "## Relabeled scored pairs",
        "",
        "Scored compile-pass pairs that each scenario relabels away from",
        "`implemented_gold`, with the primary answer rows on those pairs and the rows",
        "whose selected option is the specification gold option.",
        "",
        "| scenario | relabeled pairs | absent_or_ambiguous | implemented_non_gold | "
        "answer rows | gold option chosen |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for scenario in scenarios[1:]:
        by_label = {row["new_label"]: row for row in relabeled_rows if row["scenario"] == scenario}
        total = by_label["all"]
        label_counts = " | ".join(
            str(by_label[label]["relabeled_pairs"] if label in by_label else 0)
            for label in new_labels
        )
        lines.append(
            f"| {scenario} | {total['relabeled_pairs']} | {label_counts} | "
            f"{total['primary_answer_rows']} | {total['gold_option_rows']} "
            f"({_pct(total['gold_option_share'])}) |"
        )

    headline = _headline_scenario(scenarios)
    selected = sorted(
        (row for row in question_rows if row["scenario"] == headline and row["relabeled_pairs"]),
        key=lambda row: (-row["relabeled_pairs"], row["task_id"], row["question_id"]),
    )
    lines.extend(
        [
            "",
            f"Questions with relabeled pairs under `{headline}`, most relabeled first.",
            "Cumulative counts the relabeled pairs of a row and every row above it;",
            "`question_labels.csv` has every question in every scenario.",
            "",
            "| task | question | compile-pass artifacts | implemented_gold "
            "(baseline → scenario) | relabeled | cumulative |",
            "| --- | --- | ---: | ---: | ---: | ---: |",
        ]
    )
    cumulative = 0
    for row in selected:
        cumulative += row["relabeled_pairs"]
        lines.append(
            f"| {row['task_id']} | {row['question_id']} | {row['compile_pass_artifacts']} | "
            f"{row['baseline_implemented_gold']} → {row['scenario_implemented_gold']} | "
            f"{row['relabeled_pairs']} | {cumulative} |"
        )
    return lines


def _model_review_text(model_summary: dict[str, Any]) -> str:
    """Describe the review state of the model-assisted decisions as a predicate."""

    statuses = model_summary["review_status"]
    if statuses == [MODEL_REVIEW_APPROVED]:
        reviewed_on = ", ".join(model_summary["reviewed_on"])
        return f"were reviewed and approved by the author on {reviewed_on}"
    if statuses == [MODEL_REVIEW_PENDING]:
        return "are pending author review"
    return f"have mixed review status ({', '.join(statuses)})"


def _render_report(
    *,
    scenarios: Sequence[str],
    analysis_command: str,
    scored_label_sources: dict[str, int],
    author_decisions: Sequence[AuthorDecision],
    author_summary: dict[str, Any] | None,
    author_decisions_path: Path | None,
    label_corrections_path: Path | None,
    correction_evidence: list[dict[str, Any]],
    label_rows: list[dict[str, Any]],
    model_decisions: Sequence[ModelAssistedDecision] = (),
    model_summary: dict[str, Any] | None = None,
    model_decisions_path: Path | None = None,
    coverage_rows: list[dict[str, Any]],
    coverage_change_rows: list[dict[str, Any]],
    conditioned_accuracy_rows: list[dict[str, Any]],
    rb_rows: list[dict[str, Any]],
    rb_change_rows: list[dict[str, Any]],
    perfect_run_summary_rows: list[dict[str, Any]],
    run_metric_changes: list[dict[str, Any]],
    perfect_cell_changes: list[dict[str, Any]],
    evidence_overlap_rows: list[dict[str, Any]],
    evidence_sample_path: Path,
    relabeled_rows: list[dict[str, Any]],
    question_rows: list[dict[str, Any]],
) -> str:
    summary_labels = [
        row
        for row in label_rows
        if row["task_id"] == "all"
        and row["final_label"]
        in {"implemented_gold", "implemented_non_gold", "absent_or_ambiguous"}
    ]
    artifact_present_coverage_changes = [
        row for row in coverage_change_rows if row["scope_id"] == "artifact_present_all"
    ]
    builder_coverage = [
        row
        for row in coverage_rows
        if row["scope_id"] == "artifact_present_all" and row["grain"] == "builder"
    ]
    overall_accuracy = [row for row in conditioned_accuracy_rows if row["task_id"] == "all"]
    top_two = [row for row in rb_rows if int(row["rank"]) <= 2]
    baseline_accuracy_by_slice = {
        row["slice_id"]: float(row["accuracy"])
        for row in overall_accuracy
        if row["scenario"] == "baseline"
    }
    baseline_perfect_by_panel = {
        row["panel_id"]: row for row in perfect_run_summary_rows if row["scenario"] == "baseline"
    }
    scored_pairs = sum(scored_label_sources.values())
    scenario_descriptions, scenario_notes, scope_notes = _scenario_text(
        author_summary=author_summary,
        author_decisions_path=author_decisions_path,
        label_corrections_path=label_corrections_path,
        scored_pairs=scored_pairs,
        model_summary=model_summary,
        model_decisions_path=model_decisions_path,
    )
    scenario_correction_counts = {
        scenario: sum(
            scenario in row["included_scenarios"].split("|") for row in correction_evidence
        )
        for scenario in scenarios
    }

    lines = [
        "# Corrected-label sensitivity",
        "",
        "This deterministic, post-hoc rebuttal sensitivity overlays cumulative",
        "source-audited corrections in memory and does not modify `runs_release/`.",
        "",
        "## Scenarios",
        "",
        "| scenario | corrections | definition |",
        "| --- | ---: | --- |",
    ]
    for scenario in scenarios:
        lines.append(
            f"| {scenario} | {scenario_correction_counts[scenario]} | "
            f"{scenario_descriptions[scenario]} |"
        )
    lines.extend(["", *scenario_notes])

    if author_decisions:
        lines.extend(
            [
                "",
                "## Author decisions",
                "",
                "| item | task | artifact | question | flag basis | decision | decided | "
                "rationale |",
                "| --- | --- | --- | --- | --- | --- | --- | --- |",
            ]
        )
        for decision in author_decisions:
            lines.append(
                f"| {decision.worksheet_item} | {decision.task_id} | "
                f"`{decision.artifact_id}` | {decision.question_id} | "
                f"{decision.flag_basis} | {decision.author_decision} | "
                f"{decision.decided_by}, {decision.decided_on} | {decision.rationale} |"
            )

    if model_decisions and model_summary is not None and model_decisions_path is not None:
        lines.extend(
            [
                "",
                "## Model-assisted decisions",
                "",
                f"Per-pair decisions, evidence, and rationales are in "
                f"`{_display_path(model_decisions_path)}`; relabeled pairs also appear in",
                "`corrections.csv`. Review status: "
                + ", ".join(f"`{status}`" for status in model_summary["review_status"])
                + (
                    f" (reviewed on {', '.join(model_summary['reviewed_on'])})"
                    if model_summary["reviewed_on"]
                    else ""
                )
                + ".",
                "",
                "| decision | confidence | pass agreement | pairs |",
                "| --- | --- | --- | ---: |",
            ]
        )
        grouped = Counter(
            (decision.decision, decision.confidence, decision.pass_agreement)
            for decision in model_decisions
        )
        confidence_order = {value: index for index, value in enumerate(MODEL_DECISION_CONFIDENCES)}
        for (decision_value, confidence, agreement), count in sorted(
            grouped.items(),
            key=lambda item: (item[0][0], confidence_order[item[0][1]], item[0][2]),
        ):
            lines.append(f"| {decision_value} | {confidence} | {agreement} | {count} |")
        lines.extend(
            [
                "",
                "Model-kept pairs (stay implemented_gold in every scenario except the worst case):",
                "",
                "| task | artifact | question | option | confidence | pass agreement |",
                "| --- | --- | --- | --- | --- | --- |",
            ]
        )
        for pair in model_summary["kept_pairs"]:
            lines.append(
                f"| {pair['task_id']} | `{pair['artifact_id']}` | {pair['question_id']} | "
                f"{pair['actual_option']} | {pair['confidence']} | {pair['pass_agreement']} |"
            )

    lines.extend(
        [
            "",
            "## Correction validation",
            "",
            "| provenance | confidence | task | artifact | question | old → new | "
            "primary correct / rows | low-prior |",
            "| --- | --- | --- | --- | --- | --- | ---: | ---: |",
        ]
    )
    for row in correction_evidence:
        lines.append(
            f"| {row['provenance']} | {row['confidence']} | {row['task_id']} | "
            f"`{row['artifact_id']}` | {row['question_id']} | "
            f"{row['old_label']} → {row['new_label']} | "
            f"{row['primary_correct_answer_rows']} / "
            f"{row['primary_answer_rows']} | {row['in_low_prior']} |"
        )

    lines.extend(
        [
            "",
            "Every key resolves uniquely to one released `implemented_gold` label and",
            "exactly 36 primary rows (12 finders × 3 trials). The 24 legacy spot-audit",
            "rows match their sample manifest exactly; full-audit flags have unique",
            "keys, pass provenance, rationales, and validated sensitivity labels.",
            *(
                [
                    "Each author decision resolves to exactly one full-audit flag and",
                    "repeats that flag's basis; author-kept pairs appear in no scenario.",
                ]
                if author_decisions
                else []
            ),
            *(
                [
                    "Each model-assisted decision resolves to exactly one flag without an",
                    "author decision and repeats its basis; with the author decisions they",
                    "cover every flag. A pair whose fully-adjudicated label differs from its",
                    "worst-case label has one validation row per label.",
                ]
                if model_decisions
                else []
            ),
            "",
            "## Label and answer counts",
            "",
            "`artifact_present_all` covers all 720 released artifact-question pairs;",
            "`compile_pass_primary` is the 615-pair primary analysis subset before",
            f"correction. Its {scored_pairs} released `implemented_gold` pairs are the scored",
            "pairs; their released label sources are:",
            "",
            "| label source | scored pairs |",
            "| --- | ---: |",
            *(f"| {source} | {count} |" for source, count in scored_label_sources.items()),
            "",
            "| scenario | scope | label | pairs | answer rows | correct | answer accuracy |",
            "| --- | --- | --- | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in summary_labels:
        lines.append(
            f"| {row['scenario']} | {row['scope_id']} | {row['final_label']} | "
            f"{row['artifact_question_pairs']} | {row['answer_rows']} | "
            f"{row['correct_answer_rows']} | {_pct(row['accuracy'], digits=3)} |"
        )

    lines.extend(_relabeled_pair_lines(scenarios, relabeled_rows, question_rows))

    lines.extend(
        [
            "",
            "## Implementation coverage",
            "",
            "Only entities whose artifact-present coverage differs from baseline are",
            "shown below; the full artifact and builder tables for both scopes are in",
            "`implementation_coverage.csv`.",
            "",
            "| scenario | grain | entity | gold (baseline → scenario) | "
            "coverage (baseline → scenario) | Δ (pp) |",
            "| --- | --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in artifact_present_coverage_changes:
        delta_pp = 100.0 * float(row["implementation_coverage_delta"])
        lines.append(
            f"| {row['scenario']} | {row['grain']} | `{row['entity_id']}` | "
            f"{row['baseline_implemented_gold']} → "
            f"{row['scenario_implemented_gold']} | "
            f"{_pct(row['baseline_implementation_coverage'])} → "
            f"{_pct(row['scenario_implementation_coverage'])} | "
            f"{delta_pp:+.3f} |"
        )
    lines.extend(
        [
            "",
            "| scenario | artifact-present per-builder coverage range |",
            "| --- | ---: |",
        ]
    )
    for scenario in scenarios:
        values = [
            float(row["implementation_coverage"])
            for row in builder_coverage
            if row["scenario"] == scenario
        ]
        lines.append(f"| {scenario} | {_pct(min(values))}–{_pct(max(values))} |")

    lines.extend(
        [
            "",
            "## Conditioned recovery",
            "",
            "| scenario | slice | pairs | answer rows | correct | accuracy | Δ accuracy (pp) |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in overall_accuracy:
        delta_pp = 100.0 * (float(row["accuracy"]) - baseline_accuracy_by_slice[row["slice_id"]])
        delta = "—" if row["scenario"] == "baseline" else f"{delta_pp:+.5f}"
        lines.append(
            f"| {row['scenario']} | {row['slice_id']} | "
            f"{row['artifact_question_pairs']} | {row['answer_rows']} | "
            f"{row['correct_answer_rows']} | {_pct(row['accuracy'], digits=3)} | "
            f"{delta} |"
        )

    lines.extend(
        [
            "",
            "The numerator deltas are recomputed from the released answer rows for",
            "each corrected key; they are not assumed to be all correct.",
            "",
            "## Evidence-audit overlap",
            "",
            "Sampled answers of the post-hoc evidence-citation audit",
            f"(`{_display_path(evidence_sample_path)}`) that fall on pairs the",
            "scenario relabels. The audit labels were assigned under the released labels.",
            "",
            "| scenario | slice | Supported answers on relabeled pairs | "
            "sampled answers on relabeled pairs |",
            "| --- | --- | ---: | ---: |",
        ]
    )
    for scenario in scenarios:
        for slice_id in EVIDENCE_AUDIT_SLICES:
            selected = [
                row
                for row in evidence_overlap_rows
                if row["scenario"] == scenario and row["slice_id"] == slice_id
            ]
            supported = _one(selected, audit_label="Supported")
            lines.append(
                f"| {scenario} | {slice_id} | "
                f"{supported['on_relabeled_pairs']}/{supported['sampled_answers']} | "
                f"{sum(row['on_relabeled_pairs'] for row in selected)}/"
                f"{sum(row['sampled_answers'] for row in selected)} |"
            )
    lines.extend(
        [
            "",
            "## Primary perfect-run total-byte R_b",
            "",
            "| scenario | panel | rank | builder | R_b | cells | coverage |",
            "| --- | --- | ---: | --- | ---: | ---: | ---: |",
        ]
    )
    for row in top_two:
        lines.append(
            f"| {row['scenario']} | {row['panel_id']} | {row['rank']} | "
            f"{row['builder_id']} | {float(row['score']):.6f} | "
            f"{row['n_cells']}/{row['max_cells']} | "
            f"{_pct(row['cell_coverage'])} |"
        )
    lines.extend(
        [
            "",
            "| scenario | panel | builder order (rank 1 → last) | order vs. baseline |",
            "| --- | --- | --- | --- |",
        ]
    )
    builder_orders = _builder_orders(rb_rows)
    for scenario in scenarios:
        for panel_id in sorted({panel for _, panel in builder_orders}):
            order = builder_orders[(scenario, panel_id)]
            same = order == builder_orders[("baseline", panel_id)]
            lines.append(
                f"| {scenario} | {panel_id} | {' > '.join(order)} | "
                f"{'—' if scenario == 'baseline' else 'unchanged' if same else 'changed'} |"
            )
    lines.extend(
        [
            "",
            "| scenario | builder ranks changed | R_b scores changed |",
            "| --- | ---: | ---: |",
        ]
    )
    for scenario in scenarios[1:]:
        scenario_changes = [row for row in rb_change_rows if row["scenario"] == scenario]
        changed_ranks = sum(int(row["ranking_changed"]) for row in scenario_changes)
        changed_scores = sum(
            not math.isclose(
                float(row["baseline_score"]),
                float(row["scenario_score"]),
                rel_tol=0.0,
                abs_tol=1e-15,
            )
            for row in scenario_changes
        )
        lines.append(f"| {scenario} | {changed_ranks} | {changed_scores} |")

    lines.extend(
        [
            "",
            "## Perfect-run and cell effects",
            "",
            "| scenario | panel | perfect runs / runs | Δ perfect runs | "
            "contributing cells / observed | Δ cells |",
            "| --- | --- | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in perfect_run_summary_rows:
        baseline = baseline_perfect_by_panel[row["panel_id"]]
        perfect_delta = int(row["perfect_runs"]) - int(baseline["perfect_runs"])
        cell_delta = int(row["contributing_perfect_cells"]) - int(
            baseline["contributing_perfect_cells"]
        )
        lines.append(
            f"| {row['scenario']} | {row['panel_id']} | "
            f"{row['perfect_runs']}/{row['find_runs']} | "
            f"{perfect_delta:+d} | "
            f"{row['contributing_perfect_cells']}/{row['observed_cells']} | "
            f"{cell_delta:+d} |"
        )
    lines.extend(
        [
            "",
            "Run-change counts below cover all primary find runs, including cross-effort",
            "diagnostics; perfect-cell counts and R_b use only the matched high/high",
            "and low/low panels.",
            "",
            "| scenario | changed run metrics | changed perfect statuses | "
            "changed perfect-count cells | changed cell contribution |",
            "| --- | ---: | ---: | ---: | ---: |",
        ]
    )
    for scenario in scenarios[1:]:
        run_changes = [row for row in run_metric_changes if row["scenario"] == scenario]
        cell_changes = [row for row in perfect_cell_changes if row["scenario"] == scenario]
        lines.append(
            f"| {scenario} | {len(run_changes)} | "
            f"{sum(int(row['perfect_status_changed']) for row in run_changes)} | "
            f"{len(cell_changes)} | "
            f"{sum(int(row['contribution_changed']) for row in cell_changes)} |"
        )

    lines.extend(
        [
            "",
            "## Scope",
            "",
            *scope_notes,
            "Full run-level inspection bytes are retained, matching the released",
            "conditioned-effort contract.",
            "",
            "Regenerate with:",
            "",
            "```sh",
            analysis_command,
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def _builder_orders(rb_rows: list[dict[str, Any]]) -> dict[tuple[str, str], list[str]]:
    """Return each scenario-panel builder order sorted by R_b rank."""

    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rb_rows:
        grouped[(row["scenario"], row["panel_id"])].append(row)
    return {
        key: [row["builder_id"] for row in sorted(rows, key=lambda row: int(row["rank"]))]
        for key, rows in grouped.items()
    }


def _combine(
    scenario_results: dict[str, dict[str, Any]],
    key: str,
) -> list[dict[str, Any]]:
    return [row for scenario in scenario_results for row in scenario_results[scenario][key]]


def _one(rows: list[dict[str, Any]], **criteria: str) -> dict[str, Any]:
    matches = [
        row for row in rows if all(str(row[key]) == str(value) for key, value in criteria.items())
    ]
    if len(matches) != 1:
        raise RuntimeError(f"expected one row for {criteria}, found {len(matches)}")
    return matches[0]


def _write_csv(
    path: Path,
    rows: list[dict[str, Any]],
    fieldnames: Sequence[str],
) -> None:
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=fieldnames,
            extrasaction="raise",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)


def _ratio(numerator: int | float, denominator: int | float) -> float:
    return numerator / denominator if denominator else math.nan


def _pct(value: Any, *, digits: int = 1) -> str:
    numeric = float(value)
    if math.isnan(numeric):
        return "--"
    return f"{numeric * 100:.{digits}f}%"


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


if __name__ == "__main__":
    main()
