#!/usr/bin/env bash
# Scenario: valid claim -> Verified (+ emissions minted and rewards distributed).
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check_verified() { [ "$(count_outcome 1)" -ge 1 ] && any_status 2; }
wait_until 300 "a ValidThreshold round + a Verified heartbeat" check_verified

check_rewards() { [ "$(count_logs 'RewardsDistributed(bytes32,uint8,uint256,uint256)')" -ge 1 ]; }
wait_until 180 "rewards distributed for the verified round" check_rewards

check_epoch() {
  n=$(cast call --rpc-url "$RPC" "$EMISSIONS_CONTROLLER_L1" "mintedEpochs()(uint256)")
  [ "${n%% *}" -ge 1 ]
}
wait_until 120 "an emission epoch minted" check_epoch

echo "verified: outcome=1 rounds=$(count_outcome 1), rewards events=$(count_logs 'RewardsDistributed(bytes32,uint8,uint256,uint256)')"
