#!/usr/bin/env bash
# Phase 0.5: run trials 1 and 2 of find phase on all 8 existing scratch artifacts
# across all 4 pilot finders. Resume skips the existing trial 0 records.
#
# Output: 64 new find runs (32 artifact-finder pairs × 2 new trials).

set -u
cd "$(dirname "$0")/.."

LOG_DIR="${LOG_DIR:-$PWD/runs/phase_0_5_logs}"
mkdir -p "$LOG_DIR"

ARTIFACTS=(
  build-scratch_minidb-claude_code_opus_4_7-t0-s0-67224746
  build-scratch_minidb-claude_code_sonnet_4_6-t0-s0-d355f8a8
  build-scratch_minidb-codex_gpt5_4-t0-s0-cf68b335
  build-scratch_minidb-codex_gpt5_4_mini-t0-s0-e5fc4d08
  build-scratch_nanoweb-claude_code_opus_4_7-t0-s0-ddb1e889
  build-scratch_nanoweb-claude_code_sonnet_4_6-t0-s0-42d64026
  build-scratch_nanoweb-codex_gpt5_4-t0-s0-ee601560
  build-scratch_nanoweb-codex_gpt5_4_mini-t0-s0-a79a338b
)

FINDERS=(
  claude_code_opus_4_7
  claude_code_sonnet_4_6
  codex_gpt5_4
  codex_gpt5_4_mini
)

TOTAL=$(( ${#ARTIFACTS[@]} * ${#FINDERS[@]} ))
IDX=0
START_TS=$(date +%s)

for art in "${ARTIFACTS[@]}"; do
  for finder in "${FINDERS[@]}"; do
    IDX=$(( IDX + 1 ))
    NOW=$(date +%Y-%m-%dT%H:%M:%S)
    ELAPSED=$(( $(date +%s) - START_TS ))
    TAG="[$IDX/$TOTAL elapsed=${ELAPSED}s]"
    LOG_FILE="$LOG_DIR/${art}__${finder}.log"
    echo "$NOW $TAG START $art :: $finder (log: $LOG_FILE)"
    if uv run bnf find run --codebase "$art" --finder "$finder" --trials 3 --seed 0 >"$LOG_FILE" 2>&1; then
      END=$(date +%Y-%m-%dT%H:%M:%S)
      echo "$END $TAG OK $art :: $finder"
      tail -4 "$LOG_FILE" | sed 's/^/    /'
    else
      END=$(date +%Y-%m-%dT%H:%M:%S)
      echo "$END $TAG FAIL $art :: $finder"
      tail -10 "$LOG_FILE" | sed 's/^/    /'
    fi
  done
done

FINAL_ELAPSED=$(( $(date +%s) - START_TS ))
echo "DONE total_elapsed=${FINAL_ELAPSED}s"
