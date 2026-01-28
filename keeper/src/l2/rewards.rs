use crate::{
    clients::{L2KeeperClient, RewardPolicyInstance},
    l2::{KeeperState, RewardPolicyCache, RoundInfoView, RoundKey},
};
use alloy::primitives::{Address, U256};
use anyhow::bail;
use blacklight_contract_clients::common::{errors::decode_any_error, overestimate_gas};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const RESPONDED_BIT: u64 = 1 << 2;
const VERDICT_MASK: u64 = 0x3;
const WEIGHT_SHIFT: u32 = 3;

pub(crate) struct RewardsDistributor {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
}

impl RewardsDistributor {
    pub(crate) fn new(client: Arc<L2KeeperClient>, state: Arc<Mutex<KeeperState>>) -> Self {
        Self { client, state }
    }

    pub(crate) async fn distribute_rewards(
        &self,
        block_timestamp: u64,
        key: RoundKey,
        outcome: u8,
        members: Vec<Address>,
    ) -> anyhow::Result<()> {
        let cached_info = {
            let state = self.state.lock().await;
            state.rounds.get(&key).and_then(|round| round.round_info)
        };
        let round_info = match cached_info {
            Some(info) => info,
            None => {
                let info = self
                    .client
                    .heartbeat_manager()
                    .rounds(key.heartbeat_key, key.round)
                    .call()
                    .await?;
                let view = RoundInfoView {
                    reward: info.reward,
                    valid_stake: info.validStake,
                    invalid_stake: info.invalidStake,
                };
                let mut state = self.state.lock().await;
                if let Some(round_state) = state.rounds.get_mut(&key) {
                    round_state.round_info = Some(view);
                }
                view
            }
        };
        if !self
            .ensure_reward_budget(block_timestamp, round_info.reward, key)
            .await?
        {
            return Ok(());
        }

        let expected_verdict = if outcome == 1 { 1u8 } else { 2u8 };
        let (voters, sum_weights) = self
            .build_voter_list(key, &members, expected_verdict)
            .await?;
        let expected_stake = if outcome == 1 {
            round_info.valid_stake
        } else {
            round_info.invalid_stake
        };

        if sum_weights != expected_stake {
            warn!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                sum_weights = ?sum_weights,
                expected_stake = ?expected_stake,
                "Reward weights mismatch, skipping"
            );
            return Ok(());
        }

        info!(
            heartbeat_key = ?key.heartbeat_key,
            round = key.round,
            voters = voters.len(),
            "Distributing rewards"
        );

        let call =
            self.client
                .heartbeat_manager()
                .distributeRewards(key.heartbeat_key, key.round, voters);
        let gas_with_buffer = overestimate_gas(&call).await?;
        match call.gas(gas_with_buffer).send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
                    heartbeat_key = ?key.heartbeat_key,
                    round = key.round,
                    tx_hash = ?receipt.transaction_hash,
                    "Rewards distributed"
                );
                let mut state = self.state.lock().await;
                if let Some(round_state) = state.rounds.get_mut(&key) {
                    round_state.rewards_done = true;
                }
                Ok(())
            }
            Err(e) => {
                bail!("Failed to distribute rewareds: {e}");
            }
        }
    }

    async fn ensure_reward_budget(
        &self,
        block_timestamp: u64,
        reward_address: Address,
        key: RoundKey,
    ) -> anyhow::Result<bool> {
        if reward_address == Address::ZERO {
            warn!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                "Reward policy address is zero, skipping"
            );
            return Ok(false);
        }

        let mut cache = {
            let state = self.state.lock().await;
            state
                .reward_policies
                .get(&reward_address)
                .cloned()
                .unwrap_or_else(RewardPolicyCache::new)
        };

        let reward_policy = self.client.reward_policy(reward_address);
        let mut budget = None;
        if cache.last_checked_at == Some(block_timestamp) {
            budget = cache.last_budget;
        }
        if budget.is_none() {
            let fetched = reward_policy.spendableBudget().call().await?;
            budget = Some(fetched);
            cache.last_checked_at = Some(block_timestamp);
            cache.last_budget = Some(fetched);
            cache.last_remaining = None;
            cache.last_accounted = None;
            cache.last_balance = None;
        }
        let budget = budget.unwrap_or(U256::ZERO);
        if budget > U256::ZERO {
            self.store_reward_cache(reward_address, cache).await;
            return Ok(true);
        }

        let remaining = if let Some(value) = cache.last_remaining {
            value
        } else {
            let value = reward_policy.streamRemaining().call().await?;
            cache.last_remaining = Some(value);
            value
        };
        let accounted = if let Some(value) = cache.last_accounted {
            value
        } else {
            let value = reward_policy.accountedBalance().call().await?;
            cache.last_accounted = Some(value);
            value
        };
        let token_address = match cache.token_address {
            Some(value) => value,
            None => {
                let value = reward_policy.rewardToken().call().await?;
                cache.token_address = Some(value);
                value
            }
        };
        let token = self.client.erc20(token_address);
        let balance = if let Some(value) = cache.last_balance {
            value
        } else {
            let value = token.balanceOf(reward_address).call().await?;
            cache.last_balance = Some(value);
            value
        };
        let has_new_deposit = balance > accounted;

        let should_unlock = if remaining > U256::ZERO {
            let decimals = match cache.token_decimals {
                Some(value) => value,
                None => {
                    let value = match token.decimals().call().await {
                        Ok(value) => value,
                        Err(e) => {
                            warn!(
                                reward_token = ?token_address,
                                error = %decode_any_error(&e),
                                "Failed to read token decimals, defaulting to 18"
                            );
                            18
                        }
                    };
                    cache.token_decimals = Some(value);
                    value
                }
            };
            self.can_unlock_budget(&reward_policy, remaining, block_timestamp, decimals)
                .await?
        } else {
            false
        };

        if !has_new_deposit && !should_unlock {
            let msg = if remaining > U256::ZERO {
                "Reward budget still unlocking, skipping"
            } else {
                "Reward budget empty, skipping"
            };
            info!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                reward = ?reward_address,
                "{}", msg
            );
            self.store_reward_cache(reward_address, cache).await;
            return Ok(false);
        }

        let already_attempted = {
            let mut state = self.state.lock().await;
            let entry = state.rounds.entry(key).or_default();
            if entry.reward_sync_attempted {
                true
            } else {
                entry.reward_sync_attempted = true;
                false
            }
        };
        if already_attempted {
            debug!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                reward = ?reward_address,
                "Reward sync already attempted for round, skipping"
            );
            self.store_reward_cache(reward_address, cache).await;
            return Ok(false);
        }

        if cache.last_sync_attempt_at == Some(block_timestamp) {
            debug!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                reward = ?reward_address,
                "Reward sync already attempted for reward policy in this tick"
            );
            self.store_reward_cache(reward_address, cache).await;
            return Ok(false);
        }
        cache.last_sync_attempt_at = Some(block_timestamp);

        let sync_reason = if has_new_deposit {
            "Reward policy has new deposit, syncing"
        } else {
            "Reward budget unlocking, syncing policy"
        };
        info!(
            heartbeat_key = ?key.heartbeat_key,
            round = key.round,
            reward = ?reward_address,
            "{}", sync_reason
        );

        match reward_policy.sync().send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
                    heartbeat_key = ?key.heartbeat_key,
                    round = key.round,
                    reward = ?reward_address,
                    tx_hash = ?receipt.transaction_hash,
                    "Reward policy synced"
                );
            }
            Err(e) => {
                warn!(
                    heartbeat_key = ?key.heartbeat_key,
                    round = key.round,
                    reward = ?reward_address,
                    error = %decode_any_error(&e),
                    "Reward policy sync failed"
                );
                self.store_reward_cache(reward_address, cache).await;
                return Ok(false);
            }
        }

        let budget_after = reward_policy.spendableBudget().call().await?;
        if budget_after == U256::ZERO {
            let remaining_after = reward_policy.streamRemaining().call().await?;
            let skip_msg = if remaining_after > U256::ZERO {
                "Reward budget still unlocking after sync, skipping"
            } else {
                "Reward budget still empty after sync, skipping"
            };
            info!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                reward = ?reward_address,
                "{}", skip_msg
            );
            cache.last_budget = Some(budget_after);
            self.store_reward_cache(reward_address, cache).await;
            return Ok(false);
        }

        cache.last_budget = Some(budget_after);
        self.store_reward_cache(reward_address, cache).await;
        Ok(true)
    }

    async fn can_unlock_budget(
        &self,
        reward_policy: &RewardPolicyInstance,
        remaining: U256,
        block_timestamp: u64,
        token_decimals: u8,
    ) -> anyhow::Result<bool> {
        if remaining == U256::ZERO {
            return Ok(false);
        }

        let stream_rate = reward_policy.streamRatePerSecondWad().call().await?;
        let last_update = reward_policy.lastUpdate().call().await?;
        let stream_end = reward_policy.streamEnd().call().await?;

        if block_timestamp >= stream_end {
            return Ok(true);
        }

        if stream_rate == U256::ZERO {
            return Ok(false);
        }

        let elapsed = block_timestamp.saturating_sub(last_update);
        if elapsed == 0 {
            return Ok(false);
        }

        let elapsed_u256 = U256::from(elapsed);
        let product = elapsed_u256.checked_mul(stream_rate).unwrap_or(U256::MAX);
        let wad = U256::from(1_000_000_000_000_000_000u128);
        let unlocked = product / wad;
        let threshold = pow10_u256(token_decimals);
        Ok(unlocked >= threshold)
    }

    async fn store_reward_cache(&self, reward_address: Address, cache: RewardPolicyCache) {
        let mut state = self.state.lock().await;
        state.reward_policies.insert(reward_address, cache);
    }

    async fn build_voter_list(
        &self,
        key: RoundKey,
        members: &[Address],
        expected_verdict: u8,
    ) -> anyhow::Result<(Vec<Address>, U256)> {
        let mut voters = Vec::new();
        let mut total_weight = U256::ZERO;

        for member in members {
            let packed = self
                .client
                .heartbeat_manager()
                .getVotePacked(key.heartbeat_key, key.round, *member)
                .call()
                .await?;
            let responded = (packed & U256::from(RESPONDED_BIT)) != U256::ZERO;
            let verdict = u8::try_from(packed & U256::from(VERDICT_MASK))?;
            if responded && verdict == expected_verdict {
                let weight = packed >> WEIGHT_SHIFT;
                total_weight += weight;
                voters.push(*member);
            }
        }

        Ok((voters, total_weight))
    }
}

fn pow10_u256(exp: u8) -> U256 {
    let mut value = U256::from(1u8);
    let ten = U256::from(10u8);
    for _ in 0..exp {
        value = value.saturating_mul(ten);
    }
    value
}
