#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRY_ROOT="${BNF_DRY_ROOT:-$(mktemp -d "${TMPDIR:-/tmp}/bnf-eandd-dry.XXXXXX")}"
TASKS="scratch_minidb,scratch_nanoweb"
SEED="${BNF_FINAL_SEED:-20260501}"
BUILD_WORKERS="${BNF_BUILD_WORKERS:-2}"
FIND_WORKERS="${BNF_FIND_WORKERS:-4}"
AGENTS="claude_code_opus_4_7_high,claude_code_opus_4_7_low,claude_code_sonnet_4_6_high,claude_code_sonnet_4_6_low,codex_gpt5_5_high,codex_gpt5_5_low,codex_gpt5_4_mini_high,codex_gpt5_4_mini_low,claude_code_mimo_v2_5_pro_high,claude_code_mimo_v2_5_pro_low,claude_code_mimo_v2_5_high,claude_code_mimo_v2_5_low"

mkdir -p "$DRY_ROOT/benchmarks"
rm -rf "$DRY_ROOT/benchmarks/specs" "$DRY_ROOT/benchmarks/questions"
cp -R "$ROOT/benchmarks/specs" "$DRY_ROOT/benchmarks/specs"
cp -R "$ROOT/benchmarks/questions" "$DRY_ROOT/benchmarks/questions"

echo "dry root: $DRY_ROOT"

BNF_ROOT="$DRY_ROOT" BNF_DRY_RUN=1 uv run bnf run all \
  --tasks "$TASKS" \
  --builders "$AGENTS" \
  --finders "$AGENTS" \
  --build-trials 1 \
  --find-trials 1 \
  --seed "$SEED" \
  --build-workers "$BUILD_WORKERS" \
  --find-workers "$FIND_WORKERS" \
  --no-proxy \
  --force

BNF_ROOT="$DRY_ROOT" BNF_DRY_RUN=1 uv run python scripts/run_controls.py \
  --tasks "$TASKS" \
  --finders "$AGENTS" \
  --trials 1 \
  --seed "$SEED" \
  --conditions spec_only,question_only \
  --workers "$FIND_WORKERS" \
  --no-proxy

BNF_ROOT="$DRY_ROOT" uv run bnf analyze tables --out-dir "$DRY_ROOT/runs/tables"
BNF_ROOT="$DRY_ROOT" uv run python scripts/write_claim_evidence.py

echo "dry-run records: $(find "$DRY_ROOT/runs/records" -name '*.json' | wc -l | tr -d ' ')"
echo "dry-run tables: $DRY_ROOT/runs/tables"
echo "dry-run reports: $DRY_ROOT/runs/reports"
