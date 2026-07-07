#!/usr/bin/env bash
# Shared helpers for P1.M3 e2e scenario hooks. Hooks run with the devnet live and
# RUN_DIR/RPC/DEPLOYER_KEY/NODES exported by run.sh.

source "$RUN_DIR/contract_addresses.env"

wait_until() { # wait_until <timeout_s> <desc> <cmd...>
  local timeout=$1 desc=$2; shift 2
  local start; start=$(date +%s)
  until "$@" >/dev/null 2>&1; do
    if (( $(date +%s) - start > timeout )); then
      echo "e2e TIMEOUT: $desc" >&2
      return 1
    fi
    sleep 2
  done
}

count_logs() { # count_logs "<EventSig(...)>"
  cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$HEARTBEAT_MANAGER" "$1" --json 2>/dev/null \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))' 2>/dev/null || echo 0
}

# count finalized rounds with a given outcome (0/1/2)
count_outcome() {
  cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$HEARTBEAT_MANAGER" "RoundFinalized(bytes32,uint8,uint8)" --json 2>/dev/null \
    | python3 -c "
import json,sys
logs=json.load(sys.stdin)
print(sum(1 for l in logs if int(l['data'][-2:],16) == $1))" 2>/dev/null || echo 0
}

# count RoundStarted events for a given round number
count_round_started() {
  cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$HEARTBEAT_MANAGER" \
    "RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)" --json 2>/dev/null \
    | python3 -c "
import json,sys
logs=json.load(sys.stdin)
print(sum(1 for l in logs if int(l['data'][:66][2:66],16) == $1))" 2>/dev/null || echo 0
}

# heartbeat statuses seen on-chain: prints one status digit per enqueued heartbeat
heartbeat_statuses() {
  cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$HEARTBEAT_MANAGER" "HeartbeatEnqueued(bytes32,bytes,address)" --json 2>/dev/null \
    | python3 -c 'import json,sys; [print(l["topics"][1]) for l in json.load(sys.stdin)]' 2>/dev/null \
    | sort -u | while read -r key; do
        cast call --rpc-url "$RPC" "$HEARTBEAT_MANAGER" "heartbeats(bytes32)(uint8,uint8,uint8,uint8,uint64,bytes32,address)" "$key" 2>/dev/null | head -1
      done
}

any_status() { # any_status <status-digit>
  # capture fully, then match in bash: piping into grep -q would SIGPIPE the
  # producer at first match, which pipefail turns into a false failure
  local statuses
  statuses=$(heartbeat_statuses)
  [[ $'\n'"$statuses"$'\n' == *$'\n'"$1"$'\n'* ]]
}
