use crate::{
    clients::{L2KeeperClient, RewardPolicyInstance},
    l2::{KeeperState, RoundInfoView, RoundKey},
    metrics,
};
use alloy::primitives::{Address, U256, map::HashMap, utils::format_units};
use anyhow::{Context, anyhow, bail};
use blacklight_contract_clients::{
    ProtocolConfig::ProtocolConfigInstance,
    common::{errors::decode_any_error, overestimate_gas},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument, warn};

const MIN_NIL_SYNC_THRESHOLD: u64 = 100;
const RESPONDED_BIT: u64 = 1 << 2;
const VERDICT_MASK: u64 = 0x3;
const WEIGHT_SHIFT: u32 = 3;

#[derive(Clone, Copy)]
struct TokenContext {
    decimals: u8,
    address: Address,
}

#[derive(Clone, Copy)]
struct RewardsContext {
    token: TokenContext,
    checked_at: u64,
    synced_at: u64,
    spendable: U256,
    remaining: U256,
}

pub(crate) struct RewardsDistributor {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    rewards_context: HashMap<Address, RewardsContext>,
}

impl RewardsDistributor {
    pub(crate) fn new(client: Arc<L2KeeperClient>, state: Arc<Mutex<KeeperState>>) -> Self {
        Self {
            client,
            state,
            rewards_context: Default::default(),
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
        let token = self.fetch_token_context(reward_policy_address).await?.token;

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

    #[instrument(skip_all, fields(key = ?key.heartbeat_key, round = key.round))]
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
            .ensure_reward_budget(block_timestamp, round_info.reward)
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
                sum_weights = ?sum_weights,
                expected_stake = ?expected_stake,
                "Reward weights mismatch, skipping"
            );
            return Ok(());
        }

        info!(voters = voters.len(), "Distributing rewards");

        let call =
            self.client
                .heartbeat_manager()
                .distributeRewards(key.heartbeat_key, key.round, voters);
        let gas_with_buffer = overestimate_gas(&call).await?;
        match call.gas(gas_with_buffer).send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
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

    #[instrument(skip_all, fields(reward = ?reward_address))]
    async fn ensure_reward_budget(
        &mut self,
        block_timestamp: u64,
        reward_address: Address,
    ) -> anyhow::Result<bool> {
        if reward_address == Address::ZERO {
            warn!("Reward policy address is zero, skipping");
            return Ok(false);
        }

        let reward_policy = self.client.reward_policy(reward_address);
        let ctx = self.fetch_token_context(reward_address).await?;
        if ctx.checked_at != block_timestamp {
            // Fetch the current spendable/remaining budgets
            ctx.spendable = reward_policy.spendableBudget().call().await?;
            ctx.remaining = reward_policy.streamRemaining().call().await?;
            ctx.checked_at = block_timestamp;
            metrics::get().l2.rewards.set_spendable(ctx.spendable);
            metrics::get().l2.rewards.set_remaining(ctx.remaining);

            let spendable = format_units(ctx.spendable, ctx.token.decimals)?;
            let remaining = format_units(ctx.remaining, ctx.token.decimals)?;
            info!(
                spendable = spendable,
                remaining = remaining,
                "Fetched contract context"
            );
        }
        if ctx.spendable > U256::ZERO {
            return Ok(true);
        }

        let should_unlock = Self::can_unlock_budget(
            &reward_policy,
            ctx.remaining,
            block_timestamp,
            ctx.token.decimals,
        )
        .await?;

        if !should_unlock {
            info!("Reward budget still unlocking, skipping");
            return Ok(false);
        }

        if ctx.synced_at == block_timestamp {
            debug!("Reward sync already attempted for reward policy in this tick");
            return Ok(false);
        }
        ctx.synced_at = block_timestamp;

        info!("Reward budget unlocking, syncing policy",);
        match reward_policy.sync().send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
                    tx_hash = ?receipt.transaction_hash,
                    "Reward policy synced"
                );
            }
            Err(e) => {
                warn!("Reward policy sync failed: {}", decode_any_error(&e));
                return Ok(false);
            }
        }

        // Update our state after syncing
        ctx.spendable = reward_policy.spendableBudget().call().await?;
        ctx.remaining = reward_policy.streamRemaining().call().await?;
        if ctx.spendable == U256::ZERO {
            if ctx.remaining > U256::ZERO {
                info!("Reward budget still unlocking after sync, skipping");
            } else {
                info!("Reward budget still empty after sync, skipping");
            }
            Ok(false)
        } else {
            Ok(true)
        }
    }

    async fn can_unlock_budget(
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

    async fn fetch_token_context(
        &mut self,
        address: Address,
    ) -> anyhow::Result<&mut RewardsContext> {
        if !self.rewards_context.contains_key(&address) {
            info!("Fetching token context for rewards policy address {address}");
            let reward_policy = RewardPolicyInstance::new(address, self.client.provider());
            let token_address = reward_policy.rewardToken().call().await?;
            let erc20 = self.client.erc20(token_address);
            let decimals = erc20.decimals().call().await?;

            let token = TokenContext {
                decimals,
                address: token_address,
            };
            let context = RewardsContext {
                token,
                checked_at: 0,
                synced_at: 0,
                spendable: U256::ZERO,
                remaining: U256::ZERO,
            };
            self.rewards_context.insert(address, context);
        }
        self.rewards_context
            .get_mut(&address)
            .ok_or_else(|| anyhow!("insertion gone"))
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
