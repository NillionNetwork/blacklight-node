# Quick Start Guide - Building with Foundry

This is a condensed guide to get you started quickly. For complete documentation, see [README.md](README.md).

## 1. Install Foundry

```bash
curl -L https://foundry.paradigm.xyz | bash
foundryup
```

## 2. Start Local Node

```bash
anvil
```

Keep this terminal open. It provides a local Ethereum blockchain at `http://127.0.0.1:8545`.

## 3. Deploy Contract (New Terminal)

```bash
# From the project root: /Users/jcabrero/Repos/dev/nilav/nilAV
cd src/smart_contract/solidity

# Deploy
forge create NilAVRouter \
    --contracts . \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
```

## 4. Use Automated Scripts to test the contract

```bash
# From the project root
cd src/smart_contract/scripts

# Deploy and test everything
./deploy_local.sh

# Or interactive menu
./test_local.sh
```

## Common Commands Cheat Sheet

```bash
# Compile
forge build --contracts src/smart_contract/solidity

# Deploy
forge create NilAVRouter \
    --contracts src/smart_contract/solidity \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
    --broadcast
```

## Testing with Rust CLI

After deploying:

```bash
# From project root
cargo build --bin contract_cli

export RPC_URL="http://127.0.0.1:8545"
export PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

./target/debug/contract_cli list-nodes
./target/debug/contract_cli register-node 0x70997970C51812dc3A010C7d01b50e0d17dc79C8
./target/debug/contract_cli submit-htx '{"test":"data"}'
```

## Run Tests

```bash
# Install test dependencies
forge install foundry-rs/forge-std --no-commit

# Run all 41 tests
forge test

# Run with verbose output
forge test -vv

# Run with gas reporting
forge test --gas-report
```

See [TEST_GUIDE.md](TEST_GUIDE.md) for comprehensive testing documentation.


## Anvil Test Account #0

```
Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
Private Key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
Balance: 10000 ETH
```

This account is used for deployment and has full access to all test funds.
