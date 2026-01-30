use crate::{
    clients::{L2KeeperClient, RewardPolicyInstance},
    l2::{KeeperState, RewardPolicyCache, RoundInfoView, RoundKey},
    metrics,
};
use alloy::primitives::{Address, U256, map::HashMap, utils::format_units};
use anyhow::{Context, bail};
use blacklight_contract_clients::{
    ProtocolConfig::ProtocolConfigInstance,
    common::{errors::decode_any_error, overestimate_gas},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const MIN_NIL_SYNC_THRESHOLD: u64 = 100;
const RESPONDED_BIT: u64 = 1 << 2;
const VERDICT_MASK: u64 = 0x3;
const WEIGHT_SHIFT: u32 = 3;

#[derive(Clone, Copy)]
struct TokenContext {
    decimals: u8,
    address: Address,
}

pub(crate) struct RewardsDistributor {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    token_context: HashMap<Address, TokenContext>,
}

impl RewardsDistributor {
    pub(crate) fn new(client: Arc<L2KeeperClient>, state: Arc<Mutex<KeeperState>>) -> Self {
        Self {
            client,
            state,
            token_context: Default::default(),
        }
    }

    pub(crate) async fn sync_state(&mut self) -> anyhow::Result<()> {
        let protocol_config_address = self
            .client
            .staking_operators()
            .protocolConfig()
            .call()
            .await
            .context("Failed to get protocol config address")?;
        let protocol_config =
            ProtocolConfigInstance::new(protocol_config_address, self.client.provider());
        let reward_policy_address = protocol_config
            .rewardPolicy()
            .call()
            .await
            .context("Failed to get reward policy contract address")?;
        let token = self.fetch_token_context(reward_policy_address).await?;

        let reward_policy = self.client.reward_policy(reward_policy_address);
        let erc20 = self.client.erc20(token.address);
        let spendable = reward_policy.accountedBalance().call().await?;
        let balance = erc20.balanceOf(reward_policy_address).call().await?;
        let limit_nils = U256::try_from(MIN_NIL_SYNC_THRESHOLD)? * pow10_u256(token.decimals);
        let sync_limit = spendable.saturating_add(limit_nils);
        if balance > sync_limit {
            let balance = format_units(balance, token.decimals)?;
            let sync_limit = format_units(sync_limit, token.decimals)?;
            info!("Need to sync balance because balance ({balance}) > sync limit ({sync_limit})");
            let receipt = reward_policy
                .sync()
                .send()
                .await
                .context("Failed to sync")?
                .get_receipt()
                .await?;
            info!(tx_hash = ?receipt.transaction_hash, "Reward policy synced");
        }
        Ok(())
    }

    pub(crate) async fn distribute_rewards(
        &mut self,
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
                metrics::get().l2.rewards.inc_distributions();
                Ok(())
            }
            Err(e) => {
                bail!("Failed to distribute rewards: {e}");
            }
        }
    }

    async fn ensure_reward_budget(
        &mut self,
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
        }
        let budget = budget.unwrap_or(U256::ZERO);
        metrics::get().l2.rewards.set_budget(budget);
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
        let token_ctx = self.fetch_token_context(reward_address).await?;
        let should_unlock = if remaining > U256::ZERO {
            self.can_unlock_budget(
                &reward_policy,
                remaining,
                block_timestamp,
                token_ctx.decimals,
            )
            .await?
        } else {
            false
        };

        if !should_unlock {
            info!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                reward = ?reward_address,
                "Reward budget still unlocking, skipping"
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

        info!(
            heartbeat_key = ?key.heartbeat_key,
            round = key.round,
            reward = ?reward_address,
            "Reward budget unlocking, syncing policy",
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

    async fn fetch_token_context(&mut self, address: Address) -> anyhow::Result<TokenContext> {
        if let Some(context) = self.token_context.get(&address) {
            return Ok(*context);
        }
        info!("Fetching token context for rewards policy address {address}");
        let reward_policy = RewardPolicyInstance::new(address, self.client.provider());
        let token_address = reward_policy.rewardToken().call().await?;
        let erc20 = self.client.erc20(token_address);
        let decimals = erc20.decimals().call().await?;

        let context = TokenContext {
            decimals,
            address: token_address,
        };
        self.token_context.insert(address, context);
        Ok(context)
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
