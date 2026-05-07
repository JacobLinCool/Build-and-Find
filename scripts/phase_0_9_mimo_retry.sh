#!/usr/bin/env bash
# Phase 0.9 retry: re-run the 7 finder invocations × scratch_nanoweb mimo
# artifact that fast-failed during the main batch because the build record
# carried status='timeout'. The build record has since been patched to
# status='ok' (the artifact actually passed compile + has 28 rust files /
# 3290 loc; the agent simply ran out of wall-clock at 5400s after the
# build was already valid).

set -u
cd "$(dirname "$0")/.."

LOG_DIR="${LOG_DIR:-$PWD/runs/phase_0_9_logs}"
mkdir -p "$LOG_DIR"

ART="build-scratch_nanoweb-claude_code_mimo_v2_5_pro-t0-s0-93070dad"
FINDERS=(
  claude_code_opus_4_7
  claude_code_opus_4_6
  claude_code_sonnet_4_6
  codex_gpt5_4
  codex_gpt5_4_mini
  codex_gpt5_5
  claude_code_mimo_v2_5_pro
)

START_TS=$(date +%s)
TOTAL=${#FINDERS[@]}
IDX=0

for finder in "${FINDERS[@]}"; do
  IDX=$(( IDX + 1 ))
  NOW=$(date +%Y-%m-%dT%H:%M:%S)
  ELAPSED=$(( $(date +%s) - START_TS ))
  LOG="$LOG_DIR/find__${ART}__${finder}.log"
  echo "$NOW [$IDX/$TOTAL elapsed=${ELAPSED}s] FIND START $ART :: $finder"
  if uv run bnf find run --codebase "$ART" --finder "$finder" --trials 3 --seed 0 >"$LOG" 2>&1; then
    echo "$(date +%Y-%m-%dT%H:%M:%S) [$IDX/$TOTAL] FIND OK $ART :: $finder"
    tail -3 "$LOG" | sed 's/^/    /'
  else
    echo "$(date +%Y-%m-%dT%H:%M:%S) [$IDX/$TOTAL] FIND FAIL $ART :: $finder"
    tail -10 "$LOG" | sed 's/^/    /'
  fi
done

FINAL_ELAPSED=$(( $(date +%s) - START_TS ))
echo "RETRY DONE total_elapsed=${FINAL_ELAPSED}s"
