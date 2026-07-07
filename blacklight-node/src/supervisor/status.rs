use alloy::primitives::utils::{format_ether, format_units};
use anyhow::{Result, bail};
use blacklight_contract_clients::BlacklightClient;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use alloy::primitives::U256;

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

/// Print status and check balance after HTX processing.
///
/// Two thresholds (N5): below `low_balance_threshold` the node WARNS so the operator can
/// top up before votes are at risk (a missed vote inside the response window means
/// jailing); below the hard MIN_ETH_BALANCE floor it shuts down.
pub async fn check_minimum_balance(
    client: &BlacklightClient,
    shutdown_token: &CancellationToken,
    low_balance_threshold: U256,
) -> Result<()> {
    match client.get_balance().await {
        Ok(balance) => {
            if balance < MIN_ETH_BALANCE {
                error!(
                    balance = %format_ether(balance),
                    min_required = %format_ether(MIN_ETH_BALANCE),
                    "⚠️ ETH balance below minimum threshold. Initiating shutdown..."
                );
                shutdown_token.cancel();
                bail!("Insufficient ETH balance");
            }
            if balance < low_balance_threshold {
                warn!(
                    balance = %format_ether(balance),
                    low_balance_threshold = %format_ether(low_balance_threshold),
                    "🪫 LOW ETH BALANCE: top up the node wallet to keep paying vote gas"
                );
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to check balance after transaction");
        }
    }

    Ok(())
}
