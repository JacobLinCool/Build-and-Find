# E&D Final Logical Analysis

Generated: 2026-05-03T20:04:26.141382+00:00

## Scope Check

- Compile-pass primary formal builds: 41 / 41 (planned 48; excluded 7)
- Build exclusions by reason: {'compile_probe_failed': 7}
- Formal find records over compile-pass primary builds: 1476 / 1476 (planned 1728)
- Formal question rows: 22140
- Control find records: 144 / 144
- Control question rows: 2160
- Duplicate OK logical cells selected away: formal=0, control=0

This report is derived from raw JSON records by logical cell. It excludes older pilot records, preserved failed infrastructure attempts, status-ok builds whose release artifact path is missing, and compile-probe-failed builds from the primary estimands. Excluded builds and their find records remain in the release as validity-stress diagnostics.

## Headline

Three-trial agreement is the control headline; recovery accuracy is retained below for finder and builder diagnostics. Percentages are used for all rates in this report.
- Formal three-trial agreement: 94.6%; reliable-question rate: 76.7%
- Spec-only three-trial agreement: 99.7%; reliable-question rate: 100.0%
- Question-only three-trial agreement: 86.7%; reliable-question rate: 66.7%

## Finder Panel

| finder | formal recovery | run CI95 | spec-only reliable questions | question-only reliable questions |
| --- | ---: | ---: | ---: | ---: |
| claude_code_sonnet_4_6_low | 98.7% | 98.1%-99.3% | 100.0% | 96.7% |
| claude_code_sonnet_4_6_high | 98.0% | 97.4%-98.7% | 100.0% | 90.0% |
| claude_code_opus_4_7_low | 97.9% | 97.2%-98.6% | 100.0% | 100.0% |
| claude_code_opus_4_7_high | 97.1% | 96.3%-98.0% | 100.0% | 100.0% |
| claude_code_mimo_v2_5_high | 96.8% | 96.0%-97.6% | 96.7% | 83.3% |
| claude_code_mimo_v2_5_low | 96.8% | 95.9%-97.7% | 100.0% | 90.0% |
| claude_code_mimo_v2_5_pro_low | 96.7% | 96.0%-97.5% | 100.0% | 10.0% |
| claude_code_mimo_v2_5_pro_high | 96.7% | 95.8%-97.6% | 100.0% | 76.7% |
| codex_gpt5_5_low | 96.5% | 95.6%-97.5% | 100.0% | 100.0% |
| codex_gpt5_5_high | 95.9% | 94.8%-97.0% | 100.0% | 100.0% |
| codex_gpt5_4_mini_low | 95.2% | 94.0%-96.3% | 100.0% | 93.3% |
| codex_gpt5_4_mini_high | 94.1% | 92.8%-95.5% | 100.0% | 100.0% |

## Top Builders

| builder | downstream recovery | run CI95 |
| --- | ---: | ---: |
| claude_code_opus_4_7_high | 99.4% | 99.1%-99.8% |
| claude_code_opus_4_7_low | 99.3% | 98.9%-99.6% |
| claude_code_mimo_v2_5_pro_low | 99.3% | 98.6%-100.0% |
| codex_gpt5_4_mini_high | 99.2% | 98.8%-99.6% |
| codex_gpt5_5_high | 98.9% | 98.5%-99.3% |
| codex_gpt5_5_low | 98.7% | 98.2%-99.1% |
| claude_code_sonnet_4_6_high | 96.7% | 96.2%-97.3% |
| claude_code_mimo_v2_5_pro_high | 96.0% | 95.2%-96.9% |

## High vs Low Effort

Positive values mean high effort outperformed low effort for the same base model.

| role | base model | high-low delta |
| --- | --- | ---: |
| finder | claude_code_mimo_v2_5 | +0.0 pp |
| finder | claude_code_mimo_v2_5_pro | -0.1 pp |
| finder | codex_gpt5_5 | -0.6 pp |
| finder | claude_code_sonnet_4_6 | -0.7 pp |
| finder | claude_code_opus_4_7 | -0.8 pp |
| finder | codex_gpt5_4_mini | -1.0 pp |
| builder | codex_gpt5_4_mini | +7.0 pp |
| builder | claude_code_sonnet_4_6 | +1.8 pp |
| builder | codex_gpt5_5 | +0.2 pp |
| builder | claude_code_opus_4_7 | +0.2 pp |
| builder | claude_code_mimo_v2_5 | -2.9 pp |
| builder | claude_code_mimo_v2_5_pro | -3.2 pp |

## Affinity

- Same-family builder/finder accuracy: 97.0%
- Different-family builder/finder accuracy: 96.6%
- Same-family minus different-family: +0.4 pp
- Same-base-model builder/finder accuracy: 97.2%
- Different-base-model builder/finder accuracy: 96.6%
- Same-base-model minus different-base-model: +0.5 pp

## Task Repeatability

| task | formal 3-trial agreement | spec-only 3-trial agreement | question-only 3-trial agreement | reliable-question rate, formal/spec/question |
| --- | ---: | ---: | ---: | ---: |
| scratch_minidb | 93.8% | 99.4% | 86.1% | 60.0%/100.0%/66.7% |
| scratch_nanoweb | 95.5% | 100.0% | 87.2% | 93.3%/100.0%/66.7% |

## Control Diagnostics

| condition | 3-trial agreement | reliable-question rate |
| --- | ---: | ---: |
| with_codebase | 94.6% | 76.7% |
| spec_only | 99.7% | 100.0% |
| question_only | 86.7% | 66.7% |

## Compute Diagnostics

Token and cost totals are reported only as run-cost diagnostics. They are not used as direct recovery-efficiency metrics because formal, spec-only, and question-only runs have different inputs.

- Formal tokens: 934,533,272
- Control tokens: 19,189,616
- Formal cost USD: 616.00
- Control cost USD: 19.42

## Question Diagnostics

Hardest formal questions:

| task | question | formal recovery | spec-only recovery | question-only recovery |
| --- | --- | ---: | ---: | ---: |
| scratch_nanoweb | q_nw_15 | 77.4% | 100.0% | 97.2% |
| scratch_minidb | q_db_15 | 87.0% | 100.0% | 94.4% |
| scratch_minidb | q_db_06 | 90.5% | 100.0% | 88.9% |
| scratch_minidb | q_db_05 | 91.5% | 100.0% | 91.7% |
| scratch_minidb | q_db_13 | 91.8% | 100.0% | 94.4% |
| scratch_minidb | q_db_07 | 94.7% | 97.2% | 86.1% |
| scratch_nanoweb | q_nw_06 | 94.7% | 100.0% | 97.2% |
| scratch_nanoweb | q_nw_11 | 95.0% | 100.0% | 88.9% |

Largest artifact-conditioned lift over question-only:

| task | question | formal-question lift | formal recovery | question-only recovery |
| --- | --- | ---: | ---: | ---: |
| scratch_nanoweb | q_nw_07 | +22.2 pp | 100.0% | 77.8% |
| scratch_nanoweb | q_nw_10 | +13.7 pp | 99.9% | 86.1% |
| scratch_minidb | q_db_07 | +8.6 pp | 94.7% | 86.1% |
| scratch_nanoweb | q_nw_04 | +7.9 pp | 99.6% | 91.7% |
| scratch_minidb | q_db_01 | +6.5 pp | 95.4% | 88.9% |
| scratch_nanoweb | q_nw_11 | +6.1 pp | 95.0% | 88.9% |
| scratch_minidb | q_db_10 | +5.6 pp | 100.0% | 94.4% |
| scratch_nanoweb | q_nw_01 | +5.6 pp | 100.0% | 94.4% |

## Reviewer-Risk Notes

- Existing broad canonical tables include historical/pilot records; final paper claims should cite this logical report or regenerate paper tables from the same logical panel.
- Spec-only accuracy is high enough to require careful wording: the benchmark measures recoverability of implementation intent, not every question being implementation-exclusive.
- Finder orderings are near the ceiling, so report uncertainty, task split, and control-adjusted views instead of overclaiming small gaps.
- Same-family affinity should be treated as a diagnostic until backed by confidence intervals or a mixed-effects model.

## Output Files

- finder_summary_csv: `runs_release/reports/eandd_final/finder_summary.csv`
- builder_summary_csv: `runs_release/reports/eandd_final/builder_summary.csv`
- build_exclusions_csv: `runs_release/reports/eandd_final/build_exclusions.csv`
- builder_finder_matrix_csv: `runs_release/reports/eandd_final/builder_finder_matrix.csv`
- control_summary_csv: `runs_release/reports/eandd_final/control_summary.csv`
- question_summary_csv: `runs_release/reports/eandd_final/question_summary.csv`
- stability_summary_csv: `runs_release/reports/eandd_final/stability_summary.csv`
- question_stability_csv: `runs_release/reports/eandd_final/question_stability.csv`
- task_summary_csv: `runs_release/reports/eandd_final/task_summary.csv`
