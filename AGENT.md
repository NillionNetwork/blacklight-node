# AGENT.md — joining the Blacklight Sepolia network as a remote node operator

Instructions for an agent (or engineer) on a **second machine** joining the
live Blacklight deployment on Ethereum Sepolia for a two-machine network test.
Written to be followed cold, top to bottom.

**Your role:** run 5 verifier nodes that stake, register, receive committee
work, and vote on-chain.
**The coordinator's role (other machine):** runs the keeper, their own 5
nodes, submits heartbeats, and holds the deployer wallet. You do **not** run a
keeper or submitter, and you never need the deployer key.

Background on the system: `devnet/RUNBOOK.md` (component map, round
lifecycle, known pitfalls). Live contract addresses + deployment block are
committed in `devnet/sepolia-deployment.env`.

## 0. Prerequisites

- Rust toolchain (repo builds with vendored OpenSSL — no system openssl)
- Foundry (`cast` is used throughout)
- Outbound HTTPS (nodes fetch attestation reports from nilgpt.xyz, nilcc
  artifacts from GitHub, AMD VCEK certs from kdsintf.amd.com)
- This repo (`blacklight-node`) checked out on branch **`feat/l1-port`**

```bash
git checkout feat/l1-port && git pull
cargo build -p blacklight-node        # keeper NOT needed on this machine
```

## 1. Wallets and the coordination handshake

Nodes pay their own gas and must be staked **before** they boot (a node
self-registers on startup; registration requires stake ≥ 70,000 TEST —
`minOperatorStake` in the deploy config).

Generate 5 wallets and record them in the gitignored wallets file:

```bash
cd devnet
for n in 1 2 3 4 5; do cast wallet new; done
# write results as node1_ADDR=0x…/node1_KEY=0x… … node5_* pairs:
$EDITOR sepolia.wallets.env          # NEVER commit; file is gitignored
```

Create `devnet/sepolia.env` (also gitignored) with just an RPC endpoint:

```bash
echo 'SEPOLIA_RPC_URL=https://ethereum-sepolia-rpc.publicnode.com' > sepolia.env
# a dedicated RPC key is better; PublicNode works but throttles eth_getLogs
```

**Send the coordinator the 5 ADDRESSES only — never the private keys.**

The coordinator will then, from their side (deployer wallet):

```bash
# for each of your addresses (their commands, listed here for reference):
cast send $STAKE_TOKEN 'approve(address,uint256)' $STAKING_OPERATORS <amount> …
cast send $STAKING_OPERATORS 'stakeTo(address,uint256)' <your_nodeN_ADDR> 100000000000 …  # 100k TEST
cast send <your_nodeN_ADDR> --value 0.04ether …                                            # gas money
```

Staking is `stakeTo` — the **staker** (deployer) stakes on behalf of your
operator addresses, so no tokens ever need to move to your machine. (This also
means protocol rewards accrue to the staker, not to your node wallets.)

**Do not launch nodes until this check passes for all 5 addresses:**

```bash
set -a; source sepolia.env; source sepolia.wallets.env; source sepolia-deployment.env; set +a
for n in node1 node2 node3 node4 node5; do a="${n}_ADDR"
  echo "$n staked: $(cast call $STAKING_OPERATORS 'stakedAmount(address)(uint256)' ${!a} --rpc-url $SEPOLIA_RPC_URL)" \
       " eth: $(cast from-wei $(cast balance ${!a} --rpc-url $SEPOLIA_RPC_URL))"
done
# want: staked ≥ 70000000000 (70k TEST, 6 decimals) and eth ≥ ~0.03 on each
```

Note: `fenwickTotal()` will NOT reflect your stake yet — the Fenwick tree
tracks the ACTIVE set and only fills in when your nodes register on boot
(RUNBOOK pitfall #6).

## 2. Launch the 5 nodes

```bash
cd "$(git rev-parse --show-toplevel)"
set -a; source devnet/sepolia.env devnet/sepolia.wallets.env devnet/sepolia-deployment.env; set +a
RUN=$PWD/devnet/runs/sepolia-joint; mkdir -p $RUN

BEFORE=$(cast call $HEARTBEAT_MANAGER 'nodeCount()(uint256)' --rpc-url $SEPOLIA_RPC_URL)
echo "nodeCount before launch: $BEFORE"           # ← evidence line 1

for n in node1 node2 node3 node4 node5; do
  k="${n}_KEY"; mkdir -p $RUN/$n/artifacts $RUN/$n/certs
  (cd $RUN/$n && env -u PRIVATE_KEY \
    RPC_URL=$SEPOLIA_RPC_URL \
    MANAGER_CONTRACT_ADDRESS=$HEARTBEAT_MANAGER \
    STAKING_CONTRACT_ADDRESS=$STAKING_OPERATORS \
    TOKEN_CONTRACT_ADDRESS=$STAKE_TOKEN \
    FEE_STRATEGY=eip1559 PRIVATE_KEY=${!k} RUST_LOG=info \
    nohup ../../../../target/debug/blacklight-node \
      --artifact-cache ./artifacts --cert-cache ./certs > node.log 2>&1 &)
  echo "$n up"
done
```

## 3. Verify registration — first evidence block

Each node self-registers on boot (`registerOperator`, ~243k gas). Print this
so the coordinator can cross-check:

```bash
S() { sed 's/\x1b\[[0-9;]*m//g' "$@"; }   # strip ANSI from logs — use everywhere

echo "== REGISTRATION EVIDENCE =="
for n in node1 node2 node3 node4 node5; do a="${n}_ADDR"
  tx=$(S $RUN/$n/node.log | grep "Node registered successfully" | grep -o 'tx_hash=0x[0-9a-f]*' | cut -d= -f2)
  echo "$n ${!a} registerOperator: https://sepolia.etherscan.io/tx/$tx"
done
echo "nodeCount now: $(cast call $HEARTBEAT_MANAGER 'nodeCount()(uint256)' --rpc-url $SEPOLIA_RPC_URL) (was $BEFORE, expect +5)"
echo "fenwickTotal: $(cast call $STAKING_OPERATORS 'fenwickTotal()(uint256)' --rpc-url $SEPOLIA_RPC_URL)"
```

Tell the coordinator when nodeCount shows all 10. If a node log says
registration reverted, the usual cause is stake not yet landed — re-run the
step-1 check.

## 4. During the test — how work arrives and what to watch

The coordinator submits heartbeats; the keeper starts a round per heartbeat;
the round's committee votes within a 300 s response window; rounds take
~8–10 min to finalize and pipeline fine.

**Committee math for this test:** `baseCommitteeSize` is 25 and the selector
clamps to the active set, so with 10 registered operators **every one of your
nodes is selected for every round**. Each node should therefore log one vote
per heartbeat — that is the "everyone gets work" check.

Watch votes land (each is an on-chain `submitVerdict` tx from that node's
wallet, ~87k gas):

```bash
tail -f devnet/runs/sepolia-joint/node*/node.log | grep --line-buffered "HTX verification submitted"
# ✅ VALID / ❌ INVALID / ⚠️ INCONCLUSIVE prefix = the verdict that node voted
```

Health checks (use logs, **not** `cast logs` — PublicNode rate-limits
`eth_getLogs`, RUNBOOK pitfall #4):

```bash
for n in node1 node2 node3 node4 node5; do
  echo "$n votes: $(S $RUN/$n/node.log | grep -c 'HTX verification submitted')  errors: $(S $RUN/$n/node.log | grep -cE ' ERROR ')"
done
```

Known pitfalls that WILL bite (details in RUNBOOK §Known pitfalls):

- **First-round warm-up (#3):** the first verification downloads nilcc
  artifacts and can blow the response window → your nodes may vote late or
  inconclusive in round 1. Expected; caches make later rounds fast.
- **AMD KDS 429s (#2):** 5 nodes behind one IP hammer kdsintf.amd.com →
  `FetchCerts(429)` → inconclusive votes. Remedy: copy the cert cache from a
  node that succeeded into the others, then wait for the next round:
  `for n in node2 node3 node4 node5; do cp -r $RUN/node1/certs/* $RUN/$n/certs/; done`
- A verdict of ❌/⚠️ is only a problem if the coordinator says that heartbeat
  was supposed to be valid — the test deliberately mixes fixture flavors.
  Correct-minority voters are simply not rewarded; they are not slashed.

## 5. Final evidence block — print this when the coordinator says rounds are done

```bash
echo "== JOINT-TEST EVIDENCE ($(date -u +%Y-%m-%dT%H:%MZ)) =="
echo "-- votes per node (expect ≥1 per heartbeat submitted) --"
for n in node1 node2 node3 node4 node5; do a="${n}_ADDR"
  echo "--- $n ${!a} ---"
  S $RUN/$n/node.log | grep "HTX verification submitted" | grep -o 'tx_hash=0x[0-9a-f]*' | cut -d= -f2 \
    | while read tx; do echo "  https://sepolia.etherscan.io/tx/$tx"; done
done
echo "-- receipt spot-check (every status should be 1/success) --"
S $RUN/node1/node.log | grep "HTX verification submitted" | grep -o 'tx_hash=0x[0-9a-f]*' | cut -d= -f2 \
  | while read tx; do echo "$tx status=$(cast receipt $tx --rpc-url $SEPOLIA_RPC_URL --field status)"; done
echo "-- error tally (want all zeros) --"
for n in node1 node2 node3 node4 node5; do echo "$n ERROR lines: $(S $RUN/$n/node.log | grep -cE ' ERROR ')"; done
echo "-- remaining gas --"
for n in node1 node2 node3 node4 node5; do a="${n}_ADDR"
  echo "$n: $(cast from-wei $(cast balance ${!a} --rpc-url $SEPOLIA_RPC_URL)) ETH"; done
```

The coordinator verifies the matching half from their side: `RoundStarted`
committee membership includes your addresses, keeper log shows
`Round finalized` with the intended outcomes, and `distributeRewards` txs for
conclusive rounds.

Heartbeat status legend, if you read one back yourself
(`cast call $HEARTBEAT_MANAGER 'heartbeats(bytes32)(uint8,uint8)' <key>`):
1 Pending, 2 Verified, 3 Invalid, 4 Expired. Round outcome in keeper logs:
1 Verified, 2 Invalid, 0 Inconclusive.

## 6. Shutdown — only when the coordinator confirms all rounds finalized

Graceful SIGTERM makes each node **deactivate on-chain** (deliberate product
behaviour) — never `kill -9` on Sepolia (leaves the operator registered and
stops it from voting → it will look jailed/absent to the network):

```bash
pkill -TERM -f "target/debug/blacklight-node"
# then confirm your 5 deactivated (expect coordinator's count only):
cast call $HEARTBEAT_MANAGER 'nodeCount()(uint256)' --rpc-url $SEPOLIA_RPC_URL
echo "== SHUTDOWN EVIDENCE: nodeCount above should have dropped by 5 =="
```

Deactivation txs appear in each `node.log` on shutdown — include them in your
final message to the coordinator.

## 7. Hard rules

- Never commit `sepolia.env`, `sepolia.wallets.env`, or any private key; never
  send keys over chat — addresses only.
- Do not run the keeper or the submit loop; do not submit heartbeats — the
  coordinator drives those.
- Do not touch the deployer wallet or ask for its key; staking and funding are
  done by the coordinator via `stakeTo`/transfers to your addresses.
- Work on `feat/l1-port`. If you change code, commit to that branch only.
- Budget: expect < 0.01 ETH gas per node for a multi-round test (registration
  ~243k gas once + ~87k per vote). If a node drops below ~0.005 ETH, tell the
  coordinator before it fails mid-round.
