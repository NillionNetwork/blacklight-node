use alloy::primitives::utils::{format_ether, format_units};
use anyhow::Result;
use blacklight_contract_clients::BlacklightClient;
use tracing::info;

/// Print status information (ETH balance, staked balance, verified HTXs)
pub async fn print_status(client: &BlacklightClient, verified_count: u64) -> Result<()> {
    let eth_balance = client.get_balance().await?;
    let node_address = client.signer_address();
    let staked_balance = client.staking.stake_of(node_address).await?;

    info!(
        "📊 STATUS | ETH: {} | STAKED: {} NIL | Verified HTXs: {}",
        format_ether(eth_balance),
        format_units(staked_balance, 6)?,
        verified_count
    );

    Ok(())
}
