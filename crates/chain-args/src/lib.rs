use alloy::primitives::Address;
use anyhow::anyhow;
use clap::Args;
use contract_clients_common::chain_profile::{ChainProfile, FeeStrategy};
use state_file::StateFile;

#[derive(Args, Debug)]
pub struct ChainArgs {
    /// Ethereum RPC endpoint
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// Heartbeat manager contract address
    #[arg(long, env = "MANAGER_CONTRACT_ADDRESS")]
    pub manager_contract_address: Option<String>,

    /// blacklight staking contract address
    #[arg(long, env = "STAKING_CONTRACT_ADDRESS")]
    pub staking_contract_address: Option<String>,

    /// NIL token contract address
    #[arg(long, env = "TOKEN_CONTRACT_ADDRESS")]
    pub token_contract_address: Option<String>,

    // --- Chain profile (N7, L1 port). Defaults reproduce the pre-profile L2 behaviour. ---
    /// Fee strategy: "l2-min-priority" (default; 1-wei priority-fee rule) or "eip1559"
    /// (fee-history estimation with stuck-tx replacement, for L1)
    #[arg(long, env = "FEE_STRATEGY")]
    pub fee_strategy: Option<String>,

    /// Max fee cap in gwei (eip1559 strategy only; unset = uncapped)
    #[arg(long, env = "MAX_FEE_CAP_GWEI")]
    pub max_fee_cap_gwei: Option<u64>,

    /// Percent fee bump per stuck-tx replacement (eip1559 strategy only; default 15)
    #[arg(long, env = "FEE_BUMP_PERCENT")]
    pub fee_bump_percent: Option<u8>,

    /// Blocks without a receipt before a stuck tx is re-priced (eip1559 only; default 3)
    #[arg(long, env = "FEE_BUMP_AFTER_BLOCKS")]
    pub fee_bump_after_blocks: Option<u64>,

    /// Historical event query lookback in blocks (default 50)
    #[arg(long, env = "LOOKBACK_BLOCKS")]
    pub lookback_blocks: Option<u64>,

    /// Disable the WebSocket transport and use plain HTTP
    #[arg(long, env = "NO_WS", default_value_t = false)]
    pub no_ws: bool,
}

pub struct ChainConfig {
    pub rpc_url: String,
    pub manager_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub profile: ChainProfile,
}

impl ChainConfig {
    pub fn new(args: ChainArgs, state_file: &StateFile) -> anyhow::Result<Self> {
        // Load RPC URL with priority
        let rpc_url = args
            .rpc_url
            .or_else(|| state_file.load_value("RPC_URL"))
            .ok_or_else(|| anyhow!("no RPC url provided"))?;

        // Load contract addresses with priority
        let manager_contract_address = args
            .manager_contract_address
            .or_else(|| state_file.load_value("MANAGER_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no manager contract address provided"))?
            .parse()?;

        let staking_contract_address = args
            .staking_contract_address
            .or_else(|| state_file.load_value("STAKING_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no staking contract address provided"))?
            .parse()?;

        let token_contract_address = args
            .token_contract_address
            .or_else(|| state_file.load_value("TOKEN_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no token contract address provided"))?
            .parse()?;
        let fee_strategy_name = args
            .fee_strategy
            .or_else(|| state_file.load_value("FEE_STRATEGY"));
        let profile = ChainProfile {
            fee_strategy: FeeStrategy::resolve(
                fee_strategy_name.as_deref(),
                args.max_fee_cap_gwei,
                args.fee_bump_percent,
                args.fee_bump_after_blocks,
            )?,
            lookback_blocks: args
                .lookback_blocks
                .unwrap_or(contract_clients_common::chain_profile::DEFAULT_LOOKBACK_BLOCKS),
            ws: !args.no_ws,
        };

        Ok(Self {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
            profile,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> ChainArgs {
        ChainArgs {
            rpc_url: Some("http://localhost:8545".into()),
            manager_contract_address: Some(format!("{:?}", Address::ZERO)),
            staking_contract_address: Some(format!("{:?}", Address::ZERO)),
            token_contract_address: Some(format!("{:?}", Address::ZERO)),
            fee_strategy: None,
            max_fee_cap_gwei: None,
            fee_bump_percent: None,
            fee_bump_after_blocks: None,
            lookback_blocks: None,
            no_ws: false,
        }
    }

    #[test]
    fn no_new_keys_resolves_to_bit_identical_l2_profile() {
        let state_file = StateFile::new("chain_args_test_nonexistent.env");
        let cfg = ChainConfig::new(base_args(), &state_file).unwrap();
        assert_eq!(cfg.profile, ChainProfile::default());
        assert_eq!(cfg.profile.fee_strategy.fixed_priority_fee(), Some(1));
        assert_eq!(cfg.profile.lookback_blocks, 50);
        assert!(cfg.profile.ws);
    }

    #[test]
    fn l1_profile_resolves_eip1559() {
        let mut args = base_args();
        args.fee_strategy = Some("eip1559".into());
        args.max_fee_cap_gwei = Some(300);
        let state_file = StateFile::new("chain_args_test_nonexistent.env");
        let cfg = ChainConfig::new(args, &state_file).unwrap();
        assert_eq!(
            cfg.profile.fee_strategy,
            FeeStrategy::Eip1559 {
                max_fee_cap_gwei: Some(300),
                bump_percent: 15,
                bump_after_blocks: 3,
            }
        );
    }
}
