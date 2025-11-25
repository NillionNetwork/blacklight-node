#!/bin/bash
set -e

echo "==================================================================="
echo "NilAV Contract Deployer"
echo "==================================================================="

# Wait for Anvil to be ready
echo "Waiting for Anvil to be ready..."
max_retries=30
retry_count=0

while [ $retry_count -lt $max_retries ]; do
    if curl -s -X POST --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' ${RPC_URL} > /dev/null 2>&1; then
        echo "✓ Anvil is ready"
        break
    fi
    retry_count=$((retry_count + 1))
    echo "Waiting for Anvil... ($retry_count/$max_retries)"
    sleep 2
done

if [ $retry_count -eq $max_retries ]; then
    echo "Error: Anvil did not become ready in time"
    exit 1
fi

# Deploy the contract
echo "Deploying NilAVRouter contract..."
cd /app/contracts/nilav-router

DEPLOY_OUTPUT=$(forge create NilAVRouter \
    --rpc-url ${RPC_URL} \
    --private-key ${DEPLOYER_PRIVATE_KEY} \
    --broadcast \
    2>&1)

echo "Forge create output:"
echo "$DEPLOY_OUTPUT"

CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "Deployed to:" | awk '{print $3}')

if [ -z "$CONTRACT_ADDRESS" ]; then
    echo "Error: Failed to deploy contract"
    exit 1
fi

echo "✓ Contract deployed successfully!"
echo "Contract Address: $CONTRACT_ADDRESS"

echo "==================================================================="
echo "Deployment Complete!"
echo "==================================================================="

# Keep the container running if needed (optional)
# tail -f /dev/null
