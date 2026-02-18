use alloy::{
    network::EthereumWallet,
    primitives::{
        Address, U256,
        utils::{format_ether, format_units, parse_ether, parse_units},
    },
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol,
};
use anyhow::{Context, Result};
use clap::Args;

sol!(
    #[sol(rpc)]
    contract IERC20 {
        function transfer(address to, uint256 value) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }
);

#[derive(Args, Debug)]
pub struct WalletArgs {
    #[command(subcommand)]
    pub command: WalletCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum WalletCommand {
    /// Send ETH to an address (e.g. 0.1 for 0.1 ETH)
    SendEth { to: String, amount: String },
    /// Send NIL (staking token) to an address (e.g. 100 for 100 NIL)
    SendNil { to: String, amount: String },
    /// Check ETH balance of an address
    BalanceEth { address: String },
    /// Check NIL (staking token) balance of an address
    BalanceNil { address: String },
    /// Show current wallet address and its ETH + NIL balances
    Status,
}

fn parse_address(s: &str) -> Result<Address> {
    s.parse::<Address>()
        .with_context(|| format!("invalid address: {s}"))
}

fn load_provider() -> Result<(DynProvider, Address)> {
    let rpc_url = std::env::var("RPC_URL").context("RPC_URL not set (env or .env)")?;
    let private_key = std::env::var("PRIVATE_KEY").context("PRIVATE_KEY not set (env or .env)")?;

    let signer: PrivateKeySigner = private_key
        .parse::<PrivateKeySigner>()
        .context("invalid PRIVATE_KEY")?;
    let address = signer.address();
    let wallet = EthereumWallet::from(signer);

    let provider: DynProvider = ProviderBuilder::new()
        .wallet(wallet)
        .with_simple_nonce_management()
        .with_gas_estimation()
        .connect_http(rpc_url.parse().context("invalid RPC_URL")?)
        .erased();

    Ok((provider, address))
}

fn load_nil_token_address() -> Result<Address> {
    std::env::var("STAKE_TOKEN_ADDRESS")
        .context("STAKE_TOKEN_ADDRESS not set (env or .env)")?
        .parse::<Address>()
        .context("invalid STAKE_TOKEN_ADDRESS")
}

pub async fn run(args: WalletArgs) -> Result<()> {
    let (provider, my_address) = load_provider()?;

    match args.command {
        WalletCommand::SendEth { to, amount } => {
            let to = parse_address(&to)?;
            let amount =
                parse_ether(&amount).with_context(|| format!("invalid ETH amount: {amount}"))?;
            let tx = TransactionRequest::default().to(to).value(amount);
            let tx_hash = provider.send_transaction(tx).await?.watch().await?;
            println!("tx: {tx_hash}");
        }
        WalletCommand::SendNil { to, amount } => {
            let to = parse_address(&to)?;
            let amount: U256 = parse_units(&amount, 6)
                .with_context(|| format!("invalid NIL amount: {amount}"))?
                .into();
            let token = load_nil_token_address()?;
            let erc20 = IERC20::new(token, provider);
            let tx_hash = erc20.transfer(to, amount).send().await?.watch().await?;
            println!("tx: {tx_hash}");
        }
        WalletCommand::BalanceEth { address } => {
            let addr = parse_address(&address)?;
            let balance = provider.get_balance(addr).await?;
            println!("{} ETH", format_ether(balance));
        }
        WalletCommand::BalanceNil { address } => {
            let addr = parse_address(&address)?;
            let token = load_nil_token_address()?;
            let erc20 = IERC20::new(token, provider);
            let balance = erc20.balanceOf(addr).call().await?;
            println!("{} NIL", format_units(balance, 6)?);
        }
        WalletCommand::Status => {
            let eth_balance = provider.get_balance(my_address).await?;
            let nil_balance = match load_nil_token_address() {
                Ok(token) => {
                    let erc20 = IERC20::new(token, &provider);
                    match erc20.balanceOf(my_address).call().await {
                        Ok(b) => match format_units(b, 6) {
                            Ok(f) => format!("{f} NIL"),
                            Err(e) => format!("(error: {e})"),
                        },
                        Err(e) => format!("(error: {e})"),
                    }
                }
                Err(_) => "(STAKE_TOKEN_ADDRESS not set)".to_string(),
            };

            println!("Address:     {my_address}");
            println!("ETH balance: {} ETH", format_ether(eth_balance));
            println!("NIL balance: {nil_balance}");
        }
    }

    Ok(())
}
