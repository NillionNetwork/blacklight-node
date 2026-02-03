use alloy::primitives::{Address, B256, Bytes};
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
    rewards_done: bool,
    jailing_done: bool,
}

#[derive(Default)]
pub struct KeeperState {
    raw_htx_by_heartbeat: HashMap<B256, Bytes>,
    rounds: HashMap<RoundKey, RoundState>,
}
