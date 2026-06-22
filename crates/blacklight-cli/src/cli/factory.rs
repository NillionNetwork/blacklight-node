use alloy::{
    network::EthereumWallet,
    primitives::{
        utils::{format_ether, format_units, parse_ether, parse_units},
        Address, U256,
    },
    providers::{DynProvider, Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
};
use anyhow::{Context, Result};
use blacklight_contract_clients::{FactoryManagerClient, StakingOperatorsClient};
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
    /// Atomically update staking operators, reward policy, and token addresses
    SetDependencies {
        staking_operators: String,
        reward_policy: String,
        token: String,
    },
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
    /// Prepare a node to run: predict operator, approve staker, send ETH, and add to factory.
    /// The node private key is passed via CLI or read from a file; the funder/owner key comes from PRIVATE_KEY in env.
    PrepareNode {
        /// Node's private key (hex, e.g. 0xabc...)
        #[arg(
            long,
            conflicts_with = "node_private_key_file",
            required_unless_present = "node_private_key_file"
        )]
        node_private_key: Option<String>,
        /// Path to a file containing the node's private key
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "node_private_key",
            required_unless_present = "node_private_key"
        )]
        node_private_key_file: Option<String>,
        /// Amount of ETH to send to the node (e.g. "0.1")
        #[arg(long)]
        eth_amount: String,
    },
    /// Prepare multiple nodes from a file of private keys (one per line).
    /// Runs the full prepare-node flow for each key: predict operator, send ETH, approve staker, add to factory.
    PrepareNodes {
        /// Path to a file with node private keys (one hex key per line)
        #[arg(long)]
        keys_file: String,
        /// Amount of ETH to send to each node (e.g. "0.1")
        #[arg(long)]
        eth_amount: String,
    },
    /// Predict the operator address that will be deployed for a node
    PredictOperator { node: String },
    /// Pre-approve predicted operators for nodes (requires node private keys).
    ApproveNodes {
        /// Path to a file with node private keys (one hex key per line)
        #[arg(long)]
        file: String,
    },
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
    /// Migrate a node operator to a new owner
    MigrateOperator { operator: String, new_owner: String },
    /// Sync a single operator's config with the factory
    SyncOperatorConfig { operator: String },
    /// Sync all operators' configs with the factory
    SyncAllOperatorConfigs,
    /// Rescue stranded ERC-20 tokens from a NodeOperator
    RescueOperatorTokens {
        operator: String,
        rescue_token: String,
        to: String,
        amount: String,
    },

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

fn build_provider(rpc_url: &str, private_key: &str) -> Result<(DynProvider, Address)> {
    let signer: PrivateKeySigner = private_key
        .parse::<PrivateKeySigner>()
        .context("invalid private key")?;
    let signer_address = signer.address();
    let wallet = EthereumWallet::from(signer);

    let provider: DynProvider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse().context("invalid RPC_URL")?)
        .erased();

    Ok((provider, signer_address))
}

fn read_private_keys_from_file(file: &str) -> Result<Vec<String>> {
    let content =
        std::fs::read_to_string(file).with_context(|| format!("failed to read file: {file}"))?;
    let keys = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    Ok(keys)
}

/// Core logic shared by `PrepareNode` and `PrepareNodes`.
async fn prepare_single_node(
    rpc_url: &str,
    client: &FactoryManagerClient,
    node_private_key: &str,
    eth: U256,
    label: &str,
) -> Result<()> {
    // 1. Derive node address from private key
    let (node_provider, node_addr) = build_provider(rpc_url, node_private_key)?;
    println!("{label}Node address:       {node_addr}");

    // 2. Predict operator address
    let predicted = client
        .factory
        .predict_node_operator_address(node_addr)
        .await?;
    println!("{label}Predicted operator: {predicted}");

    // 3. Send ETH to the node (shared provider — no nonce conflict)
    let send_tx = client.send_eth(node_addr, eth).await?;
    println!(
        "{label}send-eth tx:        {send_tx} ({} ETH -> {node_addr})",
        format_ether(eth)
    );

    // 4. Approve staker (signed by the node — separate wallet, separate nonce)
    let node_tx_lock = Arc::new(Mutex::new(()));
    let staking_ops =
        StakingOperatorsClient::at_address(node_provider, client.staking.address(), node_tx_lock);
    let approve_tx = staking_ops.approve_staker(predicted).await?;
    println!("{label}approve-staker tx:  {approve_tx}");

    // 5. Add node to factory (from the owner wallet)
    let add_tx = client.factory.add_node(node_addr).await?;
    println!("{label}add-node tx:        {add_tx}");

    Ok(())
}

fn resolve_prepare_node_private_key(
    node_private_key: Option<String>,
    node_private_key_file: Option<String>,
) -> Result<String> {
    match (node_private_key, node_private_key_file) {
        (Some(key), None) => Ok(key),
        (None, Some(file)) => {
            let keys = read_private_keys_from_file(&file)?;
            match keys.as_slice() {
                [] => anyhow::bail!("no private key found in file"),
                [key] => Ok(key.clone()),
                _ => anyhow::bail!("expected exactly one private key in file"),
            }
        }
        _ => anyhow::bail!("provide exactly one of --node-private-key or --node-private-key-file"),
    }
}

pub async fn run(args: FactoryArgs) -> Result<()> {
    let env = Env::load()?;
    let client = FactoryManagerClient::new(&env.rpc_url, &env.private_key, env.factory_address)
        .await
        .context("failed to create factory manager client")?;
    let factory = &client.factory;
    let provider = client.ctx().provider();

    match args.command {
        FactoryCommand::Status => {
            println!("=== Factory Status ===\n");
            println!("  Factory:  {}", env.factory_address);
            println!("  RPC URL:  {}", env.rpc_url);

            let has_code = provider
                .get_code_at(env.factory_address)
                .await
                .map(|code| !code.is_empty())
                .unwrap_or(false);

            if !has_code {
                println!("\n  (no contract deployed at factory address)");
                return Ok(());
            }

            println!();

            macro_rules! query {
                ($label:expr, $call:expr) => {
                    match $call.await {
                        Ok(v) => println!("  {:<20}{v}", concat!($label, ":")),
                        Err(e) => println!("  {:<20}(error: {e})", concat!($label, ":")),
                    }
                };
            }
            query!("StakingOperators", factory.staking_operators());
            query!("RewardPolicy", factory.reward_policy());
            query!("Token", factory.token());
            query!("WithdrawFeeBps", factory.default_withdraw_fee_bps());
            query!("RestakeFeeBps", factory.default_restake_fee_bps());

            match factory.min_stake().await {
                Ok(v) => println!(
                    "  {:<20}{} NIL",
                    "MinStake:",
                    format_units(v, 6).unwrap_or_else(|_| format!("{v}"))
                ),
                Err(e) => println!("  {:<20}(error: {e})", "MinStake:"),
            }

            // ── Node table ───────────────────────────────
            let nodes = factory.all_nodes().await?;
            let total = nodes.len();
            let free_count = factory
                .free_node_count()
                .await
                .map(|c| format!("{c}"))
                .unwrap_or_else(|_| "?".to_string());

            println!("\n=== Nodes ({total} total, {free_count} free) ===\n");

            if nodes.is_empty() {
                println!("  (no nodes registered)");
            } else {
                println!(
                    "  {:<4} {:<44} {:<44} {:<44} {:<10} {:<18} {:<12} {:<12} Behavior",
                    "#",
                    "Node",
                    "Operator",
                    "User",
                    "Status",
                    "ETH Balance",
                    "WdrawBps",
                    "RstakeBps",
                );
                println!("  {}", "-".repeat(200));

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

                    let eth_balance = match provider.get_balance(*node).await {
                        Ok(b) => format!("{} ETH", format_ether(b)),
                        Err(_) => "?".to_string(),
                    };

                    let (wb, rb) = if operator_addr != Address::ZERO {
                        factory
                            .operator_mode_fee_bps(operator_addr)
                            .await
                            .map(|(w, r)| (format!("{w}"), format!("{r}")))
                            .unwrap_or_else(|_| ("?".to_string(), "?".to_string()))
                    } else {
                        ("-".to_string(), "-".to_string())
                    };

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
                        "  {:<4} {:<44} {:<44} {:<44} {:<10} {:<18} {:<12} {:<12} {}",
                        i + 1,
                        node,
                        operator,
                        user,
                        status,
                        eth_balance,
                        wb,
                        rb,
                        behavior
                    );
                }
            }
        }

        // ── Owner config ──────────────────────────────
        FactoryCommand::SetDependencies {
            staking_operators,
            reward_policy,
            token,
        } => {
            let staking_ops = parse_address(&staking_operators)?;
            let reward = parse_address(&reward_policy)?;
            let tok = parse_address(&token)?;
            let tx = factory.set_dependencies(staking_ops, reward, tok).await?;
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
        FactoryCommand::PrepareNode {
            node_private_key,
            node_private_key_file,
            eth_amount,
        } => {
            let eth = parse_ether(&eth_amount)
                .with_context(|| format!("invalid ETH amount: {eth_amount}"))?;
            let node_private_key =
                resolve_prepare_node_private_key(node_private_key, node_private_key_file)?;

            prepare_single_node(&env.rpc_url, &client, &node_private_key, eth, "").await?;

            println!("\nNode is ready.");
        }
        FactoryCommand::PrepareNodes {
            keys_file,
            eth_amount,
        } => {
            let eth = parse_ether(&eth_amount)
                .with_context(|| format!("invalid ETH amount: {eth_amount}"))?;
            let keys = read_private_keys_from_file(&keys_file)?;
            if keys.is_empty() {
                anyhow::bail!("no private keys found in file");
            }

            println!("Preparing {} nodes ...\n", keys.len());

            let mut success_count = 0u64;
            let mut error_count = 0u64;

            for (i, key_str) in keys.iter().enumerate() {
                let label = format!("[{}/{}] ", i + 1, keys.len());
                match prepare_single_node(&env.rpc_url, &client, key_str, eth, &label).await {
                    Ok(()) => success_count += 1,
                    Err(e) => {
                        println!("{label}ERROR: {e}");
                        error_count += 1;
                    }
                }
            }

            println!("\n=== Summary ===");
            println!("Successful: {success_count}");
            println!("Errors:     {error_count}");
        }
        FactoryCommand::PredictOperator { node } => {
            let addr = parse_address(&node)?;
            let predicted = factory.predict_node_operator_address(addr).await?;
            println!("Node:               {addr}");
            println!("Predicted operator: {predicted}");
        }
        FactoryCommand::ApproveNodes { file } => {
            let keys = read_private_keys_from_file(&file)?;
            if keys.is_empty() {
                anyhow::bail!("no private keys found in file");
            }

            let staking_ops_addr = client.staking.address();
            println!("StakingOperators:   {staking_ops_addr}");
            println!("Approving {} nodes ...\n", keys.len());

            for key_str in &keys {
                let (node_provider, node_addr) = build_provider(&env.rpc_url, key_str)?;
                let predicted = factory.predict_node_operator_address(node_addr).await?;

                let node_tx_lock = Arc::new(Mutex::new(()));
                let staking_ops = StakingOperatorsClient::at_address(
                    node_provider,
                    staking_ops_addr,
                    node_tx_lock,
                );
                let tx = staking_ops.approve_staker(predicted).await?;
                println!("  node {node_addr} -> operator {predicted}  tx: {tx}");
            }
            println!("\nDone. You can now run add-node / add-nodes.");
        }
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
        FactoryCommand::MigrateOperator {
            operator,
            new_owner,
        } => {
            let op = parse_address(&operator)?;
            let owner = parse_address(&new_owner)?;
            let tx = factory.migrate_operator(op, owner).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SyncOperatorConfig { operator } => {
            let addr = parse_address(&operator)?;
            let tx = factory.sync_operator_config(addr).await?;
            println!("tx: {tx}");
        }
        FactoryCommand::SyncAllOperatorConfigs => {
            let tx = factory.sync_all_operator_configs().await?;
            println!("tx: {tx}");
        }
        FactoryCommand::RescueOperatorTokens {
            operator,
            rescue_token,
            to,
            amount,
        } => {
            let op = parse_address(&operator)?;
            let token = parse_address(&rescue_token)?;
            let to = parse_address(&to)?;
            let amount = parse_nil(&amount)?;
            let tx = factory
                .rescue_operator_tokens(op, token, to, amount)
                .await?;
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
            let staking_token = factory.token().await?;
            let erc20 = IERC20::new(staking_token, &provider);
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
