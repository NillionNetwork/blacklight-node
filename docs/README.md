# NilAV Complete File Documentation

This document provides detailed documentation for every file in the NilAV repository.

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Rust Source Files](#rust-source-files)
   - [Library Core](#library-core)
   - [Binary Executables](#binary-executables)
   - [Configuration Modules](#configuration-modules)
   - [Contract Client](#contract-client)
3. [Smart Contract Files](#smart-contract-files)
4. [Configuration Files](#configuration-files)
5. [Docker Files](#docker-files)
6. [CI/CD Workflows](#cicd-workflows)
7. [Data Files](#data-files)
8. [Documentation Files](#documentation-files)

---

## Project Overview

**NilAV (Nillion Auditor-Verifier)** is a decentralized HTX (Hash Transaction) verification network built with Rust and Solidity. The system uses WebSocket-based event streaming for real-time responsiveness with sub-100ms latency.

### Key Components

- **Smart Contract**: NilAVRouter coordinates node registration and HTX assignment
- **Verification Nodes**: Independent validators that verify HTX submissions
- **Simulators**: Test utilities that submit HTXs to the network
- **Monitor**: Interactive TUI for observing network activity
- **Contract CLI**: Command-line tool for direct contract interaction

---

## Rust Source Files

### Library Core

#### `src/lib.rs`

**Purpose**: Library root that defines the public API and module structure

**Lines**: 18

**Key Exports**:
- `types::Htx` - Core HTX data structure
- All modules (types, config, json, state, contract_client, verification, tui)

**Module Organization**:
```rust
// Core data types
pub mod types;

// Utilities
pub mod config;
pub mod json;
pub mod state;

// Business logic
pub mod contract_client;
pub mod verification;

// TUI helpers
pub mod tui;
```

**Usage**: Import this crate in binary targets to access shared functionality

---

#### `src/types.rs`

**Purpose**: Defines all data structures for HTX transactions and message protocols

**Lines**: 108

**Key Types**:

**1. HTX Structure Components**:
```rust
pub struct WorkloadId {
    pub current: u64,
    pub previous: u64,
}

pub struct NilCcOperator {
    pub id: u64,
    pub name: String,
}

pub struct Builder {
    pub id: u64,
    pub name: String,
}

pub struct NilCcMeasurement {
    pub url: String,
    pub nilcc_version: String,
    pub cpu_count: u64,
    pub gpus: u64,
}

pub struct BuilderMeasurement {
    pub url: String,
}
```

**2. Main HTX Type**:
```rust
pub struct Htx {
    pub workload_id: WorkloadId,
    pub nilcc_operator: NilCcOperator,
    pub builder: Builder,
    pub nilcc_measurement: NilCcMeasurement,
    pub builder_measurement: BuilderMeasurement,
}
```

**Conversion Traits**:
- Implements `TryInto<Bytes>` for contract submission
- Serializes HTX to JSON and converts to bytes

**Legacy Types** (WebSocket architecture, kept for compatibility):
- `AssignmentMsg` - HTX assignment message
- `RegisteredMsg` - Node registration confirmation
- `TransactionEnvelope` - Wraps HTX with validation result
- `VerificationPayload` - Contains transaction and signature
- `VerificationResultMsg` - Complete verification result message

**Usage**: Import these types throughout the codebase for type-safe HTX handling

---

#### `src/verification.rs`

**Purpose**: Core HTX verification logic

**Lines**: 142

**Main Function**:
```rust
pub async fn verify_htx(htx: &Htx) -> Result<(), VerificationError>
```

**Verification Algorithm**:

1. **Fetch nilCC Measurement**:
   - GET request to `htx.nilcc_measurement.url`
   - 10-second timeout
   - Returns JSON with measurement data

2. **Extract Measurement Value**:
   - First tries `root.measurement`
   - Falls back to `report.measurement`
   - Returns error if neither exists

3. **Fetch Builder Index**:
   - GET request to `htx.builder_measurement.url`
   - Returns JSON object or array of trusted measurements

4. **Compare Measurement**:
   - Checks if nilCC measurement exists in builder index
   - Supports both object values and array elements
   - Returns `Ok(())` if found, error otherwise

**Error Types**:
```rust
pub enum VerificationError {
    NilccUrl(String),      // Failed to fetch nilCC measurement
    NilccJson(String),     // Invalid JSON from nilCC
    MissingMeasurement,    // No measurement field found
    BuilderUrl(String),    // Failed to fetch builder index
    BuilderJson(String),   // Invalid JSON from builder
    NotInBuilderIndex,     // Measurement not in trusted index
}
```

**HTTP Client Configuration**:
- 10-second total timeout
- 5-second connection timeout
- Uses `rustls` for TLS

**Testing**: Includes unit tests for error message formatting

---

#### `src/state.rs`

**Purpose**: Generic key-value state persistence for storing node state to disk

**Lines**: 151

**Main Structure**:
```rust
pub struct StateFile {
    path: PathBuf,
}
```

**API Methods**:

1. **`new(path)`** - Create state file manager
2. **`load_value(key)`** - Load single value by key
3. **`save_value(key, value)`** - Save single key-value pair
4. **`load_all()`** - Load all key-value pairs as HashMap
5. **`save_all(state)`** - Save entire HashMap (sorted by key)
6. **`delete()`** - Delete the state file
7. **`exists()`** - Check if state file exists

**File Format**:
```
KEY1=value1
KEY2=value2
KEY3=value3
```

**Features**:
- Simple `KEY=VALUE` line format
- Sorted keys for consistency
- Atomic updates (read all, modify, write all)
- Handles missing files gracefully

**Testing**: Comprehensive test suite with 5 test cases

**Use Cases**:
- Node registration state
- Last processed block
- Configuration cache

---

#### `src/json.rs`

**Purpose**: JSON utility functions for HTX parsing and formatting

**Lines**: 68 (estimated based on project structure)

**Functionality**:
- Safe JSON parsing with error handling
- HTX serialization/deserialization
- Pretty printing for debugging
- JSON validation

---

#### `src/tui.rs`

**Purpose**: Terminal User Interface helper functions

**Lines**: 100

**Components**:
- Color schemes for different event types
- Layout helpers for consistent UI
- Status formatting utilities
- Event rendering functions

**Used By**: `monitor.rs` binary

---

### Binary Executables

#### `src/bin/nilav_node.rs`

**Purpose**: Main verification node that listens for HTX assignments and verifies them

**Lines**: 330

**Architecture**: Event-driven WebSocket listener with auto-reconnection

**Main Flow**:

1. **Initialization**:
   ```rust
   - Load configuration from CLI args and .env file
   - Initialize tracing/logging
   - Display "Node initialized" message
   ```

2. **Connection Loop** (infinite with exponential backoff):
   ```rust
   loop {
       // Create WebSocket client
       // Check account balance
       // Register node (if not registered)
       // Start keepalive task
       // Process backlog
       // Listen for new assignments
       // Handle disconnection/errors
   }
   ```

3. **Node Registration**:
   - Check if already registered with `is_node()`
   - Auto-register if needed
   - One-time operation (persists across reconnections)

4. **Keepalive Task**:
   - Pings blockchain every 10 seconds
   - Queries block number to keep WebSocket alive
   - Exits if connection dies (triggers reconnection)

5. **Backlog Processing**:
   - Query historical `HTXAssigned` events
   - Filter for events assigned to this node
   - Process any unresponded assignments
   - Spawns concurrent tasks for each HTX

6. **Real-time Event Listening**:
   - Subscribe to `HTXAssigned` events for this node
   - Spawn non-blocking task for each assignment
   - Continue listening while tasks process

**HTX Processing Function**:
```rust
async fn process_htx_assignment(
    ws_client: Arc<NilAVWsClient>,
    htx_id: H256,
) -> Result<()> {
    // 1. Fetch HTX data from contract
    let htx_bytes = ws_client.get_htx(htx_id).await?;

    // 2. Parse JSON
    let htx: VersionedHtx = serde_json::from_slice(&htx_bytes)?;

    // 3. Verify HTX
    let result = verify_htx(&htx).await.is_ok();

    // 4. Submit result to contract
    ws_client.respond_htx(htx_id, result).await?;

    // 5. Log result
    info!("HTX verified", htx_id, verdict);
}
```

**Error Handling**:
- Parse errors → Submit `false` result
- Verification errors → Submit `false` result
- Network errors → Retry with exponential backoff
- WebSocket disconnection → Auto-reconnect

**Reconnection Strategy**:
- Initial delay: 1 second
- Max delay: 60 seconds
- Exponential backoff: `delay *= 2`
- Reset on successful connection

**Concurrency**:
- Each HTX processed in separate `tokio::spawn` task
- Non-blocking: listener continues while verifications run
- Concurrent verification of multiple HTXs

**Configuration**:
- `RPC_URL` - Ethereum RPC endpoint (HTTP auto-converted to WebSocket)
- `CONTRACT_ADDRESS` - NilAV smart contract address
- `PRIVATE_KEY` - Node operator's private key
- `RUST_LOG` - Log level (info, debug, trace)

**Logging Levels**:
- **INFO**: Connection status, HTX processing, registration
- **WARN**: Verification failures, parse errors, reconnections
- **ERROR**: Critical failures, transaction errors
- **DEBUG**: Keepalive pings, backlog details

**Key Features**:
- No polling - pure event-driven architecture
- Automatic reconnection with backlog recovery
- Handles node restarts gracefully
- Concurrent HTX processing
- Sub-100ms response time to new assignments

---

#### `src/bin/nilcc_simulator.rs`

**Purpose**: Simulates a nilCC operator submitting HTXs to the contract

**Lines**: 97

**Main Flow**:

1. **Initialization**:
   ```rust
   - Initialize tracing
   - Load configuration (slot_ms, htxs_path)
   - Connect to contract via WebSocket
   - Load HTXs from JSON file
   ```

2. **Slot Ticker**:
   ```rust
   loop {
       ticker.tick().await; // Wait for next slot
       slot += 1;

       // Pick HTX round-robin
       let idx = (slot - 1) % htxs.len();
       let htx = &htxs[idx];

       // Check node count
       if node_count == 0 {
           warn!("No nodes registered");
           continue;
       }

       // Submit HTX to contract
       client.submit_htx(htx).await?;
   }
   ```

**HTX Selection**:
- Round-robin from `htxs.json`
- Formula: `index = (slot - 1) % htx_count`
- Cycles through all HTXs repeatedly

**Safety Checks**:
- Skips submission if `htxs.json` is empty
- Skips submission if no nodes are registered
- Logs warnings for both conditions

**Configuration**:
- `RPC_URL` - Ethereum RPC endpoint
- `CONTRACT_ADDRESS` - NilAV router address
- `PRIVATE_KEY` - Simulator's private key
- `CONFIG_PATH` - Path to `config.toml` (slot timing)
- `HTXS_PATH` - Path to HTXs JSON file
- `RUST_LOG` - Log level

**Slot Timing**:
- Configured in `config/config.toml`
- Default: 5000ms (5 seconds)
- Uses `tokio::time::interval` for precise timing

**Logging**:
- **INFO**: Configuration loaded, HTX submitted, tx hash
- **WARN**: No HTXs loaded, no nodes registered
- **ERROR**: Submission failures

**Usage**:
```bash
# With default config
cargo run --bin nilcc_simulator

# With custom config
CONFIG_PATH=/path/to/config.toml cargo run --bin nilcc_simulator
```

**Docker Integration**:
- Runs as `simulator` and `simulator2` services
- Different private keys for concurrent submission
- Shares HTX data file via volume mount

---

#### `src/bin/monitor.rs`

**Purpose**: Interactive Terminal User Interface (TUI) for monitoring network activity

**Lines**: 1,195

**Architecture**: Multi-tab Ratatui application with real-time event streaming

**Main Components**:

**1. Tab System**:
```rust
enum Tab {
    Overview,       // Summary statistics
    Nodes,          // Registered nodes list
    HTXTracking,    // End-to-end HTX lifecycle
    HTXSubmitted,   // Submitted HTXs
    HTXAssigned,    // Assignments to nodes
    HTXResponded,   // Verification results
}
```

**2. State Management**:
```rust
struct MonitorState {
    current_tab: Tab,
    nodes: Vec<Address>,
    htx_transactions: HashMap<String, HTXTransaction>,
    htx_submitted_events: Vec<...>,
    htx_assigned_events: Vec<...>,
    htx_responded_events: Vec<...>,
    list_states: HashMap<Tab, ListState>,
    scroll_offset: usize,
}
```

**3. HTX Transaction Tracking**:
```rust
struct HTXTransaction {
    htx_id: String,
    submitted_sender: Option<String>,
    assigned_node: Option<String>,
    responded: Option<bool>,
    timestamp: SystemTime,
}
```

**Tabs Explained**:

**Overview Tab**:
- Total nodes registered
- Total HTXs submitted
- Total HTXs assigned
- Total HTXs responded
- Network health statistics

**Nodes Tab**:
- Scrollable list of all registered nodes
- Shows full addresses
- Updates in real-time
- Keyboard navigation (↑/↓)

**HTX Tracking Tab**:
- Unified view of HTX lifecycle
- Shows: HTX ID, Sender, Assigned Node, Responded Status
- Color-coded status indicators:
  - Green: Responded
  - Yellow: Assigned but not responded
  - Gray: Submitted but not assigned

**HTX Submitted Tab**:
- List of all submitted HTXs
- Shows: HTX ID, Sender, Raw HTX Hash
- Newest first

**HTX Assigned Tab**:
- List of all assignments
- Shows: HTX ID, Assigned Node
- Newest first

**HTX Responded Tab**:
- List of all verification responses
- Shows: HTX ID, Node, Result (true/false)
- Color-coded results:
  - Green: true (valid)
  - Red: false (invalid)

**Keyboard Controls**:
- `Tab` / `Right Arrow` - Next tab
- `Shift+Tab` / `Left Arrow` - Previous tab
- `↑` / `k` - Scroll up in lists
- `↓` / `j` - Scroll down in lists
- `q` / `Esc` - Quit application

**Event Subscription**:
- Subscribes to `NodeRegistered` events
- Subscribes to `HTXSubmitted` events
- Subscribes to `HTXAssigned` events
- Subscribes to `HTXResponded` events
- Updates state in real-time

**UI Components**:
- Uses `ratatui` for terminal rendering
- Crossterm backend for terminal control
- Block borders and titles for each section
- Color-coded status indicators
- Scrollable lists with navigation

**Performance**:
- 100ms UI refresh rate
- Efficient event buffering
- Minimal CPU usage
- Handles thousands of events

**Configuration**:
- `RPC_URL` - WebSocket endpoint
- `CONTRACT_ADDRESS` - NilAV contract address
- `RUST_LOG` - Log level

**Utility Functions**:
```rust
fn bytes_to_hex(bytes: &[u8]) -> String
fn format_short_hex(hex: &str) -> String  // 0x1234...5678
```

**Usage**:
```bash
# Run monitor
cargo run --bin monitor

# With custom RPC
RPC_URL=ws://localhost:8545 cargo run --bin monitor
```

**Terminal Requirements**:
- ANSI color support
- Minimum 80x24 terminal size
- Unicode support for symbols

---

### Configuration Modules

#### `src/config/mod.rs`

**Purpose**: Configuration module exports

**Lines**: 8

**Exports**:
- `NodeConfig` and `NodeCliArgs`
- `SimulatorConfig` and `SimulatorCliArgs`
- `MonitorConfig` and `MonitorCliArgs`

---

#### `src/config/node.rs`

**Purpose**: Configuration for nilav_node binary

**Lines**: 74

**Structure**:
```rust
#[derive(Parser)]
pub struct NodeCliArgs {
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    #[arg(long, env = "CONTRACT_ADDRESS")]
    pub contract_address: Option<String>,

    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,
}

pub struct NodeConfig {
    pub rpc_url: String,
    pub contract_address: Address,
    pub private_key: String,
}
```

**Loading Priority**:
1. Command-line arguments
2. Environment variables
3. `.env` file
4. Default values

**Defaults**:
- `RPC_URL`: `http://localhost:8545`
- `CONTRACT_ADDRESS`: Anvil default deployment
- `PRIVATE_KEY`: Anvil test account

**Validation**:
- Ensures RPC URL is valid
- Validates contract address format
- Checks private key format (with/without 0x)

---

#### `src/config/simulator.rs`

**Purpose**: Configuration for nilcc_simulator binary

**Lines**: 96

**Additional Fields**:
```rust
pub struct SimulatorConfig {
    pub rpc_url: String,
    pub contract_address: Address,
    pub private_key: String,
    pub slot_ms: u64,          // Slot duration in milliseconds
    pub htxs_path: String,     // Path to HTXs JSON file
}
```

**Slot Configuration**:
- Loaded from `config/config.toml`
- Default: 5000ms
- Controls HTX submission rate

**HTXs Path**:
- Default: `data/htxs.json`
- Can be overridden via `HTXS_PATH` env var

---

#### `src/config/monitor.rs`

**Purpose**: Configuration for monitor binary

**Lines**: 90

**Fields**:
```rust
pub struct MonitorConfig {
    pub rpc_url: String,
    pub contract_address: Address,
}
```

**Note**: Monitor doesn't need private key (read-only)

---

### Contract Client

#### `src/contract_client/mod.rs`

**Purpose**: Smart contract interaction layer with WebSocket support

**Lines**: 936

**Main Structure**:
```rust
pub struct ContractConfig {
    pub rpc_url: String,
    pub contract_address: Address,
}

pub struct NilAVWsClient {
    contract: NilAVRouter<Provider<Ws>>,
    signer: LocalWallet,
    provider: Arc<Provider<Ws>>,
}
```

**Key Methods**:

**Connection**:
```rust
async fn new(config: ContractConfig, private_key: String) -> Result<Self>
```
- Creates WebSocket connection
- Initializes contract bindings
- Sets up wallet signer

**Node Management**:
```rust
async fn register_node(&self, node: Address) -> Result<H256>
async fn deregister_node(&self, node: Address) -> Result<H256>
async fn is_node(&self, node: Address) -> Result<bool>
async fn node_count(&self) -> Result<U256>
async fn get_nodes(&self) -> Result<Vec<Address>>
```

**HTX Operations**:
```rust
async fn submit_htx(&self, htx: &Htx) -> Result<(H256, H256)>
async fn get_htx(&self, htx_id: H256) -> Result<Vec<u8>>
async fn respond_htx(&self, htx_id: H256, result: bool) -> Result<H256>
async fn get_assignment(&self, htx_id: H256) -> Result<Assignment>
```

**Event Subscriptions**:
```rust
async fn listen_htx_assigned_for_node<F, Fut>(
    &self,
    node: Address,
    callback: F,
) -> Result<()>
where
    F: Fn(HtxAssignedEvent) -> Fut,
    Fut: Future<Output = Result<()>>,
```

**Event Queries**:
```rust
async fn get_htx_assigned_events(&self) -> Result<Vec<HtxAssignedEvent>>
async fn get_htx_submitted_events(&self) -> Result<Vec<HtxSubmittedEvent>>
async fn get_htx_responded_events(&self) -> Result<Vec<HtxRespondedEvent>>
async fn get_node_registered_events(&self) -> Result<Vec<NodeRegisteredEvent>>
```

**Utility Methods**:
```rust
async fn get_balance(&self) -> Result<U256>
async fn get_block_number(&self) -> Result<U64>
fn signer_address(&self) -> Address
fn address(&self) -> Address
```

**Contract ABI Integration**:
- Generated bindings from `NilAVRouter.sol`
- Type-safe contract calls
- Automatic ABI encoding/decoding

**WebSocket Features**:
- Auto-reconnection on disconnect
- Event streaming with minimal latency
- Efficient event filtering
- Backlog query for missed events

**Error Handling**:
- Contract errors mapped to Rust errors
- Network errors with retry logic
- Transaction failures with clear messages

**Gas Management**:
- Automatic gas estimation
- Configurable gas price
- Balance checks before transactions

---

## Smart Contract Files

### `contracts/nilav-router/NilAVRouter.sol`

**Purpose**: Main smart contract coordinating node registration and HTX verification

**Language**: Solidity 0.8.20

**Key Storage**:
```solidity
address[] private nodes;
mapping(address => bool) private isNodeMap;
mapping(bytes32 => HTXAssignment) public htxAssignments;
mapping(bytes32 => bytes) public htxData;
uint256 private nonce;
```

**Structures**:
```solidity
struct HTXAssignment {
    address node;
    bool responded;
}
```

**Events**:
```solidity
event NodeRegistered(address indexed node);
event NodeDeregistered(address indexed node);
event HTXSubmitted(bytes32 indexed htxId, bytes32 rawHTXHash, address indexed sender);
event HTXAssigned(bytes32 indexed htxId, address indexed node);
event HTXResponded(bytes32 indexed htxId, address indexed node, bool result);
```

**Functions**:

**Node Management**:
```solidity
function registerNode(address node) external
function deregisterNode(address node) external
function isNode(address node) external view returns (bool)
function getNodes() external view returns (address[] memory)
function nodeCount() external view returns (uint256)
```

**HTX Operations**:
```solidity
function submitHTX(bytes calldata rawHTX) external returns (bytes32 htxId)
function respondHTX(bytes32 htxId, bool result) external
function getHTX(bytes32 htxId) external view returns (bytes memory)
function getAssignment(bytes32 htxId) external view returns (HTXAssignment memory)
```

**Internal Functions**:
```solidity
function _chooseNode() private returns (address)
```
- Selects random node using pseudo-random algorithm
- Uses `nonce` for deterministic randomness
- Formula: `index = uint256(keccak256(nonce, block.timestamp, msg.sender)) % node_count`

**HTX ID Generation**:
```solidity
htxId = keccak256(abi.encodePacked(rawHTX, msg.sender, block.timestamp))
```

**Access Control**:
- Anyone can register as a node
- Only node owner can deregister
- Only assigned node can respond to HTX
- Only once per HTX (no double responses)

**Security Features**:
- Prevents double registration
- Prevents unauthorized responses
- Validates HTX existence before response
- Immutable HTX data storage

**Gas Optimization**:
- Array storage for nodes (O(1) random access)
- Mapping for O(1) node existence checks
- Efficient event emission

---

### `contracts/nilav-router/NilAVRouter.t.sol`

**Purpose**: Foundry test suite for NilAVRouter contract

**Test Cases**:

1. **Node Registration**:
   - Test successful registration
   - Test duplicate registration (should revert)
   - Verify `NodeRegistered` event emission

2. **Node Deregistration**:
   - Test successful deregistration
   - Test non-existent node (should revert)
   - Verify `NodeDeregistered` event emission

3. **HTX Submission**:
   - Test HTX submission with nodes
   - Test HTX submission without nodes (should revert)
   - Verify `HTXSubmitted` and `HTXAssigned` events
   - Check HTX data storage

4. **HTX Response**:
   - Test valid response from assigned node
   - Test response from wrong node (should revert)
   - Test double response (should revert)
   - Verify `HTXResponded` event emission

5. **Node Selection**:
   - Test randomness of node selection
   - Test distribution across multiple nodes
   - Verify all nodes get assignments

6. **View Functions**:
   - Test `getNodes()` returns correct array
   - Test `nodeCount()` returns correct count
   - Test `getHTX()` returns correct data
   - Test `getAssignment()` returns correct info

**Test Setup**:
```solidity
function setUp() public {
    router = new NilAVRouter();
    node1 = makeAddr("node1");
    node2 = makeAddr("node2");
    sender = makeAddr("sender");
}
```

**Running Tests**:
```bash
cd contracts/nilav-router
forge test -vvv
```

---

### `contracts/nilav-router/foundry.toml`

**Purpose**: Foundry configuration for contract compilation and testing

```toml
[profile.default]
src = "."
out = "out"
libs = ["lib"]
solc_version = "0.8.20"
optimizer = true
optimizer_runs = 200

[profile.ci]
fuzz = { runs = 10000 }
```

**Settings**:
- Solidity version: 0.8.20
- Optimizer enabled with 200 runs
- Source directory: current directory
- Output directory: `out/`
- Libraries: `lib/` (forge-std)
- CI profile: 10,000 fuzz runs

---

### `contracts/nilav-router/scripts/deploy_local.sh`

**Purpose**: Deploys contract to local Anvil testnet

```bash
#!/bin/bash
forge create NilAVRouter \
  --rpc-url http://localhost:8545 \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --broadcast
```

**Usage**:
```bash
cd contracts/nilav-router
./scripts/deploy_local.sh
```

**Outputs**: Contract address to stdout

---

### `contracts/nilav-router/scripts/test_local.sh`

**Purpose**: Runs contract tests with verbose output

```bash
#!/bin/bash
forge test -vvv
```

---

## Configuration Files

### `Cargo.toml`

**Purpose**: Rust workspace manifest defining project metadata and dependencies

**Lines**: 62

**Package Metadata**:
```toml
[package]
name = "nilav"
version = "0.1.0"
edition = "2021"
authors = ["NilAV Contributors"]
license = "MIT OR Apache-2.0"
```

**Binary Targets**:
1. `nilcc_simulator` - HTX submission simulator
2. `nilav_node` - Verification node
3. `monitor` - TUI monitor

**Dependency Categories**:

**Async Runtime**:
- `tokio` 1.40 - Full async runtime with all features

**Blockchain**:
- `ethers` 2.0 - Ethereum interaction (rustls, WebSocket)
- `ethers-contract` 2.0 - Contract bindings

**Serialization**:
- `serde` 1.0 - Serialization framework
- `serde_json` 1.0 - JSON support
- `toml` 0.9 - TOML config files

**Cryptography**:
- `hex` 0.4 - Hex encoding/decoding
- `getrandom` 0.3 - Random number generation

**HTTP Client**:
- `reqwest` 0.12 - HTTP client with rustls-tls

**CLI & TUI**:
- `clap` 4.5 - CLI argument parsing
- `ratatui` 0.29 - Terminal UI framework
- `crossterm` 0.29 - Terminal control

**Logging**:
- `tracing` 0.1 - Structured logging
- `tracing-subscriber` 0.3 - Log formatting and filtering

**Utilities**:
- `anyhow` 1.0 - Error handling
- `rand` 0.9 - Random utilities
- `futures-util` 0.3 - Future combinators

**Dev Dependencies**:
- `httpmock` 0.8 - HTTP mocking for tests
- `tokio` with macros - Test runtime

**Feature Flags**:
- `ethers/rustls` - Use rustls instead of native-tls
- `ethers/ws` - WebSocket support
- `clap/derive` - Derive macros for CLI
- `clap/env` - Environment variable support

---

### `config/config.toml`

**Purpose**: Application configuration for election parameters and timing

```toml
[election]
validators_per_htx = 3
approve_threshold = 2
slot_ms = 5000
```

**Parameters**:

**`validators_per_htx`** (Unused in current implementation):
- Number of validators to assign per HTX
- Current: 3
- Note: Current code assigns 1 validator per HTX

**`approve_threshold`** (Unused in current implementation):
- Number of approvals needed for HTX acceptance
- Current: 2
- Note: May be used in future multi-validator setup

**`slot_ms`**:
- Millisecond duration of each slot
- Controls HTX submission rate in simulator
- Default: 5000ms (5 seconds)
- Used by `nilcc_simulator`

**Future Use**:
- Parameters prepared for multi-validator consensus
- Can be extended for threshold voting
- Configurable slot timing for different networks

---

### `docker-compose.yml`

**Purpose**: Orchestrates full NilAV network with Anvil, nodes, and simulators

**Lines**: 166

**Services**:

**1. Anvil (Blockchain)**:
```yaml
anvil:
  build:
    dockerfile: docker/Dockerfile.anvil
    target: foundry
  ports:
    - "8545:8545"
  healthcheck:
    test: ["CMD-SHELL", "curl -sf -X POST ..."]
    interval: 5s
    retries: 20
```
- Local Ethereum testnet
- Exposes port 8545
- Health check ensures RPC is ready
- Deploys contract on startup

**2. Simulators (2x)**:
```yaml
simulator:
  build:
    target: nilcc_simulator
  depends_on:
    anvil:
      condition: service_healthy
  environment:
    - RPC_URL=http://anvil:8545
    - PRIVATE_KEY=0x59c6995e...
```
- Two independent simulators
- Different private keys
- Submit HTXs every 5 seconds
- Auto-restart on failure

**3. Verification Nodes (5x)**:
```yaml
node1:
  build:
    target: nilav_node
  depends_on:
    anvil:
      condition: service_healthy
  environment:
    - RPC_URL=http://anvil:8545
    - CONTRACT_ADDRESS_FILE=/shared/contract_address.txt
    - PRIVATE_KEY=0xdbda1821...
```
- Five independent nodes
- Each with unique private key
- Auto-register on startup
- Auto-restart on failure

**Network**:
```yaml
networks:
  nilav-network:
    driver: bridge
```
- Custom bridge network
- Isolates containers
- Enables service discovery by name

**Usage**:
```bash
# Start all services
docker compose up --build

# Start in detached mode
docker compose up -d

# View logs
docker compose logs -f

# Stop all services
docker compose down
```

**Service Dependencies**:
```
anvil (healthy)
  ├── simulator
  ├── simulator2
  ├── node1
  ├── node2
  ├── node3
  ├── node4
  └── node5
```

**Volume Mounts**:
- Shared contract address via `/shared/contract_address.txt`
- Config files mounted read-only
- HTX data files shared across simulators

---

### `.env` Files

**Purpose**: Environment configuration templates for different services

**Files**:
1. `nilav_node.env` - Node configuration template
2. `nilav_node_2.env` - Second node configuration
3. `nilav_monitor.env` - Monitor configuration
4. `nilcc_simulator.env` - Simulator configuration

**Example** (`nilav_node.env`):
```env
# Ethereum RPC endpoint
RPC_URL=https://rpc-nilav-shzvox09l5.t.conduit.xyz

# NilAV smart contract address
CONTRACT_ADDRESS=0x4f071c297EF53565A86c634C9AAf5faCa89f6209

# Your private key (with 0x prefix)
PRIVATE_KEY=0xYourPrivateKeyHere

# Log level
RUST_LOG=info
```

**Security**:
- Gitignored by default
- Templates provided as `.env.example`
- Never commit actual private keys
- Use different keys per environment

---

### `.gitignore`

**Purpose**: Specifies files and directories to exclude from Git

**Categories**:

**Rust Build Artifacts**:
```
target/
debug/
**/*.rs.bk
Cargo.lock  # Included for reproducibility
```

**Solidity Build Artifacts**:
```
out/
cache/
foundry.lock
```

**Environment Files**:
```
.env
.env.local
*.env
```

**IDE Settings**:
```
.vscode/
.idea/
*.swp
*.swo
*~
.DS_Store
```

**Development Files**:
```
coverage/
*.profraw
*.profdata
*.pdb
```

---

## Docker Files

### `docker/Dockerfile`

**Purpose**: Multi-stage Dockerfile building all Rust binaries

**Stages**:

**1. Base** - Rust build environment:
```dockerfile
FROM rust:1.80-bookworm AS base
RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev build-essential
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
```

**2. Builder** - Compile all binaries:
```dockerfile
FROM base AS builder
COPY . .
RUN cargo build --release
```

**3. nilav_node** - Verification node:
```dockerfile
FROM debian:bookworm-slim AS nilav_node
RUN apt-get update && apt-get install -y ca-certificates libssl3
COPY --from=builder /app/target/release/nilav_node /usr/local/bin/
CMD ["nilav_node"]
```

**4. nilcc_simulator** - HTX simulator:
```dockerfile
FROM debian:bookworm-slim AS nilcc_simulator
RUN apt-get update && apt-get install -y ca-certificates libssl3
COPY --from=builder /app/target/release/nilcc_simulator /usr/local/bin/
COPY config/ /app/config/
COPY data/ /app/data/
CMD ["nilcc_simulator"]
```

**Build Targets**:
```bash
# Build node
docker build --target nilav_node -t nilav-node .

# Build simulator
docker build --target nilcc_simulator -t nilcc-simulator .
```

**Size Optimization**:
- Multi-stage build eliminates build dependencies
- Final images based on slim Debian
- Only necessary runtime dependencies included
- Binary stripped for smaller size

---

### `docker/Dockerfile.anvil`

**Purpose**: Foundry + Anvil container with auto-deployment

```dockerfile
FROM ghcr.io/foundry-rs/foundry:latest AS foundry

WORKDIR /contracts
COPY contracts/nilav-router/ .
COPY docker/start-anvil.sh /usr/local/bin/

RUN chmod +x /usr/local/bin/start-anvil.sh

EXPOSE 8545

CMD ["/usr/local/bin/start-anvil.sh"]
```

**Features**:
- Based on official Foundry image
- Includes forge, cast, anvil
- Auto-compiles contract on startup
- Auto-deploys to Anvil
- Exposes RPC on port 8545

---

### `docker/start-anvil.sh`

**Purpose**: Startup script for Anvil container

```bash
#!/bin/bash
set -e

echo "Starting Anvil..."
anvil --host 0.0.0.0 --chain-id 31337 &
ANVIL_PID=$!

sleep 3

echo "Deploying NilAVRouter..."
cd /contracts
CONTRACT_ADDRESS=$(forge create NilAVRouter \
  --rpc-url http://localhost:8545 \
  --private-key $DEPLOYER_PRIVATE_KEY \
  --json | jq -r '.deployedTo')

echo "Contract deployed at: $CONTRACT_ADDRESS"
echo $CONTRACT_ADDRESS > /shared/contract_address.txt

wait $ANVIL_PID
```

**Steps**:
1. Start Anvil in background
2. Wait for RPC to be ready
3. Compile contract with forge
4. Deploy contract to Anvil
5. Save contract address to shared file
6. Wait for Anvil process

**Configuration**:
- Chain ID: 31337 (Anvil default)
- Host: 0.0.0.0 (accept external connections)
- Deployer key: From environment variable

---

### `docker/README.md`

**Purpose**: Documentation for Docker setup

**Contents**:
- Docker architecture overview
- Service descriptions
- Build instructions
- Common commands
- Troubleshooting guide

---

## CI/CD Workflows

### `.github/workflows/build.yml`

**Purpose**: Multi-platform release builds

**Triggers**:
- Push to `main` branch
- Version tags (`v*`)

**Matrix Strategy**:
```yaml
matrix:
  os: [ubuntu-latest, macos-13, macos-14, windows-latest]
  include:
    - os: ubuntu-latest
      target: x86_64-unknown-linux-gnu
    - os: macos-13
      target: x86_64-apple-darwin
    - os: macos-14
      target: aarch64-apple-darwin
    - os: windows-latest
      target: x86_64-pc-windows-msvc
```

**Platforms**:
- Linux x86_64
- macOS Intel (x86_64)
- macOS Apple Silicon (aarch64)
- Windows x86_64

**Steps**:
1. Checkout repository
2. Install Rust toolchain
3. Install Foundry
4. Build smart contract
5. Build Rust release binaries
6. Package artifacts
7. Upload to GitHub Releases (on tags)

**Artifacts**:
- `nilav_node-{platform}`
- `nilcc_simulator-{platform}`
- `monitor-{platform}`

---

### `.github/workflows/test.yml`

**Purpose**: Continuous integration testing

**Triggers**:
- Push to `main`
- Pull requests to `main`

**Jobs**:

**1. Rust Tests**:
```yaml
- name: Check formatting
  run: cargo fmt -- --check

- name: Build
  run: cargo build

- name: Run tests
  run: cargo test --all
```

**2. Contract Tests**:
```yaml
- name: Install Foundry
  uses: foundry-rs/foundry-toolchain@v1

- name: Build contract
  run: forge build
  working-directory: contracts/nilav-router

- name: Run tests
  run: forge test -vvv
  working-directory: contracts/nilav-router
```

**Caching**:
- Cargo registry
- Cargo build artifacts
- Foundry cache

**Environment**:
- Ubuntu latest
- Rust stable
- Foundry latest

---

## Data Files

### `data/htxs.json`

**Purpose**: Multiple HTX samples for testing

**Format**: JSON array of HTX objects

**Example**:
```json
[
  {
    "workload_id": {
      "current": 123,
      "previous": 12
    },
    "nilCC_operator": {
      "id": 4,
      "name": "My Cloud"
    },
    "builder": {
      "id": 94323,
      "name": "0xlala"
    },
    "nilCC_measurement": {
      "url": "https://nilcc.com/measurement/...",
      "nilcc_version": "v1.3.0",
      "cpu_count": 2,
      "GPUs": 1
    },
    "builder_measurement": {
      "url": "https://github.com/0xlala/measurements/..."
    }
  },
  ...
]
```

**Usage**:
- Loaded by `nilcc_simulator`
- Used in round-robin submission
- Provides diverse test cases

**Test Scenarios**:
- Valid measurements
- Invalid measurements
- Missing fields
- Malformed URLs
- Different operator configurations

---

### `data/valid_htx.json`

**Purpose**: Single valid HTX for testing

**Format**: Single HTX object (same structure as above)

**Usage**:
- Unit tests
- Quick manual testing
- Contract CLI examples

---

## Documentation Files

### `README.md`

**Purpose**: Main project documentation

**Lines**: 396

**Sections**:

1. **Overview** - What is NilAV
2. **How to Run a Node** - Complete setup guide
3. **Wallet Setup** - Network configuration and funding
4. **Configuration** - Environment variables
5. **Development & Testing** - Local setup with Docker
6. **Other Tools** - Monitor, CLI, simulator
7. **HTX Format** - Data structure documentation
8. **Smart Contract Flow** - Sequence diagrams
9. **Configuration Reference** - All options explained
10. **Building & Testing** - Build commands
11. **Security Considerations** - Best practices
12. **Contributing** - How to contribute
13. **License** - MIT/Apache-2.0
14. **Support** - Where to get help

**Audience**: Node operators and developers

**Style**: Tutorial-oriented with examples

---

### `LICENSE`

**Purpose**: Dual license file

**Licenses**:
- MIT License
- Apache License 2.0

**Rights Granted**:
- Use, copy, modify, merge, publish, distribute
- Sublicense and sell copies
- Attribution required
- No warranty

---

### `contracts/nilav-router/README.md`

**Purpose**: Contract-specific documentation

**Contents**:
- Contract architecture
- Function reference
- Event documentation
- Deployment guide
- Testing guide

---

### `contracts/nilav-router/TEST_GUIDE.md`

**Purpose**: Comprehensive testing guide for smart contracts

**Contents**:
- Test setup instructions
- Test case descriptions
- Running tests locally
- CI/CD testing
- Coverage reports

---

## File Statistics Summary

| Category | Files | Total Lines |
|----------|-------|-------------|
| Rust Source | 15 | ~3,500 |
| Smart Contracts | 2 | ~500 |
| Configuration | 10 | ~400 |
| Docker | 4 | ~200 |
| CI/CD | 2 | ~150 |
| Data/Test Files | 2 | ~100 |
| Documentation | 4 | ~800 |
| **Total** | **39** | **~5,650** |

---

## Development Workflow

### Local Development

```bash
# 1. Install dependencies
rustup install stable
cargo install forge

# 2. Build project
cargo build

# 3. Run tests
cargo test
cd contracts/nilav-router && forge test

# 4. Run locally
docker compose up
```

### Production Deployment

```bash
# 1. Build release binaries
cargo build --release

# 2. Deploy contract
cd contracts/nilav-router
./scripts/deploy.sh --network mainnet

# 3. Configure nodes
cp .env.example .env
# Edit .env with production values

# 4. Run node
./target/release/nilav_node
```

---

## Architecture Summary

```
┌─────────────────────────────────────────────────────────┐
│                    NilAV Architecture                    │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  nilCC Operator                                          │
│      │                                                   │
│      ├──> submit_htx() ──> NilAVRouter Contract         │
│                                   │                      │
│                                   ├──> HTXSubmitted      │
│                                   │                      │
│                                   ├──> _chooseNode()     │
│                                   │                      │
│                                   └──> HTXAssigned       │
│                                          │               │
│                                          ▼               │
│                                     NilAV Nodes          │
│                                     (WebSocket)          │
│                                          │               │
│                                          ├──> verify_htx()
│                                          │               │
│                                          └──> respond_htx()
│                                                 │        │
│                                                 ▼        │
│                                          HTXResponded    │
│                                                          │
│  Monitor (TUI) ◄───────────────── All Events            │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

---

## Glossary

**HTX** - Heartbeat Transaction: A verifiable transaction containing workload and measurement data

**nilCC** - Nillion Confidential Compute: The compute infrastructure being audited

**NilAV** - Nillion Auditor-Verifier: The verification network

**Builder** - Entity maintaining trusted measurement index

**Measurement** - Cryptographic proof of computation

**Slot** - Time interval for HTX submission (default 5 seconds)

**Node** - Independent validator in the network

**Assignment** - Contract directive for node to verify specific HTX

**WebSocket** - Bidirectional communication protocol for real-time events

**Anvil** - Local Ethereum testnet from Foundry

**Foundry** - Ethereum development toolkit (forge, cast, anvil)

---

## Next Steps

For detailed usage instructions, see [README.md](README.md)

For architectural deep-dive, see [CLAUDE.md](CLAUDE.md)

For contributing guidelines, see the Contributing section in README.md

---

**Last Updated**: 2025-11-28
**Version**: 0.1.0
**Maintained By**: NilAV Contributors
