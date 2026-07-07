#!/usr/bin/env bash
# Blacklight local devnet harness (I2, WP9).
#
# Brings up a complete L1-configuration network on a local anvil:
#   anvil -> DeployBlacklightFromConfig (core.anvil-l1.json, Fenwick selector,
#   startRound split, bridge-free emissions) -> fund/stake N operator wallets ->
#   role grants (HEARTBEAT_SUBMITTER_ROLE -> simulator, ROUND_STARTER_ROLE -> keeper)
#   -> keeper (L1 single-chain, eip1559) + N nodes (eip1559) -> simulator drives HTXs.
#
# Usage:
#   devnet/run.sh --nodes 5                 # bring up and stay up (ctrl-c tears down)
#   devnet/run.sh --nodes 5 --ci            # wait for one finalized round, then exit 0
#   devnet/run.sh --nodes 5 --ci --rounds 3 # require 3 finalized rounds
#
# Env overrides: CONTRACTS_DIR (default ../blacklight-contracts), ANVIL_PORT (8545).
# Every process logs into devnet/runs/<timestamp>/.

set -euo pipefail

# ---------------------------------------------------------------------------
# Config / args
# ---------------------------------------------------------------------------
NODES=5
CI_MODE=0
CI_ROUNDS=1
HTXS_PATH=""
DEPLOY_CONFIG="core.anvil-l1.json"
HOOK=""
ANVIL_PORT="${ANVIL_PORT:-8545}"
NODE_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACTS_DIR="${CONTRACTS_DIR:-$NODE_REPO/../blacklight-contracts}"
RPC="http://127.0.0.1:${ANVIL_PORT}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --nodes) NODES="$2"; shift 2 ;;
    --ci) CI_MODE=1; shift ;;
    --rounds) CI_ROUNDS="$2"; shift 2 ;;
    --htxs) HTXS_PATH="$2"; shift 2 ;;
    --config) DEPLOY_CONFIG="$2"; shift 2 ;;
    --hook) HOOK="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if (( NODES < 1 || NODES > 5 )); then
  echo "--nodes must be 1..5 (limited by anvil's default accounts)" >&2
  exit 2
fi

RUN_DIR="$NODE_REPO/devnet/runs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RUN_DIR"
echo "==> logs in $RUN_DIR"

# anvil's standard deterministic accounts (mnemonic: "test test ... junk")
KEYS=(
  0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 # 0 deployer
  0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d # 1 simulator
  0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a # 2 keeper
  0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6 # 3 node1
  0x47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a # 4 node2
  0x8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba # 5 node3
  0x92db14e403b83dfe3df233f83dfa3a0d7096f21ca9b0d6d6b8d88b2b4ec1564e # 6 node4
  0x4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356 # 7 node5
)
ADDRS=(
  0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
  0x70997970C51812dc3A010C7d01b50e0d17dc79C8
  0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC
  0x90F79bf6EB2c4f870365E785982E1f101E93b906
  0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65
  0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc
  0x976EA74026E726554dB657fA54763abd0C3a0aa9
  0x14dC79964da2C08b23698B3D3cc7Ca32193d9955
)
DEPLOYER_KEY="${KEYS[0]}"
SIM_KEY="${KEYS[1]}";    SIM_ADDR="${ADDRS[1]}"
KEEPER_KEY="${KEYS[2]}"; KEEPER_ADDR="${ADDRS[2]}"

PIDS=()
teardown() {
  local code=$?
  echo "==> tearing down (exit $code)"
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  exit "$code"
}
trap teardown EXIT

wait_for() { # wait_for <timeout_s> <description> <cmd...>
  local timeout=$1 desc=$2; shift 2
  local start
  start=$(date +%s)
  until "$@" >/dev/null 2>&1; do
    if (( $(date +%s) - start > timeout )); then
      echo "TIMEOUT waiting for: $desc" >&2
      return 1
    fi
    sleep 1
  done
}

# ---------------------------------------------------------------------------
# 1. Build binaries
# ---------------------------------------------------------------------------
echo "==> building keeper, node, simulator"
(cd "$NODE_REPO" && cargo build -p keeper -p blacklight-node -p simulator) \
  > "$RUN_DIR/cargo-build.log" 2>&1
BIN="$NODE_REPO/target/debug"

# ---------------------------------------------------------------------------
# 2. anvil
# ---------------------------------------------------------------------------
echo "==> starting anvil on :$ANVIL_PORT"
anvil --port "$ANVIL_PORT" --block-time 1 > "$RUN_DIR/anvil.log" 2>&1 &
PIDS+=($!)
wait_for 30 "anvil rpc" cast chain-id --rpc-url "$RPC"

# ---------------------------------------------------------------------------
# 3. Deploy contracts (contracts repo's script + core.anvil-l1.json)
# ---------------------------------------------------------------------------
echo "==> deploying contracts ($DEPLOY_CONFIG)"
(
  cd "$CONTRACTS_DIR"
  PRIVATE_KEY="$DEPLOYER_KEY" WRITE_OUTPUT=true \
    forge script script/deployment/DeployBlacklightFromConfig.s.sol \
    --sig 'run(string)' "script/deployment/configs/$DEPLOY_CONFIG" \
    --rpc-url "$RPC" --broadcast
) > "$RUN_DIR/deploy.log" 2>&1
cp "$CONTRACTS_DIR/contract_addresses.env" "$RUN_DIR/"
# shellcheck disable=SC1091
source "$RUN_DIR/contract_addresses.env"
echo "    manager:   $HEARTBEAT_MANAGER"
echo "    staking:   $STAKING_OPERATORS"
echo "    selector:  $COMMITTEE_SELECTOR"
echo "    emissions: $EMISSIONS_CONTROLLER_L1"

send() { # send <key> <to> <sig> [args...]
  local key=$1 to=$2; shift 2
  cast send --rpc-url "$RPC" --private-key "$key" "$to" "$@" >/dev/null
}

# ---------------------------------------------------------------------------
# 4. Roles + operator staking
# ---------------------------------------------------------------------------
echo "==> granting roles (submitter -> simulator, round-starter -> keeper)"
SUBMITTER_ROLE=$(cast keccak "HEARTBEAT_SUBMITTER_ROLE")
STARTER_ROLE=$(cast keccak "ROUND_STARTER_ROLE")
send "$DEPLOYER_KEY" "$HEARTBEAT_MANAGER" "grantRole(bytes32,address)" "$SUBMITTER_ROLE" "$SIM_ADDR"
send "$DEPLOYER_KEY" "$HEARTBEAT_MANAGER" "grantRole(bytes32,address)" "$STARTER_ROLE" "$KEEPER_ADDR"

echo "==> staking $NODES operator wallets (deployer as staker)"
STAKE_AMOUNT=100000000 # 100 TEST (6 decimals); min operator stake is 10 TEST
send "$DEPLOYER_KEY" "$STAKE_TOKEN" "approve(address,uint256)" "$STAKING_OPERATORS" \
  0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
for i in $(seq 1 "$NODES"); do
  NODE_ADDR="${ADDRS[$((2 + i))]}"
  send "$DEPLOYER_KEY" "$STAKING_OPERATORS" "stakeTo(address,uint256)" "$NODE_ADDR" "$STAKE_AMOUNT"
done

# ---------------------------------------------------------------------------
# 5. Keeper (L1 single-chain, eip1559 both legs)
# ---------------------------------------------------------------------------
echo "==> starting keeper"
env -u PRIVATE_KEY \
  L2_RPC_URL="$RPC" \
  L1_RPC_URL="$RPC" \
  L2_HEARTBEAT_MANAGER_ADDRESS="$HEARTBEAT_MANAGER" \
  L2_STAKING_OPERATORS_ADDRESS="$STAKING_OPERATORS" \
  L2_JAILING_POLICY_ADDRESS="$SLASHING_POLICY" \
  L1_EMISSIONS_CONTROLLER_ADDRESS="$EMISSIONS_CONTROLLER_L1" \
  L1_SINGLE_CHAIN=true \
  L2_FEE_STRATEGY=eip1559 \
  L1_FEE_STRATEGY=eip1559 \
  TICK_INTERVAL_SECS=2 \
  EMISSIONS_INTERVAL_SECS=10 \
  PRIVATE_KEY="$KEEPER_KEY" \
  OTEL_SDK_DISABLED=true \
  RUST_LOG=info \
  "$BIN/keeper" > "$RUN_DIR/keeper.log" 2>&1 &
PIDS+=($!)

# ---------------------------------------------------------------------------
# 6. Nodes (register themselves on boot; pre-staked above)
# ---------------------------------------------------------------------------
echo "==> starting $NODES nodes"
for i in $(seq 1 "$NODES"); do
  NODE_KEY="${KEYS[$((2 + i))]}"
  mkdir -p "$RUN_DIR/node$i/artifacts" "$RUN_DIR/node$i/certs"
  (
    cd "$RUN_DIR/node$i" # state file is written to the cwd
    env -u PRIVATE_KEY \
      RPC_URL="$RPC" \
      MANAGER_CONTRACT_ADDRESS="$HEARTBEAT_MANAGER" \
      STAKING_CONTRACT_ADDRESS="$STAKING_OPERATORS" \
      TOKEN_CONTRACT_ADDRESS="$STAKE_TOKEN" \
      FEE_STRATEGY=eip1559 \
      PRIVATE_KEY="$NODE_KEY" \
      RUST_LOG=info \
      "$BIN/blacklight-node" \
      --artifact-cache "$RUN_DIR/node$i/artifacts" \
      --cert-cache "$RUN_DIR/node$i/certs" \
      > "$RUN_DIR/node$i/node.log" 2>&1 &
    echo $! > "$RUN_DIR/node$i/pid"
  )
  PIDS+=("$(cat "$RUN_DIR/node$i/pid")")
done

check_node_count() {
  local count
  count=$(cast call --rpc-url "$RPC" "$HEARTBEAT_MANAGER" "nodeCount()(uint256)")
  [[ "$count" == "$NODES" ]]
}
echo "==> waiting for $NODES nodes to register"
wait_for 120 "$NODES registered operators" check_node_count
echo "    all $NODES nodes registered"

# ---------------------------------------------------------------------------
# 7. Simulator (drives one HTX per 15s slot)
# ---------------------------------------------------------------------------
echo "==> starting simulator"
(
  cd "$RUN_DIR"
  env -u PRIVATE_KEY \
    RPC_URL="$RPC" \
    MANAGER_CONTRACT_ADDRESS="$HEARTBEAT_MANAGER" \
    STAKING_CONTRACT_ADDRESS="$STAKING_OPERATORS" \
    TOKEN_CONTRACT_ADDRESS="$STAKE_TOKEN" \
    PRIVATE_KEY="$SIM_KEY" \
    RUST_LOG=info \
    "$BIN/simulator" nilcc --htxs-path "${HTXS_PATH:-$NODE_REPO/data/htxs.json}" \
    > "$RUN_DIR/simulator.log" 2>&1 &
  echo $! > "$RUN_DIR/simulator.pid"
)
PIDS+=("$(cat "$RUN_DIR/simulator.pid")")

# ---------------------------------------------------------------------------
# 8. Healthcheck: rounds start and finalize
# ---------------------------------------------------------------------------
finalized_count() {
  cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$HEARTBEAT_MANAGER" "RoundFinalized(bytes32,uint8,uint8)" --json 2>/dev/null \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'
}
check_finalized() {
  local n
  n=$(finalized_count)
  (( n >= CI_ROUNDS ))
}

echo "==> waiting for the first RoundStarted"
check_started() {
  local n
  n=$(cast logs --rpc-url "$RPC" --from-block 0 --to-block latest \
    --address "$HEARTBEAT_MANAGER" \
    "RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)" --json 2>/dev/null \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')
  (( n >= 1 ))
}
wait_for 120 "first RoundStarted" check_started
echo "    round started"

FINALIZE_TIMEOUT=$(( 180 + CI_ROUNDS * 30 ))
echo "==> waiting for $CI_ROUNDS finalized round(s) (timeout ${FINALIZE_TIMEOUT}s)"
wait_for "$FINALIZE_TIMEOUT" "$CI_ROUNDS finalized round(s)" check_finalized
echo "    $(finalized_count) round(s) finalized"
echo "==> HEALTHY: rounds are starting and finalizing"

# Scenario hook (P1.M3 e2e): runs with the devnet live; its exit code is the
# run's result. The hook sees RUN_DIR/RPC/addresses/keys via the environment.
if [[ -n "$HOOK" ]]; then
  echo "==> running hook: $HOOK"
  if RUN_DIR="$RUN_DIR" RPC="$RPC" DEPLOYER_KEY="$DEPLOYER_KEY" NODES="$NODES" \
     bash "$HOOK"; then
    echo "==> hook: success"
    exit 0
  else
    echo "==> hook: FAILED" >&2
    exit 1
  fi
fi

if (( CI_MODE )); then
  echo "==> CI mode: success"
  exit 0
fi

echo "==> devnet is up; ctrl-c to tear down"
wait
