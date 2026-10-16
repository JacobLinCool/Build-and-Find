# Artifact Conformance Impact Analysis

Generated: 2026-10-15T21:18:38.145032+00:00

This report conditions finder metrics on artifact-question pairs labeled
`implemented_gold`. Other labels are counted as artifact conformance failures
or ambiguous implementation evidence, not as recoverable gold behavior.

Label corrections: 15 rows from `runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv` (sha256 `6430dd0b42919b7f8b25214ae10d20f0f5958cb8c784093f92bd084e08910b29`) overlaid on the released labels.

## Gate Summary

| scope | task | builds | pairs | gold | non-gold | absent/ambig | excluded | original acc | gold-only acc | excluded acc |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| artifact_present_all | scratch_minidb | 24 | 360 | 340 (94.4%) | 16 | 4 | 20 (5.6%) | 95.8% | 98.7% | 46.8% |
| artifact_present_all | scratch_nanoweb | 24 | 360 | 332 (92.2%) | 12 | 16 | 28 (7.8%) | 96.9% | 99.1% | 71.0% |
| artifact_present_all | all | 48 | 720 | 672 (93.3%) | 28 | 20 | 48 (6.7%) | 96.4% | 98.9% | 60.9% |
| compile_pass_primary | scratch_minidb | 21 | 315 | 302 (95.9%) | 10 | 3 | 13 (4.1%) | 96.4% | 98.7% | 43.6% |
| compile_pass_primary | scratch_nanoweb | 20 | 300 | 290 (96.7%) | 9 | 1 | 10 (3.3%) | 97.0% | 99.2% | 34.7% |
| compile_pass_primary | all | 41 | 615 | 592 (96.3%) | 19 | 4 | 23 (3.7%) | 96.7% | 98.9% | 39.7% |
| compile_failed_diagnostic | scratch_minidb | 3 | 45 | 38 (84.4%) | 6 | 1 | 7 (15.6%) | 91.7% | 98.9% | 52.8% |
| compile_failed_diagnostic | scratch_nanoweb | 4 | 60 | 42 (70.0%) | 3 | 15 | 18 (30.0%) | 96.4% | 98.6% | 91.2% |
| compile_failed_diagnostic | all | 7 | 105 | 80 (76.2%) | 9 | 16 | 25 (23.8%) | 94.4% | 98.8% | 80.4% |

## Primary Figure 1 Sensitivity

Higher is better. The plotted gate is simple mean audited answer recovery over contributing finder-task cells; the cells column reports coverage separately.

### high_effort

| conditioned rank | builder | original rank | original score | conditioned score | cells | mean acc | mean q |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | claude_code_mimo_v2_5_pro_high | 5 | 0.961 | 0.998 | 6/12 | 0.998 | 14.0 |
| 2 | claude_code_opus_4_7_high | 1 | 0.996 | 0.996 | 12/12 | 0.996 | 15.0 |
| 3 | claude_code_sonnet_4_6_high | 4 | 0.968 | 0.993 | 12/12 | 0.993 | 14.5 |
| 4 | codex_gpt5_4_mini_high | 2 | 0.991 | 0.991 | 12/12 | 0.991 | 15.0 |
| 5 | codex_gpt5_5_high | 3 | 0.988 | 0.988 | 12/12 | 0.988 | 15.0 |
| 6 | claude_code_mimo_v2_5_high | 6 | 0.906 | 0.970 | 12/12 | 0.970 | 13.0 |

### low_effort

| conditioned rank | builder | original rank | original score | conditioned score | cells | mean acc | mean q |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | claude_code_opus_4_7_low | 1 | 0.994 | 0.994 | 12/12 | 0.994 | 15.0 |
| 2 | codex_gpt5_5_low | 3 | 0.987 | 0.991 | 12/12 | 0.991 | 14.5 |
| 3 | claude_code_sonnet_4_6_low | 5 | 0.948 | 0.991 | 12/12 | 0.991 | 14.0 |
| 4 | claude_code_mimo_v2_5_pro_low | 2 | 0.989 | 0.989 | 6/12 | 0.989 | 15.0 |
| 5 | codex_gpt5_4_mini_low | 6 | 0.931 | 0.985 | 12/12 | 0.985 | 13.5 |
| 6 | claude_code_mimo_v2_5_low | 4 | 0.956 | 0.980 | 12/12 | 0.980 | 14.0 |


## Primary Figure 2 Sensitivity

Lower is better. Scores are within-finder-task total-byte ratios on conditioned all-correct cells.

### high_effort

| conditioned rank | builder | original rank | original score | conditioned score | cells | mean acc | mean q |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_high | 1 | 1.033 | 1.033 | 12/12 | 0.988 | 15.0 |
| 2 | codex_gpt5_4_mini_high | 2 | 1.320 | 1.320 | 12/12 | 0.991 | 15.0 |
| 3 | claude_code_opus_4_7_high | 4 | 1.514 | 1.514 | 12/12 | 0.996 | 15.0 |
| 4 | claude_code_sonnet_4_6_high | 3 | 1.483 | 1.580 | 12/12 | 0.993 | 14.5 |
| 5 | claude_code_mimo_v2_5_high | 5 | 1.715 | 1.636 | 12/12 | 0.970 | 13.2 |
| 6 | claude_code_mimo_v2_5_pro_high | 6 | 1.908 | 1.976 | 6/12 | 0.998 | 14.0 |

### low_effort

| conditioned rank | builder | original rank | original score | conditioned score | cells | mean acc | mean q |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_low | 1 | 1.137 | 1.151 | 12/12 | 0.991 | 14.8 |
| 2 | codex_gpt5_4_mini_low | 2 | 1.202 | 1.151 | 11/12 | 0.990 | 13.5 |
| 3 | claude_code_opus_4_7_low | 3 | 1.727 | 1.749 | 12/12 | 0.994 | 15.0 |
| 4 | claude_code_mimo_v2_5_pro_low | 5 | 1.799 | 1.793 | 6/12 | 0.989 | 15.0 |
| 5 | claude_code_sonnet_4_6_low | 4 | 1.750 | 1.874 | 12/12 | 0.991 | 14.2 |
| 6 | claude_code_mimo_v2_5_low | 6 | 1.927 | 1.931 | 11/12 | 0.989 | 14.0 |


## Highest Exclusion Builders

| task | compile | builder | pairs | gold rate | excluded |
| --- | --- | --- | ---: | ---: | ---: |
| scratch_nanoweb | false | claude_code_mimo_v2_5_low | 15 | 0.0% | 15 |
| scratch_minidb | false | codex_gpt5_4_mini_low | 15 | 73.3% | 4 |
| scratch_minidb | true | claude_code_mimo_v2_5_high | 30 | 80.0% | 6 |
| scratch_nanoweb | true | codex_gpt5_4_mini_low | 30 | 86.7% | 4 |
| scratch_minidb | false | claude_code_mimo_v2_5_pro_low | 30 | 90.0% | 3 |
| scratch_minidb | true | claude_code_mimo_v2_5_low | 30 | 93.3% | 2 |
| scratch_minidb | true | claude_code_mimo_v2_5_pro_high | 30 | 93.3% | 2 |
| scratch_nanoweb | false | claude_code_mimo_v2_5_pro_high | 30 | 93.3% | 2 |
| scratch_nanoweb | true | claude_code_sonnet_4_6_high | 30 | 93.3% | 2 |
| scratch_nanoweb | true | claude_code_sonnet_4_6_low | 30 | 93.3% | 2 |
| scratch_minidb | true | codex_gpt5_4_mini_low | 15 | 93.3% | 1 |
| scratch_nanoweb | false | claude_code_mimo_v2_5_pro_low | 15 | 93.3% | 1 |

## Primary Accuracy By Label

| label | pairs | answer rows | accuracy |
| --- | ---: | ---: | ---: |
| implemented_gold | 592 | 21312 | 98.9% |
| implemented_non_gold | 19 | 684 | 33.9% |
| absent_or_ambiguous | 4 | 144 | 67.4% |
| documented_only | 0 | 0 | -- |
| not_applicable_or_invalid | 0 | 0 | -- |

## Files

- `conformance_gate_summary.csv`
- `accuracy_by_conformance_label.csv`
- `builder_conformance_summary.csv`
- `conditioned_run_rows.csv`
- `conditioned_recovery_scores.csv`
- `conditioned_builder_scores.csv`
- `conditioned_builder_scores_by_task.csv`
- `score_comparison.csv`
- `manifest.json`
