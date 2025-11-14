#!/bin/bash

# Quick test script for interacting with deployed NilAVRouter
# Usage: ./test_local.sh [contract_address]

set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Default configuration
RPC_URL=${RPC_URL:-"http://127.0.0.1:8545"}
PRIVATE_KEY=${PRIVATE_KEY:-"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"}

# Contract address from argument or saved file
CONTRACT_ADDRESS=${1:-$(cat /tmp/nilav_contract_address.txt 2>/dev/null || echo "")}

if [ -z "$CONTRACT_ADDRESS" ]; then
    echo -e "${YELLOW}No contract address provided!${NC}"
    echo "Usage: $0 <contract_address>"
    echo "  or: RPC_URL=http://... PRIVATE_KEY=0x... $0 <contract_address>"
    exit 1
fi

echo "==================================================================="
echo -e "${GREEN}NilAV Router - Test Script${NC}"
echo "==================================================================="
echo "Contract: $CONTRACT_ADDRESS"
echo "RPC URL: $RPC_URL"
echo ""

export RPC_URL
export PRIVATE_KEY

CLI="./target/debug/contract_cli"

# Build if needed
if [ ! -f "$CLI" ]; then
    echo -e "${BLUE}Building CLI...${NC}"
    cargo build --bin contract_cli --quiet
fi

# Interactive menu
while true; do
    echo ""
    echo "==================================================================="
    echo "Select a test:"
    echo "==================================================================="
    echo "1. List all registered nodes"
    echo "2. Register a new node"
    echo "3. Check if address is a node"
    echo "4. Get node count"
    echo "5. Submit an HTX"
    echo "6. Get assignment details"
    echo "7. Respond to HTX (as assigned node)"
    echo "8. Query HTX events"
    echo "9. Query node events"
    echo "10. Deregister a node"
    echo "0. Exit"
    echo ""
    read -p "Enter choice: " choice

    case $choice in
        1)
            echo -e "${BLUE}Listing all registered nodes...${NC}"
            $CLI list-nodes
            ;;
        2)
            echo -e "${BLUE}Registering a new node...${NC}"
            echo "Common Anvil test addresses:"
            echo "  0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            echo "  0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
            echo "  0x90F79bf6EB2c4f870365E785982E1f101E93b906"
            read -p "Enter node address: " node_addr
            $CLI register-node $node_addr
            ;;
        3)
            echo -e "${BLUE}Checking if address is a node...${NC}"
            read -p "Enter address to check: " check_addr
            $CLI is-node $check_addr
            ;;
        4)
            echo -e "${BLUE}Getting node count...${NC}"
            $CLI node-count
            ;;
        5)
            echo -e "${BLUE}Submitting an HTX...${NC}"
            echo "Enter HTX data (JSON string, or press enter for default):"
            read -p "> " htx_data
            if [ -z "$htx_data" ]; then
                htx_data='{"workload_id":{"current":1,"previous":0},"test":"data"}'
            fi
            $CLI submit-htx "$htx_data"
            ;;
        6)
            echo -e "${BLUE}Getting assignment details...${NC}"
            read -p "Enter HTX ID (with 0x prefix): " htx_id
            $CLI get-assignment $htx_id
            ;;
        7)
            echo -e "${BLUE}Responding to HTX...${NC}"
            echo "Note: You must use the private key of the assigned node!"
            read -p "Enter HTX ID: " htx_id
            read -p "Enter result (true/false): " result
            read -p "Enter private key of assigned node: " node_key
            PRIVATE_KEY=$node_key $CLI respond-htx $htx_id $result
            ;;
        8)
            echo -e "${BLUE}Querying HTX events...${NC}"
            echo "1. Submitted events"
            echo "2. Assigned events"
            echo "3. Responded events"
            read -p "Select: " event_choice
            case $event_choice in
                1) $CLI events-submitted ;;
                2) $CLI events-assigned ;;
                3) $CLI events-responded ;;
            esac
            ;;
        9)
            echo -e "${BLUE}Querying node events...${NC}"
            $CLI events-nodes
            ;;
        10)
            echo -e "${BLUE}Deregistering a node...${NC}"
            read -p "Enter node address: " node_addr
            $CLI deregister-node $node_addr
            ;;
        0)
            echo -e "${GREEN}Exiting...${NC}"
            exit 0
            ;;
        *)
            echo -e "${YELLOW}Invalid choice!${NC}"
            ;;
    esac
done
