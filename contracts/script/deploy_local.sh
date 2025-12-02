#!/bin/bash

# Deploy NilAVRouter to local Anvil and test it
# Usage: ./deploy_local.sh [router|staking]

set -e

echo "==================================================================="
echo "NilAV - Local Deployment Script (Anvil)"
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

# Check which contract to deploy
CONTRACT=$1
case $CONTRACT in
    router)
        SCRIPT_PATH="script/DeployRouter.s.sol:DeployRouter"
        CONTRACT_NAME="NilAVRouter"
        ;;
    staking)
        SCRIPT_PATH="script/DeployStaking.s.sol:DeployStaking"
        CONTRACT_NAME="StakingOperators"
        ;;
    *)
        echo "Usage: $0 [router|staking]"
        echo "  router  - Deploy NilAVRouter contract (default)"
        echo "  staking - Deploy StakingOperators contract"
        exit 1
        ;;
esac

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
echo -e "${BLUE}Step 3: Deploying $CONTRACT_NAME...${NC}"
export PRIVATE_KEY=$DEFAULT_PRIVATE_KEY
forge script $SCRIPT_PATH \
    --rpc-url $RPC_URL \
    --broadcast

# Extract contract addresses from broadcast files
echo ""
if command -v jq &> /dev/null; then
    BROADCAST_FILE=$(find broadcast -name "run-latest.json" 2>/dev/null | head -n 1)
    if [ -n "$BROADCAST_FILE" ]; then
        echo -e "${BLUE}Extracting contract addresses from: $BROADCAST_FILE${NC}"

        # Extract all contract addresses from the broadcast file (portable way)
        ADDRESSES=()
        while IFS= read -r addr; do
            ADDRESSES+=("$addr")
        done < <(jq -r '.transactions[]? | select(.contractAddress != null) | .contractAddress' "$BROADCAST_FILE" 2>/dev/null)

        if [ ${#ADDRESSES[@]} -eq 0 ]; then
            echo -e "${YELLOW}Could not extract contract addresses from broadcast file${NC}"
            echo "Check the forge output above for the deployed addresses."
        else
            echo ""
            if [ "$CONTRACT" = "staking" ]; then
                # For staking deployment: first is TESTToken, second is StakingOperators
                if [ ${#ADDRESSES[@]} -ge 2 ]; then
                    TOKEN_ADDRESS="${ADDRESSES[0]}"
                    STAKING_ADDRESS="${ADDRESSES[1]}"
                    echo -e "${GREEN}✓ TESTToken deployed to: $TOKEN_ADDRESS${NC}"
                    echo -e "${GREEN}✓ StakingOperators deployed to: $STAKING_ADDRESS${NC}"
                    echo ""
                    echo "$TOKEN_ADDRESS" > /tmp/nilav_token_address.txt
                    echo "$STAKING_ADDRESS" > /tmp/nilav_staking_address.txt
                    echo "Addresses saved to:"
                    echo "  /tmp/nilav_token_address.txt"
                    echo "  /tmp/nilav_staking_address.txt"
                else
                    echo -e "${YELLOW}Expected 2 contracts but found ${#ADDRESSES[@]}${NC}"
                    for addr in "${ADDRESSES[@]}"; do
                        echo "  $addr"
                    done
                fi
            else
                # For router deployment: single contract
                ROUTER_ADDRESS="${ADDRESSES[0]}"
                echo -e "${GREEN}✓ NilAVRouter deployed to: $ROUTER_ADDRESS${NC}"
                echo ""
                echo "$ROUTER_ADDRESS" > /tmp/nilav_router_address.txt
                echo "Address saved to: /tmp/nilav_router_address.txt"
            fi
        fi
    fi
else
    echo -e "${YELLOW}jq is not installed. Cannot extract contract addresses automatically.${NC}"
    echo "Install jq with: brew install jq (macOS) or apt-get install jq (Linux)"
    echo "Check the forge output above for the deployed addresses."
fi

echo ""
echo "==================================================================="
echo -e "${GREEN}Deployment Complete!${NC}"
echo "==================================================================="
echo ""

if [ "$CONTRACT" = "staking" ]; then
    echo "Deployed Contracts:"
    echo "  TESTToken:        ${TOKEN_ADDRESS:-Check forge output above}"
    echo "  StakingOperators: ${STAKING_ADDRESS:-Check forge output above}"
    echo ""
    echo "RPC URL: $RPC_URL"
    echo "Deployer: $DEFAULT_ADDRESS"
    echo ""
    echo "To interact with the contracts:"
    echo "  export RPC_URL=$RPC_URL"
    echo "  export PRIVATE_KEY=$DEFAULT_PRIVATE_KEY"
    echo "  export TOKEN_ADDRESS=$TOKEN_ADDRESS"
    echo "  export STAKING_ADDRESS=$STAKING_ADDRESS"
else
    echo "Deployed Contract:"
    echo "  NilAVRouter: ${ROUTER_ADDRESS:-Check forge output above}"
    echo ""
    echo "RPC URL: $RPC_URL"
    echo "Deployer: $DEFAULT_ADDRESS"
    echo ""
    echo "To interact with the contract:"
    echo "  export RPC_URL=$RPC_URL"
    echo "  export PRIVATE_KEY=$DEFAULT_PRIVATE_KEY"
    echo "  export ROUTER_ADDRESS=$ROUTER_ADDRESS"
fi
echo ""
