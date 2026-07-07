use crate::chain_profile::FeeStrategy;
use crate::errors::{DecodedRevert, extract_revert_from_contract_error_with_custom};
use crate::overestimate_gas;
use alloy::{
    consensus::Transaction,
    contract::CallBuilder,
    eips::eip1559::Eip1559Estimation,
    primitives::{B256, Bytes},
    providers::Provider,
    rpc::types::TransactionReceipt,
};
use anyhow::{Result, bail};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Receipt poll cadence while waiting on an EIP-1559 transaction.
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Hard wall-clock ceiling on waiting for an EIP-1559 tx (incl. replacements).
const MAX_WAIT: Duration = Duration::from_secs(300);

/// Node replacement rules require at least +10% on both fee fields.
const MIN_REPLACEMENT_BUMP_PERCENT: u128 = 10;

/// A reusable error decoder function pointer.
/// Takes raw ABI-encoded revert data and returns a decoded error if recognized.
pub type ErrorDecoder = fn(&Bytes) -> Option<DecodedRevert>;

#[derive(Clone)]
pub struct TransactionSubmitter {
    tx_lock: Arc<Mutex<()>>,
    gas_buffer: bool,
    decoder: ErrorDecoder,
    fee_strategy: FeeStrategy,
}

impl TransactionSubmitter {
    pub fn new(tx_lock: Arc<Mutex<()>>, decoder: ErrorDecoder) -> Self {
        Self {
            tx_lock,
            gas_buffer: false,
            decoder,
            fee_strategy: FeeStrategy::default(),
        }
    }

    /// Use a specific fee strategy (N4/N7). The default reproduces the L2 rule exactly.
    pub fn with_fee_strategy(mut self, fee_strategy: FeeStrategy) -> Self {
        self.fee_strategy = fee_strategy;
        self
    }

    pub async fn invoke<P, D>(&self, method: &str, call: CallBuilder<P, D>) -> Result<B256>
    where
        P: Provider + Clone,
        D: alloy::contract::CallDecoder + Clone,
    {
        // Pre-simulate to catch reverts with proper error messages
        if let Err(e) = call.call().await {
            let e = self.decode_error(e);
            bail!("{method} reverted: {e}");
        }

        let (call, gas_limit) = match self.gas_buffer {
            true => {
                let gas = overestimate_gas(&call, self.decoder).await?;
                (call.gas(gas), Some(gas))
            }
            false => (call, None),
        };

        let provider = call.provider.clone();
        let estimate = provider.estimate_eip1559_fees().await?;

        let (max_fee, priority_fee) = compute_fees(&self.fee_strategy, &estimate)
            .map_err(|e| anyhow::anyhow!("{method} not sent: {e}"))?;
        let call = call
            .max_priority_fee_per_gas(priority_fee)
            .max_fee_per_gas(max_fee);

        let estimated_gas = call.clone().estimate_gas().await?;

        // Acquire lock and send. The lock is held through any stuck-tx replacement so
        // nonce management stays serialized.
        let _guard = self.tx_lock.lock().await;

        let receipt = match &self.fee_strategy {
            FeeStrategy::L2MinPriority => {
                // exactly the pre-N4 path
                let pending = match call.send().await {
                    Ok(pending) => pending,
                    Err(e) => {
                        let e = self.decode_error(e);
                        bail!("{method} failed to send: {e}");
                    }
                };
                pending.get_receipt().await?
            }
            FeeStrategy::Eip1559 {
                max_fee_cap_gwei,
                bump_percent,
                bump_after_blocks,
            } => {
                self.send_with_replacement(
                    method,
                    call,
                    max_fee,
                    priority_fee,
                    max_fee_cap_gwei.map(gwei_to_wei),
                    *bump_percent,
                    *bump_after_blocks,
                )
                .await?
            }
        };
        let tx_hash = receipt.transaction_hash;

        Self::log_fee_details(
            &provider,
            method,
            tx_hash,
            &receipt,
            estimated_gas,
            estimate.max_priority_fee_per_gas,
        )
        .await;

        // Validate success
        if !receipt.status() {
            if let Some(gas_limit) = gas_limit {
                let used = receipt.gas_used;
                if used >= gas_limit {
                    bail!(
                        "{method} ran out of gas (used {used} of {gas_limit} limit). Tx: {tx_hash:?}"
                    );
                }
            }

            bail!("{method} reverted on-chain. Tx hash: {tx_hash:?}");
        }

        Ok(tx_hash)
    }

    pub fn with_gas_buffer(&self) -> Self {
        let mut this = self.clone();
        this.gas_buffer = true;
        this
    }

    /// Send an EIP-1559 tx and watch for its receipt; if no receipt lands within
    /// `bump_after_blocks` blocks, replace it (same nonce) with fees bumped by
    /// `bump_percent`, up to the cap. Any of the candidate hashes' receipts completes
    /// the wait. On `nonce too low`/`already known` send errors the nonce is refetched
    /// and the send retried once (reorg discipline).
    #[allow(clippy::too_many_arguments)]
    async fn send_with_replacement<P, D>(
        &self,
        method: &str,
        call: CallBuilder<P, D>,
        initial_max_fee: u128,
        initial_priority_fee: u128,
        cap_wei: Option<u128>,
        bump_percent: u8,
        bump_after_blocks: u64,
    ) -> Result<TransactionReceipt>
    where
        P: Provider + Clone,
        D: alloy::contract::CallDecoder + Clone,
    {
        let provider = call.provider.clone();

        let pending = match call.clone().send().await {
            Ok(pending) => pending,
            Err(e) => {
                let msg = self.decode_error(e);
                if !is_nonce_race(&msg) {
                    bail!("{method} failed to send: {msg}");
                }
                // reorg/nonce race: the simple nonce manager refetches on the next call
                warn!(method, error = %msg, "send hit a nonce race; refetching nonce and retrying once");
                match call.clone().send().await {
                    Ok(pending) => pending,
                    Err(e) => {
                        let e = self.decode_error(e);
                        bail!("{method} failed to send after nonce retry: {e}");
                    }
                }
            }
        };

        let first_hash = *pending.tx_hash();
        let mut hashes = vec![first_hash];

        // Nonce of the in-flight tx: required to construct replacements.
        let nonce = match provider.get_transaction_by_hash(first_hash).await? {
            Some(tx) => Some(tx.nonce()),
            None => None,
        };

        let mut current_max_fee = initial_max_fee;
        let mut current_priority = initial_priority_fee;
        let mut last_bump_block = provider.get_block_number().await?;
        let started = std::time::Instant::now();

        loop {
            for h in &hashes {
                if let Some(receipt) = provider.get_transaction_receipt(*h).await? {
                    return Ok(receipt);
                }
            }

            if started.elapsed() > MAX_WAIT {
                bail!(
                    "{method} saw no receipt within {}s (candidate txs: {hashes:?})",
                    MAX_WAIT.as_secs()
                );
            }

            let now_block = provider.get_block_number().await?;
            if now_block.saturating_sub(last_bump_block) >= bump_after_blocks {
                let Some(nonce) = nonce else {
                    // could not learn the nonce (tx dropped from the pool?); keep waiting
                    warn!(
                        method,
                        "stuck tx nonce unknown; cannot replace, still waiting"
                    );
                    last_bump_block = now_block;
                    continue;
                };

                match bump_fees(current_max_fee, current_priority, bump_percent, cap_wei) {
                    Some((new_max_fee, new_priority)) => {
                        let bumped = call
                            .clone()
                            .nonce(nonce)
                            .max_fee_per_gas(new_max_fee)
                            .max_priority_fee_per_gas(new_priority);
                        match bumped.send().await {
                            Ok(p) => {
                                info!(
                                    method,
                                    nonce,
                                    old_max_fee = current_max_fee,
                                    new_max_fee,
                                    old_priority_fee = current_priority,
                                    new_priority_fee = new_priority,
                                    "stuck tx: sent fee-bumped replacement"
                                );
                                hashes.push(*p.tx_hash());
                                current_max_fee = new_max_fee;
                                current_priority = new_priority;
                            }
                            Err(e) => {
                                let msg = self.decode_error(e);
                                if is_nonce_race(&msg) {
                                    // original (or a replacement) likely just landed;
                                    // loop back to receipt polling
                                    info!(method, error = %msg, "replacement raced a landing tx");
                                } else {
                                    warn!(method, error = %msg, "replacement send failed");
                                }
                            }
                        }
                    }
                    None => {
                        warn!(
                            method,
                            current_max_fee,
                            cap_wei = ?cap_wei,
                            "stuck tx: fee cap prevents a valid (>=10%) bump; waiting at cap"
                        );
                    }
                }
                last_bump_block = now_block;
            }

            tokio::time::sleep(RECEIPT_POLL_INTERVAL).await;
        }
    }

    fn decode_error(&self, error: alloy::contract::Error) -> String {
        extract_revert_from_contract_error_with_custom(&error, self.decoder).to_string()
    }

    async fn log_fee_details<P: Provider + Clone>(
        provider: &P,
        method: &str,
        tx_hash: B256,
        receipt: &TransactionReceipt,
        estimated_gas: u64,
        estimated_priority_fee: u128,
    ) {
        // Fetch actual transaction to get the real fee parameters
        let (tx_max_fee, tx_max_priority_fee) =
            match provider.get_transaction_by_hash(tx_hash).await {
                Ok(Some(tx)) => (Some(tx.max_fee_per_gas()), tx.max_priority_fee_per_gas()),
                _ => (None, None),
            };

        // Calculate actual priority fee paid: effective_gas_price - base_fee
        let actual_priority_fee = if let Some(block_num) = receipt.block_number {
            provider
                .get_block_by_number(block_num.into())
                .await
                .ok()
                .flatten()
                .and_then(|b| b.header.base_fee_per_gas)
                .map(|base_fee| receipt.effective_gas_price.saturating_sub(base_fee as u128))
        } else {
            None
        };

        let total_cost = receipt.effective_gas_price * receipt.gas_used as u128;
        let actual_priority_fee = actual_priority_fee.unwrap_or(0);
        if actual_priority_fee < estimated_priority_fee.saturating_sub(1_000_000_000u128) {
            warn!(
                method = %method,
                tx_hash = ?tx_hash,
                effective_gas_price = receipt.effective_gas_price,
                gas_used = receipt.gas_used,
                estimated_gas = ?estimated_gas,
                total_cost,
                tx_max_fee = ?tx_max_fee,
                tx_max_priority_fee = ?tx_max_priority_fee,
                actual_priority_fee = ?actual_priority_fee,
                estimated_priority_fee = ?estimated_priority_fee,
                "💰 transaction gas details (priority fee may be too low)"
            );
        } else {
            info!(
                method = %method,
                tx_hash = ?tx_hash,
                effective_gas_price = receipt.effective_gas_price,
                gas_used = receipt.gas_used,
                estimated_gas = ?estimated_gas,
                total_cost,
                tx_max_fee = ?tx_max_fee,
                tx_max_priority_fee = ?tx_max_priority_fee,
                actual_priority_fee = ?actual_priority_fee,
                estimated_priority_fee = ?estimated_priority_fee,
                "💰 transaction gas details"
            );
        }
    }
}

/// Convert a gwei cap to wei.
fn gwei_to_wei(gwei: u64) -> u128 {
    gwei as u128 * 1_000_000_000
}

/// True for send errors that indicate a nonce race (a tx with this nonce already exists
/// or already landed) rather than a genuine failure.
fn is_nonce_race(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("nonce too low") || m.contains("already known") || m.contains("already imported")
}

/// Select (max_fee, priority_fee) per the strategy.
///
/// - `L2MinPriority`: alloy's estimate with the priority fee overridden to 1 wei — the
///   pre-N4 behaviour, byte-identical.
/// - `Eip1559`: alloy's fee-history estimate unmodified; errors if the estimated max fee
///   exceeds the configured cap (the tx stays queued for the caller to retry).
fn compute_fees(strategy: &FeeStrategy, estimate: &Eip1559Estimation) -> Result<(u128, u128)> {
    match strategy {
        FeeStrategy::L2MinPriority => Ok((estimate.max_fee_per_gas, 1u128)),
        FeeStrategy::Eip1559 {
            max_fee_cap_gwei, ..
        } => {
            if let Some(cap_gwei) = max_fee_cap_gwei {
                let cap_wei = gwei_to_wei(*cap_gwei);
                if estimate.max_fee_per_gas > cap_wei {
                    bail!(
                        "estimated max fee {} wei exceeds cap {} wei; queuing for retry",
                        estimate.max_fee_per_gas,
                        cap_wei
                    );
                }
            }
            Ok((estimate.max_fee_per_gas, estimate.max_priority_fee_per_gas))
        }
    }
}

/// Compute replacement fees bumped by `bump_percent`, respecting the cap.
///
/// Returns `None` when the cap leaves no room for a valid replacement (node rules
/// require at least +10% on both fields).
fn bump_fees(
    current_max_fee: u128,
    current_priority: u128,
    bump_percent: u8,
    cap_wei: Option<u128>,
) -> Option<(u128, u128)> {
    let pct = (bump_percent as u128).max(MIN_REPLACEMENT_BUMP_PERCENT);
    // +1 guards against zero-fee rounding never increasing
    let new_max_fee = current_max_fee + (current_max_fee * pct).div_ceil(100).max(1);
    let new_priority = current_priority + (current_priority * pct).div_ceil(100).max(1);

    if let Some(cap) = cap_wei {
        let min_valid = current_max_fee
            + (current_max_fee * MIN_REPLACEMENT_BUMP_PERCENT)
                .div_ceil(100)
                .max(1);
        if min_valid > cap {
            return None;
        }
        let capped_max = new_max_fee.min(cap);
        let capped_priority = new_priority.min(capped_max);
        return Some((capped_max, capped_priority));
    }
    Some((new_max_fee, new_priority.min(new_max_fee)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate(max_fee: u128, priority: u128) -> Eip1559Estimation {
        Eip1559Estimation {
            max_fee_per_gas: max_fee,
            max_priority_fee_per_gas: priority,
        }
    }

    #[test]
    fn l2_strategy_fee_fields_match_pre_n4_constants() {
        // regression pin: max fee from the estimate, priority hard-coded to 1 wei
        let (max_fee, priority) =
            compute_fees(&FeeStrategy::L2MinPriority, &estimate(42_000, 9_000)).unwrap();
        assert_eq!(max_fee, 42_000);
        assert_eq!(priority, 1);
    }

    #[test]
    fn eip1559_strategy_uses_estimator_unmodified() {
        let strategy = FeeStrategy::Eip1559 {
            max_fee_cap_gwei: None,
            bump_percent: 15,
            bump_after_blocks: 3,
        };
        let (max_fee, priority) = compute_fees(&strategy, &estimate(42_000, 9_000)).unwrap();
        assert_eq!(max_fee, 42_000);
        assert_eq!(priority, 9_000);
    }

    #[test]
    fn eip1559_cap_queues_instead_of_exceeding() {
        let strategy = FeeStrategy::Eip1559 {
            max_fee_cap_gwei: Some(1), // 1 gwei cap
            bump_percent: 15,
            bump_after_blocks: 3,
        };
        // estimate above the cap -> refuse to send
        assert!(compute_fees(&strategy, &estimate(2_000_000_000, 1)).is_err());
        // estimate below the cap -> pass-through
        assert!(compute_fees(&strategy, &estimate(500_000_000, 1)).is_ok());
    }

    #[test]
    fn bump_applies_percent_with_floor_of_ten() {
        // 15% bump
        assert_eq!(bump_fees(100, 50, 15, None), Some((115, 58)));
        // configured below the node minimum -> 10% floor applies
        assert_eq!(bump_fees(100, 50, 5, None), Some((110, 55)));
        // zero fees still strictly increase
        assert_eq!(bump_fees(0, 0, 15, None), Some((1, 1)));
    }

    #[test]
    fn bump_respects_cap() {
        // capped mid-bump: clamps to cap (still a valid >=10% replacement)
        assert_eq!(bump_fees(100, 100, 50, Some(120)), Some((120, 120)));
        // cap leaves no room for a valid +10% replacement -> no bump
        assert_eq!(bump_fees(100, 100, 15, Some(105)), None);
    }
}
