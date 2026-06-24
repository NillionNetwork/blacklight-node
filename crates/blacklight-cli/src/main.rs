mod cli;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "blacklight-cli",
    about = "Interact with Blacklight smart contracts"
)]
struct Cli {
    #[command(subcommand)]
    command: cli::CliCommand,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::from_filename("cli.env").ok();
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    cli::run(cli.command).await
}
