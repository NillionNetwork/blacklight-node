# nil – Test Suite

This folder contains a Foundry-based test suite (unit, integration, and invariant/property tests) for the nilAV contract system:

- `ProtocolConfig`
- `StakingOperators`
- `WeightedCommitteeSelector`
- `HeartbeatManager`
- `RewardPolicy`
- `JailingPolicy`
- `EmissionsController`

## What’s covered

- **Unit tests** for each module’s validation and core behavior.
- **Integration tests** for end-to-end flows:
  - heartbeat submission → committee selection → voting → finalization → reward distribution → claim
  - jailing enforcement and how it affects subsequent committee selection
- **Edge cases / stress**
  - large committees (e.g. 200 members)
  - batch voting limits and hard caps
  - committee size growth on escalation
- **Invariant tests**
  - `StakingOperators`: totalStaked accounting, active set integrity, tranche accounting
  - `HeartbeatManager`: round accounting sums, vote weight equals snapshot stake, committee total stake matches snapshot sum

## Running

This is a Foundry project.

```bash
forge install OpenZeppelin/openzeppelin-contracts
forge install foundry-rs/forge-std
forge test -vvv
```

> Note: This repo uses OZ imports (e.g. `@openzeppelin/contracts/...`) and expects the standard `lib/openzeppelin-contracts` and `lib/forge-std` locations (see `foundry.toml` remappings).
