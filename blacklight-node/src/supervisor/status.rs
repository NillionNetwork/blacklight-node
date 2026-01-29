use alloy::primitives::utils::{format_ether, format_units};
use anyhow::{Result, anyhow};
use blacklight_contract_clients::BlacklightClient;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::args::MIN_ETH_BALANCE;

/// Print status information (ETH balance, staked balance, verified HTXs)
pub async fn print_status(client: &BlacklightClient, verified_count: u64) -> Result<()> {
    let eth_balance = client.get_balance().await?;
    let node_address = client.signer_address();
    let staked_balance = client.staking.stake_of(node_address).await?;

    info!(
        "📊 STATUS | ETH: {} | STAKED: {} NIL | Verified HTXs since boot: {}",
        format_ether(eth_balance),
        format_units(staked_balance, 6)?,
        verified_count
    );

    Ok(())
}

/// Print status and check balance after HTX processing
pub async fn check_minimum_balance(
    client: &BlacklightClient,
    shutdown_token: &CancellationToken,
) -> Result<()> {
    // Check if balance is below minimum threshold
    match client.get_balance().await {
        Ok(balance) => {
            if balance < MIN_ETH_BALANCE {
                error!(
                    balance = %format_ether(balance),
                    min_required = %format_ether(MIN_ETH_BALANCE),
                    "⚠️ ETH balance below minimum threshold. Initiating shutdown..."
                );
                shutdown_token.cancel();
                return Err(anyhow!("Insufficient ETH balance"));
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to check balance after transaction");
        }
    }

    Ok(())
}
