#!/usr/bin/env bash
# Phase 0.9: add claude_code_mimo_v2_5_pro (Xiaomi MiMo via Anthropic-
# compatible API) to the default-effort panel. Plan A from the design
# discussion:
#
#   * Phase 1: 2 mimo builds (scratch_minidb + scratch_nanoweb, 1 trial each)
#   * Phase 2a: 7 finders (6 existing default-effort + mimo) × 2 mimo
#               artifacts × 3 trials = 42 find runs
#   * Phase 2b: mimo finder × 12 existing default-effort scratch artifacts
#               × 3 trials = 36 find runs
#
# Totals: 2 new builds + 78 new find runs = 80 runs.

set -u
cd "$(dirname "$0")/.."

LOG_DIR="${LOG_DIR:-$PWD/runs/phase_0_9_logs}"
mkdir -p "$LOG_DIR"

BUILDER="claude_code_mimo_v2_5_pro"
TASKS=(scratch_minidb scratch_nanoweb)

ALL_FINDERS=(
  claude_code_opus_4_7
  claude_code_opus_4_6
  claude_code_sonnet_4_6
  codex_gpt5_4
  codex_gpt5_4_mini
  codex_gpt5_5
  claude_code_mimo_v2_5_pro
)

# 12 existing default-effort scratch artifacts
# (6 default-effort models × 2 scratch tasks).
OLD_ARTS=(
  build-scratch_minidb-claude_code_opus_4_7-t0-s0-67224746
  build-scratch_nanoweb-claude_code_opus_4_7-t0-s0-ddb1e889
  build-scratch_minidb-claude_code_opus_4_6-t0-s0-47ed9e92
  build-scratch_nanoweb-claude_code_opus_4_6-t0-s0-bc00de08
  build-scratch_minidb-claude_code_sonnet_4_6-t0-s0-d355f8a8
  build-scratch_nanoweb-claude_code_sonnet_4_6-t0-s0-42d64026
  build-scratch_minidb-codex_gpt5_4-t0-s0-cf68b335
  build-scratch_nanoweb-codex_gpt5_4-t0-s0-ee601560
  build-scratch_minidb-codex_gpt5_4_mini-t0-s0-e5fc4d08
  build-scratch_nanoweb-codex_gpt5_4_mini-t0-s0-a79a338b
  build-scratch_minidb-codex_gpt5_5-t0-s0-2ddde80d
  build-scratch_nanoweb-codex_gpt5_5-t0-s0-a1500c8e
)

START_TS=$(date +%s)

# ------------------------------------------------------------------
# Phase 1: builds (2 runs)
# ------------------------------------------------------------------
for task in "${TASKS[@]}"; do
  NOW=$(date +%Y-%m-%dT%H:%M:%S)
  LOG="$LOG_DIR/build__${task}__${BUILDER}.log"
  echo "$NOW BUILD START $task :: $BUILDER (log: $LOG)"
  if uv run bnf build run --task "$task" --builder "$BUILDER" --trials 1 --seed 0 >"$LOG" 2>&1; then
    echo "$(date +%Y-%m-%dT%H:%M:%S) BUILD OK $task :: $BUILDER"
    tail -2 "$LOG" | sed 's/^/    /'
  else
    echo "$(date +%Y-%m-%dT%H:%M:%S) BUILD FAIL $task :: $BUILDER"
    tail -10 "$LOG" | sed 's/^/    /'
    exit 1
  fi
done

# Discover new build_run_ids.
NEW_ARTS=()
for task in "${TASKS[@]}"; do
  match=$(ls "$PWD/runs/records/" | grep -E "^build-${task}-${BUILDER}-t0-s0-.*\.json$" | head -1 | sed 's/\.json$//')
  if [[ -z "$match" ]]; then
    echo "ERROR: could not find new build artifact for $task"
    exit 1
  fi
  NEW_ARTS+=("$match")
  echo "  discovered new artifact: $match"
done

TOTAL=$(( ${#NEW_ARTS[@]} * ${#ALL_FINDERS[@]} + ${#OLD_ARTS[@]} ))
IDX=0

# ------------------------------------------------------------------
# Phase 2a: all 7 finders on the 2 new mimo artifacts (42 runs)
# ------------------------------------------------------------------
for art in "${NEW_ARTS[@]}"; do
  for finder in "${ALL_FINDERS[@]}"; do
    IDX=$(( IDX + 1 ))
    NOW=$(date +%Y-%m-%dT%H:%M:%S)
    ELAPSED=$(( $(date +%s) - START_TS ))
    LOG="$LOG_DIR/find__${art}__${finder}.log"
    echo "$NOW [$IDX/$TOTAL elapsed=${ELAPSED}s] FIND START $art :: $finder"
    if uv run bnf find run --codebase "$art" --finder "$finder" --trials 3 --seed 0 >"$LOG" 2>&1; then
      echo "$(date +%Y-%m-%dT%H:%M:%S) [$IDX/$TOTAL] FIND OK $art :: $finder"
      tail -3 "$LOG" | sed 's/^/    /'
    else
      echo "$(date +%Y-%m-%dT%H:%M:%S) [$IDX/$TOTAL] FIND FAIL $art :: $finder"
      tail -10 "$LOG" | sed 's/^/    /'
    fi
  done
done

# ------------------------------------------------------------------
# Phase 2b: mimo finder on the 12 existing default-effort artifacts (36 runs)
# ------------------------------------------------------------------
for art in "${OLD_ARTS[@]}"; do
  IDX=$(( IDX + 1 ))
  NOW=$(date +%Y-%m-%dT%H:%M:%S)
  ELAPSED=$(( $(date +%s) - START_TS ))
  LOG="$LOG_DIR/find__${art}__${BUILDER}.log"
  echo "$NOW [$IDX/$TOTAL elapsed=${ELAPSED}s] FIND START $art :: $BUILDER"
  if uv run bnf find run --codebase "$art" --finder "$BUILDER" --trials 3 --seed 0 >"$LOG" 2>&1; then
    echo "$(date +%Y-%m-%dT%H:%M:%S) [$IDX/$TOTAL] FIND OK $art :: $BUILDER"
    tail -3 "$LOG" | sed 's/^/    /'
  else
    echo "$(date +%Y-%m-%dT%H:%M:%S) [$IDX/$TOTAL] FIND FAIL $art :: $BUILDER"
    tail -10 "$LOG" | sed 's/^/    /'
  fi
done

FINAL_ELAPSED=$(( $(date +%s) - START_TS ))
echo "DONE total_elapsed=${FINAL_ELAPSED}s"
