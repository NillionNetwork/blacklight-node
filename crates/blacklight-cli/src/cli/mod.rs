pub mod drain;
pub mod factory;
pub mod wallet;

use anyhow::Result;

#[derive(clap::Subcommand, Debug)]
pub enum CliCommand {
    /// Interact with the NodeOperatorFactory contract
    Factory(factory::FactoryArgs),
    /// Send ETH/NIL and check balances
    Wallet(wallet::WalletArgs),
    /// Drain ETH from a list of wallets back to a destination address
    Drain(drain::DrainArgs),
}

pub async fn run(command: CliCommand) -> Result<()> {
    match command {
        CliCommand::Factory(args) => factory::run(args).await,
        CliCommand::Wallet(args) => wallet::run(args).await,
        CliCommand::Drain(args) => drain::run(args).await,
    }
}
