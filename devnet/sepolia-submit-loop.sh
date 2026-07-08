#!/usr/bin/env bash
# Submit heartbeats to the HeartbeatManager.
# Emits COUNT heartbeats, one every INTERVAL seconds, then exits.
# Set COUNT=0 for an unbounded soak (the original P1.M5 4-hourly driver: COUNT=0 INTERVAL=14400).
# Reads devnet/sepolia.env + sepolia.wallets.env + sepolia-deployment.env.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
set -a; source "$HERE/sepolia.env"; source "$HERE/sepolia.wallets.env"; source "$HERE/sepolia-deployment.env"; set +a
COUNT="${COUNT:-5}"          # number of heartbeats to send (0 = unbounded)
INTERVAL="${INTERVAL:-60}"   # seconds between heartbeats
RECEIPTS="${RECEIPTS:-}"     # optional file: append '<tx> <gasUsed> <effectiveGasPrice>' per submit
# submitHeartbeat is authorization-gated; override the sending key with an
# authorized one (e.g. the deployer) via SUBMITTER_KEY without touching the
# wallets file. Falls back to submitter_KEY from sepolia.wallets.env.
submitter_KEY="${SUBMITTER_KEY:-$submitter_KEY}"
RAW=0x$(xxd -p "$HERE/../data/valid_htx.json" | tr -d '\n')

label=$([ "$COUNT" = 0 ] && echo unbounded || echo "$COUNT")
echo "submitting $label heartbeat(s), one every ${INTERVAL}s"
i=0
while :; do
  i=$((i + 1))
  resp=$(cast send --json --rpc-url "$SEPOLIA_RPC_URL" --private-key "$submitter_KEY" \
    "$HEARTBEAT_MANAGER" "submitHeartbeat(bytes)" "$RAW" 2>/dev/null || true)
  if [ -n "$resp" ]; then
    tx=$(printf '%s' "$resp" | jq -r '.transactionHash // empty' 2>/dev/null || true)
    gu=$(printf '%s' "$resp" | jq -r '.gasUsed // empty' 2>/dev/null || true)
    gp=$(printf '%s' "$resp" | jq -r '.effectiveGasPrice // empty' 2>/dev/null || true)
    echo "$(date -u +%FT%TZ) heartbeat $i tx=$tx gasUsed=$gu gasPrice=$gp"
    if [ -n "$RECEIPTS" ]; then echo "$tx $gu $gp" >> "$RECEIPTS"; fi
  else
    echo "$(date -u +%FT%TZ) heartbeat $i FAILED" >&2
  fi
  if [ "$COUNT" != 0 ] && [ "$i" -ge "$COUNT" ]; then echo "done: $i heartbeat(s) submitted"; break; fi
  sleep "$INTERVAL"
done
