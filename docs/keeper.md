# Keeper

The keeper is an off-chain service that keeps the protocol moving by watching
L2 rounds and L1 emissions, then submitting the required on-chain transactions.
It uses the same Rust stack and connection style as the nodes (Alloy + WS
providers), but runs as its own binary.

## What it does

- L2 rounds: watches `HeartbeatManager` events, tracks rounds, escalates/expiring
  rounds, distributes rewards, and (optionally) enforces jailing.
- L1 emissions: calls `EmissionsController` on the epoch schedule to mint on L1
  and bridge to L2.
- Reward budget sync: after an L1->L2 deposit or when enough budget has unlocked,
  it calls `RewardPolicy.sync()` to make L2 rewards spendable.
- Resilience: reconnects on RPC disconnects with exponential backoff and replays
  recent history (`LOOKBACK_BLOCKS`) to rebuild state.

## How it works (high level)

The keeper runs two supervisors in parallel:

1) **L2 supervisor**
   - Creates a WS client to L2, subscribes to `HeartbeatManager` events, and keeps
     a local cache of round state (members, raw HTX, deadlines, outcomes).
   - Runs a periodic tick loop (`TICK_INTERVAL_SECS`) to:
     - escalate/expire rounds if deadlines passed,
     - compute reward recipients and call `distributeRewards`,
     - call `JailingPolicy.enforceJail` when enabled.
   - Uses a small in-memory cache per reward policy to avoid redundant calls.

2) **L1 supervisor**
   - Creates a WS client to L1 and periodically checks the emissions schedule
     (`EMISSIONS_INTERVAL_SECS`).
   - Calls `mintAndBridgeNextEpoch` when an epoch is ready and forwards
     `L1_BRIDGE_VALUE_WEI` if needed by the bridge.

## Running the keeper

From repo root:

```bash
export L2_RPC_URL="https://rpc.testnet.nillion.network"
export L1_RPC_URL="https://eth-sepolia.g.alchemy.com/v2/<key>"
export L2_HEARTBEAT_MANAGER_ADDRESS="0x..."
export L1_EMISSIONS_CONTROLLER_ADDRESS="0x..."
export PRIVATE_KEY="0x..."

# Optional: disable jailing if slashing/jailing is not active
export DISABLE_JAILING=true

RUST_LOG=info cargo run --bin keeper
```

You can also run the built binary:

```bash
RUST_LOG=info target/release/keeper --help
```

## Configuration options

All options are available via CLI flags or env vars (env names shown):

- `L2_RPC_URL` (required): HTTP/HTTPS RPC URL. The keeper converts it to WS/WSS.
- `L1_RPC_URL` (required): HTTP/HTTPS RPC URL. The keeper converts it to WS/WSS.
- `L2_HEARTBEAT_MANAGER_ADDRESS` (required): L2 `HeartbeatManager` address.
- `L1_EMISSIONS_CONTROLLER_ADDRESS` (required): L1 `EmissionsController` address.
- `PRIVATE_KEY` (required): keeper signer used for all on-chain txs.
- `L2_JAILING_POLICY_ADDRESS` (optional): L2 `JailingPolicy` address.
- `DISABLE_JAILING` (optional, default: false): force-disable jailing even if
  a policy address is set.
- `L1_BRIDGE_VALUE_WEI` (optional, default: 0): ETH value to attach to
  `mintAndBridgeNextEpoch`.
- `LOOKBACK_BLOCKS` (optional, default: 50): how far back to replay events on
  reconnect/startup.
- `TICK_INTERVAL_SECS` (optional, default: 5): L2 tick loop interval.
- `EMISSIONS_INTERVAL_SECS` (optional, default: 30): L1 emissions check interval.

CLI flags mirror the envs (e.g. `--l2-rpc-url`, `--l1-rpc-url`, etc.).

## State file and wallet behavior

- The keeper stores state in `niluv_keeper.env` (repo root).
- If no `PRIVATE_KEY` is provided, it generates one, saves it to the state file,
  and exits so you can fund it.
- The keeper enforces a minimum balance on *both* L1 and L2
  (`MIN_ETH_BALANCE = 0.00001 ETH`). It will refuse to start if underfunded.

## Operational notes

- Use WS-capable RPC endpoints. The keeper converts `http(s)` to `ws(s)`; if your
  provider does not support WS, connections will fail.
- Reward syncs are intentionally throttled to avoid repeated calls in the same
  tick or for the same round.
- When `DISABLE_JAILING=true` or no jailing policy is configured, the keeper
  never attempts jailing transactions.

