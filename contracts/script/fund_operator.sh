#!/usr/bin/env bash

# Fund Operator Script
# ====================
# Mints TEST tokens, stakes them for an operator, and transfers ETH for gas fees.
#
# Usage: ./fund_operator.sh <operator_address> <token_amount> <eth_amount>
#
# See README_FUND_OPERATOR.md for detailed documentation.

set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Check arguments
if [ $# -lt 3 ]; then
    echo -e "${RED}Error: Missing required arguments${NC}"
    echo "Usage: $0 [env_file] <operator_address> <token_amount> <eth_amount>"
    echo ""
    echo "Arguments:"
    echo "  env_file          - (Optional) Path to environment file"
    echo "                      Default: script/fund_operator.env"
    echo "  operator_address  - The address of the operator to fund"
    echo "  token_amount      - Amount of TEST tokens to mint and stake (in wei)"
    echo "                      Example: 1000000000000000000000 (1000 tokens)"
    echo "  eth_amount        - Amount of ETH to transfer for gas fees (in ether)"
    echo "                      Example: 10 (10 ETH)"
    echo ""
    echo "Examples:"
    echo "  # Use default env file (script/fund_operator.env)"
    echo "  $0 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 1000000000000000000000 10"
    echo ""
    echo "  # Use custom env file"
    echo "  $0 my_custom.env 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 1000000000000000000000 10"
    exit 1
fi

# Parse arguments - check if first arg is an env file
if [ $# -eq 4 ]; then
    # 4 arguments: env_file operator_address token_amount eth_amount
    ENV_FILE="$1"
    OPERATOR_ADDRESS="$2"
    TOKEN_AMOUNT="$3"
    ETH_AMOUNT="$4"
elif [ $# -eq 3 ]; then
    # 3 arguments: operator_address token_amount eth_amount (use default env file)
    ENV_FILE="script/fund_operator.env"
    OPERATOR_ADDRESS="$1"
    TOKEN_AMOUNT="$2"
    ETH_AMOUNT="$3"
else
    echo -e "${RED}Error: Invalid number of arguments${NC}"
    exit 1
fi

# Load environment file if it exists
if [ -f "$ENV_FILE" ]; then
    echo -e "${GREEN}Loading environment from: ${NC}$ENV_FILE"
    source "$ENV_FILE"
    echo ""
else
    echo -e "${YELLOW}Warning: Environment file not found: ${NC}$ENV_FILE"
    echo "Will use existing environment variables if set."
    echo ""
fi

RPC_URL="${RPC_URL:-http://localhost:8545}"

# Check required environment variables
if [ -z "$PRIVATE_KEY" ]; then
    echo -e "${RED}Error: PRIVATE_KEY environment variable is required${NC}"
    echo "Please set it in your .env file or: export PRIVATE_KEY=0xYourKey"
    exit 1
fi

if [ -z "$TEST_TOKEN_ADDRESS" ]; then
    echo -e "${RED}Error: TEST_TOKEN_ADDRESS environment variable is required${NC}"
    echo "Please set it in your .env file or: export TEST_TOKEN_ADDRESS=0xYourAddress"
    exit 1
fi

if [ -z "$STAKING_OPERATORS_ADDRESS" ]; then
    echo -e "${RED}Error: STAKING_OPERATORS_ADDRESS environment variable is required${NC}"
    echo "Please set it in your .env file or: export STAKING_OPERATORS_ADDRESS=0xYourAddress"
    exit 1
fi

echo -e "${GREEN}=== Fund Operator Script ===${NC}"
echo -e "${YELLOW}RPC URL:${NC} $RPC_URL"
echo -e "${YELLOW}Operator:${NC} $OPERATOR_ADDRESS"
echo -e "${YELLOW}Token Amount:${NC} $TOKEN_AMOUNT TEST"
echo -e "${YELLOW}ETH Amount:${NC} $ETH_AMOUNT ETH"
echo -e "${YELLOW}TEST Token:${NC} $TEST_TOKEN_ADDRESS"
echo -e "${YELLOW}Staking Operators:${NC} $STAKING_OPERATORS_ADDRESS"
echo ""

# Export for Forge script
export OPERATOR_ADDRESS
export TOKEN_AMOUNT
export ETH_AMOUNT
export RPC_URL

# Run the Forge script
forge script script/FundOperator.s.sol:FundOperator \
    --rpc-url "$RPC_URL" \
    --broadcast \
    -vvv

echo ""
echo -e "${GREEN}✓ Operator funded successfully!${NC}"

