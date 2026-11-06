# Core Finder Selection

- Selection id: `eandd_core_finders_v1`
- Script: `scripts/select_eandd_core_finders.py`
- Default core finders: none
- Prior-stress audit finders: `claude_code_mimo_v2_5_pro_low`

## Interpretation

The default core gates select 0 of 12 candidates.

Spec-only controls are also saturated. The selector records that fact as a validity limitation and uses finder-specific question-only lift, low-prior lift, and implementation-signal lift as the prior controls that can still discriminate finder behavior.

## Selected Finders

| role | finder | formal | q-only lift | low-prior lift | impl.-signal lift | mean tokens | tau | affinity |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| prior_stress_audit | `claude_code_mimo_v2_5_pro_low` | 96.7% | 32.3% | 37.2% | 37.6% | 412001 | 0.600 | -1.9% |

## Non-Selected Summary

| finder | formal | q-only lift | mean tokens | tau | main reason |
| --- | ---: | ---: | ---: | ---: | --- |
| `claude_code_sonnet_4_6_low` | 98.7% | 0.9% | 670960 | 0.613 | mean tokens > 150000 |
| `claude_code_sonnet_4_6_high` | 98.0% | 2.5% | 729460 | 0.493 | mean tokens > 150000 |
| `claude_code_opus_4_7_low` | 97.9% | -2.1% | 521407 | 0.107 | mean tokens > 150000; formal <= question-only; low-prior formal <= question-only; implementation-signal formal <= question-only; rank-stability mean tau < 0.40 |
| `claude_code_opus_4_7_high` | 97.1% | -2.9% | 912645 | 0.293 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000; formal <= question-only; low-prior formal <= question-only; implementation-signal formal <= question-only; rank-stability mean tau < 0.40 |
| `claude_code_mimo_v2_5_low` | 96.8% | 1.2% | 397627 | 0.600 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000 |
| `claude_code_mimo_v2_5_high` | 96.8% | 3.5% | 404049 | 0.547 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000 |
| `claude_code_mimo_v2_5_pro_high` | 96.7% | 6.7% | 413189 | 0.667 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000 |
| `codex_gpt5_5_low` | 96.5% | -3.5% | 389499 | 0.573 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000; formal <= question-only; low-prior formal <= question-only; implementation-signal formal <= question-only |
| `codex_gpt5_5_high` | 95.9% | -4.1% | 546827 | 0.693 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000; formal <= question-only; low-prior formal <= question-only; implementation-signal formal <= question-only |
| `codex_gpt5_4_mini_low` | 95.2% | -2.6% | 329206 | 0.573 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000; formal <= question-only; low-prior formal <= question-only; implementation-signal formal <= question-only |
| `codex_gpt5_4_mini_high` | 94.1% | -5.9% | 638067 | 0.693 | formal CI low < 0.970; formal CI width > 0.015; mean tokens > 150000; formal <= question-only; low-prior formal <= question-only; implementation-signal formal <= question-only |
