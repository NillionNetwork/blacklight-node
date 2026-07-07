#!/usr/bin/env bash
# Scenario: non-participant node jailed. Requires --nodes 4. Widens the committee to
# 4-of-4 so a killed node's absence still leaves a 75% quorum (>= 67%) and a
# conclusive outcome, which makes the absentee jailable.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cast send --rpc-url "$RPC" --private-key "$DEPLOYER_KEY" "$PROTOCOL_CONFIG" \
  "setParams(uint32,uint32,uint32,uint8,uint16,uint16,uint256,uint256,uint256,uint256,uint256)" \
  4 0 5 0 6700 6700 60 120 30 100 10000000 >/dev/null

victim_pid=$(cat "$RUN_DIR/node4/pid")
# SIGKILL: a plain SIGTERM triggers the node's graceful shutdown, which deactivates
# the operator on-chain - a polite deregistration, not an absent participant. kill -9
# leaves it registered and silent, which is what this scenario tests.
kill -9 "$victim_pid"
echo "killed node4 (pid $victim_pid)"

check_jailed() {
  n=$(cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$STAKING_OPERATORS" "Jailed(address,uint64)" --json 2>/dev/null \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))' 2>/dev/null || echo 0)
  [ "$n" -ge 1 ]
}
wait_until 420 "the absent node to be jailed" check_jailed

check_active_dropped() {
  n=$(cast call --rpc-url "$RPC" "$HEARTBEAT_MANAGER" "nodeCount()(uint256)")
  [ "${n%% *}" -le 3 ]
}
wait_until 120 "active set to shrink" check_active_dropped
echo "jailing: absent node jailed and removed from the active set"
