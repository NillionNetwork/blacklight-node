# Nillion Auditor-Verifier (nilAV) <a href="https://github.com/NillionNetwork/nilAV/blob/main/LICENSE" target="_blank"><img src="https://img.shields.io/badge/License-MIT-green.svg" alt="MIT License"/></a>

A Rust-based HTX (Hash Transaction) verification system with real-time WebSocket event streaming with low latency.

## Overview

NilAV is a decentralized verification network where nodes verify HTX (Hash Transaction) submissions from nilCC operators. The system uses a smart contract to coordinate node registration, HTX assignment, and verification results.

**How it works:**
1. **nilCC Operators** submit HTXs to the NilAVRouter smart contract
2. **Smart Contract** randomly assigns HTXs to registered nilAV nodes
3. **nilAV Nodes** receive assignments via WebSocket, verify the HTX, and submit results
4. **Verification** checks if nilCC measurements exist in the builder's trusted index

**Key Architecture:**
- **Smart Contract** (Solidity): NilAVRouter manages node registration and HTX assignment, Staking Operator manages the staking and the TEST token is an ERC20 token to manage the code.
- **Rust Binaries**: Three independent executables for different roles in the network: nilAV node, nilCC simulator and a network monitor.
- **Event-Driven**: WebSocket streaming for real-time responsiveness


## 🚀 How to Run a Node

Follow these steps to run your own nilAV verification node on the network.

Choose your preferred method to run a nilAV node:

- **[Option A: Pre-built Docker Image (Recommended)](#option-a-using-pre-built-docker-image-recommended)** - Fastest setup, no compilation needed
- **[Option B: Build Docker Image from Source](#option-b-build-docker-image-from-source)** - Build your own Docker image
- **[Option C: Native Binary (Rust)](#option-c-native-binary-rust)** - Compile and run directly on your system
- **[Option D: Pre-built Binary Download](#option-d-pre-built-binary-download)** - Download compiled binaries from releases

All options require the same [Wallet Setup](#wallet-setup-all-options) process to fund your node with ETH and stake TEST tokens.

---


### Option A: Using Pre-built Docker Image (Recommended)

This is the quickest way to get started - no compilation required!

#### Prerequisites

1. **Install Docker** (if not already installed):

   **Linux:**
   ```bash
   curl -fsSL https://get.docker.com -o get-docker.sh
   sudo sh get-docker.sh
   ```

   **macOS:**
   ```bash
   brew install --cask docker
   # Or download from https://www.docker.com/products/docker-desktop
   ```

   **Windows:**
   - Download and install [Docker Desktop](https://www.docker.com/products/docker-desktop)

2. **Pull the latest nilAV node image:**
   ```bash
   docker pull ghcr.io/nillionnetwork/nilav/nilav_node:latest
   ```

#### Setup & Run

1. **Run the node for initial setup:**
   ```bash
   docker run -it --rm -v ./nilav_node:/app/ ghcr.io/nillionnetwork/nilav/nilav_node:latest
   ```

   On first run, the node will:
   - Generate a new wallet and save it to `./nilav_node/nilav_node.env`
   - Display your wallet address and balances
   - Stop and prompt you to fund your wallet with ETH and stake TEST tokens

2. **Continue to [Wallet Setup](#wallet-setup-all-options)** below to fund your wallet and stake TEST tokens before running the node.

---

### Option B: Build Docker Image from Source

Build your own Docker image from the repository.

#### Prerequisites

1. **Install Docker** (see Option A above)

2. **Install Git** (if not already installed):
   ```bash
   # Linux (Debian/Ubuntu)
   sudo apt update && sudo apt install git

   # macOS
   brew install git

   # Windows
   # Download from https://git-scm.com/download/win
   ```

#### Build & Run

1. **Clone the repository:**
   ```bash
   git clone https://github.com/NillionNetwork/nilAV.git
   git submodule update --init --recursive
   cd nilAV
   git submodule update --init --recursive
   ```

2. **Build the Docker image:**
   ```bash
   docker build -t ghcr.io/nillionnetwork/nilav/nilav_node:latest -f docker/Dockerfile --target nilav_node .
   ```

3. **Run the node for initial setup:**
   ```bash
   docker run -it --rm -v ./nilav_node:/app/ ghcr.io/nillionnetwork/nilav/nilav_node:latest
   ```

   On first run, the node will:
   - Generate a new wallet and save it to `./nilav_node/nilav_node.env`
   - Display your wallet address and balances
   - Stop and prompt you to fund your wallet with ETH and stake TEST tokens

4. **Continue to [Wallet Setup](#wallet-setup-all-options)** below to complete the setup.

---

### Option C: Compile from Source (For Developers)

Compile the binaries directly on your system.

#### Prerequisites

1. **Install Rust** (latest stable version):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```

2. **Install Foundry** (needed to compile the smart contract):
   ```bash
   curl -L https://foundry.paradigm.xyz | bash
   foundryup
   ```
   > 💡 Re-open your shell or run `source ~/.foundry/bin/foundryup` so `forge` is available in your `PATH`.

3. **Install Git** (if not already installed):
   ```bash
   # Linux (Debian/Ubuntu)
   sudo apt update && sudo apt install git

   # macOS
   brew install git
   ```

#### Build & Run

1. **Clone the repository:**
   ```bash
   git clone https://github.com/NillionNetwork/nilAV.git
   git submodule update --init --recursive
   cd nilAV
   git submodule update --init --recursive
   ```

2. **Compile the smart contract:**
   ```bash
   cd contracts
   forge build
   cd ..
   ```
   > 🔧 This step is required before building Rust binaries because the Rust code generates type-safe contract bindings from the compiled Solidity artifacts.

3. **Build the native binaries:**
   ```bash
   cargo build --release
   ```
   
   The compiled binaries will be available at `target/release/nilav_node`

4. **Run the node for initial setup:**
   ```bash
   ./target/release/nilav_node
   ```

   On first run, the node will:
   - Generate a new wallet and save it to `./nilav_node/nilav_node.env`
   - Display your wallet address and balances
   - Stop and prompt you to fund your wallet with ETH and stake TEST tokens

5. **Continue to [Wallet Setup](#wallet-setup-all-options)** below to complete the setup.

---

### Option D: Pre-built Binary Download

Download pre-compiled binaries for your platform without building from source.

> Note: This option currently does not support binary signing for Windows and macOS.

#### Prerequisites

No special prerequisites - just download and run! Pre-built binaries are available for:
- **Linux** (x86_64)
- **macOS** (Intel x86_64 and Apple Silicon ARM64)
- **Windows** (x86_64)

#### Download & Run

1. **Download the latest release:**

   Visit the [Releases page](https://github.com/NillionNetwork/nilAV/releases/latest) and download the archive for your platform:
   
   - `nilav-linux-x86_64.tar.gz` - Linux 64-bit
   - `nilav-macos-x86_64.tar.gz` - macOS Intel
   - `nilav-macos-aarch64.tar.gz` - macOS Apple Silicon
   - `nilav-windows-x86_64.zip` - Windows 64-bit

2. **Extract the archive:**

   **Linux/macOS:**
   ```bash
   # For Linux
   tar -xzf nilav-linux-x86_64.tar.gz
   cd nilav-linux-x86_64
   
   # For macOS Intel
   tar -xzf nilav-macos-x86_64.tar.gz
   cd nilav-macos-x86_64
   
   # For macOS Apple Silicon
   tar -xzf nilav-macos-aarch64.tar.gz
   cd nilav-macos-aarch64
   ```

   **Windows:**
   - Right-click the `.zip` file and select "Extract All..."
   - Or use PowerShell:
     ```powershell
     Expand-Archive nilav-windows-x86_64.zip -DestinationPath nilav-windows-x86_64
     cd nilav-windows-x86_64
     ```

3. **Make the binary executable (Linux/macOS only):**
   ```bash
   chmod +x nilav_node
   ```

4. **Run the node for initial setup:**

   **Linux/macOS:**
   ```bash
   ./nilav_node
   ```

   **Windows:**
   ```powershell
   .\nilav_node.exe
   ```

   On first run, the node will:
   - Generate a new wallet and save it to `./nilav_node/nilav_node.env`
   - Display your wallet address and balances
   - Stop and prompt you to fund your wallet with ETH and stake TEST tokens

5. **Continue to [Wallet Setup](#wallet-setup-all-options)** below to complete the setup.

> 💡 **Tip:** Add the binary directory to your `PATH` for easier access, or move the binary to a location already in your `PATH` (e.g., `/usr/local/bin` on Linux/macOS).

---

## Wallet Setup (All Options)

After choosing your installation method above, follow these steps to set up your node wallet.

#### 1. Initialize your node wallet

Once you've run the node the first time, it will have generated a to generate a fresh wallet and `nilav_node.env` file. On the first launch the program stops after creating the wallet so you can fund it before proceeding:

**If using Docker:**
```bash
docker run -it --rm \
  -v ./nilav_node.env:/app/nilav_node.env \
  ghcr.io/nillionnetwork/nilav/nilav_node:latest
```

**If compiled from source:**
```bash
cargo run --release --bin nilav_node
# Or if already built:
./target/release/nilav_node
```

**If using pre-built binary:**
```bash
# Linux/macOS
./nilav_node

# Windows
.\nilav_node.exe
```

This prints your on-chain address and creates `nilav_node.env` with the private key and RPC defaults. **Back up this file and keep it secret.**

```bash
╔══════════════════════════════════════════════════════════════════════════════════╗
║                        ✅  Account Created Successfully ✅                       ║
╠═════════════════════════════════════╦════════════════════════════════════════════╣
║                             Address ║ 0x1234123412341234123412341234123412341234 ║
╠═════════════════════════════════════╬════════════════════════════════════════════╣
║                         ETH Balance ║ 0.000000000000000000 ETH                   ║
╠═════════════════════════════════════╬════════════════════════════════════════════╣
║                         TEST Staked ║ 0.000000000000000000 TEST                  ║
╠═════════════════════════════════════╬════════════════════════════════════════════╣
║                             RPC URL ║ https://rpc-nilav-shzvox09l5.t.conduit.xyz ║
╠═════════════════════════════════════╩════════════════════════════════════════════╣
║     ❗ Please fund this address with ETH and stake TEST tokens to continue ❗    ║
╚══════════════════════════════════════════════════════════════════════════════════╝
```

#### 2. Get ETH Sepolia and TEST to stake. 

Right now, there is not an official flow to get TEST tokens and ETH Sepolia to run your node.

Please contact someone on #nilav on Slack to get your address staked and funded.

#### 3. Run the node with funded wallet and staked tokens

After confirming your ETH balance and TEST token stake, start the verifier again:

**If using Docker:**
```bash
docker run -it --rm \
  --env-file nilav_node.env \
  ghcr.io/nillionnetwork/nilav/nilav_node:latest
```

**If compiled from source:**
```bash
./target/release/nilav_node
```

**If using pre-built binary:**
```bash
# Linux/macOS
./nilav_node

# Windows
.\nilav_node.exe
```

The node loads `nilav_node.env` automatically and begins operation.

**What happens next:**

1. ✅ Node connects to the blockchain via WebSocket
2. ✅ Checks your account balance (must have ETH for gas)
3. ✅ Verifies you have staked TEST tokens (required to receive assignments)
4. ✅ Auto-registers with the NilAV contract (if not already registered)
5. ✅ Listens for HTX assignments in real-time
6. ✅ Processes assignments: fetches HTX data → verifies → submits result
7. ✅ Earns verification fees (if implemented in contract)

### Monitoring Your Node

Watch your node logs for activity:

```
INFO Node initialized
INFO WebSocket connection established balance=0.5 ETH
INFO Node already registered
INFO HTX verified htx_id=0x123... verdict=VALID tx_hash=0xabc...
```

**Log Levels:**
- **INFO**: Normal operations (connections, HTX processing)
- **WARN**: Recoverable issues (reconnections, failed verifications)
- **ERROR**: Critical failures (out of gas, network issues)

For detailed logging:
```bash
RUST_LOG=debug cargo run --release --bin nilav_node
```

---
