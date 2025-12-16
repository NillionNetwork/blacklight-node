# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

NilAV (Nillion Auditor-Verifier) is a Rust-based HTX (Hash Transaction) verification system with both local simulation and blockchain deployment modes. The system uses WebSocket-based event streaming for sub-100ms latency.

**Key Architecture:**
- **Smart Contracts** (Solidity):
  - NilAVRouter: Manages HTX submission and stake-weighted verification assignment
  - StakingOperators: Handles node registration, activation/deactivation, and stake tracking
  - TESTToken: ERC20 token used for staking (test environment)
- **Rust Binaries**: Three independent executables that interact with the contract or simulate the network
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

# Run tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Check compilation without building
cargo check

# Format code
cargo fmt

# Format check (CI)
cargo fmt --all -- --check
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
```

## Binaries & Their Purposes

### 1. `nilav_node` (src/bin/nilav_node.rs)
The verification node that connects to the blockchain and processes HTX assignments.

**Environment configuration** (`nilav_node.env`):
```env
RPC_URL=https://rpc-url-here.com
ROUTER_CONTRACT_ADDRESS=0xYourRouterAddress
STAKING_CONTRACT_ADDRESS=0xYourStakingAddress
TOKEN_CONTRACT_ADDRESS=0xYourTokenAddress
PRIVATE_KEY=0xYourPrivateKey
ARTIFACT_CACHE=/path/to/artifact/cache  # Optional: for HTX verification artifacts
CERT_CACHE=/path/to/cert/cache          # Optional: for attestation certificates
```

**Key behavior:**
- Connects via WebSocket to blockchain RPC
- Auto-registers with StakingOperators contract if not registered
- Requires staked TEST tokens to receive assignments
- Listens for `HTXAssigned` events in real-time
- Fetches HTX data from original transaction calldata via event logs
- Runs verification logic:
  - Fetches and verifies attestation reports from nilCC measurement URLs
  - Generates expected measurement hash from docker-compose hash and metadata
  - Checks if measurement exists in builder's trusted index
- Submits verification result (true/false) back to contract
- Implements exponential backoff reconnection (1s to 60s)
- Processes historical assignments on reconnect
- Gracefully deactivates from contract on shutdown (Ctrl+C)

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

The verification process validates attestation reports and checks measurements against the builder's trusted index:

1. **Fetch Attestation Report**: Download report bundle from `htx.nilcc_measurement.url`
2. **Generate Expected Measurement**:
   - Uses `MeasurementGenerator` from `attestation-verification` crate
   - Inputs: `docker_compose_hash`, `cpu_count`, `vm_type`, and metadata
   - Requires artifacts cached in `ARTIFACT_CACHE` for the specified `nilcc_version`
3. **Verify Report**: Validates the attestation report against expected measurement using AMD SEV-SNP verification
4. **Check Builder Index**:
   - Fetch builder's trusted index from `htx.builder_measurement.url`
   - Verify the measurement hash exists in the index (as object values or array elements)
5. Returns `Ok(report)` if all checks pass, `Err(VerificationError)` otherwise

**Verification Dependencies:**
- `attestation-verification` crate (from NilCC repository)
- Cached artifacts in `ARTIFACT_CACHE` (downloaded from S3: `https://nilcc.s3.eu-west-1.amazonaws.com`)
- AMD certificate chain in `CERT_CACHE`

### Event Streaming Architecture

The contract clients (src/contract_client/) use Alloy framework for WebSocket connectivity:
- **NilAVRouterClient**: Subscribes to HTX events (`HTXAssigned`, `HTXSubmitted`, `HTXResponded`)
- **StakingOperatorsClient**: Manages operator registration and stake tracking
- **TESTTokenClient**: Handles token approvals and transfers for staking
- All clients share a single WebSocket provider for efficiency
- Maintains persistent connection with automatic reconnection on disconnect
- Auto-converts HTTP RPC URLs to WebSocket (http -> ws, https -> wss)
- Implements `DEFAULT_LOOKBACK_BLOCKS = 1000` for historical event queries
- Uses transaction mutex (`tx_lock`) to prevent nonce conflicts in concurrent operations

### Configuration System

Each binary has its own config struct in `src/config/`:
- `NodeConfig` - for nilav_node
- `SimulatorConfig` - for nilcc_simulator
- `MonitorConfig` - for monitor

Config loading priority: CLI args > Environment variables > Config files > Defaults

### Transaction Management

**Gas Estimation & Buffers:**
- `submitHTX`: Estimates gas, then adds 50% buffer for variable node selection costs
- Other transactions: Use automatic gas estimation from Alloy

**Retry Logic (Simulator):**
- Max retries: 3 attempts with 500ms delay between retries
- Only retries on-chain reverts (state race conditions)
- Fails immediately on simulation errors
- Adds random nonce to `workload_id` to ensure HTX uniqueness per submission

**Nonce Management:**
- Uses `tx_lock` (Arc<Mutex<()>>) to serialize transactions from the same signer
- Prevents nonce conflicts in concurrent operations

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

**Critical: JSON Canonicalization**

HTX serialization uses `stable_stringify` (from `src/json.rs`) which canonicalizes JSON by sorting all object keys recursively. This ensures:

1. **Deterministic hashing**: Same HTX always produces the same on-chain hash
2. **Consistent HTX IDs**: Since `htxId = keccak256(abi.encode(rawHTXHash, msg.sender, block.number))`, the raw HTX hash must be deterministic
3. **Verification correctness**: Nodes can reliably match assignments

Without canonicalization, the same HTX could serialize with different key orderings, producing different hashes and breaking the entire verification flow.

### Smart Contract Assignment (NilAVRouter.sol)

The NilAVRouter uses stake-weighted multi-node assignment:

```solidity
struct NodeResponse {
    bool responded;
    bool result;
}

struct Assignment {
    address[] nodes;                                // Array of nodes assigned to this HTX
    mapping(address => NodeResponse) responses;     // Track each node's response
    uint256 requiredStake;                          // Minimum stake required (50% of total)
    uint256 assignedStake;                          // Total stake of assigned nodes
    uint256 respondedCount;                         // Number of nodes that have responded
}
```

**Node Selection Algorithm** (`_selectNodesByStake`):
- Uses Fisher-Yates shuffle for randomized selection
- Continues selecting nodes until `assignedStake >= requiredStake`
- `requiredStake = totalStake * MIN_STAKE_BPS / BPS_DENOMINATOR` (50% by default)
- Pseudo-random seed from `keccak256(block.prevrandao, htxId)`
- Multiple nodes can be assigned to a single HTX for redundancy

## Contract ABI Generation

The project uses Alloy's `sol!` macro to generate type-safe contract bindings:

```rust
sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    NilAVRouter,
    "./contracts/out/NilAVRouter.sol/NilAVRouter.json"
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

**Contract Clients:**
- `NilAVRouterClient` (src/contract_client/nilav_router.rs)
- `StakingOperatorsClient` (src/contract_client/staking_operators.rs)
- `TESTTokenClient` (src/contract_client/test_token.rs)
- `NilAVClient` (src/contract_client/nilav_client.rs) - Unified client wrapping all three

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
│   ├── StakingOperators.t.sol
│   └── integration/       # Integration tests
├── script/                # Deployment scripts
│   ├── DeployRouter.s.sol
│   ├── DeployStaking.s.sol
│   ├── deploy.sh
│   ├── deploy_local.sh
│   ├── fund_operator.sh
│   └── transfer_and_stake.sh
├── lib/                   # Dependencies
│   ├── forge-std/
│   └── openzeppelin-contracts/
└── out/                   # Compiled artifacts
```

### StakingOperators Contract

Manages node operator registration and staking:

**Key Functions:**
- `registerOperator(string name)`: Register as an operator (requires TEST token approval)
- `activateOperator()`: Activate after staking required amount
- `deactivateOperator()`: Deactivate to stop receiving assignments (stake remains locked)
- `withdrawStake()`: Withdraw stake after deactivation
- `stakeOf(address)`: Query operator's staked amount
- `isActiveOperator(address)`: Check if operator is active
- `getActiveOperators()`: Get array of all active operator addresses
- `totalStaked()`: Get total amount staked across all operators

**Access Control:**
- Only the StakingOperators contract owner can set minimum stake amount
- Operators manage their own registration, activation, and deactivation

## Error Handling Patterns

### Contract Errors
The codebase decodes Solidity `Error(string)` reverts using `decode_error_string()` in `contract_client/mod.rs`. This extracts human-readable error messages from revert data.

### Verification Errors
The `VerificationError` enum provides detailed error context:
- `NilccUrl` / `BuilderUrl` - HTTP fetch failures
- `NilccJson` / `BuilderJson` - JSON parsing failures
- `FetchReport` - Failed to fetch attestation report bundle
- `VerifyReport` - Attestation report verification failed
- `MeasurementHash` - Failed to generate expected measurement hash
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
ROUTER_CONTRACT_ADDRESS=0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0 \
STAKING_CONTRACT_ADDRESS=0xe7f1725e7734ce288f8367e1bb143e90bb3f0512 \
TOKEN_CONTRACT_ADDRESS=0x5fbdb2315678afecb367f032d93f642f64180aa3 \
cargo run --bin nilav_node
```

4. Submit HTXs:
```bash
RPC_URL=http://localhost:8545 \
ROUTER_CONTRACT_ADDRESS=0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0 \
STAKING_CONTRACT_ADDRESS=0xe7f1725e7734ce288f8367e1bb143e90bb3f0512 \
TOKEN_CONTRACT_ADDRESS=0x5fbdb2315678afecb367f032d93f642f64180aa3 \
cargo run --bin nilcc_simulator
```

### Production Deployment

Configure `nilav_node.env` for deployed contracts:
```env
RPC_URL=https://rpc-nilav-shzvox09l5.t.conduit.xyz
ROUTER_CONTRACT_ADDRESS=0x4f071c297EF53565A86c634C9AAf5faCa89f6209
STAKING_CONTRACT_ADDRESS=0xYourStakingAddress
TOKEN_CONTRACT_ADDRESS=0xYourTokenAddress
PRIVATE_KEY=0xYourPrivateKey
```

Run the node:
```bash
cargo run --release --bin nilav_node
```

## CI/CD Workflows

### GitHub Actions

The repository includes automated workflows:

#### 1. CI Tests (`.github/workflows/test.yml`)

Runs on every push to `main` and all pull requests:

- **Rust format check**: `cargo fmt --all -- --check`
- **Smart contract compilation**: `forge build` in contracts directory
- **Rust tests**: `cargo test --all`
- **Caching**: Uses Rust cache and Foundry cache for faster builds

This workflow ensures code quality and prevents breaking changes.

#### 2. Docker Build & Push (`.github/workflows/docker.yml`)

Triggers on version tags (`v*.*.*`) or manual dispatch. Builds and pushes Docker image to GHCR:

- **Image**: `nilav_node` (currently only building node image)
- **Registry**: `ghcr.io/nillionnetwork/nilav/nilav_node`
- **Tags**:
  - `latest` - most recent tag from default branch
  - `v1.0.0`, `v1.0`, `v1` - semantic version tags
  - `sha-abc123` - commit SHA tags
- **Platforms**: linux/amd64, linux/arm64

```bash
# Images are automatically built and pushed on release tags
# To manually trigger: Go to Actions → docker-build-push → Run workflow

# Create and push a version tag to trigger build
git tag v1.0.0
git push origin v1.0.0

# Pull and use images
docker pull ghcr.io/nillionnetwork/nilav/nilav_node:latest
docker run --rm \
  -e RPC_URL=https://rpc-nilav-shzvox09l5.t.conduit.xyz \
  -e ROUTER_CONTRACT_ADDRESS=0x4f071c297EF53565A86c634C9AAf5faCa89f6209 \
  -e STAKING_CONTRACT_ADDRESS=0xYourStakingAddress \
  -e TOKEN_CONTRACT_ADDRESS=0xYourTokenAddress \
  -e PRIVATE_KEY=0xYourPrivateKey \
  ghcr.io/nillionnetwork/nilav/nilav_node:latest
```

**Build Process:**
1. Multi-stage Dockerfile compiles Rust binaries with Foundry support
2. Separate build stages for each binary (nilav_node, nilcc_simulator, monitor)
3. Minimal Debian-based runtime images for smaller image sizes
4. Build caches (GitHub Actions cache) for faster subsequent builds
5. Cross-platform builds for amd64 and arm64 architectures

## Important Notes

- **RPC URL Conversion**: HTTP URLs are automatically converted to WebSocket (http://localhost:8545 becomes ws://localhost:8545)
- **Private Keys**: Never commit private keys. Use environment variables or `.env` files (gitignored)
- **Reconnection Logic**: Nodes implement exponential backoff (1s → 60s max) with automatic historical event processing
- **HTX ID Derivation**: `htxId = keccak256(abi.encode(rawHTXHash, msg.sender, block.number))`
- **Node Selection**: Uses stake-weighted selection via Fisher-Yates shuffle with `block.prevrandao` seed (not production-secure)
- **Multi-Node Assignment**: Each HTX is assigned to multiple nodes (≥50% of total stake) for redundancy
- **Staking Requirement**: Nodes must stake TEST tokens and activate in StakingOperators contract to receive assignments
- **Contract Limitations**: The contracts lack timeout/reassignment logic and use `block.prevrandao` for randomness (not production-grade)

## Logging

Set log levels via `RUST_LOG` environment variable:
```bash
RUST_LOG=debug cargo run --bin nilav_node
RUST_LOG=nilav=info,ethers=warn cargo run --bin nilav_node
```

Log levels: `error`, `warn`, `info`, `debug`, `trace`
