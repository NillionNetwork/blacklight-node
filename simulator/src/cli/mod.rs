pub mod factory;
pub mod wallet;

use anyhow::Result;
use clap::Args;

#[derive(Args, Debug)]
pub struct CliArgs {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum CliCommand {
    /// Interact with the NodeOperatorFactory contract
    Factory(factory::FactoryArgs),
    /// Send ETH/NIL and check balances
    Wallet(wallet::WalletArgs),
}

pub async fn run(args: CliArgs) -> Result<()> {
    dotenv::from_filename("simulator.env").ok();
    match args.command {
        CliCommand::Factory(args) => factory::run(args).await,
        CliCommand::Wallet(args) => wallet::run(args).await,
    }
}
