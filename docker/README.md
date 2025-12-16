# NilUV Docker Setup

Complete Docker setup for running the NilUV system with Anvil blockchain, smart contracts, and multiple nodes.

## Services

The docker-compose setup includes:

1. **Anvil** - Local Ethereum testnet (port 8545)
2. **Contract Deployer** - Deploys the NilUVRouter smart contract
3. **Simulator** - Submits HTXs to the contract for verification
4. **5 NilUV Nodes** - Verify HTXs and respond with results
5. **Monitor** (optional) - TUI for monitoring contract events

## Quick Start

### Start the entire system:

```bash
docker-compose up -d
```

### View logs:

```bash
# All services
docker-compose logs -f

# Specific service
docker-compose logs -f node1
docker-compose logs -f simulator

# Multiple services
docker-compose logs -f node1 node2 node3
```

### Start with the monitor (interactive TUI):

```bash
docker-compose --profile monitor up
```

### Stop the system:

```bash
docker-compose down
```

### Clean up everything (including volumes):

```bash
docker-compose down -v
```

## Architecture

```
┌─────────────┐
│   Anvil     │  Port 8545
│ (Blockchain)│
└──────┬──────┘
       │
       ├── Deploy Contract
       │   (NilUVRouter)
       │
       ├──────────────────────────────┐
       │                              │
┌──────▼──────┐              ┌────────▼────────┐
│  Simulator  │              │  5 NilUV Nodes  │
│             │              │                 │
│ Submits HTXs├─────────────►│ Verify HTXs    │
│ periodically│              │ & submit results│
└─────────────┘              └─────────────────┘
```

## Node Addresses

Each node runs with a different Ethereum address from Anvil's default accounts:

- **Node 1**: `0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC`
- **Node 2**: `0x90F79bf6EB2c4f870365E785982E1f101E93b906`
- **Node 3**: `0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65`
- **Node 4**: `0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc`
- **Node 5**: `0x976EA74026E726554dB657fA54763abd0C3a0aa9`

- **Simulator**: `0x70997970C51812dc3A010C7d01b50e0d17dc79C8`
- **Deployer**: `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266`

## Configuration

### Environment Variables

You can customize the setup by creating a `.env` file:

```env
# Blockchain
RPC_URL=http://anvil:8545

# Logging
RUST_LOG=info  # Options: trace, debug, info, warn, error
```

### Log Levels

Control verbosity per service by modifying the `RUST_LOG` environment variable in `docker-compose.yml`:

- `RUST_LOG=trace` - Very verbose, all details
- `RUST_LOG=debug` - Debug information
- `RUST_LOG=info` - Normal operation (default)
- `RUST_LOG=warn` - Warnings and errors only
- `RUST_LOG=error` - Errors only

Example for a specific node:
```yaml
node1:
  environment:
    - RUST_LOG=debug  # More verbose for node1
```

## Development Workflow

### Rebuild after code changes:

```bash
docker-compose build
docker-compose up -d
```

### Rebuild specific service:

```bash
docker-compose build node1
docker-compose up -d node1
```

### Interact with Anvil directly:

```bash
# Get contract address
cat /tmp/niluv_contract_address.txt  # If saved locally

# Or from the shared volume
docker run --rm -v niluv_shared-data:/shared alpine cat /shared/contract_address.txt
```

## Monitoring

### Using the Monitor TUI:

```bash
docker-compose --profile monitor up monitor
```

Navigate with:
- `Tab` / `Shift+Tab` / `←→` - Switch tabs
- `↑↓` - Navigate lists
- `r` - Refresh data
- `q` - Quit

### Using Docker logs:

```bash
# Watch all node activity
docker-compose logs -f node1 node2 node3 node4 node5

# Watch simulator submissions
docker-compose logs -f simulator

# Follow all logs
docker-compose logs -f
```

## Troubleshooting

### Container won't start:

```bash
# Check logs
docker-compose logs <service-name>

# Restart a specific service
docker-compose restart <service-name>
```

### Contract address not found:

```bash
# Check if deployer ran successfully
docker-compose logs deployer

# Verify shared volume
docker run --rm -v niluv_shared-data:/shared alpine ls -la /shared
```

### Nodes not registering:

```bash
# Check if Anvil is accessible
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
  http://localhost:8545

# Check node logs
docker-compose logs node1
```

### Start fresh:

```bash
# Remove everything and start over
docker-compose down -v
docker-compose build --no-cache
docker-compose up -d
```

## Network Access

- **Anvil RPC**: `http://localhost:8545` (from host)
- **Anvil RPC**: `http://anvil:8545` (from containers)

## Data Persistence

Data is persisted in Docker volumes:

- `shared-data` - Contract address and deployment info
- `node1-data` through `node5-data` - Node state files (node IDs, signing keys)

To inspect node data:
```bash
docker run --rm -v niluv_node1-data:/data alpine ls -la /data
```

## Advanced Usage

### Scale nodes:

```bash
# Add more nodes by duplicating a node section in docker-compose.yml
# Make sure to use unique:
# - NODE_PRIVATE_KEY
# - NODE_ID
# - container_name
# - volume name
```

### Custom HTX data:

Mount your own HTX data file:
```yaml
simulator:
  volumes:
    - ./my-htxs.json:/app/data/htxs.json:ro
```

### Custom configuration:

Mount your own config file:
```yaml
simulator:
  volumes:
    - ./my-config.toml:/app/config/config.toml:ro
```
