# Reproducibility Index

This index maps the paper's main results to the scripts, tables, and reports that produce them, and records the hashes of the canonical tables. The implementation-label audits are in `rebuttal/full_conformance_audit/` and `rebuttal/conformance_spot_audit/`.

- Primary panel: `paper_final_scratch`
- Artifact-present diagnostic panel: `artifact_present_diagnostic`
- Table schema: `2.3`
- Table manifest: `runs_release/tables/MANIFEST.json`
- Manifest generated at: `2026-05-03T20:04:26.141382+00:00`

## Scope

- Final-study evidence is restricted to the `scratch_*` task IDs.
- Tasks: `scratch_minidb`, `scratch_nanoweb`
- Canonical artifact-present build rows in `artifact_present_diagnostic`: 48
- Canonical artifact-present find rows in `artifact_present_diagnostic`: 1728
- Compile-pass primary build rows in `paper_final_scratch`: 41
- Compile-pass primary find rows in `paper_final_scratch`: 1476

## Final Logical Estimand Used in the Manuscript

- Planned formal build cells: 48
- Compile-pass primary formal build cells: 41 / 41
- Excluded build cells: 7 ({'compile_probe_failed': 7})
- Formal find records over compile-pass primary builds: 1476 / 1476
- Planned formal find records before primary exclusions: 1728
- Formal question rows: 22140
- Control find records: 144 / 144
- Control question rows: 2160
- Duplicate OK formal logical cells selected away: 0
- Duplicate OK control logical cells selected away: 0
- Formal three-trial agreement, all answer rows: 94.6%; 76.7% reliable questions
- Spec-only three-trial agreement: 99.7%; 100.0% reliable questions
- Question-only three-trial agreement, all released runs: 86.7%; 66.7% reliable questions
- Paper Table 2 formal: 97.5% three-trial agreement; 29 / 30 reliable questions (1476 find runs, 21312 answer rows)
- Paper Table 2 spec_only: 99.7% three-trial agreement; 30 / 30 reliable questions (72 find runs, 1080 answer rows)
- Paper Table 2 question_only: 94.4% three-trial agreement; 24 / 30 reliable questions (61 find runs, 915 answer rows)
- Formal tokens: 934533272
- Control tokens: 19189616
- Formal cost USD: 616.00
- Control cost USD: 19.42

## Agents

- Builders: `claude_code_mimo_v2_5_high`, `claude_code_mimo_v2_5_low`, `claude_code_mimo_v2_5_pro_high`, `claude_code_mimo_v2_5_pro_low`, `claude_code_opus_4_7_high`, `claude_code_opus_4_7_low`, `claude_code_sonnet_4_6_high`, `claude_code_sonnet_4_6_low`, `codex_gpt5_4_mini_high`, `codex_gpt5_4_mini_low`, `codex_gpt5_5_high`, `codex_gpt5_5_low`
- Finders: `claude_code_mimo_v2_5_high`, `claude_code_mimo_v2_5_low`, `claude_code_mimo_v2_5_pro_high`, `claude_code_mimo_v2_5_pro_low`, `claude_code_opus_4_7_high`, `claude_code_opus_4_7_low`, `claude_code_sonnet_4_6_high`, `claude_code_sonnet_4_6_low`, `codex_gpt5_4_mini_high`, `codex_gpt5_4_mini_low`, `codex_gpt5_5_high`, `codex_gpt5_5_low`

## Metric Provenance

- `retrieval_bytes`, `tool_call_bytes`, and `total_bytes` are the canonical inspection-effort fields.
- `session_log_*` columns are retained only as diagnostics.

- `proxy_novel_usage_events`: 1476 find rows

## Status Coverage

- build `ok`: 41
- find `ok`: 1476

## Trial Coverage

- 3 finder trial(s): 492 task/builder/finder/build-trial cells

## Sources of the Main Results

### Benchmark scope and controls

- Claim: the final manuscript panel reports compile-pass primary coverage, excluded-build diagnostics, and three-trial spec-only and question-only prior controls.
- Analysis output: `runs_release/reports/eandd_final/insights.md`
- Tables: `runs_release/reports/eandd_final/metrics.json`, `stability_summary.csv`, `question_stability.csv`, `control_summary.csv`, `question_summary.csv`, `task_summary.csv`
- Script: `scripts/analyze_eandd_final.py`

### Finder validity and builder recovery context

- Claim: finder choice is measurable and builder recovery accuracy is a gate/calibration signal in this high-prior task pack, not sufficient for artifact-quality claims without stability, effort, and control diagnostics.
- Analysis output: `runs_release/reports/eandd_final/insights.md`
- Tables: `finder_summary.csv`, `builder_summary.csv`, `builder_finder_matrix.csv`
- Script: `scripts/analyze_eandd_final.py`

### Core finder selection

- Claim: the post-calibration routine finder panel is selected from frozen calibration outputs rather than manual cherry-picking.
- Analysis output: `runs_release/reports/eandd_final/core_finder_selection/core_finders.md`
- Tables: `core_finder_selection/finder_selection.csv`, `core_finder_selection/core_finders.json`
- Script: `scripts/select_eandd_core_finders.py`

### Control-conditioned robustness views

- Claim: builder and finder conclusions are checked under top-finder, cross-family, implementation-signal, and low-prior slices.
- Analysis output: `runs_release/reports/eandd_final/pivots/pivot_insights.md`
- Tables: `runs_release/reports/eandd_final/pivots/agent_views.csv`, `question_signal.csv`
- Script: `scripts/analyze_eandd_pivots.py`

### Within-finder builder efficiency

- Claim: the GPT-5.5 rows show low-effort artifact recovery on all 12 finder-task cells under the conditional within-finder effort diagnostic in the reported compile-pass panel.
- Analysis output: `runs_release/reports/eandd_final/within_finder_efficiency/within_finder_efficiency_insights.md`
- Tables: `builder_scores.csv`, `recovery_scores.csv`, `rank_stability.csv`
- Script: `scripts/analyze_eandd_within_finder_efficiency.py`

### Validity stress diagnostics

- Claim: question-only controls, compile-failed artifacts, and the observed empty artifact expose the released task pack's prior and stress calibration.
- Analysis output: `runs_release/reports/eandd_final/validity_stress/validity_stress_insights.md`
- Tables: `stress_summary.csv`, `question_prior_lift.csv`, `excluded_builds.csv`, `bootstrap_lift.csv`
- Scripts: `scripts/analyze_eandd_validity_stress.py`, `scripts/analyze_eandd_calibration_uncertainty.py`

### Low-prior and evidence-supported recovery

- Claim: low-prior and evidence-audit slices test whether recovery is artifact-evidenced rather than only prior-driven.
- Analysis output: `runs_release/reports/eandd_final/low_prior/low_prior_insights.md`, `runs_release/reports/evidence_citation_audit.md`
- Tables: `low_prior/within_finder_efficiency/builder_scores.csv`, `low_prior/within_finder_efficiency/recovery_scores.csv`, `evidence_citation_audit_sample.csv`
- Scripts: `scripts/analyze_eandd_low_prior.py`, `scripts/audit_finder_evidence_sample.py`, `scripts/summarize_evidence_audit.py`

### Question-only control exclusions

- Claim: question-only control runs whose transcripts show reads outside their workspace are excluded; control-dependent values (calibration and repeatability rows, matched differences, per-finder calibration, low-prior accuracy and lift, the core-finder selector rerun) use the corrected control, while the frozen low-prior and implementation-signal question sets stay as selected on the released controls.
- Analysis output: `runs_release/reports/eandd_final/control_exclusions/report.md`
- Tables: `excluded_question_only_runs.csv`, `printed_values.csv`, `calibration_rows.csv`, `contrasts.csv`, `repeatability.csv`, `finder_calibration.csv`, `finder_selection.csv`, `summary.json`
- Script: `scripts/analyze_control_exclusions.py`

### Primary-panel transcript exposure

- Claim: no primary-panel finder read the specification or the gold-annotated question files, and excluding the primary-panel runs whose transcripts reach outside their workspace changes accuracy by at most 0.11 pp and leaves every R_b rank unchanged.
- Analysis output: `runs_release/reports/eandd_final/transcript_audit/report.md`
- Tables: `primary_panel_exposure.csv`, `summary.json`
- Script: `scripts/analyze_transcript_exposure.py`

### Implementation labels (v1.1)

- Claim: implementation-conditioned results use the released v1.0 labels with the v1.1 corrections overlaid; the build without source files is labeled `absent_or_ambiguous` on all of its questions (`rule_empty_artifact`).
- Analysis output: `runs_release/reports/artifact_conformance_consensus/impact/impact_report.md`, `runs_release/reports/artifact_conformance_consensus/final/README.md`
- Tables: `final/artifact_conformance_final.csv` (v1.0), `final/label_corrections_v1_1.csv`, `impact/conformance_gate_summary.csv`, `impact/builder_conformance_summary.csv`, `impact/conditioned_builder_scores.csv`, `impact/manifest.json`
- Script: `scripts/analyze_artifact_conformance_impact.py`

### Implementation-label sensitivity

- Claim: under the adjudicated implementation labels (43 author decisions and 162 model-assisted decisions reviewed and approved by the author), the high-effort R_b order is unchanged, the two low-effort leaders swap, and low-effort ranks three to six keep their order; per-builder implementation coverage over artifact-present builds ranges from 35.0% to 83.3%, implemented-gold accuracy is 99.7% (99.4% low-prior), and 23 of the 65 supported evidence-audit answers fall on relabeled pairs. The paper's other analyses use the released labels.
- Analysis output: `rebuttal/full_conformance_audit/label_sensitivity_adjudicated/report.md`
- Tables: `rb_rankings.csv`, `rb_rank_changes.csv`, `corrections.csv`, `implementation_coverage.csv`, `conditioned_accuracy.csv`, `evidence_audit_overlap.csv`, `relabeled_pairs.csv`, `question_labels.csv`, `manifest.json`
- Decisions: `rebuttal/full_conformance_audit/author_decisions.csv`, `rebuttal/full_conformance_audit/model_assisted_decisions.csv`
- Script: `scripts/analyze_rebuttal_label_sensitivity.py` (`make label-sensitivity`)

### Per-task R_b

- Claim: the total-byte R_b of Figure 1 is recomputed on each task separately under the released labels, with a bootstrap over that task's finder clusters and the pooled rank of each builder (paper Table 16).
- Analysis output: `runs_release/reports/artifact_conformance_consensus/impact/conditioned_builder_scores_by_task.csv`
- Script: `scripts/analyze_artifact_conformance_impact.py`

### Option-free check

- Claim: in the additional option-free check (eight low-prior questions, one artifact per task, two finders, three trials), free-text answers are correct in 38/48 responses with artifact access and 13/48 from the question alone (+52.1 pp); the matched multiple-choice arms are correct in 48/48 and 48/48 answer rows.
- Analysis output: `runs_release/reports/option_free_check/report.md`
- Tables: `summary.json`, `grading_key.csv`, `grading_sheet.pass1.csv`, `grading_sheet.pass2.csv`, `llm_prelabels.csv`, `rubric.md`, `records/openfind-*.json`
- Scripts: `scripts/run_open_pilot.py` (sessions), `scripts/analyze_option_free_check.py`

### Builder-written test suites

- Claim: the builder-written test suites pass in full in 40 of 41 compile-pass builds, and 2060 of 2061 tests pass; the suites indicate self-consistency of each artifact with its own tests.
- Analysis output: `runs_release/reports/runtime_tests/report.md`
- Tables: `artifact_tests.csv`, `summary.json`
- Scripts: `scripts/run_artifact_tests.py` (test run), `scripts/summarize_runtime_tests.py`

### Builder-finder affinity

- Claim: affinity residuals diagnose panel-local builder--finder interaction effects after accounting for builder and finder marginals.
- Analysis output: `runs_release/reports/eandd_final/plots/recovery_affinity_residual_12x12.csv`
- Tables: `plots/recovery_affinity_residual_12x12.csv`, `plots/recovery_affinity_residual_family.csv`
- Script: `scripts/plot_affinity_residuals.py`

## Table Hashes

- `build_runs.csv`: sha256 `6ef9dc045cc4b004e591c17c862b121aeae4ccabb20ff63b225ea098e63d031c`, bytes 30278, rows 52
- `experiments.json`: sha256 `00c4fa6e46eda9a11bbac210bdda06f5d33faa87905011b5c69f0e9cd3df0b5d`, bytes 15526, rows n/a
- `find_runs.csv`: sha256 `5f1285bfb746a5c53da54ad57f69e7a451a670bc58b09c283fb9c31082645540`, bytes 1322367, rows 1872
- `panel_membership.csv`: sha256 `eb0e5d63a33f36cb86fe4346dec14fd1577fc2af5df1e101c0a7c010d8f1d402`, bytes 18202760, rows 151428
- `question_answers.csv`: sha256 `30ca51f75d2d87cda16c410f83eb20b89f5a028f03a2c5d8f8e33eca88fa01df`, bytes 24554824, rows 28080
