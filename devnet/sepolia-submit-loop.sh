#!/usr/bin/env bash
# P1.M5 soak driver: submit one heartbeat every INTERVAL seconds (default 4h),
# sized to the soak wallet budget. Reads devnet/sepolia.env + sepolia.wallets.env
# + sepolia-deployment.env. Run under nohup/systemd/docker for the soak.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
set -a; source "$HERE/sepolia.env"; source "$HERE/sepolia.wallets.env"; source "$HERE/sepolia-deployment.env"; set +a
INTERVAL="${INTERVAL:-14400}"
RAW=0x$(xxd -p "$HERE/../data/valid_htx.json" | tr -d '\n')
echo "submitting one heartbeat every ${INTERVAL}s"
while true; do
  if cast send --rpc-url "$SEPOLIA_RPC_URL" --private-key "$submitter_KEY" \
    "$HEARTBEAT_MANAGER" "submitHeartbeat(bytes)" "$RAW" >/dev/null 2>&1; then
    echo "$(date -u +%FT%TZ) submitted heartbeat"
  else
    echo "$(date -u +%FT%TZ) submit FAILED (will retry next slot)" >&2
  fi
  sleep "$INTERVAL"
done
