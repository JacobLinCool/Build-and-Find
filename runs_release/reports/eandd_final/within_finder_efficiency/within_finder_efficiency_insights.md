# Within-Finder Efficiency Analysis

Generated: 2026-05-03T20:04:26.141382+00:00

This report uses the paper estimand: builder efficiency is the same finder-task
relative inspection effort over builders. It does not use build tokens or
finder-global token consumption as the builder efficiency metric.

## Primary Builder R_b

Lower is better. `n` counts contributing `(finder, task)` cells; max is 12 in each primary panel.

### high_effort

| rank | builder | R_total_bytes | n | coverage | mean cell acc |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_high | 1.033 | 12/12 | 1.000 | 0.988 |
| 2 | codex_gpt5_4_mini_high | 1.320 | 12/12 | 1.000 | 0.991 |
| 3 | claude_code_sonnet_4_6_high | 1.483 | 9/12 | 0.750 | 0.979 |
| 4 | claude_code_opus_4_7_high | 1.514 | 12/12 | 1.000 | 0.996 |
| 5 | claude_code_mimo_v2_5_high | 1.715 | 6/12 | 0.500 | 0.952 |
| 6 | claude_code_mimo_v2_5_pro_high | 1.908 | 5/12 | 0.417 | 0.967 |

### low_effort

| rank | builder | R_total_bytes | n | coverage | mean cell acc |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_low | 1.137 | 12/12 | 1.000 | 0.987 |
| 2 | codex_gpt5_4_mini_low | 1.202 | 7/12 | 0.583 | 0.946 |
| 3 | claude_code_opus_4_7_low | 1.727 | 12/12 | 1.000 | 0.994 |
| 4 | claude_code_sonnet_4_6_low | 1.750 | 12/12 | 1.000 | 0.948 |
| 5 | claude_code_mimo_v2_5_pro_low | 1.799 | 6/12 | 0.500 | 0.989 |
| 6 | claude_code_mimo_v2_5_low | 1.927 | 11/12 | 0.917 | 0.965 |

## Metric Cross-Checks

All-correct-cell R_b under component metrics.

| panel | metric | rank 1 | score | rank 2 | score |
| --- | --- | --- | ---: | --- | ---: |
| high_effort | retrieval_bytes | codex_gpt5_5_high | 1.035 | codex_gpt5_4_mini_high | 1.342 |
| high_effort | tool_call_bytes | codex_gpt5_5_high | 1.031 | claude_code_sonnet_4_6_high | 1.124 |
| high_effort | total_bytes | codex_gpt5_5_high | 1.033 | codex_gpt5_4_mini_high | 1.320 |
| high_effort | tool_call_count | codex_gpt5_5_high | 1.095 | claude_code_sonnet_4_6_high | 1.111 |
| high_effort | turn_count | codex_gpt5_5_high | 1.007 | claude_code_sonnet_4_6_high | 1.010 |
| high_effort | wall_clock_seconds | claude_code_sonnet_4_6_high | 1.074 | codex_gpt5_4_mini_high | 1.140 |
| low_effort | retrieval_bytes | codex_gpt5_5_low | 1.155 | codex_gpt5_4_mini_low | 1.221 |
| low_effort | tool_call_bytes | claude_code_mimo_v2_5_pro_low | 1.022 | codex_gpt5_5_low | 1.134 |
| low_effort | total_bytes | codex_gpt5_5_low | 1.137 | codex_gpt5_4_mini_low | 1.202 |
| low_effort | tool_call_count | claude_code_mimo_v2_5_pro_low | 1.023 | codex_gpt5_5_low | 1.109 |
| low_effort | turn_count | codex_gpt5_4_mini_low | 1.013 | codex_gpt5_5_low | 1.015 |
| low_effort | wall_clock_seconds | claude_code_mimo_v2_5_pro_low | 1.089 | claude_code_opus_4_7_low | 1.181 |

## Full-Task Recovery Diagnostic

Higher is better. Preferred recovery uses `accuracy^question_count / effort`,
normalized within each `(finder, task)` by the maximum builder value. This is a
diagnostic approximation rather than a calibrated probability; it penalizes
low per-question accuracy at the task level instead of linearly trading
partial answer accuracy against retrieval cost.

### high_effort

| rank | builder | recovery | n | mean acc | mean full-recovery signal | mean retrieval bytes |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_high | 0.931 | 12/12 | 0.988 | 0.855 | 101860 |
| 2 | codex_gpt5_4_mini_high | 0.760 | 12/12 | 0.991 | 0.885 | 129311 |
| 3 | claude_code_opus_4_7_high | 0.716 | 12/12 | 0.996 | 0.950 | 157592 |
| 4 | claude_code_sonnet_4_6_high | 0.441 | 12/12 | 0.968 | 0.657 | 164961 |
| 5 | claude_code_mimo_v2_5_pro_high | 0.363 | 6/12 | 0.961 | 0.587 | 225722 |
| 6 | claude_code_mimo_v2_5_high | 0.156 | 12/12 | 0.906 | 0.321 | 163528 |

### low_effort

| rank | builder | recovery | n | mean acc | mean full-recovery signal | mean retrieval bytes |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_low | 0.945 | 12/12 | 0.987 | 0.853 | 73070 |
| 2 | claude_code_opus_4_7_low | 0.682 | 12/12 | 0.994 | 0.915 | 117136 |
| 3 | claude_code_mimo_v2_5_pro_low | 0.566 | 6/12 | 0.989 | 0.857 | 106195 |
| 4 | codex_gpt5_4_mini_low | 0.411 | 12/12 | 0.931 | 0.409 | 64660 |
| 5 | claude_code_mimo_v2_5_low | 0.330 | 12/12 | 0.956 | 0.572 | 123450 |
| 6 | claude_code_sonnet_4_6_low | 0.304 | 12/12 | 0.948 | 0.486 | 124080 |

## Recovery Cross-Checks

| panel | variant | effort | rank 1 | score | rank 2 | score |
| --- | --- | --- | --- | ---: | --- | ---: |
| high_effort | full_task_recovery | retrieval_bytes | codex_gpt5_5_high | 0.931 | codex_gpt5_4_mini_high | 0.760 |
| high_effort | full_task_recovery | total_bytes | codex_gpt5_5_high | 0.930 | codex_gpt5_4_mini_high | 0.770 |
| high_effort | all_correct_rate | retrieval_bytes | codex_gpt5_5_high | 0.872 | codex_gpt5_4_mini_high | 0.773 |
| high_effort | all_correct_rate | total_bytes | codex_gpt5_5_high | 0.872 | codex_gpt5_4_mini_high | 0.784 |
| high_effort | answer_accuracy | retrieval_bytes | codex_gpt5_5_high | 0.966 | codex_gpt5_4_mini_high | 0.758 |
| high_effort | answer_accuracy | total_bytes | codex_gpt5_5_high | 0.968 | codex_gpt5_4_mini_high | 0.771 |
| low_effort | full_task_recovery | retrieval_bytes | codex_gpt5_5_low | 0.945 | claude_code_opus_4_7_low | 0.682 |
| low_effort | full_task_recovery | total_bytes | codex_gpt5_5_low | 0.945 | claude_code_opus_4_7_low | 0.701 |
| low_effort | all_correct_rate | retrieval_bytes | codex_gpt5_5_low | 0.884 | claude_code_opus_4_7_low | 0.669 |
| low_effort | all_correct_rate | total_bytes | codex_gpt5_5_low | 0.884 | claude_code_opus_4_7_low | 0.688 |
| low_effort | answer_accuracy | retrieval_bytes | codex_gpt5_5_low | 0.878 | codex_gpt5_4_mini_low | 0.866 |
| low_effort | answer_accuracy | total_bytes | codex_gpt5_5_low | 0.895 | codex_gpt5_4_mini_low | 0.875 |
## Rank Stability

Kendall tau across finder pairs on builder cost order, using total bytes on all-correct cells.

| panel | task | slice | n pairs | median tau | positive fraction |
| --- | --- | --- | ---: | ---: | ---: |
| high_effort | scratch_minidb | all_pairs | 15 | 0.600 | 0.733 |
| high_effort | scratch_minidb | same_family_pairs | 3 | 0.800 | 1.000 |
| high_effort | scratch_minidb | cross_family_pairs | 12 | 0.600 | 0.667 |
| high_effort | scratch_nanoweb | all_pairs | 15 | 0.800 | 0.667 |
| high_effort | scratch_nanoweb | same_family_pairs | 3 | 0.800 | 0.667 |
| high_effort | scratch_nanoweb | cross_family_pairs | 12 | 0.700 | 0.667 |
| low_effort | scratch_minidb | all_pairs | 15 | 0.333 | 0.800 |
| low_effort | scratch_minidb | same_family_pairs | 3 | 0.667 | 1.000 |
| low_effort | scratch_minidb | cross_family_pairs | 12 | 0.333 | 0.750 |
| low_effort | scratch_nanoweb | all_pairs | 15 | 0.467 | 0.867 |
| low_effort | scratch_nanoweb | same_family_pairs | 3 | 0.733 | 0.667 |
| low_effort | scratch_nanoweb | cross_family_pairs | 12 | 0.467 | 0.917 |

## Files

- `within_finder_cells.csv`
- `builder_scores.csv`
- `recovery_scores.csv`
- `rank_stability.csv`
- `manifest.json`
