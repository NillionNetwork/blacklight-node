# Fund Operator Script

This script automates the process of funding and setting up an operator node with TEST tokens and ETH.

## Quick Start

```bash
# 1. Deploy contracts (if not already done)
cd contracts
bash ./script/deploy_local.sh all

# 2. Create and configure environment file
cp script/fund_operator.env.example script/fund_operator.env
# The default addresses in .env match the local Anvil deployment

# 3. Load environment and fund operator
bash ./script/fund_operator.sh fund_operator.env 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 1000 10

# Done! The operator now has 1000 TEST tokens staked + 10 ETH for gas
```

