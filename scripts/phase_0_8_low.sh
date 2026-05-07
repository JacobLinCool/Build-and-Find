#!/usr/bin/env bash
# Phase 0.8 (low effort): introduce reasoning_effort=low variants for all
# 6 panel models. Build 12 new artifacts (6 _low builders × 2 scratch
# tasks), then run the find phase across:
#
#   * the 12 new _low artifacts × all 12 finders (6 default + 6 _low)
#   * the 12 existing default artifacts ×  6 _low finders
#
# Totals: 12 new builds + 216 new find runs.
#
# Designed to be re-runnable: every individual step uses --resume-friendly
# CLI calls (bnf records dedupe by (run_id_inputs)), and the script exits
# fail-fast on the first build error so wiring problems surface early.

set -u
cd "$(dirname "$0")/.."

LOG_DIR="${LOG_DIR:-$PWD/runs/phase_0_8_low_logs}"
mkdir -p "$LOG_DIR"

LOW_BUILDERS=(
  claude_code_opus_4_7_low
  claude_code_opus_4_6_low
  claude_code_sonnet_4_6_low
  codex_gpt5_4_low
  codex_gpt5_4_mini_low
  codex_gpt5_5_low
)

DEFAULT_FINDERS=(
  claude_code_opus_4_7
  claude_code_opus_4_6
  claude_code_sonnet_4_6
  codex_gpt5_4
  codex_gpt5_4_mini
  codex_gpt5_5
)

LOW_FINDERS=(
  claude_code_opus_4_7_low
  claude_code_opus_4_6_low
  claude_code_sonnet_4_6_low
  codex_gpt5_4_low
  codex_gpt5_4_mini_low
  codex_gpt5_5_low
)

ALL_FINDERS=("${DEFAULT_FINDERS[@]}" "${LOW_FINDERS[@]}")

TASKS=(scratch_minidb scratch_nanoweb)

# 12 existing default-effort scratch artifacts (6 builders × 2 tasks).
OLD_ARTS=(
  build-scratch_minidb-claude_code_opus_4_7-t0-s0-67224746
  build-scratch_minidb-claude_code_opus_4_6-t0-s0-47ed9e92
  build-scratch_minidb-claude_code_sonnet_4_6-t0-s0-d355f8a8
  build-scratch_minidb-codex_gpt5_4-t0-s0-cf68b335
  build-scratch_minidb-codex_gpt5_4_mini-t0-s0-e5fc4d08
  build-scratch_minidb-codex_gpt5_5-t0-s0-2ddde80d
  build-scratch_nanoweb-claude_code_opus_4_7-t0-s0-ddb1e889
  build-scratch_nanoweb-claude_code_opus_4_6-t0-s0-bc00de08
  build-scratch_nanoweb-claude_code_sonnet_4_6-t0-s0-42d64026
  build-scratch_nanoweb-codex_gpt5_4-t0-s0-ee601560
  build-scratch_nanoweb-codex_gpt5_4_mini-t0-s0-a79a338b
  build-scratch_nanoweb-codex_gpt5_5-t0-s0-a1500c8e
)

START_TS=$(date +%s)

# ------------------------------------------------------------------
# Phase 1: builds (6 builders × 2 tasks = 12 runs)
# ------------------------------------------------------------------
echo "=== Phase 1: builds (12 runs) ==="
for builder in "${LOW_BUILDERS[@]}"; do
  for task in "${TASKS[@]}"; do
    NOW=$(date +%Y-%m-%dT%H:%M:%S)
    LOG="$LOG_DIR/build__${task}__${builder}.log"
    echo "$NOW BUILD START $task :: $builder (log: $LOG)"
    if uv run bnf build run --task "$task" --builder "$builder" --trials 1 --seed 0 >"$LOG" 2>&1; then
      echo "$(date +%Y-%m-%dT%H:%M:%S) BUILD OK $task :: $builder"
      tail -2 "$LOG" | sed 's/^/    /'
    else
      echo "$(date +%Y-%m-%dT%H:%M:%S) BUILD FAIL $task :: $builder"
      tail -10 "$LOG" | sed 's/^/    /'
      exit 1
    fi
  done
done

# Discover the 12 new build artifact ids by matching the records dir.
NEW_ARTS=()
for builder in "${LOW_BUILDERS[@]}"; do
  for task in "${TASKS[@]}"; do
    match=$(ls "$PWD/runs/records/" | grep -E "^build-${task}-${builder}-t0-s0-.*\.json$" | head -1 | sed 's/\.json$//')
    if [[ -z "$match" ]]; then
      echo "ERROR: could not find new build artifact for $task :: $builder"
      exit 1
    fi
    NEW_ARTS+=("$match")
    echo "  discovered new artifact: $match"
  done
done

# ------------------------------------------------------------------
# Phase 2a: all 12 finders on the 12 new _low artifacts (144 runs)
# ------------------------------------------------------------------
TOTAL=$(( ${#NEW_ARTS[@]} * ${#ALL_FINDERS[@]} + ${#OLD_ARTS[@]} * ${#LOW_FINDERS[@]} ))
IDX=0
echo "=== Phase 2a: 12 finders × 12 new artifacts (144 runs; total $TOTAL) ==="
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
# Phase 2b: 6 _low finders × 12 existing default artifacts (72 runs)
# ------------------------------------------------------------------
echo "=== Phase 2b: 6 _low finders × 12 existing artifacts (72 runs) ==="
for art in "${OLD_ARTS[@]}"; do
  for finder in "${LOW_FINDERS[@]}"; do
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

FINAL_ELAPSED=$(( $(date +%s) - START_TS ))
echo "DONE total_elapsed=${FINAL_ELAPSED}s"
