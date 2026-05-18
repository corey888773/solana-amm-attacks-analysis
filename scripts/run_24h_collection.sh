#!/bin/zsh
# 24h Raydium CPMM + CLMM data collection wrapper. Two CPMM runs:
#   - Sunday 08:00 (covers Saturday's 24h, backup in case Monday fails)
#   - Monday 08:00 (covers Sunday's 24h, same window as CLMM live-collect)
# Plus one CLMM live-collect (Sunday 08:00 → Monday 08:00, 24h forward).
#
# Sequence:
#   1. Wait until Sun 2026-05-17 08:00 CEST
#   2. CPMM run #1 (saturday data) → results/cpmm_24h_saturday/
#   3. CLMM live-collect 24h forward (background) → results/clmm_24h_sunday/
#   4. Wait for CLMM to finish (~Mon 08:00)
#   5. CLMM build-live-candidates + evaluate
#   6. CPMM run #2 (sunday data, same window as CLMM) → results/cpmm_24h_sunday/
#
# Usage:
#   caffeinate -i nohup ./scripts/run_24h_collection.sh > scripts/wrapper.log 2>&1 &
#   disown
#
# Requirements: Mac on AC power, lid open (clamshell sleep ignores caffeinate),
# stable wifi, Helius RPC URL in .env.

set -e
cd /Users/piotrgasiorek/Studia/magisterka

set -a
source .env
set +a

if [[ -z "$RPC" ]]; then
    echo "FATAL: RPC env var not set (source .env failed?)"
    exit 1
fi

LOG_DIR="logs"
mkdir -p "$LOG_DIR"

CPMM_SAT_DIR="results/cpmm_24h_saturday"
CPMM_SUN_DIR="results/cpmm_24h_sunday"
CLMM_DIR="results/clmm_24h_sunday"
mkdir -p "$CPMM_SAT_DIR" "$CPMM_SUN_DIR" "$CLMM_DIR"

# --- 1. Sleep until Sun 08:00 CEST ---
TARGET_EPOCH=$(date -j -f "%Y-%m-%d %H:%M:%S" "2026-05-17 08:00:00" "+%s")
NOW=$(date +%s)
SLEEP_SECS=$((TARGET_EPOCH - NOW))

echo "[$(date)] wrapper started, target=2026-05-17 08:00 CEST, sleeping ${SLEEP_SECS}s"

if (( SLEEP_SECS > 0 )); then
    sleep "$SLEEP_SECS"
fi

echo "[$(date)] woke up at target time"

# --- 2. CLMM live-collect 24h forward (BACKGROUND, long-running) ---
CLMM_LOG="$LOG_DIR/clmm_24h_$(date +%Y%m%d_%H%M).log"
echo "[$(date)] starting CLMM live-collect (24h, bg) -> $CLMM_LOG, results -> $CLMM_DIR"
./target/release/historical_clmm \
    --rpc "$RPC" \
    --results-dir "$CLMM_DIR" \
    --tx-cost-per-leg 0 \
    live-collect \
    --duration-seconds 86400 \
    --interval-seconds 10 \
    --poll-limit 50 \
    > "$CLMM_LOG" 2>&1 &
CLMM_PID=$!
echo "[$(date)] CLMM PID=$CLMM_PID"

# --- 3. CPMM run #1: Saturday data (backup) ---
CPMM_SAT_LOG="$LOG_DIR/cpmm_24h_saturday_$(date +%Y%m%d_%H%M).log"
echo "[$(date)] CPMM run #1 (saturday data) -> $CPMM_SAT_LOG, results -> $CPMM_SAT_DIR"
./target/release/historical_cpmm \
    --rpc "$RPC" \
    --results-dir "$CPMM_SAT_DIR" \
    --cache-root "fork/cache/historical_cpmm" \
    --since-hours-ago 24 \
    --limit-per-pool 50000 \
    --tx-cost-per-leg 0 \
    run-all \
    --min-victim 10000 \
    > "$CPMM_SAT_LOG" 2>&1
CPMM_SAT_EXIT=$?
echo "[$(date)] CPMM run #1 done (exit $CPMM_SAT_EXIT)"

# Move cache aside so run #2 starts clean
if [[ -d "fork/cache/historical_cpmm" ]]; then
    mv "fork/cache/historical_cpmm" "fork/cache/historical_cpmm_saturday_$(date +%Y%m%d_%H%M)"
fi

# --- 4. Wait for CLMM to finish (Mon 08:00) ---
echo "[$(date)] waiting for CLMM (PID=$CLMM_PID) to finish 24h collection..."
wait "$CLMM_PID"
CLMM_EXIT=$?
echo "[$(date)] CLMM live-collect done (exit $CLMM_EXIT)"

# --- 5. CLMM post-process ---
echo "[$(date)] CLMM build-live-candidates"
./target/release/historical_clmm \
    --results-dir "$CLMM_DIR" \
    --tx-cost-per-leg 0 \
    build-live-candidates \
    >> "$CLMM_LOG" 2>&1

echo "[$(date)] CLMM evaluate-live-attacks"
./target/release/historical_clmm \
    --results-dir "$CLMM_DIR" \
    --tx-cost-per-leg 0 \
    evaluate-live-attacks \
    --max-steps 200 \
    --replay-tolerance-bps 100 \
    --min-victim 10000 \
    >> "$CLMM_LOG" 2>&1

# --- 6. CPMM run #2: Sunday data (same window as CLMM) ---
CPMM_SUN_LOG="$LOG_DIR/cpmm_24h_sunday_$(date +%Y%m%d_%H%M).log"
echo "[$(date)] CPMM run #2 (sunday data, same window as CLMM) -> $CPMM_SUN_LOG"
./target/release/historical_cpmm \
    --rpc "$RPC" \
    --results-dir "$CPMM_SUN_DIR" \
    --cache-root "fork/cache/historical_cpmm" \
    --since-hours-ago 24 \
    --limit-per-pool 50000 \
    --tx-cost-per-leg 0 \
    run-all \
    --min-victim 10000 \
    > "$CPMM_SUN_LOG" 2>&1
CPMM_SUN_EXIT=$?
echo "[$(date)] CPMM run #2 done (exit $CPMM_SUN_EXIT)"

echo "[$(date)] ALL DONE."
echo "  CPMM saturday data: $CPMM_SAT_DIR/  log: $CPMM_SAT_LOG  exit=$CPMM_SAT_EXIT"
echo "  CPMM sunday data:   $CPMM_SUN_DIR/  log: $CPMM_SUN_LOG  exit=$CPMM_SUN_EXIT"
echo "  CLMM sunday data:   $CLMM_DIR/      log: $CLMM_LOG      exit=$CLMM_EXIT"
