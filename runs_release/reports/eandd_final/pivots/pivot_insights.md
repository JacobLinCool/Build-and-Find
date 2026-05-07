# E&D Final Pivot Analysis

Generated: 2026-05-03T20:04:26.141382+00:00

## View Sets

- Top 4 builders: claude_code_opus_4_7_high, claude_code_opus_4_7_low, claude_code_mimo_v2_5_pro_low, codex_gpt5_4_mini_high
- Top 6 builders: claude_code_opus_4_7_high, claude_code_opus_4_7_low, claude_code_mimo_v2_5_pro_low, codex_gpt5_4_mini_high, codex_gpt5_5_high, codex_gpt5_5_low
- Top 4 finders: claude_code_sonnet_4_6_low, claude_code_sonnet_4_6_high, claude_code_opus_4_7_low, claude_code_opus_4_7_high
- Top 6 finders: claude_code_sonnet_4_6_low, claude_code_sonnet_4_6_high, claude_code_opus_4_7_low, claude_code_opus_4_7_high, claude_code_mimo_v2_5_high, claude_code_mimo_v2_5_low
- Positive-lift finders: claude_code_sonnet_4_6_low, claude_code_sonnet_4_6_high, claude_code_mimo_v2_5_high, claude_code_mimo_v2_5_low, claude_code_mimo_v2_5_pro_low, claude_code_mimo_v2_5_pro_high
- Implementation-signal questions: 20
- Low-prior questions: 10
- Hard formal questions: 2

## Builder Robustness

### builder: all_finders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_opus_4_7_high | 0.994 | 2160 |
| 2 | claude_code_opus_4_7_low | 0.993 | 2160 |
| 3 | claude_code_mimo_v2_5_pro_low | 0.993 | 540 |
| 4 | codex_gpt5_4_mini_high | 0.992 | 2160 |
| 5 | codex_gpt5_5_high | 0.989 | 2160 |
| 6 | codex_gpt5_5_low | 0.987 | 2160 |

### builder: top4_formal_finders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_mimo_v2_5_pro_low | 1.000 | 180 |
| 2 | claude_code_opus_4_7_high | 0.999 | 720 |
| 3 | claude_code_opus_4_7_low | 0.999 | 720 |
| 4 | codex_gpt5_5_high | 0.997 | 720 |
| 5 | codex_gpt5_4_mini_high | 0.994 | 720 |
| 6 | codex_gpt5_5_low | 0.990 | 720 |

### builder: top6_formal_finders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_mimo_v2_5_pro_low | 1.000 | 270 |
| 2 | claude_code_opus_4_7_high | 0.997 | 1080 |
| 3 | claude_code_opus_4_7_low | 0.997 | 1080 |
| 4 | codex_gpt5_5_high | 0.994 | 1080 |
| 5 | codex_gpt5_4_mini_high | 0.994 | 1080 |
| 6 | codex_gpt5_5_low | 0.987 | 1080 |

### builder: positive_lift_finders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_mimo_v2_5_pro_low | 0.996 | 270 |
| 2 | claude_code_opus_4_7_high | 0.995 | 1080 |
| 3 | claude_code_opus_4_7_low | 0.993 | 1080 |
| 4 | codex_gpt5_4_mini_high | 0.991 | 1080 |
| 5 | codex_gpt5_5_high | 0.986 | 1080 |
| 6 | codex_gpt5_5_low | 0.982 | 1080 |

### builder: cross_family_finders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_opus_4_7_high | 0.992 | 1440 |
| 2 | codex_gpt5_4_mini_high | 0.992 | 1440 |
| 3 | claude_code_mimo_v2_5_pro_low | 0.992 | 360 |
| 4 | claude_code_opus_4_7_low | 0.990 | 1440 |
| 5 | codex_gpt5_5_high | 0.990 | 1440 |
| 6 | codex_gpt5_5_low | 0.985 | 1440 |

### builder: implementation_signal_questions

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_mimo_v2_5_pro_low | 1.000 | 324 |
| 2 | claude_code_sonnet_4_6_low | 0.995 | 1440 |
| 3 | claude_code_opus_4_7_high | 0.994 | 1440 |
| 4 | codex_gpt5_4_mini_low | 0.992 | 1044 |
| 5 | claude_code_sonnet_4_6_high | 0.990 | 1440 |
| 6 | claude_code_opus_4_7_low | 0.989 | 1440 |


## Finder Robustness

### finder: all_builders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_sonnet_4_6_low | 0.987 | 1845 |
| 2 | claude_code_sonnet_4_6_high | 0.980 | 1845 |
| 3 | claude_code_opus_4_7_low | 0.979 | 1845 |
| 4 | claude_code_opus_4_7_high | 0.971 | 1845 |
| 5 | claude_code_mimo_v2_5_high | 0.968 | 1845 |
| 6 | claude_code_mimo_v2_5_low | 0.968 | 1845 |

### finder: top4_builders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_opus_4_7_high | 0.998 | 585 |
| 2 | claude_code_opus_4_7_low | 0.998 | 585 |
| 3 | codex_gpt5_5_high | 0.998 | 585 |
| 4 | claude_code_sonnet_4_6_high | 0.997 | 585 |
| 5 | claude_code_sonnet_4_6_low | 0.997 | 585 |
| 6 | codex_gpt5_5_low | 0.997 | 585 |

### finder: top6_builders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_opus_4_7_low | 0.999 | 945 |
| 2 | codex_gpt5_5_high | 0.997 | 945 |
| 3 | claude_code_opus_4_7_high | 0.996 | 945 |
| 4 | codex_gpt5_5_low | 0.996 | 945 |
| 5 | claude_code_sonnet_4_6_high | 0.995 | 945 |
| 6 | claude_code_sonnet_4_6_low | 0.995 | 945 |

### finder: cross_family_builders

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_sonnet_4_6_low | 0.985 | 1125 |
| 2 | claude_code_mimo_v2_5_high | 0.978 | 1395 |
| 3 | claude_code_opus_4_7_low | 0.978 | 1125 |
| 4 | claude_code_sonnet_4_6_high | 0.977 | 1125 |
| 5 | claude_code_mimo_v2_5_low | 0.975 | 1395 |
| 6 | claude_code_mimo_v2_5_pro_high | 0.973 | 1395 |

### finder: implementation_signal_questions

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_opus_4_7_low | 0.993 | 1233 |
| 2 | claude_code_opus_4_7_high | 0.992 | 1233 |
| 3 | codex_gpt5_5_low | 0.990 | 1233 |
| 4 | claude_code_sonnet_4_6_high | 0.989 | 1233 |
| 5 | claude_code_sonnet_4_6_low | 0.989 | 1233 |
| 6 | codex_gpt5_5_high | 0.985 | 1233 |

### finder: low_prior_questions

| order | agent | accuracy | rows |
| ---: | --- | ---: | ---: |
| 1 | claude_code_opus_4_7_low | 0.974 | 615 |
| 2 | claude_code_opus_4_7_high | 0.969 | 615 |
| 3 | claude_code_sonnet_4_6_high | 0.969 | 615 |
| 4 | claude_code_sonnet_4_6_low | 0.967 | 615 |
| 5 | codex_gpt5_5_low | 0.966 | 615 |
| 6 | codex_gpt5_5_high | 0.958 | 615 |


## Effort Checks

| role | view | base model | high-low |
| --- | --- | --- | ---: |
| builder | all_finders | claude_code_mimo_v2_5 | -0.029 |
| builder | all_finders | claude_code_mimo_v2_5_pro | -0.032 |
| builder | all_finders | claude_code_opus_4_7 | 0.002 |
| builder | all_finders | claude_code_sonnet_4_6 | 0.018 |
| builder | all_finders | codex_gpt5_4_mini | 0.070 |
| builder | all_finders | codex_gpt5_5 | 0.002 |
| builder | cross_family_finders | claude_code_mimo_v2_5 | -0.031 |
| builder | cross_family_finders | claude_code_mimo_v2_5_pro | -0.032 |
| builder | cross_family_finders | claude_code_opus_4_7 | 0.003 |
| builder | cross_family_finders | claude_code_sonnet_4_6 | 0.022 |
| builder | cross_family_finders | codex_gpt5_4_mini | 0.053 |
| builder | cross_family_finders | codex_gpt5_5 | 0.005 |
| builder | implementation_signal_questions | claude_code_mimo_v2_5 | 0.002 |
| builder | implementation_signal_questions | claude_code_mimo_v2_5_pro | -0.019 |
| builder | implementation_signal_questions | claude_code_opus_4_7 | 0.006 |
| builder | implementation_signal_questions | claude_code_sonnet_4_6 | -0.006 |
| builder | implementation_signal_questions | codex_gpt5_4_mini | -0.004 |
| builder | implementation_signal_questions | codex_gpt5_5 | 0.003 |
| builder | top4_formal_finders | claude_code_mimo_v2_5 | -0.025 |
| builder | top4_formal_finders | claude_code_mimo_v2_5_pro | -0.019 |
| builder | top4_formal_finders | claude_code_opus_4_7 | 0.000 |
| builder | top4_formal_finders | claude_code_sonnet_4_6 | 0.011 |
| builder | top4_formal_finders | codex_gpt5_4_mini | 0.056 |
| builder | top4_formal_finders | codex_gpt5_5 | 0.007 |
| finder | all_builders | claude_code_mimo_v2_5 | 0.000 |
| finder | all_builders | claude_code_mimo_v2_5_pro | -0.001 |
| finder | all_builders | claude_code_opus_4_7 | -0.008 |
| finder | all_builders | claude_code_sonnet_4_6 | -0.007 |
| finder | all_builders | codex_gpt5_4_mini | -0.010 |
| finder | all_builders | codex_gpt5_5 | -0.006 |
| finder | cross_family_builders | claude_code_mimo_v2_5 | 0.003 |
| finder | cross_family_builders | claude_code_mimo_v2_5_pro | 0.001 |
| finder | cross_family_builders | claude_code_opus_4_7 | -0.012 |
| finder | cross_family_builders | claude_code_sonnet_4_6 | -0.008 |
| finder | cross_family_builders | codex_gpt5_4_mini | -0.012 |
| finder | cross_family_builders | codex_gpt5_5 | -0.009 |
| finder | implementation_signal_questions | claude_code_mimo_v2_5 | -0.001 |
| finder | implementation_signal_questions | claude_code_mimo_v2_5_pro | 0.002 |
| finder | implementation_signal_questions | claude_code_opus_4_7 | -0.001 |
| finder | implementation_signal_questions | claude_code_sonnet_4_6 | 0.001 |
| finder | implementation_signal_questions | codex_gpt5_4_mini | -0.001 |
| finder | implementation_signal_questions | codex_gpt5_5 | -0.005 |
| finder | top4_builders | claude_code_mimo_v2_5 | 0.000 |
| finder | top4_builders | claude_code_mimo_v2_5_pro | 0.000 |
| finder | top4_builders | claude_code_opus_4_7 | 0.000 |
| finder | top4_builders | claude_code_sonnet_4_6 | 0.000 |
| finder | top4_builders | codex_gpt5_4_mini | 0.002 |
| finder | top4_builders | codex_gpt5_5 | 0.002 |

## Accuracy-Token Diagnostics

Diagnostic only; not the paper's within-finder builder-efficiency metric.

| role | view | frontier agent | accuracy | mean tokens | acc / M tok |
| --- | --- | --- | ---: | ---: | ---: |
| builder | all_finders | claude_code_opus_4_7_high | 0.994 | 6418915 | 0.155 |
| builder | all_finders | claude_code_opus_4_7_low | 0.993 | 4744350 | 0.209 |
| builder | all_finders | codex_gpt5_5_high | 0.989 | 3216598 | 0.307 |
| builder | all_finders | codex_gpt5_5_low | 0.987 | 1921738 | 0.513 |
| builder | implementation_signal_questions | claude_code_mimo_v2_5_pro_low | 1.000 | 7278835 | 0.137 |
| builder | implementation_signal_questions | claude_code_sonnet_4_6_low | 0.995 | 3280870 | 0.303 |
| builder | implementation_signal_questions | codex_gpt5_4_mini_low | 0.992 | 2536506 | 0.391 |
| builder | implementation_signal_questions | codex_gpt5_5_low | 0.983 | 1921738 | 0.511 |
| builder | low_prior_questions | claude_code_mimo_v2_5_pro_low | 1.000 | 7278835 | 0.137 |
| builder | low_prior_questions | codex_gpt5_4_mini_low | 0.993 | 2536506 | 0.391 |
| builder | low_prior_questions | codex_gpt5_5_low | 0.965 | 1921738 | 0.502 |
| builder | top4_formal_finders | claude_code_mimo_v2_5_pro_low | 1.000 | 7278835 | 0.137 |
| builder | top4_formal_finders | claude_code_opus_4_7_low | 0.999 | 4744350 | 0.210 |
| builder | top4_formal_finders | codex_gpt5_5_high | 0.997 | 3216598 | 0.310 |
| builder | top4_formal_finders | codex_gpt5_5_low | 0.990 | 1921738 | 0.515 |
| finder | all_builders | claude_code_sonnet_4_6_low | 0.987 | 116084 | 8.502 |
| finder | implementation_signal_questions | claude_code_opus_4_7_low | 0.993 | 469590 | 2.114 |
| finder | implementation_signal_questions | codex_gpt5_5_low | 0.990 | 389858 | 2.540 |
| finder | implementation_signal_questions | claude_code_sonnet_4_6_high | 0.989 | 133456 | 7.414 |
| finder | implementation_signal_questions | claude_code_sonnet_4_6_low | 0.989 | 116084 | 8.517 |
| finder | low_prior_questions | claude_code_opus_4_7_low | 0.974 | 469590 | 2.074 |
| finder | low_prior_questions | claude_code_sonnet_4_6_high | 0.969 | 133456 | 7.262 |
| finder | low_prior_questions | claude_code_sonnet_4_6_low | 0.967 | 116084 | 8.334 |
| finder | top4_builders | claude_code_opus_4_7_low | 0.998 | 469590 | 2.126 |
| finder | top4_builders | claude_code_sonnet_4_6_low | 0.997 | 116084 | 8.585 |

## Low-Cost Near-Ceiling Points

| role | view | band | agent | accuracy | mean tokens |
| --- | --- | --- | --- | ---: | ---: |
| builder | all_finders | within 0.5pp | claude_code_opus_4_7_low | 0.993 | 4744350 |
| builder | all_finders | within 1.0pp | codex_gpt5_5_low | 0.987 | 1921738 |
| builder | all_finders | within 2.0pp | codex_gpt5_5_low | 0.987 | 1921738 |
| builder | implementation_signal_questions | within 0.5pp | claude_code_sonnet_4_6_low | 0.995 | 3280870 |
| builder | implementation_signal_questions | within 1.0pp | codex_gpt5_4_mini_low | 0.992 | 2536506 |
| builder | implementation_signal_questions | within 2.0pp | codex_gpt5_5_low | 0.983 | 1921738 |
| builder | low_prior_questions | within 0.5pp | claude_code_mimo_v2_5_pro_low | 1.000 | 7278835 |
| builder | low_prior_questions | within 1.0pp | codex_gpt5_4_mini_low | 0.993 | 2536506 |
| builder | low_prior_questions | within 2.0pp | codex_gpt5_4_mini_low | 0.993 | 2536506 |
| builder | top4_formal_finders | within 0.5pp | codex_gpt5_5_high | 0.997 | 3216598 |
| builder | top4_formal_finders | within 1.0pp | codex_gpt5_5_low | 0.990 | 1921738 |
| builder | top4_formal_finders | within 2.0pp | codex_gpt5_5_low | 0.990 | 1921738 |
| finder | all_builders | within 0.5pp | claude_code_sonnet_4_6_low | 0.987 | 116084 |
| finder | all_builders | within 1.0pp | claude_code_sonnet_4_6_low | 0.987 | 116084 |
| finder | all_builders | within 2.0pp | claude_code_sonnet_4_6_low | 0.987 | 116084 |
| finder | implementation_signal_questions | within 0.5pp | claude_code_sonnet_4_6_low | 0.989 | 116084 |
| finder | implementation_signal_questions | within 1.0pp | claude_code_sonnet_4_6_low | 0.989 | 116084 |
| finder | implementation_signal_questions | within 2.0pp | claude_code_sonnet_4_6_low | 0.989 | 116084 |
| finder | low_prior_questions | within 0.5pp | claude_code_sonnet_4_6_high | 0.969 | 133456 |
| finder | low_prior_questions | within 1.0pp | claude_code_sonnet_4_6_low | 0.967 | 116084 |
| finder | low_prior_questions | within 2.0pp | claude_code_sonnet_4_6_low | 0.967 | 116084 |
| finder | top4_builders | within 0.5pp | claude_code_sonnet_4_6_low | 0.997 | 116084 |
| finder | top4_builders | within 1.0pp | claude_code_sonnet_4_6_low | 0.997 | 116084 |
| finder | top4_builders | within 2.0pp | claude_code_sonnet_4_6_low | 0.997 | 116084 |

## Family Pair Matrix

| builder family | finder family | accuracy | rows |
| --- | --- | ---: | ---: |
| claude | claude | 0.984 | 2880 |
| claude | codex | 0.969 | 2880 |
| claude | mimo | 0.974 | 2880 |
| codex | claude | 0.983 | 2700 |
| codex | codex | 0.969 | 2700 |
| codex | mimo | 0.974 | 2700 |
| mimo | claude | 0.967 | 1800 |
| mimo | codex | 0.909 | 1800 |
| mimo | mimo | 0.947 | 1800 |

## Strongest Same-Family Affinity

| role | agent | same-family | different-family | delta |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| finder | codex_gpt5_4_mini_high | 0.959 | 0.932 | 0.027 |
| finder | codex_gpt5_5_high | 0.976 | 0.950 | 0.027 |
| finder | codex_gpt5_4_mini_low | 0.966 | 0.944 | 0.022 |
| builder | claude_code_sonnet_4_6_low | 0.964 | 0.942 | 0.022 |
| finder | codex_gpt5_5_low | 0.976 | 0.959 | 0.017 |
| builder | claude_code_mimo_v2_5_high | 0.925 | 0.910 | 0.015 |
| finder | claude_code_opus_4_7_high | 0.979 | 0.966 | 0.013 |
| builder | claude_code_sonnet_4_6_high | 0.975 | 0.963 | 0.012 |

## Implementation-Signal Questions

| task | question | formal 3-trial agreement | question-only 3-trial agreement | agreement lift | formal acc. | question-only acc. |
| --- | --- | ---: | ---: | ---: |
| scratch_nanoweb | q_nw_07 | 1.000 | 0.667 | 0.333 | 1.000 | 0.778 |
| scratch_nanoweb | q_nw_10 | 0.996 | 0.750 | 0.246 | 0.999 | 0.861 |
| scratch_nanoweb | q_nw_04 | 0.988 | 0.750 | 0.238 | 0.996 | 0.917 |
| scratch_minidb | q_db_01 | 0.881 | 0.667 | 0.214 | 0.954 | 0.889 |
| scratch_nanoweb | q_nw_01 | 1.000 | 0.833 | 0.167 | 1.000 | 0.944 |
| scratch_minidb | q_db_06 | 0.821 | 0.667 | 0.155 | 0.905 | 0.889 |
| scratch_nanoweb | q_nw_11 | 0.900 | 0.750 | 0.150 | 0.950 | 0.889 |
| scratch_minidb | q_db_07 | 0.889 | 0.750 | 0.139 | 0.947 | 0.861 |
| scratch_minidb | q_db_05 | 0.853 | 0.750 | 0.103 | 0.915 | 0.917 |
| scratch_minidb | q_db_03 | 1.000 | 0.917 | 0.083 | 1.000 | 0.972 |
| scratch_minidb | q_db_09 | 1.000 | 0.917 | 0.083 | 1.000 | 0.972 |
| scratch_minidb | q_db_10 | 1.000 | 0.917 | 0.083 | 1.000 | 0.944 |

## Files

- `agent_views.csv`
- `effort_comparisons.csv`
- `agent_token_diagnostics.csv`
- `affinity_by_agent.csv`
- `family_pair_matrix.csv`
- `task_agent_views.csv`
- `question_signal.csv`
- `manifest.json`
