"""Write the reproducibility index from canonical analysis tables.

The index maps the paper's main results to the scripts, tables, and reports that
produce them, and records the hashes of the canonical tables.

The canonical primary paper panel contains compile-passing scratch builds. The
artifact-present diagnostic panel retains status-ok builds whose release artifact
path exists, including compile-failed diagnostic artifacts used by validity
stress reports.
"""

from __future__ import annotations

import csv
import json
from collections import Counter, defaultdict
from pathlib import Path

from bnf.analysis.tables import (
    ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
    PAPER_FINAL_PANEL_ID,
    SCRATCH_TASKS,
    load_build_run_table,
    load_find_run_table,
    require_analysis_tables,
)
from bnf.config import paths


ROOT = Path(__file__).resolve().parents[1]


def _percent(value: float) -> str:
    return f"{value * 100:.1f}%"


def _coverage(rows: list[dict]) -> list[str]:
    counts: dict[tuple[str, str, str, int], set[int]] = defaultdict(set)
    for row in rows:
        counts[
            (
                row["task_id"],
                row["builder_id"],
                row["finder_id"],
                row["build_trial_id"],
            )
        ].add(row["finder_trial_id"])
    distribution = Counter(len(v) for v in counts.values())
    return [f"- {n} finder trial(s): {c} task/builder/finder/build-trial cells" for n, c in sorted(distribution.items())]


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def _row_count_label(entry: dict[str, object]) -> object:
    row_count = entry.get("row_count")
    return row_count if row_count not in ("", None) else "n/a"


def _paper_repeatability(path: Path) -> list[str]:
    """Table 2 rows as printed in the paper, with the corrected question-only control."""
    if not path.exists():
        return ["- Paper Table 2 repeatability: n/a"]
    with path.open(newline="", encoding="utf-8") as handle:
        rows = [row for row in csv.DictReader(handle) if row["table"] == "table2"]
    return [
        f"- Paper Table 2 {row['slice_id']}: {row['agreement_pct']}% three-trial agreement; "
        f"{row['reliable_questions']} / {row['questions']} reliable questions "
        f"({row['find_runs']} find runs, {row['answer_rows']} answer rows)"
        for row in rows
    ]


def _count(rate: dict[str, object]) -> str:
    return f"{rate['correct']}/{rate['total']}"


def _option_free_claim(path: Path) -> str:
    """Option-free counts as printed in Section 4.2 and Appendix L.1."""
    if not path.exists():
        return "- Claim: option-free check: n/a"
    summary = json.loads(path.read_text(encoding="utf-8"))
    overall = summary["option_free"]["all"]
    mcq = summary["multiple_choice"]
    return (
        "- Claim: in the additional option-free check (eight low-prior questions, one "
        "artifact per task, two finders, three trials), free-text answers are correct in "
        f"{_count(overall['artifact_open'])} responses with artifact access and "
        f"{_count(overall['question_only_open'])} from the question alone "
        f"({overall['difference_pp']} pp); the matched multiple-choice arms are correct in "
        f"{_count(mcq['artifact_present'])} and {_count(mcq['question_only'])} answer rows."
    )


def _runtime_tests_claim(path: Path) -> str:
    """Builder-written test-suite counts as printed in Section 6 and Appendix G."""
    if not path.exists():
        return "- Claim: builder-written test suites: n/a"
    summary = json.loads(path.read_text(encoding="utf-8"))
    suites = summary["suites"]
    tests = summary["tests"]
    return (
        f"- Claim: the builder-written test suites pass in full in {suites['passed']} of "
        f"{suites['run']} compile-pass builds, and {tests['passed']} of {tests['total']} tests "
        "pass; the suites indicate self-consistency of each artifact with its own tests."
    )


def main() -> None:
    p = paths()
    require_analysis_tables(p.tables_dir)
    final_metrics_path = p.reports_dir / "eandd_final" / "metrics.json"
    final_metrics = (
        json.loads(final_metrics_path.read_text(encoding="utf-8"))
        if final_metrics_path.exists()
        else None
    )
    primary_find_rows = [
        row
        for row in load_find_run_table(p.tables_dir, panel_id=PAPER_FINAL_PANEL_ID)
        if row["task_id"] in SCRATCH_TASKS
    ]
    primary_build_rows = [
        row
        for row in load_build_run_table(p.tables_dir, panel_id=PAPER_FINAL_PANEL_ID)
        if row["task_id"] in SCRATCH_TASKS
    ]
    artifact_present_find_rows = [
        row
        for row in load_find_run_table(
            p.tables_dir,
            panel_id=ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
        )
        if row["task_id"] in SCRATCH_TASKS
    ]
    artifact_present_build_rows = [
        row
        for row in load_build_run_table(
            p.tables_dir,
            panel_id=ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
        )
        if row["task_id"] in SCRATCH_TASKS
    ]
    control_exclusions_dir = p.reports_dir / "eandd_final" / "control_exclusions"
    transcript_audit_dir = p.reports_dir / "eandd_final" / "transcript_audit"
    conformance_dir = p.reports_dir / "artifact_conformance_consensus"
    option_free_dir = p.reports_dir / "option_free_check"
    runtime_tests_dir = p.reports_dir / "runtime_tests"
    label_sensitivity_dir = (
        ROOT / "rebuttal" / "full_conformance_audit" / "label_sensitivity_adjudicated"
    )
    experiments = json.loads((p.tables_dir / "experiments.json").read_text(encoding="utf-8"))
    manifest = json.loads((p.tables_dir / "MANIFEST.json").read_text(encoding="utf-8"))

    metric_sources = Counter(row["metric_source"] for row in primary_find_rows)
    find_statuses = Counter(row["find_status"] for row in primary_find_rows)
    build_statuses = Counter(row["build_status"] for row in primary_build_rows)
    scope = final_metrics["scope"] if final_metrics else {}
    headline = final_metrics["headline"] if final_metrics else {}
    duplicates = final_metrics["duplicates"] if final_metrics else {}
    stability_all = {
        row["condition"]: row
        for row in (final_metrics.get("stability_summary", []) if final_metrics else [])
        if row["task_id"] == "all"
    }

    lines = [
        "# Reproducibility Index",
        "",
        "This index maps the paper's main results to the scripts, tables, and reports "
        "that produce them, and records the hashes of the canonical tables. The "
        "implementation-label audits are in `rebuttal/full_conformance_audit/` and "
        "`rebuttal/conformance_spot_audit/`.",
        "",
        f"- Primary panel: `{PAPER_FINAL_PANEL_ID}`",
        f"- Artifact-present diagnostic panel: `{ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID}`",
        f"- Table schema: `{experiments['schema_version']}`",
        f"- Table manifest: `{_display_path(p.tables_dir / 'MANIFEST.json')}`",
        f"- Manifest generated at: `{manifest['generated_at']}`",
        "",
        "## Scope",
        "",
        "- Final-study evidence is restricted to the `scratch_*` task IDs.",
        f"- Tasks: {', '.join(f'`{task}`' for task in sorted(SCRATCH_TASKS))}",
        f"- Canonical artifact-present build rows in `{ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID}`: {len(artifact_present_build_rows)}",
        f"- Canonical artifact-present find rows in `{ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID}`: {len(artifact_present_find_rows)}",
        f"- Compile-pass primary build rows in `{PAPER_FINAL_PANEL_ID}`: {len(primary_build_rows)}",
        f"- Compile-pass primary find rows in `{PAPER_FINAL_PANEL_ID}`: {len(primary_find_rows)}",
        "",
        "## Final Logical Estimand Used in the Manuscript",
        "",
        f"- Planned formal build cells: {scope.get('planned_formal_build_cells', 'n/a')}",
        f"- Compile-pass primary formal build cells: {scope.get('formal_build_cells', 'n/a')} / {scope.get('expected_formal_build_cells', 'n/a')}",
        f"- Excluded build cells: {scope.get('excluded_build_cells', 'n/a')} ({scope.get('excluded_build_reason_counts', {})})",
        f"- Formal find records over compile-pass primary builds: {scope.get('formal_find_cells', 'n/a')} / {scope.get('expected_formal_find_cells', 'n/a')}",
        f"- Planned formal find records before primary exclusions: {scope.get('planned_formal_find_cells', 'n/a')}",
        f"- Formal question rows: {scope.get('formal_question_rows', 'n/a')}",
        f"- Control find records: {scope.get('control_find_cells', 'n/a')} / {scope.get('expected_control_find_cells', 'n/a')}",
        f"- Control question rows: {scope.get('control_question_rows', 'n/a')}",
        f"- Duplicate OK formal logical cells selected away: {duplicates.get('formal_logical_cells_with_multiple_ok_records', 'n/a')}",
        f"- Duplicate OK control logical cells selected away: {duplicates.get('control_logical_cells_with_multiple_ok_records', 'n/a')}",
        (
            "- Formal three-trial agreement, all answer rows: "
            f"{_percent(stability_all['with_codebase']['stable_cell_rate'])}; "
            f"{_percent(stability_all['with_codebase']['stable_questions_ge_090'] / stability_all['with_codebase']['question_count'])} reliable questions"
        )
        if stability_all
        else "- Formal three-trial agreement: n/a",
        (
            "- Spec-only three-trial agreement: "
            f"{_percent(stability_all['spec_only']['stable_cell_rate'])}; "
            f"{_percent(stability_all['spec_only']['stable_questions_ge_090'] / stability_all['spec_only']['question_count'])} reliable questions"
        )
        if stability_all
        else "- Spec-only three-trial agreement: n/a",
        (
            "- Question-only three-trial agreement, all released runs: "
            f"{_percent(stability_all['question_only']['stable_cell_rate'])}; "
            f"{_percent(stability_all['question_only']['stable_questions_ge_090'] / stability_all['question_only']['question_count'])} reliable questions"
        )
        if stability_all
        else "- Question-only three-trial agreement: n/a",
        *_paper_repeatability(control_exclusions_dir / "repeatability.csv"),
        f"- Formal tokens: {headline.get('formal_total_tokens', 'n/a')}",
        f"- Control tokens: {headline.get('control_total_tokens', 'n/a')}",
        f"- Formal cost USD: {headline.get('formal_total_cost_usd', float('nan')):.2f}" if headline else "- Formal cost USD: n/a",
        f"- Control cost USD: {headline.get('control_total_cost_usd', float('nan')):.2f}" if headline else "- Control cost USD: n/a",
        "",
        "## Agents",
        "",
        f"- Builders: {', '.join(f'`{x}`' for x in sorted({row['builder_id'] for row in primary_find_rows}))}",
        f"- Finders: {', '.join(f'`{x}`' for x in sorted({row['finder_id'] for row in primary_find_rows}))}",
        "",
        "## Metric Provenance",
        "",
        "- `retrieval_bytes`, `tool_call_bytes`, and `total_bytes` are the canonical inspection-effort fields.",
        "- `session_log_*` columns are retained only as diagnostics.",
        "",
        *[f"- `{source}`: {count} find rows" for source, count in sorted(metric_sources.items())],
        "",
        "## Status Coverage",
        "",
        *[f"- build `{status}`: {count}" for status, count in sorted(build_statuses.items())],
        *[f"- find `{status}`: {count}" for status, count in sorted(find_statuses.items())],
        "",
        "## Trial Coverage",
        "",
        *_coverage(primary_find_rows),
        "",
        "## Sources of the Main Results",
        "",
        "### Benchmark scope and controls",
        "",
        "- Claim: the final manuscript panel reports compile-pass primary coverage, excluded-build diagnostics, and three-trial spec-only and question-only prior controls.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'insights.md')}`",
        f"- Tables: `{_display_path(p.reports_dir / 'eandd_final' / 'metrics.json')}`, `stability_summary.csv`, `question_stability.csv`, `control_summary.csv`, `question_summary.csv`, `task_summary.csv`",
        "- Script: `scripts/analyze_eandd_final.py`",
        "",
        "### Finder validity and builder recovery context",
        "",
        "- Claim: finder choice is measurable and builder recovery accuracy is a gate/calibration signal in this high-prior task pack, not sufficient for artifact-quality claims without stability, effort, and control diagnostics.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'insights.md')}`",
        "- Tables: `finder_summary.csv`, `builder_summary.csv`, `builder_finder_matrix.csv`",
        "- Script: `scripts/analyze_eandd_final.py`",
        "",
        "### Core finder selection",
        "",
        "- Claim: the post-calibration routine finder panel is selected from frozen calibration outputs rather than manual cherry-picking.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'core_finder_selection' / 'core_finders.md')}`",
        "- Tables: `core_finder_selection/finder_selection.csv`, `core_finder_selection/core_finders.json`",
        "- Script: `scripts/select_eandd_core_finders.py`",
        "",
        "### Control-conditioned robustness views",
        "",
        "- Claim: builder and finder conclusions are checked under top-finder, cross-family, implementation-signal, and low-prior slices.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'pivots' / 'pivot_insights.md')}`",
        f"- Tables: `{_display_path(p.reports_dir / 'eandd_final' / 'pivots' / 'agent_views.csv')}`, `question_signal.csv`",
        "- Script: `scripts/analyze_eandd_pivots.py`",
        "",
        "### Within-finder builder efficiency",
        "",
        "- Claim: the GPT-5.5 rows show low-effort artifact recovery on all 12 finder-task cells under the conditional within-finder effort diagnostic in the reported compile-pass panel.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'within_finder_efficiency' / 'within_finder_efficiency_insights.md')}`",
        "- Tables: `builder_scores.csv`, `recovery_scores.csv`, `rank_stability.csv`",
        "- Script: `scripts/analyze_eandd_within_finder_efficiency.py`",
        "",
        "### Validity stress diagnostics",
        "",
        "- Claim: question-only controls, compile-failed artifacts, and the observed empty artifact expose the released task pack's prior and stress calibration.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'validity_stress' / 'validity_stress_insights.md')}`",
        "- Tables: `stress_summary.csv`, `question_prior_lift.csv`, `excluded_builds.csv`, `bootstrap_lift.csv`",
        "- Scripts: `scripts/analyze_eandd_validity_stress.py`, `scripts/analyze_eandd_calibration_uncertainty.py`",
        "",
        "### Low-prior and evidence-supported recovery",
        "",
        "- Claim: low-prior and evidence-audit slices test whether recovery is artifact-evidenced rather than only prior-driven.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'low_prior' / 'low_prior_insights.md')}`, `{_display_path(p.reports_dir / 'evidence_citation_audit.md')}`",
        "- Tables: `low_prior/within_finder_efficiency/builder_scores.csv`, `low_prior/within_finder_efficiency/recovery_scores.csv`, `evidence_citation_audit_sample.csv`",
        "- Scripts: `scripts/analyze_eandd_low_prior.py`, `scripts/audit_finder_evidence_sample.py`, `scripts/summarize_evidence_audit.py`",
        "",
        "### Question-only control exclusions",
        "",
        "- Claim: question-only control runs whose transcripts show reads outside their workspace are excluded; control-dependent values (calibration and repeatability rows, matched differences, per-finder calibration, low-prior accuracy and lift, the core-finder selector rerun) use the corrected control, while the frozen low-prior and implementation-signal question sets stay as selected on the released controls.",
        f"- Analysis output: `{_display_path(control_exclusions_dir / 'report.md')}`",
        "- Tables: `excluded_question_only_runs.csv`, `printed_values.csv`, `calibration_rows.csv`, `contrasts.csv`, `repeatability.csv`, `finder_calibration.csv`, `finder_selection.csv`, `summary.json`",
        "- Script: `scripts/analyze_control_exclusions.py`",
        "",
        "### Primary-panel transcript exposure",
        "",
        "- Claim: no primary-panel finder read the specification or the gold-annotated question files, and excluding the primary-panel runs whose transcripts reach outside their workspace changes accuracy by at most 0.11 pp and leaves every R_b rank unchanged.",
        f"- Analysis output: `{_display_path(transcript_audit_dir / 'report.md')}`",
        "- Tables: `primary_panel_exposure.csv`, `summary.json`",
        "- Script: `scripts/analyze_transcript_exposure.py`",
        "",
        "### Implementation labels (v1.1)",
        "",
        "- Claim: implementation-conditioned results use the released v1.0 labels with the v1.1 corrections overlaid; the build without source files is labeled `absent_or_ambiguous` on all of its questions (`rule_empty_artifact`).",
        f"- Analysis output: `{_display_path(conformance_dir / 'impact' / 'impact_report.md')}`, `{_display_path(conformance_dir / 'final' / 'README.md')}`",
        "- Tables: `final/artifact_conformance_final.csv` (v1.0), `final/label_corrections_v1_1.csv`, `impact/conformance_gate_summary.csv`, `impact/builder_conformance_summary.csv`, `impact/conditioned_builder_scores.csv`, `impact/manifest.json`",
        "- Script: `scripts/analyze_artifact_conformance_impact.py`",
        "",
        "### Implementation-label sensitivity",
        "",
        "- Claim: under the adjudicated implementation labels (43 author decisions and 162 model-assisted decisions reviewed and approved by the author), the high-effort R_b order is unchanged, the two low-effort leaders swap, and low-effort ranks three to six keep their order; per-builder implementation coverage over artifact-present builds ranges from 35.0% to 83.3%, implemented-gold accuracy is 99.7% (99.4% low-prior), and 23 of the 65 supported evidence-audit answers fall on relabeled pairs. The paper's other analyses use the released labels.",
        f"- Analysis output: `{_display_path(label_sensitivity_dir / 'report.md')}`",
        "- Tables: `rb_rankings.csv`, `rb_rank_changes.csv`, `corrections.csv`, `implementation_coverage.csv`, `conditioned_accuracy.csv`, `evidence_audit_overlap.csv`, `relabeled_pairs.csv`, `question_labels.csv`, `manifest.json`",
        f"- Decisions: `{_display_path(label_sensitivity_dir.parent / 'author_decisions.csv')}`, `{_display_path(label_sensitivity_dir.parent / 'model_assisted_decisions.csv')}`",
        "- Script: `scripts/analyze_rebuttal_label_sensitivity.py` (`make label-sensitivity`)",
        "",
        "### Per-task R_b",
        "",
        "- Claim: the total-byte R_b of Figure 1 is recomputed on each task separately under the released labels, with a bootstrap over that task's finder clusters and the pooled rank of each builder (paper Table 16).",
        f"- Analysis output: `{_display_path(conformance_dir / 'impact' / 'conditioned_builder_scores_by_task.csv')}`",
        "- Script: `scripts/analyze_artifact_conformance_impact.py`",
        "",
        "### Option-free check",
        "",
        _option_free_claim(option_free_dir / "summary.json"),
        f"- Analysis output: `{_display_path(option_free_dir / 'report.md')}`",
        "- Tables: `summary.json`, `grading_key.csv`, `grading_sheet.pass1.csv`, `grading_sheet.pass2.csv`, `llm_prelabels.csv`, `rubric.md`, `records/openfind-*.json`",
        "- Scripts: `scripts/run_open_pilot.py` (sessions), `scripts/analyze_option_free_check.py`",
        "",
        "### Builder-written test suites",
        "",
        _runtime_tests_claim(runtime_tests_dir / "summary.json"),
        f"- Analysis output: `{_display_path(runtime_tests_dir / 'report.md')}`",
        "- Tables: `artifact_tests.csv`, `summary.json`",
        "- Scripts: `scripts/run_artifact_tests.py` (test run), `scripts/summarize_runtime_tests.py`",
        "",
        "### Builder-finder affinity",
        "",
        "- Claim: affinity residuals diagnose panel-local builder--finder interaction effects after accounting for builder and finder marginals.",
        f"- Analysis output: `{_display_path(p.reports_dir / 'eandd_final' / 'plots' / 'recovery_affinity_residual_12x12.csv')}`",
        "- Tables: `plots/recovery_affinity_residual_12x12.csv`, `plots/recovery_affinity_residual_family.csv`",
        "- Script: `scripts/plot_affinity_residuals.py`",
        "",
        "## Table Hashes",
        "",
        *[
            f"- `{name}`: sha256 `{entry['sha256']}`, bytes {entry['bytes']}, rows {_row_count_label(entry)}"
            for name, entry in sorted(manifest["files"].items())
        ],
        "",
    ]

    out = p.reports_dir / "reproducibility_index.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
