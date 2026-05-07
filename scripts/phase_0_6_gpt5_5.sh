#!/usr/bin/env bash
# Phase 0.6: add codex_gpt5_5 to the panel. Build 2 new artifacts
# (scratch_minidb + scratch_nanoweb), then find-phase across all 5 finders
# on the new artifacts, and the new gpt5_5 finder on the 8 old artifacts.
#
# Totals: 2 new builds + 54 new find runs.
#
# Run results feed back into the final paper analysis scripts.

set -u
cd "$(dirname "$0")/.."

LOG_DIR="${LOG_DIR:-$PWD/runs/phase_0_6_logs}"
mkdir -p "$LOG_DIR"

BUILDER="codex_gpt5_5"
TASKS=(scratch_minidb scratch_nanoweb)

ALL_FINDERS=(
  claude_code_opus_4_7
  claude_code_sonnet_4_6
  codex_gpt5_4
  codex_gpt5_4_mini
  codex_gpt5_5
)

OLD_ARTS=(
  build-scratch_minidb-claude_code_opus_4_7-t0-s0-67224746
  build-scratch_minidb-claude_code_sonnet_4_6-t0-s0-d355f8a8
  build-scratch_minidb-codex_gpt5_4-t0-s0-cf68b335
  build-scratch_minidb-codex_gpt5_4_mini-t0-s0-e5fc4d08
  build-scratch_nanoweb-claude_code_opus_4_7-t0-s0-ddb1e889
  build-scratch_nanoweb-claude_code_sonnet_4_6-t0-s0-42d64026
  build-scratch_nanoweb-codex_gpt5_4-t0-s0-ee601560
  build-scratch_nanoweb-codex_gpt5_4_mini-t0-s0-a79a338b
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

# Discover the newly created build_run_ids.
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

# ------------------------------------------------------------------
# Phase 2a: all finders on new artifacts
# ------------------------------------------------------------------
TOTAL=$(( ${#NEW_ARTS[@]} * ${#ALL_FINDERS[@]} + ${#OLD_ARTS[@]} ))
IDX=0
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
# Phase 2b: gpt5_5 finder on existing artifacts
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
