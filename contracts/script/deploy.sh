#!/bin/bash
set -e

echo "==================================================================="
echo "NilAV Contract Deployer"
echo "==================================================================="

# Check which contract to deploy
CONTRACT=${1:-"router"}
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

# Check required environment variables
if [ -z "$RPC_URL" ]; then
    echo "Error: RPC_URL environment variable is not set"
    echo "Example: export RPC_URL=http://localhost:8545"
    exit 1
fi

if [ -z "$PRIVATE_KEY" ]; then
    echo "Error: PRIVATE_KEY environment variable is not set"
    echo "Example: export PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    exit 1
fi

echo "Deploying $CONTRACT_NAME..."
echo "RPC URL: $RPC_URL"
echo ""

# Deploy using forge script
forge script $SCRIPT_PATH \
    --rpc-url $RPC_URL \
    --broadcast \
    --verify \
    -vvv

echo ""
echo "==================================================================="
echo "Deployment Complete!"
echo "==================================================================="
echo ""

# Extract contract addresses from broadcast files if jq is available
if command -v jq &> /dev/null; then
    BROADCAST_FILE=$(find broadcast -name "run-latest.json" 2>/dev/null | head -n 1)
    if [ -n "$BROADCAST_FILE" ]; then
        echo "Extracting contract addresses from: $BROADCAST_FILE"

        # Extract all contract addresses from the broadcast file (portable way)
        ADDRESSES=()
        while IFS= read -r addr; do
            ADDRESSES+=("$addr")
        done < <(jq -r '.transactions[]? | select(.contractAddress != null) | .contractAddress' "$BROADCAST_FILE" 2>/dev/null)

        if [ ${#ADDRESSES[@]} -eq 0 ]; then
            echo "Could not extract contract addresses. Check output above."
        else
            echo ""
            if [ "$CONTRACT" = "staking" ]; then
                # For staking deployment: first is TESTToken, second is StakingOperators
                if [ ${#ADDRESSES[@]} -ge 2 ]; then
                    TOKEN_ADDRESS="${ADDRESSES[0]}"
                    STAKING_ADDRESS="${ADDRESSES[1]}"
                    echo "✓ TESTToken deployed to:        $TOKEN_ADDRESS"
                    echo "✓ StakingOperators deployed to: $STAKING_ADDRESS"
                    echo ""
                    echo "Save these for your .env file:"
                    echo "  export TOKEN_ADDRESS=$TOKEN_ADDRESS"
                    echo "  export STAKING_ADDRESS=$STAKING_ADDRESS"
                    echo "  export RPC_URL=$RPC_URL"
                else
                    echo "Expected 2 contracts but found ${#ADDRESSES[@]}"
                    for addr in "${ADDRESSES[@]}"; do
                        echo "  $addr"
                    done
                fi
            else
                # For router deployment: single contract
                ROUTER_ADDRESS="${ADDRESSES[0]}"
                echo "✓ NilAVRouter deployed to: $ROUTER_ADDRESS"
                echo ""
                echo "Save this for your .env file:"
                echo "  export ROUTER_ADDRESS=$ROUTER_ADDRESS"
                echo "  export RPC_URL=$RPC_URL"
            fi
        fi
    fi
else
    echo "Install jq for automatic address extraction: brew install jq"
fi

echo ""
echo "Deployment details saved in:"
echo "  contracts/broadcast/Deploy*/*/run-latest.json"
echo ""
