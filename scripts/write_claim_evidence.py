"""Write an auditable claim-evidence index from canonical analysis tables.

The canonical primary paper panel contains compile-passing scratch builds. The
artifact-present diagnostic panel retains status-ok builds whose release artifact
path exists, including compile-failed diagnostic artifacts used by validity
stress reports.
"""

from __future__ import annotations

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
        "# Claim Evidence",
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
            "- Formal three-trial agreement: "
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
            "- Question-only three-trial agreement: "
            f"{_percent(stability_all['question_only']['stable_cell_rate'])}; "
            f"{_percent(stability_all['question_only']['stable_questions_ge_090'] / stability_all['question_only']['question_count'])} reliable questions"
        )
        if stability_all
        else "- Question-only three-trial agreement: n/a",
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
        "## Major Claim Sources",
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
        "- Claim: GPT-5.5 rows are full-coverage, low-effort examples under the conditional within-finder effort diagnostic in the reported compile-pass panel.",
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

    out = p.reports_dir / "claim_evidence.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
