use alloy::primitives::{Address, B256, Bytes, U256};
use std::collections::HashMap;

mod escalator;
mod events;
mod jailing;
mod rewards;
mod supervisor;

pub use supervisor::L2Supervisor;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
struct RoundKey {
    heartbeat_key: B256,
    round: u8,
}

#[derive(Debug, Clone, Default)]
struct RoundState {
    members: Vec<Address>,
    raw_htx: Option<Bytes>,
    deadline: Option<u64>,
    outcome: Option<u8>,
    round_info: Option<RoundInfoView>,
    rewards_done: bool,
    reward_sync_attempted: bool,
    jailing_done: bool,
}

#[derive(Debug, Clone, Copy)]
struct RoundInfoView {
    reward: Address,
    valid_stake: U256,
    invalid_stake: U256,
}

#[derive(Debug, Clone)]
struct RewardPolicyCache {
    last_checked_at: Option<u64>,
    last_budget: Option<U256>,
    last_remaining: Option<U256>,
    last_sync_attempt_at: Option<u64>,
}

impl RewardPolicyCache {
    fn new() -> Self {
        Self {
            last_checked_at: None,
            last_budget: None,
            last_remaining: None,
            last_sync_attempt_at: None,
        }
    }
}

#[derive(Default)]
pub struct KeeperState {
    raw_htx_by_heartbeat: HashMap<B256, Bytes>,
    rounds: HashMap<RoundKey, RoundState>,
    reward_policies: HashMap<Address, RewardPolicyCache>,
}
