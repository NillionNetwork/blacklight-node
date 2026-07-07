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
