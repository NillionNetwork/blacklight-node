# Keeper

The **Keeper** is an L1/L2 supervisor service that manages critical operational aspects of the Blacklight verification network. It acts as an automated protocol operator that coordinates token emissions on Layer 1 and manages verification round lifecycle on Layer 2.

## Overview

The Blacklight network relies on the Keeper to:

1. **Mint and bridge token epochs** from L1 to L2 (emissions management)
2. **Enforce round deadlines** by escalating stalled verification rounds
3. **Distribute rewards** to validators who voted correctly
4. **Jail misbehaving validators** through the jailing policy contract

Without the Keeper, the protocol would stall: rounds would never finalize, rewards would not be distributed, and token emissions would not flow from L1 to L2.

## Architecture

```
                          ┌─────────────────────────────────────┐
                          │              Keeper                 │
                          │                                     │
     ┌────────────────────┼─────────────────────────────────────┼────────────────────┐
     │                    │                                     │                    │
     │    ┌───────────────▼───────────────┐   ┌─────────────────▼────────────────┐   │
     │    │        L1 Supervisor          │   │          L2 Supervisor           │   │
     │    │   (emissions_interval: 30s)   │   │     (tick_interval: 5s)          │   │
     │    └───────────────┬───────────────┘   └─────────────────┬────────────────┘   │
     │                    │                                     │                    │
     │                    ▼                                     ▼                    │
     │    ┌───────────────────────────────┐   ┌──────────────────────────────────┐   │
     │    │     L1 EmissionsClient        │   │        L2 KeeperClient           │   │
     │    │                               │   │                                  │   │
     │    │  • EmissionsController        │   │  • HeartbeatManager              │   │
     │    │    - mintAndBridgeNextEpoch() │   │  • JailingPolicy                 │   │
     │    │    - epoch tracking           │   │  • RewardPolicy                  │   │
     │    └───────────────┬───────────────┘   │  • ERC20 (reward tokens)         │   │
     │                    │                   └─────────────────┬────────────────┘   │
     │                    │                                     │                    │
     └────────────────────┼─────────────────────────────────────┼────────────────────┘
                          │                                     │
                          ▼                                     ▼
              ┌───────────────────────┐           ┌──────────────────────────────┐
              │     L1 Blockchain     │           │        L2 Blockchain         │
              │                       │  bridge   │                              │
              │  EmissionsController  │ ───────▶  │  HeartbeatManager            │
              │                       │           │  JailingPolicy               │
              │                       │           │  RewardPolicy                │
              └───────────────────────┘           └──────────────────────────────┘
```

## Key Concepts

### Emissions (L1)

The Blacklight network uses a token emission schedule controlled by the `EmissionsController` contract on L1. Tokens are minted in discrete **epochs**, each with a predetermined release timestamp.

The Keeper periodically checks if the next epoch is ready:
- Compares `mintedEpochs` vs `epochs` (total planned)
- Checks `nextEpochReadyAt` timestamp against current block
- When ready, calls `mintAndBridgeNextEpoch()` to mint tokens and bridge them to L2

This ensures a controlled, scheduled release of tokens to the L2 network where rewards are distributed.

### Verification Rounds (L2)

When an HTX (heartbeat transaction) is submitted to the HeartbeatManager contract, a verification round begins:

1. **RoundStarted**: Contract assigns random validators (members) to verify the HTX
2. **Validators vote**: Each member submits their verification result
3. **RoundFinalized**: Once enough votes are collected or deadline passes, outcome is determined

The Keeper monitors these events and takes action after finalization.

### Escalation

If a round stalls (validators don't vote in time), the Keeper enforces deadlines:

- Monitors pending rounds without outcomes
- When `block_timestamp > deadline`, calls `escalateOrExpire()`
- This either advances the round to the next phase or expires it

### Rewards Distribution

After a round finalizes with a valid/invalid outcome:

1. Keeper identifies voters who voted with the correct outcome
2. Validates the reward policy has sufficient budget (streaming budget + unlocked tokens)
3. If budget is empty, attempts to `sync()` to unlock more tokens
4. Calls `distributeRewards()` to send tokens to correct voters weighted by stake

### Jailing

Validators who consistently misbehave (fail to vote, vote incorrectly) are jailed:

1. `recordRound()`: Registers the round occurrence with the jailing policy
2. `enforceJailFromMembers()`: Applies jailing rules based on validator behavior

Jailed validators cannot participate in future rounds until unjailed.

## Module Structure

```
keeper/
├── Cargo.toml
└── src/
    ├── main.rs          # Entry point, initialization, signal handling
    ├── args.rs          # CLI arguments and configuration
    ├── clients.rs       # L1/L2 contract client wrappers
    ├── contracts.rs     # Alloy sol! bindings for JailingPolicy, RewardPolicy, etc.
    ├── metrics.rs       # OpenTelemetry metrics definitions
    └── l2/
        ├── mod.rs       # KeeperState and RoundState definitions
        ├── supervisor.rs # Main L2 supervisor loop
        ├── events.rs    # Event subscription and handling
        ├── escalator.rs # Deadline enforcement and escalation
        ├── rewards.rs   # Reward distribution logic
        └── jailing.rs   # Jailing enforcement
```

### `main.rs`
- Parses CLI arguments and builds configuration
- Validates wallet has sufficient ETH on both L1 and L2
- Optionally initializes OpenTelemetry metrics exporter
- Spawns L1 and L2 supervisors as independent background tasks
- Handles graceful shutdown on SIGTERM/CTRL-C

### `args.rs`
- Defines `CliArgs` struct with clap for CLI parsing
- Converts HTTP RPC URLs to WebSocket URLs automatically
- Builds `KeeperConfig` with all resolved values

### `clients.rs`
- `L2KeeperClient`: WebSocket-based client for L2 contracts (HeartbeatManager, JailingPolicy, RewardPolicy)
- `L1EmissionsClient`: WebSocket-based client for L1 EmissionsController

### `contracts.rs`
- Alloy `sol!` macro definitions for contract ABIs
- JailingPolicy: `recordRound()`, `enforceJailFromMembers()`
- RewardPolicy: `sync()`, `spendableBudget()`, streaming budget queries
- EmissionsController: `mintAndBridgeNextEpoch()`, epoch tracking

### `metrics.rs`
- L1Metrics: ETH balance, epochs minted/total
- L2Metrics: Events received, reward distributions, budget, escalations, ETH balance

### `l2/mod.rs`
- `KeeperState`: In-memory state cache protected by Arc<Mutex>
- `RoundState`: Tracks members, deadline, outcome, reward/jailing status per round
- `RewardPolicyCache`: Caches token info and budget to reduce RPC calls

### `l2/supervisor.rs`
- Loads historical events on startup (lookback_blocks)
- Spawns event listener for real-time updates
- Runs main processing loop on tick interval
- Coordinates escalation, rewards, and jailing

### `l2/events.rs`
- Subscribes to 6 event types from HeartbeatManager
- Updates KeeperState based on events
- Handles slashing callback retry logic

### `l2/escalator.rs`
- Identifies rounds past their deadline
- Calls `escalateOrExpire()` to enforce deadline
- Consolidates multiple rounds of same heartbeat

### `l2/rewards.rs`
- Validates reward budget (spendable + sync if needed)
- Builds voter list from round members who voted correctly
- Calls `distributeRewards()` weighted by stake

### `l2/jailing.rs`
- Two-phase jailing: recordRound + enforceJailFromMembers
- Gracefully handles disabled jailing policy
- Updates state to prevent re-processing

## Configuration

| Environment Variable | Required | Default | Description |
|---------------------|----------|---------|-------------|
| `L2_RPC_URL` | Yes | - | L2 blockchain RPC endpoint (HTTP, converted to WS) |
| `L1_RPC_URL` | Yes | - | L1 blockchain RPC endpoint (HTTP, converted to WS) |
| `PRIVATE_KEY` | Yes | - | Keeper wallet private key (hex) |
| `L2_HEARTBEAT_MANAGER_ADDRESS` | Yes | - | HeartbeatManager contract address |
| `L1_EMISSIONS_CONTROLLER_ADDRESS` | Yes | - | EmissionsController contract address |
| `L2_JAILING_POLICY_ADDRESS` | No | - | JailingPolicy contract (if not set, jailing disabled) |
| `DISABLE_JAILING` | No | `false` | Explicitly disable jailing operations |
| `L1_BRIDGE_VALUE_WEI` | No | `0` | ETH to send with bridge transactions |
| `LOOKBACK_BLOCKS` | No | `50` | Blocks to scan for historical events on startup |
| `TICK_INTERVAL_SECS` | No | `5` | L2 processing loop frequency |
| `EMISSIONS_INTERVAL_SECS` | No | `30` | L1 emission check frequency |
| `OTEL_ENDPOINT` | No | - | OpenTelemetry collector endpoint for metrics |
| `OTEL_EXPORT_INTERVAL_SECS` | No | `15` | Metrics export interval |
| `OTEL_EXPORT_TIMEOUT_SECS` | No | `30` | Metrics export timeout |

## Running the Keeper

### Prerequisites

- Rust toolchain
- Access to L1 and L2 RPC endpoints (WebSocket support required)
- Funded wallet on both L1 and L2 (minimum 0.00001 ETH each)

### Build

```bash
cargo build -p keeper --release
```

### Run

```bash
# Minimal configuration
L1_RPC_URL=http://localhost:8545 \
L2_RPC_URL=http://localhost:8546 \
PRIVATE_KEY=0x... \
L2_HEARTBEAT_MANAGER_ADDRESS=0x... \
L1_EMISSIONS_CONTROLLER_ADDRESS=0x... \
./target/release/keeper

# Full configuration with jailing and metrics
L1_RPC_URL=http://localhost:8545 \
L2_RPC_URL=http://localhost:8546 \
PRIVATE_KEY=0x... \
L2_HEARTBEAT_MANAGER_ADDRESS=0x... \
L2_JAILING_POLICY_ADDRESS=0x... \
L1_EMISSIONS_CONTROLLER_ADDRESS=0x... \
L1_BRIDGE_VALUE_WEI=100000000000000 \
TICK_INTERVAL_SECS=3 \
EMISSIONS_INTERVAL_SECS=60 \
OTEL_ENDPOINT=http://localhost:4317 \
./target/release/keeper
```

### Docker Compose

The keeper is included in the project's docker-compose setup:

```bash
docker compose up -d keeper
docker compose logs -f keeper
```

## State Management

The Keeper is **stateless** between restarts:

- All state is in-memory only
- On startup, loads historical events via `lookback_blocks` to catch up
- No database or file storage required
- This enables simple horizontal scaling and easy restarts

The in-memory state includes:
- `raw_htx_by_heartbeat`: Cached HTX data for verification
- `rounds`: Round state (members, deadline, outcome, reward/jail status)
- `reward_policies`: Cached token info and budgets to reduce RPC calls

## Metrics

When `OTEL_ENDPOINT` is configured, the Keeper exports the following metrics:

### L1 Metrics
- `l1.eth.total` - Wallet ETH balance on L1
- `l1.epochs.minted` - Number of epochs minted so far
- `l1.epochs.total` - Total planned epochs

### L2 Metrics
- `l2.events.received` - Counter for contract events by type
- `l2.rewards.distributions` - Number of reward distributions executed
- `l2.rewards.budget` - Current spendable reward budget
- `l2.escalations.total` - Total escalations executed
- `l2.escalations.block` - Latest block used for escalations
- `l2.eth.total` - Wallet ETH balance on L2

## Error Handling

The Keeper is designed for resilience:

- L1 and L2 supervisors run independently; one failing doesn't affect the other
- Individual round processing errors are logged but don't halt the loop
- Failed escalations, rewards, or jailing are logged and retried on next tick
- WebSocket reconnection is handled by the underlying alloy provider
- Graceful shutdown flushes metrics before exit

## Rationale

### Why a separate Keeper service?

The Keeper exists because certain protocol operations cannot be triggered by end users:

1. **Emissions**: Token minting should follow a predetermined schedule, not user-triggered
2. **Deadlines**: Someone must call `escalateOrExpire()` when rounds timeout
3. **Rewards**: Distribution requires aggregating voter data and calling the contract
4. **Jailing**: Punishment logic must be enforced consistently

Having a dedicated service ensures these operations happen reliably and on schedule.

### Why split L1/L2 supervisors?

- Different tick intervals (emissions are less frequent than round processing)
- Independent failure domains (L1 issues shouldn't block L2 operations)
- Cleaner code organization
- Easier to reason about each layer's responsibilities

### Why in-memory state?

- Simplicity: No database to manage, backup, or migrate
- Catchup via events: Blockchain is the source of truth
- Stateless deployment: Easy to restart, scale, or failover
- The lookback mechanism ensures no rounds are missed on restart

### Why cache reward policy data?

- Token decimals and addresses don't change, expensive to query repeatedly
- Spendable budget calculation involves multiple contract reads
- Caching per-block reduces RPC load significantly
- Cache invalidation is straightforward (new block = refresh)
