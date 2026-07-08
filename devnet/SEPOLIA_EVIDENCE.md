# Blacklight L1 on Ethereum Sepolia — on-chain evidence

First live deployment and verified round of the Phase 1 L1 port.
Deployed and exercised **2026-07-07**; all links resolve on
[sepolia.etherscan.io](https://sepolia.etherscan.io). Deployment block: **11224367**.

## Contract addresses

| Contract | Address |
|---|---|
| HeartbeatManager | [`0xca90d9Df0b1a61C51c4cDA20C065c26673BF70F9`](https://sepolia.etherscan.io/address/0xca90d9Df0b1a61C51c4cDA20C065c26673BF70F9) |
| StakingOperators (with storage Fenwick tree) | [`0xD0D5F9e905Ff86b5A864210b4E547cD020267A13`](https://sepolia.etherscan.io/address/0xD0D5F9e905Ff86b5A864210b4E547cD020267A13) |
| FenwickCommitteeSelector | [`0x709aC26099E0651969791461F268FE98D71cCcb3`](https://sepolia.etherscan.io/address/0x709aC26099E0651969791461F268FE98D71cCcb3) |
| ProtocolConfig | [`0xC79c2b77e21b9Fd92125083bb68E1AeEcaA5Bc01`](https://sepolia.etherscan.io/address/0xC79c2b77e21b9Fd92125083bb68E1AeEcaA5Bc01) |
| RewardPolicy | [`0x565A1F0bAB2f63d9c6C4Fc8cf305caD06B719176`](https://sepolia.etherscan.io/address/0x565A1F0bAB2f63d9c6C4Fc8cf305caD06B719176) |
| JailingPolicy | [`0xA6e447Fe9aD7e7bFDf77363161768e3d9557Fc61`](https://sepolia.etherscan.io/address/0xA6e447Fe9aD7e7bFDf77363161768e3d9557Fc61) |
| EmissionsControllerL1 | [`0xdeD9677338D78Afc66e203acaB80c4d4763841dF`](https://sepolia.etherscan.io/address/0xdeD9677338D78Afc66e203acaB80c4d4763841dF) |
| TEST token (stake/reward) | [`0x33216E882Aaa1E35e10a308420983Ae78772c2c4`](https://sepolia.etherscan.io/address/0x33216E882Aaa1E35e10a308420983Ae78772c2c4) |

Operator wallets: keeper `0x69FEBb0B92fA9c003253B40f3071b0041e95081a`,
submitter `0x3BdDc45da66820FD4450CC0280fAF25C04e6F023`, nodes
`0xe1A4…38c3`, `0xDB22…FE3F`, `0x4af4…605C`, `0xb3Ae…c4F4`, `0x49a8…D8B7`.

## The verified round (heartbeat `0x1545d16d…46a0`)

A live nilgpt.xyz TEE attestation, submitted, committee-selected, verified by
all 5 nodes, finalized **Verified**, and rewarded — end to end on L1:

| Step | Tx | Gas |
|---|---|---|
| 1. `submitHeartbeat` (submitter wallet) | [`0x24f35271…73aa`](https://sepolia.etherscan.io/tx/0x24f35271446324f34d24c63ef3c16c33b9f73e61f182edb428bbd4483e5273aa) | 125,672 |
| 2. `startRound` — keeper crank, Fenwick committee selection | [`0x33bc2941…c74d`](https://sepolia.etherscan.io/tx/0x33bc2941eeb9d3351188178a1dcaacd843ca243f77087af2674f876c004dc74d) | 421,678 |
| 3. `submitVerdict` node1 | [`0xe71c6713…af5a`](https://sepolia.etherscan.io/tx/0xe71c6713dc6567158b2d633395366ad3bb398c545772f9d40520a7e70b9daf5a) | 87,042 |
| 3. `submitVerdict` node2 | [`0x8578235e…2134`](https://sepolia.etherscan.io/tx/0x8578235eb4c499d9fcd7a513e951ec1762550ef998ecffec92594e25e5202134) | ~87k |
| 3. `submitVerdict` node3 | [`0xca864fea…7ce5`](https://sepolia.etherscan.io/tx/0xca864fea083fa75c77dc8c10fa303cbf0cf308c0d27ab91af58a29379b4c7ce5) | ~87k |
| 3. `submitVerdict` node4 | [`0x267f9493…c709`](https://sepolia.etherscan.io/tx/0x267f9493810cb434daba6efb7ed52161d78e7ad4e99013d0c6aafb4203aec709) | ~87k |
| 3. `submitVerdict` node5 | [`0xfac7d939…d6ef`](https://sepolia.etherscan.io/tx/0xfac7d939e5e4c6cb1c1966940b708c77f5dfd3b7216c554ec4eff8ffee06d6ef) | ~87k |
| 4. `distributeRewards` (5 correct voters paid) | [`0x8b4cdd2b…cbe9`](https://sepolia.etherscan.io/tx/0x8b4cdd2bac2301b62953fb6f73f34b3f487193f5f6b705806258a402d070cbe9) | 225,296 |

Also on chain:

- **Emissions epoch minted to RewardPolicy (atomic `sync()`):**
  [`0x36452e5f…8166`](https://sepolia.etherscan.io/tx/0x36452e5f07b43756297da3a898bb76e47252cff51bdfb3470d72e76a604b8166) — 214,030 gas
- Earlier rounds 1–2 finalized Inconclusive while node caches warmed (AMD KDS
  cert rate-limiting on a shared IP) — visible as `RoundFinalized(outcome=0)`
  events on the manager; the jailing/expiry semantics behaved as designed.
- Every receipt above: `status: SUCCESS`, blocks 11224393–11224701.

Browse everything at once: the
[HeartbeatManager events tab](https://sepolia.etherscan.io/address/0xca90d9Df0b1a61C51c4cDA20C065c26673BF70F9#events)
shows `HeartbeatEnqueued → RoundStarted → OperatorVoted ×5 → RoundFinalized →
RewardsDistributed` in sequence.

## Notable measured numbers (real L1, k=5)

- vote gas **87,042** — the forge model predicted ~87k
- keeper round legs: startRound 422k + finalize ~90k + rewards 225k
- soak running at a 4-hourly heartbeat cadence since 2026-07-07 ~20:20 UTC

## 2026-07-08 — resume + 8-heartbeat outcome-matrix test

Network resumed from graceful shutdown (all 5 operators self-re-registered on
boot: `nodeCount` 0 → 5), then 8 heartbeats submitted from the submitter
wallet at ~2-minute spacing with a known outcome mix. Rounds pipelined and
finalized in ~8–10 min each. **8/8 outcomes matched intent**; all 40 committee
votes (5 nodes × 8 rounds) landed; keeper and node logs show **zero ERROR and
zero WARN lines** for the session. Every tx below: `status: SUCCESS` confirmed
via `cast receipt`. Submits in blocks 11227455–11227527; rewards in
11227907–11227923.

### Outcome matrix

Fixtures: *valid* = refreshed `data/valid_htx.json` (live nilgpt.xyz
deployment); *invalid* = same with corrupted `docker_compose_hash` → nodes
vote failure; *non-verifying* = same with `workload_measurement.url` set to
`http://127.0.0.1:9/report` → fetch rejected → nodes vote inconclusive, and
with `maxEscalations=0` the round expires the heartbeat.

| # | Flavor | submitHeartbeat tx | Heartbeat key | Intended → actual |
|---|---|---|---|---|
| 1 | valid | [`0x02cfe35e…21f9`](https://sepolia.etherscan.io/tx/0x02cfe35eb02a261ef0cd46eb14f67f3f242971f7c7149ce898acab96edd221f9) | `0x8e5e771c…6202` | Verified → **outcome=1, status 2 (Verified)** |
| 2 | invalid | [`0x7a4911d5…6a2f`](https://sepolia.etherscan.io/tx/0x7a4911d56d78d8a57bbf5a8da67ebefb21d93351bd6df3f8b68d9954cf6d6a2f) | `0x69690adf…2827` | Invalid → **outcome=2, status 3 (Invalid)** |
| 3 | non-verifying | [`0x79d81b5a…5279`](https://sepolia.etherscan.io/tx/0x79d81b5af2a7b10fd5a97f9a31ae8669fd74295c795791670373d7e69c9a5279) | `0xb6196201…a850` | Inconclusive → **outcome=0, status 4 (Expired)** |
| 4 | valid | [`0xe8722539…1313`](https://sepolia.etherscan.io/tx/0xe872253937764eb871ede807662ca358f01d3945700a688f3b9e775585d71313) | `0x23524655…30de` | Verified → **outcome=1, status 2 (Verified)** |
| 5 | invalid | [`0x7d119a32…0d0f`](https://sepolia.etherscan.io/tx/0x7d119a32d33ec182120dca6c24b5371aa6a7d90d32e00a98d928e04b54130d0f) | `0x6e919d92…a4e8` | Invalid → **outcome=2, status 3 (Invalid)** |
| 6 | non-verifying | [`0x45777dcc…dd01`](https://sepolia.etherscan.io/tx/0x45777dcc6c545e0650cf5e4afe31902438dfdea09474953aa314d84c910ddd01) | `0x74b1d7f9…85bb` | Inconclusive → **outcome=0, status 4 (Expired)** |
| 7 | valid | [`0x0d36b317…648b`](https://sepolia.etherscan.io/tx/0x0d36b317596fbf56ebffe7952f7d0633b540591c1266e899db67c3f291d7648b) | `0x4b81f92d…1630` | Verified → **outcome=1, status 2 (Verified)** |
| 8 | valid | [`0x900dda4b…ec7b`](https://sepolia.etherscan.io/tx/0x900dda4bfa042b9b56a4888089454b139bd8aed1ea89d2233684ea63cef0ec7b) | `0x28c4db15…4a8d` | Verified → **outcome=1, status 2 (Verified)** |

`submitHeartbeat` gas: 125,672 (125,101 for the shorter non-verifying HTX).
On-chain heartbeat status read back via `heartbeats(bytes32)` for all 8.

### One verdict tx per outcome class (node1 wallet)

| Class | submitVerdict tx | Gas | Node log evidence |
|---|---|---|---|
| success | [`0x69f51933…2ac9`](https://sepolia.etherscan.io/tx/0x69f51933c349f2cdf4272c9b94135be026811d9a1bd5b4b87d45978deac42ac9) | 87,042 | `✅ VALID HTX verification submitted` |
| failure | [`0x51c51c27…926c`](https://sepolia.etherscan.io/tx/0x51c51c27b8789c39556f90d304a2cfb838d105b29297752dc7a49ed3552d926c) | 87,067 | `❌ INVALID … invalid measurement hash` |
| inconclusive | [`0xcc02b98f…a4c9`](https://sepolia.etherscan.io/tx/0xcc02b98f4321b94155884ddfc17187e2de910753e172990eff9aec4313aaa4c9) | 87,056 | `⚠️ INCONCLUSIVE … workload URL does not use https scheme` |

### Reward distribution (keeper wallet, ~196k gas each)

All 6 conclusive rounds of this test, plus the one round left un-rewarded from
the 2026-07-07 soak backlog (`0x13d5717e…e2c7`):

| Round (heartbeat) | distributeRewards tx |
|---|---|
| valid1 `0x8e5e771c…` | [`0x4d7bf17c…cd5e`](https://sepolia.etherscan.io/tx/0x4d7bf17ca98899bbd1c68beee95e50f37652583412ce6bc7dc24d964242acd5e) |
| invalid1 `0x69690adf…` | [`0xfba7dcb2…6460`](https://sepolia.etherscan.io/tx/0xfba7dcb2d558a5da57158df039ff2ac0c1335b9f5b7ce2dfd048eb3002e16460) |
| invalid2 `0x6e919d92…` | [`0x73d802df…e53e`](https://sepolia.etherscan.io/tx/0x73d802dfa6b4c91c1cbfb586bbb1e0d3f578a7a09dbff77471159fcf2c1ee53e) |
| valid2 `0x23524655…` | [`0xfbfe907b…33a4`](https://sepolia.etherscan.io/tx/0xfbfe907b57e0ee0b4dfdbc1d850939f00afbc405307e19fa75ac73fd645933a4) |
| soak backlog `0x13d5717e…` | [`0x4dcc528a…a138`](https://sepolia.etherscan.io/tx/0x4dcc528a98e393d20f561693656d0a7b1952aaf6a1b05d0c69ea84728d4ca138) |
| valid3 `0x4b81f92d…` | [`0xe999a714…cb84`](https://sepolia.etherscan.io/tx/0xe999a7143ba9469b07afa9d490ed4b2bec40a7bf78464332d55a4cba9f1acb84) |
| valid4 `0x28c4db15…` | [`0x4ece0902…1c1a`](https://sepolia.etherscan.io/tx/0x4ece090239e91f8eb97f3c47ff7efde6af5337c8cdf63ce6d283e28871851c1a) |

### Deviation found & fixed: reward-budget starvation

The rounds initially finalized without rewards: with
`maxPayoutPerFinalize == 0` (uncapped) the first `distributeRewards` after the
07-07 emissions mint had swept the **entire** epoch budget (1e12) into one
round's outstanding rewards (accrued to the deployer — the sole staker), and
the next emissions epoch is time-locked until 2026-07-14. Fix, making the
deployment self-sustaining for repeated e2e tests:

- `setMaxPayoutPerFinalize(1e9)` — cap 1,000 TEST/round:
  [`0x7783e46b…4636`](https://sepolia.etherscan.io/tx/0x7783e46b48931388ea2a8e80c4e24b0ca137f16e8c8234e3dd77b42c827b4636)
- 100k TEST top-up to the RewardPolicy (~100 rounds of budget):
  [`0xb9918d4b…20ede`](https://sepolia.etherscan.io/tx/0xb9918d4b76325c7604a72b0131866b2456382dcc7913f6542e7390d890f20ede)
- keeper auto-`sync()` registering the deposit:
  [`0x2a935620…31ea`](https://sepolia.etherscan.io/tx/0x2a9356200a480402cad2b8e19cf3488319a39cfc5c1f02408a544ca81b9a31ea)

A related keeper bug was found and fixed in the same session
(`keeper/src/l2/rewards.rs`): with several reward jobs queued in one tick, the
cached `spendable` budget went stale after the first distribution consumed it,
making the remaining jobs revert in pre-simulation and log ERRORs. The cache
is now refreshed after each successful distribution; all 7 queued jobs then
cleared across consecutive ticks with zero errors. See RUNBOOK pitfalls #8–#9.
