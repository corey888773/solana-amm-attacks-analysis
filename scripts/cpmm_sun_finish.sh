#!/bin/zsh
# Continuation of CPMM Sunday run after fetch-transactions resume.
# Waits for any in-flight historical_cpmm process (the resumed fetch) to exit,
# then runs build-decoded + evaluate-attacks + add_real_feasibility.

set -e
cd /Users/piotrgasiorek/Studia/magisterka
set -a
source .env
set +a

LOG="logs/cpmm_sun_finish_$(date +%Y%m%d_%H%M).log"

{
echo "[$(date)] waiting for in-flight historical_cpmm to finish (fetch-transactions resume)"
while pgrep -f "target/release/historical_cpmm" > /dev/null 2>&1; do
    sleep 60
done
echo "[$(date)] fetch-transactions done, starting build-decoded"

./target/release/historical_cpmm \
    --rpc "$RPC" \
    --results-dir results/cpmm_24h_sunday \
    --cache-root fork/cache/historical_cpmm \
    --tx-cost-per-leg 0 \
    build-decoded

echo "[$(date)] build-decoded done, starting evaluate-attacks"

./target/release/historical_cpmm \
    --rpc "$RPC" \
    --results-dir results/cpmm_24h_sunday \
    --cache-root fork/cache/historical_cpmm \
    --tx-cost-per-leg 0 \
    evaluate-attacks --min-victim 10000

echo "[$(date)] evaluate-attacks done, augmenting with real-feasibility"

python3 scripts/add_real_feasibility.py results/cpmm_24h_sunday/historical_cpmm_candidates.csv

echo "[$(date)] ALL DONE for CPMM Sunday"
} 2>&1 | tee "$LOG"
