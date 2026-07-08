# Blacklight L1 — single-machine runbook

How to spin up the complete Blacklight L1 system — contracts, keeper, verifier
nodes, and heartbeat submissions — from one machine, on a local devnet or on
Ethereum Sepolia. Written for an agent or engineer picking this up cold.
State as of 2026-07-07 (Phase 1 port complete; live Sepolia deployment running).

## What the system is

Stake-weighted committees verify TEE attestations ("heartbeats"/HTXs):

- **Contracts** (`blacklight-contracts`, Foundry): `HeartbeatManager` (rounds,
  votes, finalization), `StakingOperators` (staking + storage Fenwick tree for
  committee selection), `FenwickCommitteeSelector`, `RewardPolicy`,
  `JailingPolicy`, `EmissionsControllerL1` (mints test-NIL to RewardPolicy).
- **Node** (`blacklight-node/blacklight-node`): registers itself as an operator
  on boot, listens for `RoundStarted`, verifies the attestation in the HTX, and
  votes `submitVerdict` on-chain from its own wallet (pays its own gas).
- **Keeper** (`blacklight-node/keeper`): permissionless cranks — `startRound`
  (committee selection), `escalateOrExpire` (finalization), `distributeRewards`,
  jailing enforcement, `mintNextEpoch` (emissions).
- **Submitter**: any wallet holding `HEARTBEAT_SUBMITTER_ROLE`; posts raw HTX
  JSON bytes via `submitHeartbeat(bytes)`.

Round lifecycle: submit → keeper cranks `startRound` (or anyone after
`startRoundDelay`) → committee of nodes votes within `responseWindow` →
keeper finalizes → Verified / Invalid / Inconclusive(→escalate/expire) →
rewards distributed to correct voters.

## Prerequisites

- Rust toolchain (repo builds with vendored OpenSSL — no system openssl needed)
- Foundry (`forge`, `cast`, `anvil`)
- The four sibling repos checked out side by side (this file lives in
  `blacklight-node/devnet/`; the deploy script is consumed from
  `../blacklight-contracts`)
- Outbound HTTPS (nodes fetch attestation reports from the live workload,
  nilcc artifacts from GitHub, and AMD VCEK certs from kdsintf.amd.com)

## Part 1 — Local devnet (free, disposable, fast)

One command brings up anvil + deploy + keeper + N nodes + simulator:

```bash
cd blacklight-node
./devnet/run.sh --nodes 5                # stays up; ctrl-c tears down
./devnet/run.sh --nodes 5 --ci           # exit 0 after 1 finalized round
./devnet/run.sh --nodes 3 --ci --rounds 50   # soak: 50 finalized rounds
./devnet/run.sh --nodes 3 --htxs "$PWD/data/valid_htx_devnet.json" \
  --hook myscript.sh                     # drive a fixture; hook runs with the
                                         # devnet live (gets RUN_DIR/RPC env)
```

Logs land in `devnet/runs/<timestamp>/` (anvil, deploy, keeper, per-node,
simulator). Contract addresses: `devnet/runs/<ts>/contract_addresses.env`.

**E2E scenario suite (P1.M3):**

```bash
./devnet/refresh_valid_htx.sh   # re-derive the valid fixture from live nilgpt
./devnet/e2e.sh                 # all five scenarios (~25 min)
./devnet/e2e.sh jailed          # substring-filter a single scenario
```

Scenarios: valid→Verified (+emissions+rewards), false→Invalid,
inconclusive→escalation→expiry, inconclusive→expiry, non-participant jailed.

## Part 2 — Ethereum Sepolia

### Current live deployment (2026-07-07)

Addresses + deployment block: `devnet/sepolia-deployment.env` (committed).
Secrets (deployer key, RPC): `devnet/sepolia.env` (gitignored).
Operator wallet keys: `devnet/sepolia.wallets.env` (gitignored — keeper,
submitter, node1..5; generated with `cast wallet new`).

### Spin-up from scratch (or redeploy)

```bash
cd blacklight-node
set -a; source devnet/sepolia.env; set +a
PK="0x${SEPOLIA_PRIVATE_KEY#0x}"        # tolerate missing 0x prefix

# 1. Deploy (from the contracts repo; ~0.015 ETH at ~1 gwei)
cd ../blacklight-contracts
PRIVATE_KEY=$PK WRITE_OUTPUT=true forge script \
  script/deployment/DeployBlacklightFromConfig.s.sol \
  --sig 'run(string)' script/deployment/configs/core.l1.json \
  --rpc-url "$SEPOLIA_RPC_URL" --broadcast
# record: cp contract_addresses.env + block number into
# ../blacklight-node/devnet/sepolia-deployment.env

# 2. Wallets: generate keeper/submitter/node1..5 via `cast wallet new` into
#    devnet/sepolia.wallets.env (gitignored), as ROLE_ADDR= / ROLE_KEY= pairs.

# 3. Fund from the deployer (sized for a 2-week 4-hourly soak):
#    keeper 0.35 ETH, submitter 0.05, nodes 0.04 each.

# 4. Roles (deployer holds admin):
cast send $HEARTBEAT_MANAGER "grantRole(bytes32,address)" \
  $(cast keccak ROUND_STARTER_ROLE) $keeper_ADDR --private-key $PK --rpc-url $SEPOLIA_RPC_URL
cast send $HEARTBEAT_MANAGER "grantRole(bytes32,address)" \
  $(cast keccak HEARTBEAT_SUBMITTER_ROLE) $submitter_ADDR --private-key $PK --rpc-url $SEPOLIA_RPC_URL

# 5. Stake operators (deployer holds 1B TEST from the deploy; min stake 70k):
cast send $STAKE_TOKEN "approve(address,uint256)" $STAKING_OPERATORS <max> ...
cast send $STAKING_OPERATORS "stakeTo(address,uint256)" $nodeN_ADDR 100000000000 ...
# NOTE: fenwickTotal() stays 0 until nodes REGISTER (they do it on boot).
```

### Launching keeper + nodes

```bash
cd blacklight-node && cargo build -p keeper -p blacklight-node
set -a; source devnet/sepolia.env; source devnet/sepolia.wallets.env; source devnet/sepolia-deployment.env; set +a
SOAK=$PWD/devnet/runs/sepolia-soak; mkdir -p $SOAK

# Keeper: L1 single-chain mode, EIP-1559 fees on both legs
env -u PRIVATE_KEY \
  L2_RPC_URL=$SEPOLIA_RPC_URL L1_RPC_URL=$SEPOLIA_RPC_URL \
  L2_HEARTBEAT_MANAGER_ADDRESS=$HEARTBEAT_MANAGER \
  L2_STAKING_OPERATORS_ADDRESS=$STAKING_OPERATORS \
  L2_JAILING_POLICY_ADDRESS=$SLASHING_POLICY \
  L1_EMISSIONS_CONTROLLER_ADDRESS=$EMISSIONS_CONTROLLER_L1 \
  L1_SINGLE_CHAIN=true L2_FEE_STRATEGY=eip1559 L1_FEE_STRATEGY=eip1559 \
  TICK_INTERVAL_SECS=15 EMISSIONS_INTERVAL_SECS=300 \
  PRIVATE_KEY=$keeper_KEY OTEL_SDK_DISABLED=true RUST_LOG=info \
  nohup target/debug/keeper > $SOAK/keeper.log 2>&1 &

# Nodes (one per wallet; each self-registers on boot):
for n in node1 node2 node3 node4 node5; do
  k="${n}_KEY"; mkdir -p $SOAK/$n/artifacts $SOAK/$n/certs
  (cd $SOAK/$n && env -u PRIVATE_KEY \
    RPC_URL=$SEPOLIA_RPC_URL \
    MANAGER_CONTRACT_ADDRESS=$HEARTBEAT_MANAGER \
    STAKING_CONTRACT_ADDRESS=$STAKING_OPERATORS \
    TOKEN_CONTRACT_ADDRESS=$STAKE_TOKEN \
    FEE_STRATEGY=eip1559 PRIVATE_KEY=${!k} RUST_LOG=info \
    nohup ../../../../target/debug/blacklight-node \
      --artifact-cache ./artifacts --cert-cache ./certs > node.log 2>&1 &)
done

# wait for registration:
cast call $HEARTBEAT_MANAGER 'nodeCount()(uint256)' --rpc-url $SEPOLIA_RPC_URL  # -> 5
```

### Sending heartbeats

One-off (raw HTX = the JSON file bytes, hex-encoded):

```bash
RAW=0x$(xxd -p data/valid_htx.json | tr -d '\n')
cast send $HEARTBEAT_MANAGER "submitHeartbeat(bytes)" $RAW \
  --private-key $submitter_KEY --rpc-url $SEPOLIA_RPC_URL
```

Soak cadence (one heartbeat per 4h, budget-sized):

```bash
nohup devnet/sepolia-submit-loop.sh > $SOAK/submitter.log 2>&1 &
# INTERVAL=<seconds> overrides the 4h default
```

If `data/valid_htx.json` has gone stale (nilgpt redeployed), run
`devnet/refresh_valid_htx.sh` first — it re-derives `artifacts_version`,
`cpus/gpus`, and the compose hash from the live endpoints.

### Health checks

Prefer the local logs over RPC `eth_getLogs` (PublicNode throttles it):

```bash
S() { sed 's/\x1b\[[0-9;]*m//g' "$@"; }
S $SOAK/keeper.log | grep -E "Round started|Round finalized"   # outcome=1 is Verified
S $SOAK/node1/node.log | grep "HTX verification submitted"     # per-node verdicts
grep -cE ' ERROR ' $SOAK/keeper.log                            # should be 0
cast call $HEARTBEAT_MANAGER 'nodeCount()(uint256)' --rpc-url $SEPOLIA_RPC_URL
cast call $STAKING_OPERATORS 'fenwickTotal()(uint256)' --rpc-url $SEPOLIA_RPC_URL
cast from-wei $(cast balance $keeper_ADDR --rpc-url $SEPOLIA_RPC_URL)  # top up < 0.1
```

Verdict of a specific heartbeat: `heartbeats(bytes32)` → status
(1 Pending, 2 Verified, 3 Invalid, 4 Expired) + currentRound.

### Stopping / sweeping

```bash
pkill -f target/debug/keeper; pkill -f target/debug/blacklight-node
pkill -f sepolia-submit-loop
# funds: send balances from the wallets in sepolia.wallets.env back to the deployer
```

Do NOT stop nodes with plain SIGTERM if you want them to stay registered:
graceful shutdown deactivates the operator on-chain (deliberate product
behaviour). `kill -9` leaves registration intact (used by the jailing e2e test).

## Known pitfalls (each cost real debugging time)

1. **Private key prefix**: `vm.envUint` needs `0x…`; normalize with
   `PK="0x${KEY#0x}"`.
2. **AMD KDS 429s**: N nodes on one IP hammer kdsintf.amd.com for VCEK certs →
   `FetchCerts(429)` → inconclusive votes. Fix: copy a successful node's
   `certs/` into the others' cert caches (contents are identical for the same
   workload). Real multi-operator fleets don't share an IP.
3. **First-round warm-up**: nilcc artifact download on first verification can
   exceed the response window → first round often Inconclusive. Caches make
   subsequent rounds fast.
4. **PublicNode getLogs throttling**: polling `cast logs` every ~20s gets rate
   limited. Use the keeper/node logs for monitoring, or a dedicated RPC key.
5. **Stale valid-HTX fixture**: pins the LIVE nilgpt deployment's measurements;
   breaks silently when nilgpt redeploys → run `refresh_valid_htx.sh`.
6. **fenwickTotal()==0 after staking** is normal — the tree tracks the ACTIVE
   set; it fills as nodes register on boot.
7. **anvil rejects below-base-fee txs at submission** (no queueing like a real
   mempool) — stuck-tx tests use `--no-mining` + `anvil_dropTransaction`
   (see `crates/contract-clients-common/tests/eip1559_anvil.rs`).
8. **Reward budget exhaustion**: with `maxPayoutPerFinalize == 0` (uncapped),
   the FIRST `distributeRewards` after an emissions mint sweeps the entire
   spendable budget into that one round's outstanding rewards; every later
   conclusive round then starves ("Reward budget still unlocking, skipping")
   until the next emissions epoch. Fixed on Sepolia 2026-07-08:
   `setMaxPayoutPerFinalize(1e9)` (1,000 TEST/round) + a 100k TEST top-up to
   the RewardPolicy (the keeper auto-`sync()`s any deposit > 100 TEST). If
   rounds ever starve again, top up the same way — never fund from a node
   wallet's claimed rewards (rewards accrue to the STAKER, i.e. the deployer).
9. **Keeper stale reward context (fixed 2026-07-08)**: with several reward
   jobs pending in one tick the keeper cached `spendable` across jobs; after
   the first distribution consumed it, the remaining jobs reverted in
   pre-simulation and logged ERRORs. `keeper/src/l2/rewards.rs` now refreshes
   the cached context after each successful distribution.

## Measured L1 costs (Sepolia, 2026-07-07, k=5 committee)

| Item | Gas |
|---|---|
| `submitHeartbeat` (~700B HTX) | ~126k |
| `startRound` incl. Fenwick selection | ~422k |
| `submitVerdict` (per node) | ~87k (first vote of a round ~121k) |
| `distributeRewards` (5 voters) | ~225k |

Forge-measured N=1,000/k=25 numbers and the keeper-burn model live in
`blacklight-l1-prod-tech-specs/fees-and-costs.md` §1.
