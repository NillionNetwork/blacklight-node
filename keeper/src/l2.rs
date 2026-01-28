use crate::{
    args::KeeperConfig,
    clients::{L2KeeperClient, RewardPolicyInstance},
};
use alloy::{
    eips::BlockNumberOrTag,
    primitives::{Address, B256, Bytes, U256},
    providers::Provider,
};
use anyhow::{Result, bail};
use blacklight_contract_clients::{
    common::{errors::decode_any_error, overestimate_gas},
    heartbeat_manager::{
        HeartbeatEnqueuedEvent, RewardDistributionAbandonedEvent, RewardsDistributedEvent,
        RoundFinalizedEvent, RoundStartedEvent, SlashingCallbackFailedEvent,
    },
};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Notify},
    task::JoinSet,
    time::interval,
};
use tracing::{debug, error, info, warn};

const RESPONDED_BIT: u64 = 1 << 2;
const VERDICT_MASK: u64 = 0x3;
const WEIGHT_SHIFT: u32 = 3;
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
struct RoundKey {
    heartbeat_key: B256,
    round: u8,
}

#[derive(Debug, Clone)]
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

impl RoundState {
    fn new() -> Self {
        Self {
            members: Vec::new(),
            raw_htx: None,
            deadline: None,
            outcome: None,
            round_info: None,
            rewards_done: false,
            reward_sync_attempted: false,
            jailing_done: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RoundInfoView {
    reward: Address,
    valid_stake: U256,
    invalid_stake: U256,
}

#[derive(Debug, Clone)]
struct RewardPolicyCache {
    token_address: Option<Address>,
    token_decimals: Option<u8>,
    last_checked_at: Option<u64>,
    last_budget: Option<U256>,
    last_remaining: Option<U256>,
    last_accounted: Option<U256>,
    last_balance: Option<U256>,
    last_sync_attempt_at: Option<u64>,
}

impl RewardPolicyCache {
    fn new() -> Self {
        Self {
            token_address: None,
            token_decimals: None,
            last_checked_at: None,
            last_budget: None,
            last_remaining: None,
            last_accounted: None,
            last_balance: None,
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

#[derive(Clone, Copy)]
struct TickContext {
    now: Option<u64>,
}

pub async fn run_l2_supervisor(
    config: KeeperConfig,
    state: Arc<Mutex<KeeperState>>,
    shutdown_notify: Arc<Notify>,
) -> Result<()> {
    let mut reconnect_delay = INITIAL_RECONNECT_DELAY;
    let max_delay = MAX_RECONNECT_DELAY;

    loop {
        let l2_client = match create_l2_client_with_retry(&config, shutdown_notify.clone()).await {
            Ok(client) => client,
            Err(_) => break,
        };
        let l2_client = Arc::new(l2_client);

        if let Err(e) =
            load_historical_events(l2_client.clone(), state.clone(), config.lookback_blocks).await
        {
            warn!(error = %e, "Failed to load historical events");
        }

        let loop_handle = tokio::spawn(run_l2_keeper_loop(
            l2_client.clone(),
            state.clone(),
            config.tick_interval,
            shutdown_notify.clone(),
        ));

        match run_l2_event_listeners(l2_client.clone(), state.clone(), shutdown_notify.clone())
            .await
        {
            Ok(()) => {
                break;
            }
            Err(e) => {
                warn!(error = %e, reconnect_delay = ?reconnect_delay, "Listener error, reconnecting");
            }
        }

        loop_handle.abort();

        tokio::select! {
            _ = tokio::time::sleep(reconnect_delay) => {
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_delay);
            }
            _ = shutdown_notify.notified() => {
                break;
            }
        }
    }

    Ok(())
}

async fn create_l2_client_with_retry(
    config: &KeeperConfig,
    shutdown_notify: Arc<Notify>,
) -> Result<L2KeeperClient> {
    let mut delay = INITIAL_RECONNECT_DELAY;
    let max_delay = MAX_RECONNECT_DELAY;

    loop {
        match L2KeeperClient::new(
            config.l2_rpc_url.clone(),
            config.l2_heartbeat_manager_address,
            config.l2_jailing_policy_address,
            config.private_key.clone(),
        )
        .await
        {
            Ok(client) => return Ok(client),
            Err(e) => {
                warn!(error = %e, delay = ?delay, "Failed to connect L2, retrying");
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {
                        delay = std::cmp::min(delay * 2, max_delay);
                    }
                    _ = shutdown_notify.notified() => {
                        return Err(anyhow::anyhow!("Shutdown requested"));
                    }
                }
            }
        }
    }
}

async fn run_l2_event_listeners(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    shutdown_notify: Arc<Notify>,
) -> Result<()> {
    let mut tasks = JoinSet::new();
    tasks.spawn(listen_heartbeat_enqueued(l2_client.clone(), state.clone()));
    tasks.spawn(listen_round_started(l2_client.clone(), state.clone()));
    tasks.spawn(listen_round_finalized(l2_client.clone(), state.clone()));
    tasks.spawn(listen_rewards_distributed(l2_client.clone(), state.clone()));
    tasks.spawn(listen_reward_distribution_abandoned(
        l2_client.clone(),
        state.clone(),
    ));
    tasks.spawn(listen_slashing_callback_failed(l2_client.clone()));

    tokio::select! {
        res = tasks.join_next() => {
            match res {
                Some(Ok(Ok(()))) => {
                    tasks.abort_all();
                    bail!("Listener exited unexpectedly")
                }
                Some(Ok(Err(e))) => {
                    tasks.abort_all();
                    Err(e)
                }
                Some(Err(e)) => {
                    tasks.abort_all();
                    bail!("Listener task failed: {e}")
                }
                None => {
                    bail!("No listener tasks running")
                }
            }
        }
        _ = shutdown_notify.notified() => {
            tasks.abort_all();
            Ok(())
        }
    }
}

async fn load_historical_events(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    lookback_blocks: u64,
) -> Result<()> {
    let latest = l2_client.provider().get_block_number().await?;
    let from_block = latest.saturating_sub(lookback_blocks);

    let enqueued = l2_client
        .heartbeat_manager()
        .event_filter::<HeartbeatEnqueuedEvent>()
        .from_block(from_block)
        .query()
        .await?;
    let rounds_started = l2_client
        .heartbeat_manager()
        .event_filter::<RoundStartedEvent>()
        .from_block(from_block)
        .query()
        .await?;
    let rounds_finalized = l2_client
        .heartbeat_manager()
        .event_filter::<RoundFinalizedEvent>()
        .from_block(from_block)
        .query()
        .await?;
    let rewards_done = l2_client
        .heartbeat_manager()
        .event_filter::<RewardsDistributedEvent>()
        .from_block(from_block)
        .query()
        .await?;
    let rewards_abandoned = l2_client
        .heartbeat_manager()
        .event_filter::<RewardDistributionAbandonedEvent>()
        .from_block(from_block)
        .query()
        .await?;

    let mut guard = state.lock().await;
    for (event, _log) in enqueued {
        guard
            .raw_htx_by_heartbeat
            .insert(event.heartbeatKey, event.rawHTX);
    }
    for (event, _log) in rounds_started {
        let key = RoundKey {
            heartbeat_key: event.heartbeatKey,
            round: event.round,
        };
        let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
        entry.members = event.members;
        entry.raw_htx = Some(event.rawHTX.clone());
        entry.deadline = Some(event.deadline);
        guard
            .raw_htx_by_heartbeat
            .insert(event.heartbeatKey, event.rawHTX);
    }
    for (event, _log) in rounds_finalized {
        let key = RoundKey {
            heartbeat_key: event.heartbeatKey,
            round: event.round,
        };
        let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
        entry.outcome = Some(event.outcome);
    }
    for (event, _log) in rewards_done {
        let key = RoundKey {
            heartbeat_key: event.heartbeatKey,
            round: event.round,
        };
        let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
        entry.rewards_done = true;
    }
    for (event, _log) in rewards_abandoned {
        let key = RoundKey {
            heartbeat_key: event.heartbeatKey,
            round: event.round,
        };
        let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
        entry.rewards_done = true;
    }

    info!(
        from_block,
        heartbeats = guard.raw_htx_by_heartbeat.len(),
        rounds = guard.rounds.len(),
        "Loaded historical keeper state"
    );

    Ok(())
}

async fn run_l2_keeper_loop(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    tick_interval: Duration,
    shutdown_notify: Arc<Notify>,
) -> Result<()> {
    let mut ticker = interval(tick_interval);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown_notify.notified() => {
                return Ok(());
            }
        }

        let tick = TickContext {
            now: match l2_client
                .provider()
                .get_block_by_number(BlockNumberOrTag::Latest)
                .await
            {
                Ok(Some(block)) => Some(block.header.timestamp),
                Ok(None) => {
                    warn!("Missing latest block during tick");
                    None
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load latest block during tick");
                    None
                }
            },
        };

        if let Err(e) = process_escalations(l2_client.clone(), state.clone(), tick).await {
            error!(error = %e, "Failed to process escalations");
        }
        if let Err(e) = process_rewards_and_jailing(l2_client.clone(), state.clone(), tick).await {
            error!(error = %e, "Failed to process rewards/jailing");
        }
    }
}

async fn process_escalations(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    tick: TickContext,
) -> Result<()> {
    let (candidates, fallback_heartbeats) = {
        let guard = state.lock().await;
        let mut best_rounds: HashMap<B256, (u8, u64, Bytes)> = HashMap::new();
        let mut fallback = Vec::new();

        for (key, round) in guard.rounds.iter() {
            if round.outcome.is_some() {
                continue;
            }
            let deadline = match round.deadline {
                Some(value) => value,
                None => continue,
            };
            let raw_htx = round
                .raw_htx
                .clone()
                .or_else(|| guard.raw_htx_by_heartbeat.get(&key.heartbeat_key).cloned());
            let Some(raw_htx) = raw_htx else {
                continue;
            };

            let entry = best_rounds
                .entry(key.heartbeat_key)
                .or_insert_with(|| (key.round, deadline, raw_htx.clone()));
            if key.round > entry.0 {
                *entry = (key.round, deadline, raw_htx);
            }
        }

        if best_rounds.is_empty() {
            fallback = guard
                .raw_htx_by_heartbeat
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect();
        }

        (
            best_rounds
                .into_iter()
                .map(|(k, (round, deadline, raw_htx))| (k, round, deadline, raw_htx))
                .collect::<Vec<_>>(),
            fallback,
        )
    };

    for (heartbeat_key, round, deadline, raw_htx) in candidates {
        if let Some(now) = tick.now {
            if now <= deadline {
                continue;
            }
        } else {
            let should_escalate = l2_client
                .heartbeat_manager()
                .isPastDeadline(heartbeat_key)
                .call()
                .await?;
            if !should_escalate {
                continue;
            }
        }

        info!(
            heartbeat_key = ?heartbeat_key,
            round,
            deadline,
            "Escalating or expiring round"
        );
        let call = l2_client
            .heartbeat_manager()
            .escalateOrExpire(heartbeat_key, raw_htx.clone());

        match call.send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
                    heartbeat_key = ?heartbeat_key,
                    tx_hash = ?receipt.transaction_hash,
                    "Escalate/expire confirmed"
                );
            }
            Err(e) => {
                warn!(
                    heartbeat_key = ?heartbeat_key,
                    error = %format_contract_error(&e),
                    "Escalate/expire failed"
                );
            }
        }
    }

    for (heartbeat_key, raw_htx) in fallback_heartbeats {
        let should_escalate = l2_client
            .heartbeat_manager()
            .isPastDeadline(heartbeat_key)
            .call()
            .await?;
        if !should_escalate {
            continue;
        }

        info!(heartbeat_key = ?heartbeat_key, "Escalating or expiring round");
        let call = l2_client
            .heartbeat_manager()
            .escalateOrExpire(heartbeat_key, raw_htx.clone());

        match call.send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
                    heartbeat_key = ?heartbeat_key,
                    tx_hash = ?receipt.transaction_hash,
                    "Escalate/expire confirmed"
                );
            }
            Err(e) => {
                warn!(
                    heartbeat_key = ?heartbeat_key,
                    error = %format_contract_error(&e),
                    "Escalate/expire failed"
                );
            }
        }
    }

    Ok(())
}

async fn process_rewards_and_jailing(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    tick: TickContext,
) -> Result<()> {
    let mut reward_jobs = Vec::new();
    let mut jail_jobs = Vec::new();
    let has_jailing_policy = l2_client.jailing_policy().is_some();

    {
        let guard = state.lock().await;
        for (key, round) in guard.rounds.iter() {
            if let Some(outcome) = round.outcome {
                if !round.rewards_done && !round.members.is_empty() {
                    reward_jobs.push((*key, outcome, round.members.clone()));
                }
                if has_jailing_policy && !round.jailing_done && !round.members.is_empty() {
                    jail_jobs.push((*key, round.members.clone()));
                }
            }
        }
    }

    for (key, outcome, members) in reward_jobs {
        if outcome == 0 {
            continue;
        }
        if let Err(e) = try_distribute_rewards(
            l2_client.clone(),
            state.clone(),
            tick,
            key,
            outcome,
            members,
        )
        .await
        {
            warn!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                error = %e,
                "Reward distribution failed"
            );
        }
    }

    for (key, members) in jail_jobs {
        if let Err(e) = try_enforce_jailing(l2_client.clone(), state.clone(), key, members).await {
            warn!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                error = %e,
                "Jailing enforcement failed"
            );
        }
    }

    Ok(())
}

async fn try_distribute_rewards(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    tick: TickContext,
    key: RoundKey,
    outcome: u8,
    members: Vec<Address>,
) -> Result<()> {
    let cached_info = {
        let guard = state.lock().await;
        guard.rounds.get(&key).and_then(|round| round.round_info)
    };
    let round_info = match cached_info {
        Some(info) => info,
        None => {
            let info = l2_client
                .heartbeat_manager()
                .rounds(key.heartbeat_key, key.round)
                .call()
                .await?;
            let view = RoundInfoView {
                reward: info.reward,
                valid_stake: info.validStake,
                invalid_stake: info.invalidStake,
            };
            let mut guard = state.lock().await;
            if let Some(round_state) = guard.rounds.get_mut(&key) {
                round_state.round_info = Some(view);
            }
            view
        }
    };
    if !ensure_reward_budget(
        l2_client.clone(),
        state.clone(),
        tick,
        round_info.reward,
        key,
    )
    .await?
    {
        return Ok(());
    }

    let expected_verdict = if outcome == 1 { 1u8 } else { 2u8 };
    let (voters, sum_weights) =
        build_voter_list(l2_client.clone(), key, &members, expected_verdict).await?;
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
        l2_client
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
            let mut guard = state.lock().await;
            if let Some(round_state) = guard.rounds.get_mut(&key) {
                round_state.rewards_done = true;
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(format_contract_error(&e)));
        }
    }

    Ok(())
}

async fn ensure_reward_budget(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    tick: TickContext,
    reward_address: Address,
    key: RoundKey,
) -> Result<bool> {
    if reward_address == Address::ZERO {
        warn!(
            heartbeat_key = ?key.heartbeat_key,
            round = key.round,
            "Reward policy address is zero, skipping"
        );
        return Ok(false);
    }

    let mut cache = {
        let guard = state.lock().await;
        guard
            .reward_policies
            .get(&reward_address)
            .cloned()
            .unwrap_or_else(RewardPolicyCache::new)
    };

    let reward_policy = l2_client.reward_policy(reward_address);
    let mut budget = None;
    if let Some(now) = tick.now
        && cache.last_checked_at == Some(now)
    {
        budget = cache.last_budget;
    }
    if budget.is_none() {
        let fetched = reward_policy.spendableBudget().call().await?;
        budget = Some(fetched);
        cache.last_checked_at = tick.now;
        cache.last_budget = Some(fetched);
        cache.last_remaining = None;
        cache.last_accounted = None;
        cache.last_balance = None;
    }
    let budget = budget.unwrap_or(U256::ZERO);
    if budget > U256::ZERO {
        store_reward_cache(state, reward_address, cache).await;
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
    let token = l2_client.erc20(token_address);
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
                            error = %format_contract_error(&e),
                            "Failed to read token decimals, defaulting to 18"
                        );
                        18
                    }
                };
                cache.token_decimals = Some(value);
                value
            }
        };
        can_unlock_budget(l2_client.clone(), &reward_policy, remaining, tick, decimals).await?
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
        store_reward_cache(state, reward_address, cache).await;
        return Ok(false);
    }

    let already_attempted = {
        let mut guard = state.lock().await;
        let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
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
        store_reward_cache(state, reward_address, cache).await;
        return Ok(false);
    }

    if let Some(now) = tick.now {
        if cache.last_sync_attempt_at == Some(now) {
            debug!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                reward = ?reward_address,
                "Reward sync already attempted for reward policy in this tick"
            );
            store_reward_cache(state, reward_address, cache).await;
            return Ok(false);
        }
        cache.last_sync_attempt_at = Some(now);
    }

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
                error = %format_contract_error(&e),
                "Reward policy sync failed"
            );
            store_reward_cache(state, reward_address, cache).await;
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
        store_reward_cache(state, reward_address, cache).await;
        return Ok(false);
    }

    cache.last_budget = Some(budget_after);
    store_reward_cache(state, reward_address, cache).await;
    Ok(true)
}

async fn can_unlock_budget(
    l2_client: Arc<L2KeeperClient>,
    reward_policy: &RewardPolicyInstance,
    remaining: U256,
    tick: TickContext,
    token_decimals: u8,
) -> Result<bool> {
    if remaining == U256::ZERO {
        return Ok(false);
    }

    let stream_rate = reward_policy.streamRatePerSecondWad().call().await?;
    let last_update = reward_policy.lastUpdate().call().await?;
    let stream_end = reward_policy.streamEnd().call().await?;
    let now = match tick.now {
        Some(value) => value,
        None => {
            let latest = l2_client
                .provider()
                .get_block_by_number(BlockNumberOrTag::Latest)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Missing latest block"))?;
            latest.header.timestamp
        }
    };

    if now >= stream_end {
        return Ok(true);
    }

    if stream_rate == U256::ZERO {
        return Ok(false);
    }

    let elapsed = now.saturating_sub(last_update);
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

async fn store_reward_cache(
    state: Arc<Mutex<KeeperState>>,
    reward_address: Address,
    cache: RewardPolicyCache,
) {
    let mut guard = state.lock().await;
    guard.reward_policies.insert(reward_address, cache);
}

fn pow10_u256(exp: u8) -> U256 {
    let mut value = U256::from(1u8);
    let ten = U256::from(10u8);
    for _ in 0..exp {
        value = value.saturating_mul(ten);
    }
    value
}

async fn try_enforce_jailing(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    key: RoundKey,
    members: Vec<Address>,
) -> Result<()> {
    let Some(policy) = l2_client.jailing_policy() else {
        return Ok(());
    };

    info!(
        heartbeat_key = ?key.heartbeat_key,
        round = key.round,
        members = members.len(),
        "Enforcing jailing"
    );

    if let Err(e) = policy
        .recordRound(key.heartbeat_key, key.round)
        .send()
        .await
    {
        warn!(
            heartbeat_key = ?key.heartbeat_key,
            round = key.round,
            error = %format_contract_error(&e),
            "Failed to record round in jailing policy"
        );
    }

    match policy
        .enforceJailFromMembers(key.heartbeat_key, key.round, members)
        .send()
        .await
    {
        Ok(pending) => {
            let receipt = pending.get_receipt().await?;
            info!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                tx_hash = ?receipt.transaction_hash,
                "Jailing enforced"
            );
            let mut guard = state.lock().await;
            if let Some(round_state) = guard.rounds.get_mut(&key) {
                round_state.jailing_done = true;
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(format_contract_error(&e)));
        }
    }

    Ok(())
}

async fn retry_slashing(
    l2_client: Arc<L2KeeperClient>,
    heartbeat_key: B256,
    round: u8,
) -> Result<()> {
    let call = l2_client
        .heartbeat_manager()
        .retrySlashing(heartbeat_key, round);
    match call.send().await {
        Ok(pending) => {
            let receipt = pending.get_receipt().await?;
            info!(
                heartbeat_key = ?heartbeat_key,
                round,
                tx_hash = ?receipt.transaction_hash,
                "Slashing callback retried"
            );
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!(format_contract_error(&e))),
    }
}

async fn build_voter_list(
    l2_client: Arc<L2KeeperClient>,
    key: RoundKey,
    members: &[Address],
    expected_verdict: u8,
) -> Result<(Vec<Address>, U256)> {
    let mut voters = Vec::new();
    let mut total_weight = U256::ZERO;

    for member in members {
        let packed = l2_client
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

async fn listen_heartbeat_enqueued(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
) -> Result<()> {
    let subscription = match l2_client
        .heartbeat_manager()
        .event_filter::<HeartbeatEnqueuedEvent>()
        .subscribe()
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to subscribe to HeartbeatEnqueued events: {e}"
            ));
        }
    };
    let mut events = subscription.into_stream();

    while let Some(event) = events.next().await {
        match event {
            Ok((event, _log)) => {
                let mut guard = state.lock().await;
                guard
                    .raw_htx_by_heartbeat
                    .insert(event.heartbeatKey, event.rawHTX.clone());
                debug!(heartbeat_key = ?event.heartbeatKey, "Heartbeat enqueued");
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Error receiving HeartbeatEnqueued event: {e}"
                ));
            }
        }
    }

    Err(anyhow::anyhow!(
        "HeartbeatEnqueued event stream ended unexpectedly"
    ))
}

async fn listen_round_started(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
) -> Result<()> {
    let subscription = match l2_client
        .heartbeat_manager()
        .event_filter::<RoundStartedEvent>()
        .subscribe()
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to subscribe to RoundStarted events: {e}"
            ));
        }
    };
    let mut events = subscription.into_stream();

    while let Some(event) = events.next().await {
        match event {
            Ok((event, _log)) => {
                let key = RoundKey {
                    heartbeat_key: event.heartbeatKey,
                    round: event.round,
                };
                let mut guard = state.lock().await;
                guard
                    .raw_htx_by_heartbeat
                    .insert(event.heartbeatKey, event.rawHTX.clone());
                let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
                entry.members = event.members.clone();
                entry.raw_htx = Some(event.rawHTX.clone());
                entry.deadline = Some(event.deadline);
                info!(
                    heartbeat_key = ?event.heartbeatKey,
                    round = event.round,
                    deadline = event.deadline,
                    members = entry.members.len(),
                    "Round started"
                );
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Error receiving RoundStarted event: {e}"));
            }
        }
    }

    Err(anyhow::anyhow!(
        "RoundStarted event stream ended unexpectedly"
    ))
}

async fn listen_round_finalized(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
) -> Result<()> {
    let subscription = match l2_client
        .heartbeat_manager()
        .event_filter::<RoundFinalizedEvent>()
        .subscribe()
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to subscribe to RoundFinalized events: {e}"
            ));
        }
    };
    let mut events = subscription.into_stream();

    while let Some(event) = events.next().await {
        match event {
            Ok((event, _log)) => {
                let key = RoundKey {
                    heartbeat_key: event.heartbeatKey,
                    round: event.round,
                };
                let mut guard = state.lock().await;
                let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
                entry.outcome = Some(event.outcome);
                info!(
                    heartbeat_key = ?event.heartbeatKey,
                    round = event.round,
                    outcome = event.outcome,
                    "Round finalized"
                );
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Error receiving RoundFinalized event: {e}"));
            }
        }
    }

    Err(anyhow::anyhow!(
        "RoundFinalized event stream ended unexpectedly"
    ))
}

async fn listen_rewards_distributed(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
) -> Result<()> {
    let subscription = match l2_client
        .heartbeat_manager()
        .event_filter::<RewardsDistributedEvent>()
        .subscribe()
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to subscribe to RewardsDistributed events: {e}"
            ));
        }
    };
    let mut events = subscription.into_stream();

    while let Some(event) = events.next().await {
        match event {
            Ok((event, _log)) => {
                let key = RoundKey {
                    heartbeat_key: event.heartbeatKey,
                    round: event.round,
                };
                let mut guard = state.lock().await;
                let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
                entry.rewards_done = true;
                info!(
                    heartbeat_key = ?event.heartbeatKey,
                    round = event.round,
                    voter_count = ?event.voterCount,
                    "Rewards distributed"
                );
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Error receiving RewardsDistributed event: {e}"
                ));
            }
        }
    }

    Err(anyhow::anyhow!(
        "RewardsDistributed event stream ended unexpectedly"
    ))
}

async fn listen_reward_distribution_abandoned(
    l2_client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
) -> Result<()> {
    let subscription = match l2_client
        .heartbeat_manager()
        .event_filter::<RewardDistributionAbandonedEvent>()
        .subscribe()
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to subscribe to RewardDistributionAbandoned events: {e}"
            ));
        }
    };
    let mut events = subscription.into_stream();

    while let Some(event) = events.next().await {
        match event {
            Ok((event, _log)) => {
                let key = RoundKey {
                    heartbeat_key: event.heartbeatKey,
                    round: event.round,
                };
                let mut guard = state.lock().await;
                let entry = guard.rounds.entry(key).or_insert_with(RoundState::new);
                entry.rewards_done = true;
                info!(
                    heartbeat_key = ?event.heartbeatKey,
                    round = event.round,
                    "Reward distribution abandoned"
                );
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Error receiving RewardDistributionAbandoned event: {e}"
                ));
            }
        }
    }

    Err(anyhow::anyhow!(
        "RewardDistributionAbandoned event stream ended unexpectedly"
    ))
}

async fn listen_slashing_callback_failed(l2_client: Arc<L2KeeperClient>) -> Result<()> {
    let subscription = match l2_client
        .heartbeat_manager()
        .event_filter::<SlashingCallbackFailedEvent>()
        .subscribe()
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to subscribe to SlashingCallbackFailed events: {e}"
            ));
        }
    };
    let mut events = subscription.into_stream();

    while let Some(event) = events.next().await {
        match event {
            Ok((event, _log)) => {
                info!(
                    heartbeat_key = ?event.heartbeatKey,
                    round = event.round,
                    "Slashing callback failed, retrying"
                );
                if let Err(e) =
                    retry_slashing(l2_client.clone(), event.heartbeatKey, event.round).await
                {
                    error!(error = %e, "Failed to retry slashing callback");
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Error receiving SlashingCallbackFailed event: {e}"
                ));
            }
        }
    }

    Err(anyhow::anyhow!(
        "SlashingCallbackFailed event stream ended unexpectedly"
    ))
}

fn format_contract_error<E: std::fmt::Display + std::fmt::Debug>(err: &E) -> String {
    decode_any_error(err).to_string()
}
