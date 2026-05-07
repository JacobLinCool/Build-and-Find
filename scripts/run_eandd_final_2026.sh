#!/usr/bin/env bash
set -euo pipefail

TASKS="scratch_minidb,scratch_nanoweb"
SEED="${BNF_FINAL_SEED:-20260501}"
BUILD_WORKERS="${BNF_BUILD_WORKERS:-1}"
FIND_WORKERS="${BNF_FIND_WORKERS:-1}"

AGENTS="claude_code_opus_4_7_high,claude_code_opus_4_7_low,claude_code_sonnet_4_6_high,claude_code_sonnet_4_6_low,codex_gpt5_5_high,codex_gpt5_5_low,codex_gpt5_4_mini_high,codex_gpt5_4_mini_low,claude_code_mimo_v2_5_pro_high,claude_code_mimo_v2_5_pro_low,claude_code_mimo_v2_5_high,claude_code_mimo_v2_5_low"

uv run bnf run all \
  --tasks "$TASKS" \
  --builders "$AGENTS" \
  --finders "$AGENTS" \
  --build-trials 2 \
  --find-trials 3 \
  --seed "$SEED" \
  --build-workers "$BUILD_WORKERS" \
  --find-workers "$FIND_WORKERS"

if [[ "${RUN_CONTROLS:-1}" == "1" ]]; then
  uv run python scripts/run_controls.py \
    --tasks "$TASKS" \
    --finders "$AGENTS" \
    --trials 3 \
    --seed "$SEED" \
    --conditions spec_only,question_only \
    --workers "$FIND_WORKERS"
fi

uv run bnf analyze tables
uv run python scripts/analyze_eandd_final.py
uv run python scripts/analyze_eandd_pivots.py
uv run python scripts/analyze_eandd_within_finder_efficiency.py
uv run python scripts/write_claim_evidence.py
