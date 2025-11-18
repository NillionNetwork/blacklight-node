#!/bin/bash
set -e

echo "Starting Anvil in background..."
anvil --host 0.0.0.0 --block-time 2 &
ANVIL_PID=$!

echo "Waiting for Anvil to be ready..."
sleep 5

# echo "Deploying contract..."
cd /app/contracts/nilav-router/
bash ./scripts/deploy-contract.sh

echo "Contract deployed. Keeping Anvil running..."
wait $ANVIL_PID
