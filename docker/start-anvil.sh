#!/bin/bash
set -e

echo "Starting Anvil in background..."
anvil --host 0.0.0.0 --block-time 2 --accounts 100 &
ANVIL_PID=$!

echo "Waiting for Anvil to be ready..."
sleep 5

# echo "Deploying contract..."
cd /app/contracts/
bash ./script/deploy.sh all

# Fund operators with N tokens and M ETH
## bash ./script/fund-operator.sh <env_file> <operator_address> <N> <M>
echo "Funding operators... (1/5)"
bash ./script/fund_operator.sh ./script/fund_operator.env 0x976EA74026E726554dB657fA54763abd0C3a0aa9 50 10

echo "Funding operators... (2/5)"
bash ./script/fund_operator.sh ./script/fund_operator.env 0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc 75 10

echo "Funding operators... (3/5)"
bash ./script/fund_operator.sh ./script/fund_operator.env 0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65 100 10

echo "Funding operators... (4/5)"
bash ./script/fund_operator.sh ./script/fund_operator.env 0x90F79bf6EB2c4f870365E785982E1f101E93b906 125 10

echo "Funding operators... (5/5)"
bash ./script/fund_operator.sh ./script/fund_operator.env 0x23618e81E3f5cdF7f54C3d65f7FBc0aBf5B21E8f 150 10



echo "Contract deployed. Keeping Anvil running..."
wait $ANVIL_PID
