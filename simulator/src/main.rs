use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod cli;
mod common;
mod erc8004;
mod nilcc;

#[derive(Parser, Debug)]
#[command(name = "simulator")]
#[command(about = "Blacklight simulators: HTX submission (nilcc) and ERC-8004 validation requests", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Submit HTXs to the HeartbeatManager contract (nilCC attestations)
    Nilcc(nilcc::NilccArgs),
    /// Register agents and submit ERC-8004 validation requests
    Erc8004(erc8004::Erc8004Args),
    /// One-shot CLI commands for interacting with contracts
    Cli(cli::CliArgs),
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Command::Nilcc(args) => common::run_simulator::<nilcc::NilccSimulator>(args).await,
        Command::Erc8004(args) => common::run_simulator::<erc8004::Erc8004Simulator>(args).await,
        Command::Cli(args) => cli::run(args).await,
    }
}
