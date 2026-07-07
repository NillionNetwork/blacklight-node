//! WP7 (N4) integration tests against a local anvil: fee-strategy behaviour of
//! TransactionSubmitter, including the stuck-tx replacement path.

use alloy::{
    consensus::Transaction,
    contract::CallBuilder,
    network::EthereumWallet,
    node_bindings::Anvil,
    primitives::{Address, Bytes, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use contract_clients_common::chain_profile::FeeStrategy;
use contract_clients_common::errors::DecodedRevert;
use contract_clients_common::tx_submitter::TransactionSubmitter;
use std::sync::Arc;
use tokio::sync::Mutex;

fn no_decode(_: &Bytes) -> Option<DecodedRevert> {
    None
}

async fn provider_for(anvil: &alloy::node_bindings::AnvilInstance) -> DynProvider {
    let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
    let wallet = EthereumWallet::from(signer);
    ProviderBuilder::new()
        .wallet(wallet)
        .with_simple_nonce_management()
        .with_gas_estimation()
        .connect_http(anvil.endpoint().parse().unwrap())
        .erased()
}

fn transfer_call(provider: DynProvider, to: Address) -> CallBuilder<DynProvider, ()> {
    CallBuilder::new_raw(provider, Bytes::new())
        .to(to)
        .value(U256::from(1))
}

fn submitter(strategy: FeeStrategy) -> TransactionSubmitter {
    TransactionSubmitter::new(Arc::new(Mutex::new(())), no_decode).with_fee_strategy(strategy)
}

/// L2 regression pin: the default strategy produces the exact pre-N4 fee fields
/// (priority fee = 1 wei, max fee from the estimator).
#[tokio::test]
async fn l2_min_priority_path_fee_fields_byte_identical() {
    let anvil = Anvil::new().spawn();
    let provider = provider_for(&anvil).await;
    let recipient = Address::repeat_byte(0x42);

    let expected_max_fee = provider
        .estimate_eip1559_fees()
        .await
        .unwrap()
        .max_fee_per_gas;

    let call = transfer_call(provider.clone(), recipient);
    let tx_hash = submitter(FeeStrategy::L2MinPriority)
        .invoke("transfer", call)
        .await
        .unwrap();

    let tx = provider
        .get_transaction_by_hash(tx_hash)
        .await
        .unwrap()
        .expect("tx must exist");
    assert_eq!(
        tx.max_priority_fee_per_gas(),
        Some(1),
        "L2 rule: priority fee must be exactly 1 wei"
    );
    assert_eq!(
        tx.max_fee_per_gas(),
        expected_max_fee,
        "L2 rule: max fee must come from the estimator unmodified"
    );
}

/// EIP-1559 path: the estimator's priority fee is NOT overridden to 1 wei.
#[tokio::test]
async fn eip1559_path_uses_estimator_priority_fee() {
    let anvil = Anvil::new().spawn();
    let provider = provider_for(&anvil).await;
    let recipient = Address::repeat_byte(0x43);

    // Seed fee history: a few blocks whose txs tip 2 gwei, so the estimator's reward
    // percentile suggests a real (>1 wei) priority fee.
    for _ in 0..3 {
        let tx = alloy::rpc::types::TransactionRequest {
            to: Some(alloy::primitives::TxKind::Call(recipient)),
            value: Some(U256::from(1)),
            max_fee_per_gas: Some(5_000_000_000),
            max_priority_fee_per_gas: Some(2_000_000_000),
            ..Default::default()
        };
        provider
            .send_transaction(tx)
            .await
            .unwrap()
            .get_receipt()
            .await
            .unwrap();
    }

    let estimate = provider.estimate_eip1559_fees().await.unwrap();
    assert!(
        estimate.max_priority_fee_per_gas > 1,
        "fee-history seeding failed; estimator still suggests {} wei",
        estimate.max_priority_fee_per_gas
    );

    let call = transfer_call(provider.clone(), recipient);
    let strategy = FeeStrategy::Eip1559 {
        max_fee_cap_gwei: None,
        bump_percent: 15,
        bump_after_blocks: 3,
    };
    let tx_hash = submitter(strategy).invoke("transfer", call).await.unwrap();

    let tx = provider
        .get_transaction_by_hash(tx_hash)
        .await
        .unwrap()
        .expect("tx must exist");
    assert_eq!(
        tx.max_priority_fee_per_gas(),
        Some(estimate.max_priority_fee_per_gas)
    );
}

/// Cap respected: an estimate above the cap queues (errors) instead of sending.
#[tokio::test]
async fn eip1559_cap_queues_instead_of_sending() {
    let anvil = Anvil::new().spawn();
    let provider = provider_for(&anvil).await;
    let recipient = Address::repeat_byte(0x44);

    let call = transfer_call(provider.clone(), recipient);
    let strategy = FeeStrategy::Eip1559 {
        max_fee_cap_gwei: Some(0), // impossible cap
        bump_percent: 15,
        bump_after_blocks: 3,
    };
    let err = submitter(strategy)
        .invoke("transfer", call)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("exceeds cap"), "unexpected error: {err}");

    // nothing was sent
    let nonce = provider
        .get_transaction_count(anvil.addresses()[0])
        .await
        .unwrap();
    assert_eq!(nonce, 0);
}

/// Stuck-tx replacement: the first tx never lands (dropped from the pool, as in a
/// base-fee spike or mempool eviction); after `bump_after_blocks` blocks with no
/// receipt the submitter sends a same-nonce replacement with fees bumped exactly once,
/// which lands. (Anvil rejects below-base-fee txs at submission rather than queueing
/// them like a real L1 mempool, so the spike itself is rehearsed end-to-end on the
/// devnet in WP8/WP9; this pins the replacement machinery deterministically.)
#[tokio::test]
async fn stuck_tx_replaced_with_exactly_one_bump() {
    let anvil = Anvil::new().arg("--no-mining").spawn();
    let provider = provider_for(&anvil).await;
    let sender = anvil.addresses()[0];
    let recipient = Address::repeat_byte(0x45);

    // What invoke() will estimate (no blocks mined in between).
    let estimate = provider.estimate_eip1559_fees().await.unwrap();

    let strategy = FeeStrategy::Eip1559 {
        max_fee_cap_gwei: None,
        bump_percent: 15,
        bump_after_blocks: 2,
    };
    let call = transfer_call(provider.clone(), recipient);
    let sub = submitter(strategy);
    let invoke_task = tokio::spawn(async move { sub.invoke("transfer", call).await });

    // Let the tx get sent (accepted into the pool; nothing mines in --no-mining mode).
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // Find and drop the pending tx: it is now "stuck" and can never land.
    let pool: serde_json::Value = provider
        .raw_request("txpool_content".into(), ())
        .await
        .unwrap();
    let sender_key = format!("{sender:?}");
    let pending = &pool["pending"];
    let by_sender = pending
        .as_object()
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&sender_key))
                .map(|(_, v)| v)
        })
        .expect("sender must have a pending tx");
    let first_hash = by_sender["0"]["hash"]
        .as_str()
        .expect("pending nonce-0 tx must exist")
        .to_string();
    let _: serde_json::Value = provider
        .raw_request("anvil_dropTransaction".into(), (first_hash.clone(),))
        .await
        .unwrap();

    // Advance 3 empty blocks (>= bump_after_blocks) with no receipt.
    let _: serde_json::Value = provider
        .raw_request("anvil_mine".into(), (U256::from(3), U256::from(0)))
        .await
        .unwrap();

    // Give the submitter time to notice and send the single replacement.
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

    // Mine it.
    let _: serde_json::Value = provider
        .raw_request("anvil_mine".into(), (U256::from(1), U256::from(0)))
        .await
        .unwrap();

    let tx_hash = invoke_task
        .await
        .unwrap()
        .expect("invoke must succeed via the replacement");
    assert_ne!(
        format!("{tx_hash:?}"),
        first_hash,
        "receipt must come from the replacement, not the dropped tx"
    );

    let tx = provider
        .get_transaction_by_hash(tx_hash)
        .await
        .unwrap()
        .expect("tx must exist");

    // exactly one 15% bump over the original estimate
    let expected_max =
        estimate.max_fee_per_gas + (estimate.max_fee_per_gas * 15).div_ceil(100).max(1);
    let expected_priority_uncapped = estimate.max_priority_fee_per_gas
        + (estimate.max_priority_fee_per_gas * 15)
            .div_ceil(100)
            .max(1);
    let expected_priority = expected_priority_uncapped.min(expected_max);

    assert_eq!(tx.nonce(), 0, "replacement must reuse the original nonce");
    assert_eq!(
        tx.max_fee_per_gas(),
        expected_max,
        "landed tx must carry exactly one bump step"
    );
    assert_eq!(tx.max_priority_fee_per_gas(), Some(expected_priority));
}
