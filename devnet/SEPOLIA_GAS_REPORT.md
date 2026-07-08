# Blacklight gas report — Sepolia full-system test (2026-07-08)

Measured on the live Sepolia deployment running the **full system** on one machine:
submitter + keeper + 5 verifier nodes. Driver: `sepolia-submit-loop.sh` emitting
**5 heartbeats, one every 60 s** (`COUNT=5 INTERVAL=60`).

- **Contracts:** HeartbeatManager `0xca90d9Df…70F9`, StakingOperators `0xD0D5F9e9…7A13`
- **Committee size this run:** 5 (baseCommitteeSize clamps to the active set)
- **Outcome:** 5 rounds started, **25 verdicts** cast (5 nodes × 5 rounds), 4 rounds
  finalized + rewarded, 1 round left inconclusive (AMD-KDS-429 warm-up — infra, not gas).

## 1. Measured gas per operation (units — price-independent)

These are the numbers that matter for any projection; ETH-cost just multiplies them
by a gas price.

| Operation | Paid by | Avg gas | Samples | When |
|---|---:|---:|---:|---|
| `submitHeartbeat` | submitter | **125,672** | 5 | per heartbeat |
| `startRound` | keeper | **430,599** | 5 | per round |
| `submitVerdict` | node | **95,235** | 25 | per node per round |
| `distributeRewards` | keeper | **194,041** | 4 | per conclusive round |
| `escalateOrExpire` | keeper | **195,963** | 4 | only on inconclusive rounds |
| `registerOperator` | node | **237,297** | 5 | one-time, on node boot |

`deactivateOperator` (one-time, on graceful shutdown) was not exercised in this run;
it is of the same order as `registerOperator`.

## 2. Actual spend on Sepolia during this test

Effective gas price during the run was **~1.1–1.4 gwei**. Balance deltas (baseline → final):

| Role | Wallet | Spent (ETH) | Covers |
|---|---|---:|---|
| Submitter | `0x3fA9…4f08` (deployer)¹ | 0.00091775 | 5 × submitHeartbeat |
| Keeper | `0xea82…dCa8` | 0.00535784 | 5 startRound + 4 escalateOrExpire + 4 distributeRewards |
| node1 | `0x23e4…4A19` | 0.00059738 | 5 verdicts |
| node2 | `0xA848…9802` | 0.00055595 | 5 verdicts |
| node3 | `0xAFC0…4625` | 0.00069257 | 5 verdicts |
| node4 | `0xA59D…cEF4` | 0.00055601 | 5 verdicts |
| node5 | `0xEB7e…9Da0` | 0.00064817 | 5 verdicts |
| **Total** | | **≈ 0.00933 ETH** | whole 5-round test |

¹ Submission is gated by `HEARTBEAT_SUBMITTER_ROLE`; this run submitted from the
deployer. `submitHeartbeat` gas is identical regardless of which authorized wallet pays.

Per-node cost ≈ **0.00061 ETH** for 5 votes ⇒ ~0.00012 ETH/vote at ~1.2 gwei.

## 3. Cost of one healthy round (committee = 5, conclusive)

| Party | Gas | Operations |
|---|---:|---|
| Submitter | 125,672 | 1 × submitHeartbeat |
| Keeper | 624,640 | 1 × startRound + 1 × distributeRewards |
| Nodes (all 5) | 476,175 | 5 × submitVerdict |
| **Total on-chain / round** | **1,226,487** | |

(Inconclusive rounds add ~195,963 gas each escalation to the keeper.)

## 4. Mainnet projection

**Assumptions:** ETH = **$1,800**; committee = 5. Mainnet gas as of 2026-07-08 is
unusually low (~0.3–0.5 gwei), so three scenarios are shown. USD = gas × gwei × 1e-9 × 1800.

### Per operation (USD)

| Operation | Gas | @0.5 gwei | @5 gwei | @30 gwei |
|---|---:|---:|---:|---:|
| submitHeartbeat | 125,672 | $0.11 | $1.13 | $6.79 |
| startRound | 430,599 | $0.39 | $3.88 | $23.25 |
| submitVerdict (1 node) | 95,235 | $0.09 | $0.86 | $5.14 |
| distributeRewards | 194,041 | $0.17 | $1.75 | $10.48 |
| escalateOrExpire | 195,963 | $0.18 | $1.76 | $10.58 |
| registerOperator (one-time) | 237,297 | $0.21 | $2.14 | $12.81 |

### Per healthy round (committee = 5)

| Party | Gas | @0.5 gwei | @5 gwei | @30 gwei |
|---|---:|---:|---:|---:|
| Submitter | 125,672 | $0.11 | $1.13 | $6.79 |
| Keeper | 624,640 | $0.56 | $5.62 | $33.73 |
| Nodes (all 5) | 476,175 | $0.43 | $4.29 | $25.71 |
| **Total / round** | **1,226,487** | **$1.10** | **$11.04** | **$66.23** |

### Scaling out

- **Per node, per round:** 95,235 gas → $0.09 / $0.86 / $5.14.
- **Committee of N:** node cost scales linearly (keeper + submitter fixed per round);
  total/round ≈ 750,312 + N × 95,235 gas.
- **Throughput example** — 1 heartbeat/min continuous (~43,200 rounds/month) at
  committee 5: ~53.0 B gas/month ⇒ **≈ $47.7k/mo @0.5 gwei**, **$477k/mo @5 gwei**,
  **$2.86M/mo @30 gwei** (all parties combined).

## 5. Notes & caveats

- Gas **units** are the reliable output; ETH/USD figures rescale with live price. At
  today's ~0.3–0.5 gwei, mainnet cost ≈ Sepolia cost.
- `startRound` (~431k) dominates keeper cost — it does committee selection + snapshot.
- The 1 inconclusive round came from AMD-KDS 429 rate-limiting during cert warm-up
  (5 nodes behind one IP); it added `escalateOrExpire` gas but no `distributeRewards`.
  A production setup with warm cert caches would run all rounds conclusive.
- One-time role grants (`grantRole` ×2, paid by the deployer/admin) are setup, not
  per-round operating cost, and are excluded from the round totals above.

**Sources (live prices):**
[Etherscan gas tracker](https://etherscan.io/gastracker) ·
[Ethereum gas chart](https://etherscan.io/chart/gasprice) ·
[Coinbase ETH price](https://www.coinbase.com/price/ethereum) ·
[CoinMarketCap ETH](https://coinmarketcap.com/currencies/ethereum/)
