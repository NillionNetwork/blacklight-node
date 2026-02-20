use alloy::{
    network::EthereumWallet,
    primitives::{
        Address, U256,
        utils::{format_units, parse_units},
    },
    providers::{DynProvider, Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
};
use anyhow::{Context, Result};
use blacklight_contract_clients::NodeOperatorFactoryClient;
use clap::Args;
use std::sync::Arc;
use tokio::sync::Mutex;

sol!(
    #[sol(rpc)]
    contract IERC20 {
        function approve(address spender, uint256 value) external returns (bool);
    }
);

#[derive(Args, Debug)]
pub struct FactoryArgs {
    #[command(subcommand)]
    pub command: FactoryCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum FactoryCommand {
    /// Show factory config, all nodes, their operators, and assignments
    Status,

    // ── Owner config ──────────────────────────────────
    /// Set the StakingOperators contract address
    SetStakingOperators { address: String },
    /// Set the RewardPolicy contract address
    SetRewardPolicy { address: String },
    /// Set the staking token address
    SetStakingToken { address: String },
    /// Set the reward token address
    SetRewardToken { address: String },
    /// Set the default mode fee in basis points (withdraw_bps restake_bps)
    SetDefaultModeFeeBps {
        withdraw_bps: String,
        restake_bps: String,
    },
    /// Set operator-specific mode fee in basis points
    SetOperatorModeFeeBps {
        operator: String,
        withdraw_bps: String,
        restake_bps: String,
    },
    /// Set the minimum stake amount (in NIL, e.g. 1000)
    SetMinStake { amount: String },

    // ── Node management ──────────────────────────────
    /// Add a node to the factory
    AddNode { node: String },
    /// Add multiple nodes to the factory (inline or from file)
    AddNodes {
        /// Node addresses as positional args
        nodes: Vec<String>,
        /// Path to a file containing node addresses (one per line)
        #[arg(long)]
        file: Option<String>,
    },
    /// Remove a node from the factory
    RemoveNode { node: String },

    // ── Rewards ──────────────────────────────────────
    /// Harvest rewards for a specific operator
    HarvestRewards { operator: String },
    /// Harvest rewards for all operators
    HarvestAllRewards,
    /// Withdraw collected fees (amount in NIL, e.g. 100)
    WithdrawFees { amount: String, to: String },

    // ── Staking ──────────────────────────────────────
    /// Stake tokens (amount in NIL, e.g. 1000000)
    Stake { amount: String },
    /// Request unstake of tokens (amount in NIL)
    RequestUnstake { amount: String },
    /// Withdraw unstaked tokens after unbonding
    WithdrawUnstaked,
    /// Claim user rewards
    ClaimRewards,
    /// Set reward behavior (0 = WithdrawToUser, 1 = AutoRestake)
    SetRewardBehavior { behavior: u8 },
    /// Check pending rewards for a user address
    PendingRewards { user: String },
}

fn parse_address(s: &str) -> Result<Address> {
    s.parse::<Address>()
        .with_context(|| format!("invalid address: {s}"))
}

fn parse_u256(s: &str) -> Result<U256> {
    U256::from_str_radix(s, 10).with_context(|| format!("invalid uint256: {s}"))
}

/// Parse a human-readable NIL amount (6 decimals) into its smallest unit.
fn parse_nil(s: &str) -> Result<U256> {
    Ok(parse_units(s, 6)
        .with_context(|| format!("invalid NIL amount: {s}"))?
        .into())
}

fn fmt_addr(addr: Address) -> String {
    if addr == Address::ZERO {
        "(none)".to_string()
    } else {
        format!("{addr}")
    }
}

struct Env {
    rpc_url: String,
    private_key: String,
    factory_address: Address,
}

impl Env {
    fn load() -> Result<Self> {
        let rpc_url = std::env::var("RPC_URL").context("RPC_URL not set (env or .env)")?;
        let private_key =
            std::env::var("PRIVATE_KEY").context("PRIVATE_KEY not set (env or .env)")?;
        let factory_address = std::env::var("NODE_OPERATOR_FACTORY_ADDRESS")
            .context("NODE_OPERATOR_FACTORY_ADDRESS not set (env or .env)")?
            .parse::<Address>()
            .context("invalid NODE_OPERATOR_FACTORY_ADDRESS")?;
        Ok(Self {
            rpc_url,
            private_key,
            factory_address,
        })
    }
}

fn build_provider(env: &Env) -> Result<DynProvider> {
    let signer: PrivateKeySigner = env
        .private_key
        .parse::<PrivateKeySigner>()
        .context("invalid PRIVATE_KEY")?;
    let wallet = EthereumWallet::from(signer);

    let provider: DynProvider = ProviderBuilder::new()
        .wallet(wallet)
        .with_simple_nonce_management()
        .with_gas_estimation()
        .connect_http(env.rpc_url.parse().context("invalid RPC_URL")?)
        .erased();

    Ok(provider)
}

pub async fn run(args: FactoryArgs) -> Result<()> {
    let env = Env::load()?;
    let provider = build_provider(&env)?;
    let tx_lock = Arc::new(Mutex::new(()));
    let factory = NodeOperatorFactoryClient::new(provider.clone(), env.factory_address, tx_lock);

    match args.command {
        FactoryCommand::Status => {
            // ── Factory config ────────────────────────────
            println!("Factory:            {}", env.factory_address);
            println!("RPC URL:            {}", env.rpc_url);

            let has_code = provider
                .get_code_at(env.factory_address)
                .await
                .map(|code| !code.is_empty())
                .unwrap_or(false);

            if !has_code {
                println!("\n(no contract deployed at factory address)");
                return Ok(());
            }

            macro_rules! query {
                ($label:expr, $call:expr) => {
                    match $call.await {
                        Ok(v) => println!("{:<20}{v}", concat!($label, ":")),
                        Err(e) => println!("{:<20}(error: {e})", concat!($label, ":")),
                    }
                };
            }
            query!("StakingOperators", factory.staking_operators());
            query!("RewardPolicy", factory.reward_policy());
            query!("StakingToken", factory.staking_token());
            query!("RewardToken", factory.reward_token());
            query!("WithdrawFeeBps", factory.default_withdraw_fee_bps());
            query!("RestakeFeeBps", factory.default_restake_fee_bps());

            match factory.min_stake().await {
                Ok(v) => println!(
                    "{:<20}{} NIL",
                    "MinStake:",
                    format_units(v, 6).unwrap_or_else(|_| format!("{v}"))
                ),
                Err(e) => println!("{:<20}(error: {e})", "MinStake:"),
            }

            // ── Node table ───────────────────────────────
            let nodes = factory.all_nodes().await?;
            let total = nodes.len();
            let free_count = factory
                .free_node_count()
                .await
                .map(|c| format!("{c}"))
                .unwrap_or_else(|_| "?".to_string());

            println!("\nNodes: {total} total, {free_count} free\n");

            if nodes.is_empty() {
                println!("  (no nodes registered)");
            } else {
                println!(
                    "  {:<4} {:<44} {:<44} {:<44} {:<10} {:<14} {:<14} {}",
                    "#",
                    "Node",
                    "Operator",
                    "User",
                    "Status",
                    "WithdrawBps",
                    "RestakeBps",
                    "Behavior"
                );
                println!("  {}", "-".repeat(190));

                for (i, node) in nodes.iter().enumerate() {
                    let operator_addr = factory
                        .node_to_operator(*node)
                        .await
                        .unwrap_or(Address::ZERO);
                    let operator = fmt_addr(operator_addr);
                    let free = factory.is_free_node(*node).await.unwrap_or(false);
                    let user_addr = factory.node_to_user(*node).await.unwrap_or(Address::ZERO);
                    let user = fmt_addr(user_addr);
                    let status = if free { "free" } else { "assigned" };

                    // Fetch per-operator fee bps
                    let (withdraw_bps, restake_bps) = if operator_addr != Address::ZERO {
                        factory
                            .operator_mode_fee_bps(operator_addr)
                            .await
                            .unwrap_or((U256::ZERO, U256::ZERO))
                    } else {
                        (U256::ZERO, U256::ZERO)
                    };

                    // Fetch reward behavior for assigned users
                    let behavior = if !free && user_addr != Address::ZERO {
                        match factory.my_reward_behavior(user_addr).await {
                            Ok(0) => "Withdraw",
                            Ok(1) => "AutoRestake",
                            Ok(_) => "Unknown",
                            Err(_) => "?",
                        }
                    } else {
                        "-"
                    };

                    println!(
                        "  {:<4} {:<44} {:<44} {:<44} {:<10} {:<14} {:<14} {}",
                        i + 1,
                        node,
                        operator,
                        user,
                        status,
                        withdraw_bps,
                        restake_bps,
                        behavior
                    );
                }
            }
        }

        // ── Owner config ──────────────────────────────
        FactoryCommand::SetStakingOperators { address } => {
            let addr = parse_address(&address)?;
            let tx = factory.set_staking_operators(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetRewardPolicy { address } => {
            let addr = parse_address(&address)?;
            let tx = factory.set_reward_policy(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetStakingToken { address } => {
            let addr = parse_address(&address)?;
            let tx = factory.set_staking_token(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetRewardToken { address } => {
            let addr = parse_address(&address)?;
            let tx = factory.set_reward_token(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetDefaultModeFeeBps {
            withdraw_bps,
            restake_bps,
        } => {
            let withdraw = parse_u256(&withdraw_bps)?;
            let restake = parse_u256(&restake_bps)?;
            let tx = factory.set_default_mode_fee_bps(withdraw, restake).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetOperatorModeFeeBps {
            operator,
            withdraw_bps,
            restake_bps,
        } => {
            let addr = parse_address(&operator)?;
            let withdraw = parse_u256(&withdraw_bps)?;
            let restake = parse_u256(&restake_bps)?;
            let tx = factory
                .set_operator_mode_fee_bps(addr, withdraw, restake)
                .await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetMinStake { amount } => {
            let amount = parse_nil(&amount)?;
            let tx = factory.set_min_stake(amount).await?;
            println!("tx: {tx}");
        }

        // ── Node management ──────────────────────────
        FactoryCommand::AddNode { node } => {
            let addr = parse_address(&node)?;
            let tx = factory.add_node(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::AddNodes { nodes, file } => {
            let mut all_nodes = nodes;
            if let Some(path) = file {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("failed to read file: {path}"))?;
                let from_file: Vec<String> = content
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .collect();
                all_nodes.extend(from_file);
            }
            if all_nodes.is_empty() {
                anyhow::bail!("no node addresses provided (use positional args or --file)");
            }
            let addrs: Vec<Address> = all_nodes
                .iter()
                .map(|n| parse_address(n))
                .collect::<Result<_>>()?;
            println!("Adding {} nodes ...", addrs.len());
            let tx = factory.add_nodes(addrs).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::RemoveNode { node } => {
            let addr = parse_address(&node)?;
            let tx = factory.remove_node(addr).await?;
            println!("tx: {tx}");
        }

        // ── Rewards ──────────────────────────────────
        FactoryCommand::HarvestRewards { operator } => {
            let addr = parse_address(&operator)?;
            let tx = factory.harvest_rewards(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::HarvestAllRewards => {
            let tx = factory.harvest_all_rewards().await?;
            println!("tx: {tx}");
        }
        FactoryCommand::WithdrawFees { amount, to } => {
            let amount = parse_nil(&amount)?;
            let to = parse_address(&to)?;
            let tx = factory.withdraw_fees(amount, to).await?;
            println!("tx: {tx}");
        }

        // ── Staking ──────────────────────────────────
        FactoryCommand::Stake { amount } => {
            let amount = parse_nil(&amount)?;
            let staking_token = factory.staking_token().await?;
            let erc20 = IERC20::new(staking_token, provider);
            let approve_tx = erc20
                .approve(env.factory_address, amount)
                .send()
                .await?
                .watch()
                .await?;
            println!("approve tx: {approve_tx}");
            let tx = factory.stake(amount).await?;
            println!("stake tx: {tx}");
        }
        FactoryCommand::RequestUnstake { amount } => {
            let amount = parse_nil(&amount)?;
            let tx = factory.request_unstake(amount).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::WithdrawUnstaked => {
            let tx = factory.withdraw_unstaked().await?;
            println!("tx: {tx}");
        }
        FactoryCommand::ClaimRewards => {
            let tx = factory.claim_rewards().await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SetRewardBehavior { behavior } => {
            let tx = factory.set_my_reward_behavior(behavior).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::PendingRewards { user } => {
            let addr = parse_address(&user)?;
            let rewards = factory.pending_rewards(addr).await?;
            println!(
                "Pending rewards: {} NIL",
                format_units(rewards, 6).unwrap_or_else(|_| format!("{rewards}"))
            );
        }
    }

    Ok(())
}
