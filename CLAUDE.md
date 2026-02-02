# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Nillion Blacklight is a decentralized verification network where nodes verify HTX (heartbeat transaction) attestations emitted by nilCC (Nillion's Confidential Compute layer) operators. The system coordinates node registration, HTX assignment, and verification results through smart contracts.

## Build and Test Commands

```bash
# Build entire workspace
cargo build

# Run all tests
cargo test --all

# Run tests for a specific crate
cargo test -p blacklight-node
cargo test -p blacklight-contract-clients

# Format check (CI enforces this)
cargo fmt --all -- --check

# Format code
cargo fmt --all
```

## Local Development with Docker

```bash
# Start local Anvil blockchain + nodes + simulators
docker compose up -d

# View logs
docker compose logs -f node
docker compose logs -f simulator

# Start with monitor TUI
docker compose --profile monitor up

# Rebuild after code changes
docker compose build && docker compose up -d

# Clean restart
docker compose down -v && docker compose up -d
```

## Architecture

### Workspace Structure

- **blacklight-node** - Main verification node binary. Listens for HTX assignments, verifies attestations, submits results.
- **keeper** - L1/L2 supervisor service that manages emissions and jailing policies.
- **monitor** - TUI for monitoring contract events (ratatui-based).
- **simulator** - Test tool with subcommands: `nilcc` (submit HTXs to contracts), `erc8004` (ERC-8004 validation requests).

### Shared Crates

- **blacklight-contract-clients** - Alloy-based clients for HeartbeatManager, StakingOperators, NilToken, and ProtocolConfig contracts.
- **erc-8004-contract-clients** - Alloy-based clients for IdentityRegistry and ValidationRegistry contracts.
- **contract-clients-common** - Shared transaction submission and event handling utilities.
- **chain-args** - CLI argument parsing for chain configuration.
- **state-file** - Node state persistence (wallet, node ID).

### Contract Interaction Flow

1. nilCC operators submit HTXs to HeartbeatManager contract
2. Contract randomly assigns HTXs to registered blacklight nodes
3. Nodes verify attestations (AMD certificate chain, measurement hashes)
4. Nodes submit verification results (Success/Failure/Inconclusive) back to contract

### HTX Types

The system handles three HTX formats (see `crates/blacklight-contract-clients/src/htx.rs`):
- **NillionHtx** - nilCC attestations with workload/builder measurements
- **PhalaHtx** - Phala network attestations
- **Erc8004Htx** - ERC-8004 agent validation requests

## Key Dependencies

- **alloy** - Ethereum interaction (contracts, providers, WebSocket subscriptions)
- **attestation-verification** - AMD attestation verification (from nilcc repo)
- **tokio** - Async runtime

## Environment Variables

Node configuration via environment:
- `RPC_URL` - Ethereum RPC endpoint
- `PRIVATE_KEY` - Node wallet private key
- `MANAGER_CONTRACT_ADDRESS`, `STAKING_CONTRACT_ADDRESS`, `TOKEN_CONTRACT_ADDRESS` - Contract addresses
- `RUST_LOG` - Log level (trace/debug/info/warn/error)

## Integration Tests

Integration tests in `blacklight-contract-clients` require a running Ethereum node:
```bash
TEST_RPC_URL=http://localhost:8545 cargo test -p blacklight-contract-clients -- --ignored
```
