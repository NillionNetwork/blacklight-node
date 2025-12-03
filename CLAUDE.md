# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

NilAV (Nillion Auditor-Verifier) is a Rust-based HTX (Hash Transaction) verification system with both local simulation and blockchain deployment modes. The system uses WebSocket-based event streaming for sub-100ms latency.

**Key Architecture:**
- **Smart Contract** (Solidity): NilAVRouter manages node registration, HTX submission, and verification assignment
- **Rust Binaries**: Four independent executables that interact with the contract or simulate the network
- **Event-Driven**: WebSocket streaming replaces polling for real-time responsiveness

## Build & Test Commands

### Rust

```bash
# Build all binaries
cargo build

# Build for release (optimized)
cargo build --release

# Build specific binary
cargo build --bin nilav_node
cargo build --bin nilcc_simulator
cargo build --bin monitor
cargo build --bin contract_cli

# Run tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Check compilation without building
cargo check
```

### Smart Contracts (Foundry Monorepo)

```bash
cd contracts

# Compile all contracts
forge build

# Run tests
forge test

# Run tests with verbosity
forge test -vvv

# Format Solidity code
forge fmt

# Deploy to local Anvil
./script/deploy_local.sh router    # Deploy NilAVRouter
./script/deploy_local.sh staking   # Deploy StakingOperators

# Deploy to any chain
export RPC_URL=<your-rpc-url>
export PRIVATE_KEY=<your-private-key>
./script/deploy.sh router    # Deploy NilAVRouter
./script/deploy.sh staking   # Deploy StakingOperators with TESTToken
```

### Docker

```bash
# Run full local environment (Anvil + nodes + simulators)
docker compose up --build

# Run specific service
docker compose up anvil
docker compose up node1

# Scale nodes
docker compose up --scale node1=5

# Stop all services
docker compose down

# Use pre-built images from GHCR
docker pull ghcr.io/nillionnetwork/nilav/nilav_node:latest
docker pull ghcr.io/nillionnetwork/nilav/nilcc_simulator:latest
docker pull ghcr.io/nillionnetwork/nilav/monitor:latest
docker pull ghcr.io/nillionnetwork/nilav/contract_cli:latest
```

## Binaries & Their Purposes

### 1. `nilav_node` (src/bin/nilav_node.rs)
The verification node that connects to the blockchain and processes HTX assignments.

**Environment configuration** (`nilav_node.env`):
```env
RPC_URL=https://rpc-url-here.com
CONTRACT_ADDRESS=0xYourContractAddress
PRIVATE_KEY=0xYourPrivateKey
```

**Key behavior:**
- Connects via WebSocket to blockchain RPC
- Auto-registers with the contract if not registered
- Listens for `HTXAssigned` events in real-time
- Fetches HTX data from transaction calldata
- Runs verification logic (checks measurement URLs)
- Submits verification result (true/false) back to contract
- Implements exponential backoff reconnection (1s to 60s)
- Processes historical assignments on reconnect

**Run:**
```bash
cargo run --release --bin nilav_node
```

### 2. `nilcc_simulator` (src/bin/nilcc_simulator.rs)
Simulates a nilCC operator by periodically submitting HTXs to the contract.

**Key behavior:**
- Reads HTXs from `data/htxs.json`
- Submits HTXs round-robin every `slot_ms` (configured in `config/config.toml`)
- Checks node count before submission
- Waits for contract assignment

**Run:**
```bash
cargo run --release --bin nilcc_simulator
```

### 3. `monitor` (src/bin/monitor.rs)
Interactive TUI for monitoring contract activity in real-time.

**Features:**
- Shows registered nodes
- Displays HTX transactions (submitted, assigned, responded)
- Real-time event updates via WebSocket
- Keyboard navigation

**Run:**
```bash
cargo run --release --bin monitor
```

### 4. `contract_cli` (src/bin/contract_cli.rs)
Command-line interface for direct contract interaction.

**Commands:**
```bash
# Register a node
cargo run --bin contract_cli -- register-node 0xNodeAddress

# Deregister a node
cargo run --bin contract_cli -- deregister-node 0xNodeAddress

# List registered nodes
cargo run --bin contract_cli -- list-nodes

# Submit HTX from JSON file
cargo run --bin contract_cli -- submit-htx data/htxs.json

# Get assignment info
cargo run --bin contract_cli -- get-assignment 0xHtxId
```

## Architecture Patterns

### Smart Contract Flow

```
nilCC Operator                 NilAVRouter Contract              nilAV Node
      |                               |                                |
      |---submitHTX(rawHTX)---------->|                                |
      |                               |---_chooseNode()--------------->|
      |                               |---emit HTXAssigned------------>|
      |                               |                                |
      |                               |<---respondHTX(htxId, result)---|
      |                               |---emit HTXResponded----------->|
```

### HTX Verification Logic (verify_htx)

The verification process checks if a nilCC measurement exists in the builder's index:

1. Fetch nilCC measurement from `htx.nilcc_measurement.url`
2. Extract measurement value (tries `root.measurement` then `report.measurement`)
3. Fetch builder index from `htx.builder_measurement.url`
4. Check if measurement exists in builder index (as object values or array elements)
5. Returns `Ok(())` if found, `Err(VerificationError)` otherwise

### Event Streaming Architecture

The `NilAVWsClient` (src/contract_client/mod.rs) uses ethers-rs WebSocket provider:
- Subscribes to contract events (`HTXAssigned`, `HTXSubmitted`, `HTXResponded`)
- Maintains persistent connection with keepalive pings
- Auto-converts HTTP RPC URLs to WebSocket (http -> ws, https -> wss)
- Implements `DEFAULT_LOOKBACK_BLOCKS = 50` to avoid querying from block 0

### Configuration System

Each binary has its own config struct in `src/config/`:
- `NodeConfig` - for nilav_node
- `SimulatorConfig` - for nilcc_simulator
- `MonitorConfig` - for monitor

Config loading priority: CLI args > Environment variables > Config files > Defaults

## Key Data Types

### HTX Structure (src/types.rs)
```rust
pub struct Htx {
    pub workload_id: WorkloadId,
    pub nilcc_operator: NilCcOperator,
    pub builder: Builder,
    pub nilcc_measurement: NilCcMeasurement,  // Contains URL to fetch
    pub builder_measurement: BuilderMeasurement,  // Contains URL to fetch
}
```

### Smart Contract Assignment (NilAVRouter.sol)
```solidity
struct Assignment {
    address node;      // nilAV node chosen for this HTX
    bool responded;    // has the node responded?
    bool result;       // True/False from the node
}
```

## Contract ABI Generation

The project uses `ethers-rs` `abigen!` macro to generate type-safe contract bindings:

```rust
abigen!(
    NilAVRouter,
    "./contracts/out/NilAVRouter.sol/NilAVRouter.json",
    event_derives(serde::Deserialize, serde::Serialize)
);
```

**Important:** After modifying the Solidity contract, regenerate the ABI:
```bash
cd contracts
forge build
```

Then rebuild Rust to regenerate bindings:
```bash
cargo build
```

## Contract Structure

The contracts are organized in a monorepo structure:

```
contracts/
├── src/
│   ├── core/              # Core contract implementations
│   │   ├── NilAVRouter.sol
│   │   ├── StakingOperators.sol
│   │   └── TESTToken.sol
│   ├── interfaces/        # Shared interfaces
│   │   └── Interfaces.sol
│   └── libraries/         # Shared utility libraries
├── test/                  # All tests
│   ├── NilAVRouter.t.sol
│   └── integration/       # Integration tests
├── script/                # Deployment scripts
│   ├── DeployRouter.s.sol
│   ├── DeployStaking.s.sol
│   ├── deploy.sh
│   └── deploy_local.sh
├── lib/                   # Dependencies
│   ├── forge-std/
│   └── openzeppelin-contracts/
└── out/                   # Compiled artifacts
```

## Error Handling Patterns

### Contract Errors
The codebase decodes Solidity `Error(string)` reverts using `decode_error_string()` in `contract_client/mod.rs`. This extracts human-readable error messages from revert data.

### Verification Errors
The `VerificationError` enum provides detailed error context:
- `NilccUrl` / `BuilderUrl` - HTTP fetch failures
- `NilccJson` / `BuilderJson` - JSON parsing failures
- `MissingMeasurement` - Missing required field
- `NotInBuilderIndex` - Measurement not found in builder index

## Development Workflow

### Local Testing with Anvil

1. Start Anvil (local Ethereum testnet):
```bash
docker compose up anvil
```

2. Deploy contract (automatically done in docker/start-anvil.sh)

3. Run node:
```bash
RPC_URL=http://localhost:8545 \
CONTRACT_ADDRESS=0x5FbDB2315678afecb367f032d93F642f64180aa3 \
cargo run --bin nilav_node
```

4. Submit HTXs:
```bash
RPC_URL=http://localhost:8545 \
cargo run --bin nilcc_simulator
```

### Production Deployment

Configure `nilav_node.env` for deployed contract:
```env
RPC_URL=https://rpc-nilav-shzvox09l5.t.conduit.xyz
CONTRACT_ADDRESS=0x4f071c297EF53565A86c634C9AAf5faCa89f6209
PRIVATE_KEY=0xYourPrivateKey
```

Run the node:
```bash
cargo run --release --bin nilav_node
```

## CI/CD Workflows

### GitHub Actions

The repository includes two automated workflows:

#### 1. Build Binaries (`.github/workflows/build.yml`)

Triggers on version tags (`v*.*.*`) and builds native binaries for multiple platforms:

- **Platforms**: Linux (x64), macOS (Intel & ARM), Windows (x64)
- **Artifacts**: `nilav_node`, `nilcc_simulator`, `monitor`
- **Distribution**: Archives uploaded to GitHub Releases (`.tar.gz` for Unix, `.zip` for Windows)

```bash
# Create a release
git tag v1.0.0
git push origin v1.0.0
# Workflow automatically builds and publishes release artifacts
```

#### 2. Docker Build & Push (`.github/workflows/docker.yml`)

Triggers on version tags or manual dispatch. Builds and pushes four Docker images to GHCR:

- **Images**: `nilav_node`, `nilcc_simulator`, `monitor`, `contract_cli`
- **Registry**: `ghcr.io/nillionnetwork/nilav/<image-name>`
- **Tags**: 
  - `latest` - most recent release
  - `v1.0.0`, `v1.0`, `v1` - semantic version tags
  - `main-sha256abc` - commit-specific tags
- **Platforms**: linux/amd64, linux/arm64

```bash
# Images are automatically built and pushed on release tags
# To manually trigger: Go to Actions → docker-build-push → Run workflow

# Pull and use images
docker pull ghcr.io/nillionnetwork/nilav/nilav_node:latest
docker run --rm \
  -e RPC_URL=https://rpc-nilav-shzvox09l5.t.conduit.xyz \
  -e CONTRACT_ADDRESS=0x4f071c297EF53565A86c634C9AAf5faCa89f6209 \
  -e PRIVATE_KEY=0xYourPrivateKey \
  ghcr.io/nillionnetwork/nilav/nilav_node:latest
```

**Build Process:**
1. Multi-stage Dockerfile compiles Rust binaries with Foundry support
2. Each binary gets its own minimal Debian-based runtime image
3. Build caches are used for faster subsequent builds
4. Images include attestation for supply chain security

## Important Notes

- **RPC URL Conversion**: HTTP URLs are automatically converted to WebSocket (http://localhost:8545 becomes ws://localhost:8545)
- **Private Keys**: Never commit private keys. Use environment variables or `.env` files (gitignored)
- **Reconnection Logic**: Nodes implement exponential backoff (1s → 60s max) with automatic historical event processing
- **HTX ID Derivation**: `htxId = keccak256(abi.encode(rawHTXHash, msg.sender, block.number))`
- **Node Selection**: Current implementation uses pseudo-random selection via `block.prevrandao` (not production-secure)
- **Contract is a Stub**: The NilAVRouter contract lacks access controls, secure randomness, and timeout/reassignment logic

## Logging

Set log levels via `RUST_LOG` environment variable:
```bash
RUST_LOG=debug cargo run --bin nilav_node
RUST_LOG=nilav=info,ethers=warn cargo run --bin nilav_node
```

Log levels: `error`, `warn`, `info`, `debug`, `trace`
