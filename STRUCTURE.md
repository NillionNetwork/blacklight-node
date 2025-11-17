# NilAV Project Structure

This document describes the structure of the NilAV HTX verification system.

## Directory Structure

```
nilAV/
├── Cargo.toml                    # Single package configuration
├── src/
│   ├── lib.rs                    # Library root (exports all modules)
│   ├── types.rs                  # Core data types (HTX, etc.)
│   ├── json.rs                   # Canonical JSON utilities
│   ├── config.rs                 # Configuration management
│   ├── crypto.rs                 # Ed25519 key management
│   ├── state.rs                  # State persistence
│   ├── verification.rs           # HTX verification logic
│   ├── tui.rs                    # TUI helper utilities
│   ├── contract_client/          # Smart contract interaction
│   │   └── mod.rs
│   └── bin/                      # Binary executables
│       ├── nilcc_simulator.rs    # HTX submission simulator
│       ├── nilav_node.rs         # Verification node
│       ├── contract_cli.rs       # Contract CLI (commands only)
│       └── monitor.rs            # Interactive TUI monitor
├── contracts/                    # Solidity smart contracts
│   └── nilav-router/
│       ├── NilAVRouter.sol
│       ├── NilAVRouter.t.sol
│       ├── foundry.toml
│       ├── scripts/
│       └── README.md
├── config/                       # Configuration files
│   └── config.toml
├── data/                         # Test HTX data
│   ├── htxs.json
│   └── valid_htx.json
├── docker/                       # Docker configurations
│   ├── Dockerfile.node
│   └── Dockerfile.server
└── out/                          # Compiled contract ABIs
```

## Module Organization

### Library Modules (src/)

#### Core Types (`types.rs`)
- **Purpose**: Centralized data type definitions
- **Key Types**:
  - `Htx` - Main HTX transaction structure
  - `WorkloadId`, `NilCcOperator`, `Builder`
  - `NilCcMeasurement`, `BuilderMeasurement`
  - Legacy WebSocket types (for compatibility)
- **Dependencies**: serde, ethers

#### JSON Utilities (`json.rs`)
- **Purpose**: Deterministic JSON serialization
- **Functions**:
  - `canonicalize_json()` - Recursively sort JSON keys
  - `stable_stringify()` - Serialize with sorted keys
- **Dependencies**: serde_json

#### Configuration (`config.rs`)
- **Purpose**: Load and manage application configuration
- **Types**: `Config`, `ElectionConfig`
- **Functions**: `load_config_from_path()`
- **Dependencies**: toml, serde, rand

#### Cryptography (`crypto.rs`)
- **Purpose**: Ed25519 key generation and management
- **Functions**:
  - `load_or_generate_signing_key()`
  - `signing_key_from_hex()`
  - `generate_signing_key()`
  - `verifying_key_from_signing()`
- **Dependencies**: ed25519-dalek, blake3, hex, rand

#### State Persistence (`state.rs`)
- **Purpose**: Generic key-value state file management
- **Type**: `StateFile` - Manages .env-style files
- **Methods**: `load_value()`, `save_value()`, `load_all()`, `save_all()`
- **Dependencies**: None!

#### HTX Verification (`verification.rs`)
- **Purpose**: Core HTX verification implementation
- **Function**: `verify_htx()` - Async verification
- **Error**: `VerificationError` - Detailed error cases
- **Dependencies**: reqwest, serde_json

#### Smart Contract Client (`contract_client/mod.rs`)
- **Purpose**: Type-safe Ethereum contract client
- **Type**: `NilAVClient` - Main client for contract interaction
- **Features**:
  - Auto-generated bindings via `abigen!`
  - Node management (register, deregister, query)
  - HTX submission and response
  - Event querying (all event types)
- **Dependencies**: ethers, ethers-contract

#### TUI Helpers (`tui.rs`)
- **Purpose**: Reusable TUI components
- **Types**: `Tab` - Tab navigation enum
- **Functions**: `poll_event()`, `is_key()`
- **Dependencies**: ratatui, crossterm

### Binary Executables (src/bin/)

#### `nilcc_simulator.rs` - HTX Submission Simulator
- **Binary Name**: `nilcc_simulator`
- **Purpose**: Submit HTXs to smart contract on interval
- **Features**:
  - Loads HTXs from JSON file
  - Round-robin selection
  - Slot-based submission
- **Usage**:
  ```bash
  cargo run --bin nilcc_simulator -- --rpc-url http://localhost:8545
  ```

#### `nilav_node.rs` - Verification Node
- **Binary Name**: `nilav_node`
- **Purpose**: Verify HTXs assigned by the smart contract
- **Features**:
  - Automatic node registration
  - Event-driven assignment processing
  - HTX verification
  - State persistence
  - Colored console output
- **Usage**:
  ```bash
  cargo run --bin nilav_node -- --rpc-url http://localhost:8545
  ```

#### `contract_cli.rs` - Contract Management CLI
- **Binary Name**: `contract_cli`
- **Purpose**: CLI tool for contract management operations
- **Features**:
  - Node registration/deregistration
  - HTX submission from JSON files
  - Query contract state
  - View event logs
- **Usage**:
  ```bash
  cargo run --bin contract_cli -- list-nodes
  cargo run --bin contract_cli -- submit-htx-file data/valid_htx.json
  ```

#### `monitor.rs` - Interactive TUI Monitor
- **Binary Name**: `monitor`
- **Purpose**: Full-screen interactive monitor for contract activity
- **Features**:
  - Real-time contract statistics
  - Tab-based navigation (Overview, Nodes, HTX events)
  - Node deregistration from UI
  - Auto-refresh every 5 seconds
- **Usage**:
  ```bash
  cargo run --bin monitor
  ```

## Building and Running

### Build Everything
```bash
cargo build --release
```

### Build Specific Binary
```bash
cargo build --bin nilcc_simulator --release
cargo build --bin nilav_node --release
cargo build --bin contract_cli --release
cargo build --bin monitor --release
```

### Run Binaries
```bash
# HTX Simulator
./target/release/nilcc_simulator

# Verification Node
./target/release/nilav_node

# Contract CLI
./target/release/contract_cli list-nodes

# Interactive Monitor
./target/release/monitor
```

### Run in Development
```bash
cargo run --bin nilcc_simulator
cargo run --bin nilav_node
cargo run --bin contract_cli -- list-nodes
cargo run --bin monitor
```

## Testing

```bash
# Test library
cargo test --lib

# Test specific binary
cargo test --bin nilav_node

# Test everything
cargo test
```

## Docker

```bash
# Build node image
docker build -f docker/Dockerfile.node -t nilav-node .

# Build simulator image
docker build -f docker/Dockerfile.server -t nilav-simulator .
```

## Module Dependencies

```
nilcc_simulator → config, contract_client, types
nilav_node      → crypto, state, verification, contract_client, types
contract_cli    → contract_client, types
monitor         → contract_client, types, tui

contract_client → types
verification    → types
config          → (standalone)
crypto          → (standalone)
state           → (standalone)
json            → (standalone)
tui             → (standalone)
types           → (standalone)
```

## Key Benefits of This Structure

1. **Simplicity**: Single `Cargo.toml`, single crate
2. **Fast Builds**: No cross-crate boundaries
3. **Easy Refactoring**: Move code between modules freely
4. **Clear Organization**: Modules group related functionality
5. **Standard Pattern**: Idiomatic Rust for applications
6. **Easy Navigation**: All source code under `src/`

## Importing from Modules

Within the crate, use relative imports:

```rust
// In a binary (src/bin/*.rs)
use nilav::{
    contract_client::{ContractConfig, NilAVClient},
    types::Htx,
    verification::verify_htx,
};

// Or use the re-exports
use nilav::{Config, Htx, load_config_from_path};
```

Within modules, use crate-relative imports:

```rust
// In a module (src/*.rs)
use crate::types::Htx;
use crate::contract_client::NilAVClient;
```

## Architecture

**Smart Contract-Centric Event-Driven System**

1. **Simulator** submits HTX → Smart Contract
2. **Smart Contract** assigns HTX to random node → Emits event
3. **Node** polls for events → Detects assignment
4. **Node** retrieves HTX from contract → Verifies → Submits result
5. **Smart Contract** records result → Emits event
6. **CLI Monitor** queries events for visualization

## Questions?

For issues or questions, please refer to individual module documentation or open an issue.
