#!/usr/bin/env bash
# Resume the Sepolia soak after a shutdown (keeper + 5 nodes + 4-hourly submitter).
# Nodes self-re-register on boot (registerOperator re-activates a deactivated
# operator). LOOKBACK_BLOCKS is raised so the keeper recovers round state across
# the offline gap. Companion to devnet/RUNBOOK.md Part 2.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; NODE_REPO="$(cd "$HERE/.." && pwd)"
set -a; source "$HERE/sepolia.env"; source "$HERE/sepolia.wallets.env"; source "$HERE/sepolia-deployment.env"; set +a
RPC="$SEPOLIA_RPC_URL"; SOAK="$NODE_REPO/devnet/runs/sepolia-soak"; mkdir -p "$SOAK"
BIN="$NODE_REPO/target/debug"

(cd "$NODE_REPO" && cargo build -p keeper -p blacklight-node) > "$SOAK/build.log" 2>&1
echo "built"

env -u PRIVATE_KEY \
  L2_RPC_URL="$RPC" L1_RPC_URL="$RPC" \
  L2_HEARTBEAT_MANAGER_ADDRESS="$HEARTBEAT_MANAGER" \
  L2_STAKING_OPERATORS_ADDRESS="$STAKING_OPERATORS" \
  L2_JAILING_POLICY_ADDRESS="$SLASHING_POLICY" \
  L1_EMISSIONS_CONTROLLER_ADDRESS="$EMISSIONS_CONTROLLER_L1" \
  L1_SINGLE_CHAIN=true L2_FEE_STRATEGY=eip1559 L1_FEE_STRATEGY=eip1559 \
  TICK_INTERVAL_SECS=15 EMISSIONS_INTERVAL_SECS=300 \
  LOOKBACK_BLOCKS=20000 \
  PRIVATE_KEY="$keeper_KEY" OTEL_SDK_DISABLED=true RUST_LOG=info \
  nohup "$BIN/keeper" >> "$SOAK/keeper.log" 2>&1 &
echo "keeper up (pid $!)"

for n in node1 node2 node3 node4 node5; do
  k="${n}_KEY"; mkdir -p "$SOAK/$n/artifacts" "$SOAK/$n/certs"
  (cd "$SOAK/$n" && env -u PRIVATE_KEY \
    RPC_URL="$RPC" \
    MANAGER_CONTRACT_ADDRESS="$HEARTBEAT_MANAGER" \
    STAKING_CONTRACT_ADDRESS="$STAKING_OPERATORS" \
    TOKEN_CONTRACT_ADDRESS="$STAKE_TOKEN" \
    FEE_STRATEGY=eip1559 PRIVATE_KEY="${!k}" RUST_LOG=info \
    nohup "$BIN/blacklight-node" --artifact-cache ./artifacts --cert-cache ./certs >> node.log 2>&1 &)
  echo "$n up"
done

echo "waiting for re-registration..."
until [ "$(cast call --rpc-url "$RPC" "$HEARTBEAT_MANAGER" 'nodeCount()(uint256)' 2>/dev/null)" = "5" ]; do sleep 15; done
echo "all 5 nodes re-registered"

nohup "$HERE/sepolia-submit-loop.sh" >> "$SOAK/submitter.log" 2>&1 &
echo "submitter loop up (pid $!, 4-hourly)"
echo "RESUMED. Health: see devnet/RUNBOOK.md 'Health checks'."
