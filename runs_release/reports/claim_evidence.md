# Claim Evidence

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
- Formal three-trial agreement: 94.6%; 76.7% reliable questions
- Spec-only three-trial agreement: 99.7%; 100.0% reliable questions
- Question-only three-trial agreement: 86.7%; 66.7% reliable questions
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

## Major Claim Sources

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

- Claim: GPT-5.5 rows are full-coverage, low-effort examples under the conditional within-finder effort diagnostic in the reported compile-pass panel.
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

### Builder-finder affinity

- Claim: affinity residuals diagnose panel-local builder--finder interaction effects after accounting for builder and finder marginals.
- Analysis output: `runs_release/reports/eandd_final/plots/recovery_affinity_residual_12x12.csv`
- Tables: `plots/recovery_affinity_residual_12x12.csv`, `plots/recovery_affinity_residual_family.csv`
- Script: `scripts/plot_affinity_residuals.py`

## Table Hashes

- `build_runs.csv`: sha256 `5955be048aff64d406ec59e6ad50011a2dde68e9fc394fc5cd3a2792fc8ce01e`, bytes 30331, rows 52
- `experiments.json`: sha256 `00c4fa6e46eda9a11bbac210bdda06f5d33faa87905011b5c69f0e9cd3df0b5d`, bytes 15526, rows n/a
- `find_runs.csv`: sha256 `3d327214f19c5983adc0c5c7c34935cbdf1ef886ee8895ef7fdf837186ebcb40`, bytes 1324240, rows 1872
- `panel_membership.csv`: sha256 `1b6dce751931238968e65651de7944d90947672a3e972c801bb7221164f0b37b`, bytes 18354189, rows 151428
- `question_answers.csv`: sha256 `586290493ae3dc388e040e565cd30fb600ffd940c0d99cfb1df1f2985d3bbb74`, bytes 24582905, rows 28080
