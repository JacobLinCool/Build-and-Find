# Low-Prior Subset Sensitivity

Generated: 2026-05-03T20:04:26.141382+00:00

Questions: scratch_minidb:q_db_01, scratch_minidb:q_db_05, scratch_minidb:q_db_06, scratch_minidb:q_db_07, scratch_minidb:q_db_15, scratch_nanoweb:q_nw_01, scratch_nanoweb:q_nw_04, scratch_nanoweb:q_nw_07, scratch_nanoweb:q_nw_10, scratch_nanoweb:q_nw_11

Accuracy and all-correct gates are computed only on the low-prior questions.
Effort metrics remain full find-run costs because the trace has no per-question cost attribution.

## Builder R_b

### high_effort

| rank | builder | R_total_bytes | n | mean subset acc |
| ---: | --- | ---: | ---: | ---: |
| 1 | codex_gpt5_5_high | 1.033 | 12/12 | 0.975 |
| 2 | codex_gpt5_4_mini_high | 1.310 | 12/12 | 0.978 |
| 3 | claude_code_opus_4_7_high | 1.509 | 12/12 | 0.997 |
| 4 | claude_code_sonnet_4_6_high | 1.566 | 12/12 | 0.983 |
| 5 | claude_code_mimo_v2_5_high | 1.600 | 9/12 | 0.896 |
| 6 | claude_code_mimo_v2_5_pro_high | 1.873 | 5/12 | 0.907 |

### low_effort

| rank | builder | R_total_bytes | n | mean subset acc |
| ---: | --- | ---: | ---: | ---: |
| 1 | codex_gpt5_4_mini_low | 1.122 | 12/12 | 0.986 |
| 2 | codex_gpt5_5_low | 1.166 | 12/12 | 0.969 |
| 3 | claude_code_opus_4_7_low | 1.763 | 12/12 | 0.981 |
| 4 | claude_code_mimo_v2_5_pro_low | 1.841 | 6/12 | 1.000 |
| 5 | claude_code_sonnet_4_6_low | 1.891 | 12/12 | 0.936 |
| 6 | claude_code_mimo_v2_5_low | 1.938 | 11/12 | 0.939 |

## Recovery Diagnostic

### high_effort

| rank | builder | recovery | n | mean subset acc | mean retrieval bytes |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_5_high | 0.962 | 12/12 | 0.975 | 101860 |
| 2 | codex_gpt5_4_mini_high | 0.763 | 12/12 | 0.978 | 129311 |
| 3 | claude_code_opus_4_7_high | 0.731 | 12/12 | 0.997 | 157592 |
| 4 | claude_code_sonnet_4_6_high | 0.653 | 12/12 | 0.983 | 164961 |
| 5 | claude_code_mimo_v2_5_pro_high | 0.357 | 6/12 | 0.889 | 225722 |
| 6 | claude_code_mimo_v2_5_high | 0.270 | 12/12 | 0.842 | 163528 |

### low_effort

| rank | builder | recovery | n | mean subset acc | mean retrieval bytes |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | codex_gpt5_4_mini_low | 0.882 | 12/12 | 0.986 | 64660 |
| 2 | codex_gpt5_5_low | 0.767 | 12/12 | 0.969 | 73070 |
| 3 | claude_code_mimo_v2_5_pro_low | 0.551 | 6/12 | 1.000 | 106195 |
| 4 | claude_code_opus_4_7_low | 0.534 | 12/12 | 0.981 | 117136 |
| 5 | claude_code_sonnet_4_6_low | 0.380 | 12/12 | 0.936 | 124080 |
| 6 | claude_code_mimo_v2_5_low | 0.345 | 12/12 | 0.928 | 123450 |

## Rank Stability

| panel | task | slice | median tau | positive fraction |
| --- | --- | --- | ---: | ---: |
| high_effort | scratch_minidb | all_pairs | 0.600 | 0.733 |
| high_effort | scratch_minidb | same_family_pairs | 0.800 | 1.000 |
| high_effort | scratch_minidb | cross_family_pairs | 0.533 | 0.667 |
| high_effort | scratch_nanoweb | all_pairs | 0.600 | 1.000 |
| high_effort | scratch_nanoweb | same_family_pairs | 0.600 | 1.000 |
| high_effort | scratch_nanoweb | cross_family_pairs | 0.600 | 1.000 |
| low_effort | scratch_minidb | all_pairs | 0.600 | 0.800 |
| low_effort | scratch_minidb | same_family_pairs | 0.600 | 1.000 |
| low_effort | scratch_minidb | cross_family_pairs | 0.600 | 0.750 |
| low_effort | scratch_nanoweb | all_pairs | 0.600 | 0.867 |
| low_effort | scratch_nanoweb | same_family_pairs | 0.800 | 0.667 |
| low_effort | scratch_nanoweb | cross_family_pairs | 0.600 | 0.917 |

## Family Affinity Residual

| builder family | OpenAI/Codex finder | Anthropic/Claude finder | MiMo finder |
| --- | ---: | ---: | ---: |
| OpenAI/Codex | 0.079 | -0.088 | 0.009 |
| Anthropic/Claude | -0.017 | 0.033 | -0.017 |
| MiMo | -0.062 | 0.054 | 0.008 |

