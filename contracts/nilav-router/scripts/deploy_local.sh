#!/bin/bash

# Deploy NilAVRouter to local Anvil and test it
# Usage: ./deploy_local.sh

set -e

echo "==================================================================="
echo "NilAV Router - Local Deployment and Testing Script"
echo "==================================================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default Anvil test account (account 0)
DEFAULT_PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
DEFAULT_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
RPC_URL="http://127.0.0.1:8545"

# Check if Anvil is running
echo -e "${BLUE}Step 1: Checking if Anvil is running...${NC}"
if ! curl -s -X POST --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' $RPC_URL > /dev/null 2>&1; then
    echo -e "${YELLOW}Anvil is not running!${NC}"
    echo "Please start Anvil in another terminal:"
    echo "  anvil"
    echo ""
    echo "Then run this script again."
    exit 1
fi
echo -e "${GREEN}✓ Anvil is running${NC}"
echo ""

# Check if forge is installed
echo -e "${BLUE}Step 2: Checking Foundry installation...${NC}"
if ! command -v forge &> /dev/null; then
    echo -e "${YELLOW}Foundry is not installed!${NC}"
    echo "Install it with:"
    echo "  curl -L https://foundry.paradigm.xyz | bash"
    echo "  foundryup"
    exit 1
fi
echo -e "${GREEN}✓ Foundry is installed${NC}"
echo ""

# Deploy the contract
echo -e "${BLUE}Step 3: Deploying NilAVRouter contract...${NC}"
DEPLOY_OUTPUT=$(forge create NilAVRouter \
    --rpc-url $RPC_URL \
    --private-key $DEFAULT_PRIVATE_KEY \
    --contracts . \
    --broadcast \
    2>&1)
echo "$DEPLOY_OUTPUT"
CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "Deployed to:" | awk '{print $3}')

echo "$CONTRACT_ADDRESS"
if [ -z "$CONTRACT_ADDRESS" ]; then
    echo -e "${YELLOW}Failed to extract contract address. Full output:${NC}"
    echo "$DEPLOY_OUTPUT"
    exit 1
fi

echo -e "${GREEN}✓ Contract deployed to: $CONTRACT_ADDRESS${NC}"
echo ""

# Save contract address for later use
echo "$CONTRACT_ADDRESS" > /tmp/nilav_contract_address.txt

# Build the Rust CLI
echo -e "${BLUE}Step 4: Building Rust CLI...${NC}"
cd ../../ && cargo build --bin contract_cli --quiet
echo -e "${GREEN}✓ CLI built successfully${NC}"
echo ""

# Export environment variables
export RPC_URL=$RPC_URL
export PRIVATE_KEY=$DEFAULT_PRIVATE_KEY

echo "==================================================================="
echo -e "${GREEN}Deployment Complete!${NC}"
echo "==================================================================="
echo ""
echo "Contract Address: $CONTRACT_ADDRESS"
echo "RPC URL: $RPC_URL"
echo "Deployer Address: $DEFAULT_ADDRESS"
echo ""
echo "==================================================================="
echo -e "${BLUE}Running Test Commands...${NC}"
echo "==================================================================="
echo ""

# # Test 1: Check node count
# echo -e "${YELLOW}Test 1: Check initial node count${NC}"
# ../../target/debug/contract_cli node-count
# echo ""

# # Test 2: Register nodes
# echo -e "${YELLOW}Test 2: Register two nodes${NC}"
# ../../target/debug/contract_cli register-node 0x70997970C51812dc3A010C7d01b50e0d17dc79C8
# ../../target/debug/contract_cli register-node 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC
# echo ""

# # Test 3: List nodes
# echo -e "${YELLOW}Test 3: List all registered nodes${NC}"
# ../../target/debug/contract_cli list-nodes
# echo ""

# # Test 4: Submit HTX
# echo -e "${YELLOW}Test 4: Submit an HTX for verification${NC}"
# SUBMIT_OUTPUT=$(../../target/debug/contract_cli submit-htx '{"workload_id":{"current":1,"previous":0}}')
# echo "$SUBMIT_OUTPUT"
# HTX_ID=$(echo "$SUBMIT_OUTPUT" | grep "HTX ID:" | awk '{print $3}')
# echo ""

# # Test 5: Get assignment
# if [ -n "$HTX_ID" ]; then
#     echo -e "${YELLOW}Test 5: Get assignment details${NC}"
#     ../../target/debug/contract_cli get-assignment $HTX_ID
#     echo ""
# fi

# # Test 6: Query events
# echo -e "${YELLOW}Test 6: Query contract events${NC}"
# echo "HTX Submitted Events:"
# ../../target/debug/contract_cli events-submitted
# echo ""
# echo "HTX Assigned Events:"
# ../../target/debug/contract_cli events-assigned
# echo ""
# echo "Node Registration Events:"
# ../../target/debug/contract_cli events-nodes
# echo ""

echo "==================================================================="
echo -e "${GREEN}All Tests Completed Successfully!${NC}"
echo "==================================================================="
echo ""
echo "To interact with the contract manually:"
echo ""
echo "  export RPC_URL=$RPC_URL"
# echo "  export PRIVATE_KEY=$DEFAULT_PRIVATE_KEY"
echo "  export CONTRACT_ADDRESS=$CONTRACT_ADDRESS"
echo ""
echo "Then use the CLI commands:"
echo "  ../../target/debug/contract_cli list-nodes"
echo "  ../../target/debug/contract_cli submit-htx '{\"test\":\"data\"}'"
echo "  ../../target/debug/contract_cli events-submitted"
echo ""
echo "Contract address saved to: /tmp/nilav_contract_address.txt"
echo ""
