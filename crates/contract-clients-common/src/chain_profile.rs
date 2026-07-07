//! Chain profiles (N7, L1 port): per-deployment fee/reorg/transport behaviour, so one
//! binary can serve both the OP-stack L2 (1-wei priority-fee rule) and Ethereum L1
//! (real EIP-1559 estimation with stuck-tx replacement) purely via configuration.
//!
//! Defaults reproduce today's L2 behaviour bit-for-bit — a node or keeper started with
//! no new configuration keys behaves identically (the L2 regression gate).

use serde::{Deserialize, Serialize};

/// Priority fee (wei) hard-coded for the OP-stack L2 path — the pre-profile behaviour.
pub const L2_MIN_PRIORITY_FEE_WEI: u128 = 1;

/// Default historical-event lookback (blocks) — the pre-profile constant.
pub const DEFAULT_LOOKBACK_BLOCKS: u64 = 50;

/// Default fee bump per stuck-tx replacement (percent). Must be >= 10 to satisfy node
/// replacement rules.
pub const DEFAULT_FEE_BUMP_PERCENT: u8 = 15;

/// Default number of blocks without a receipt before a stuck tx is re-priced.
pub const DEFAULT_FEE_BUMP_AFTER_BLOCKS: u64 = 3;

/// How transaction fees are chosen and managed for a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FeeStrategy {
    /// Today's L2 rule: alloy EIP-1559 estimation with the priority fee overridden to
    /// 1 wei. No replacement logic.
    L2MinPriority,
    /// Real EIP-1559 handling for L1: fee-history-based estimation (alloy's estimator,
    /// unoverridden), optional max-fee cap, and stuck-tx replacement with bumped fees.
    Eip1559 {
        /// Refuse to price a tx above this cap (gwei); the tx stays queued instead.
        max_fee_cap_gwei: Option<u64>,
        /// Percent fee bump per replacement (>= 10 per node replacement rules).
        bump_percent: u8,
        /// Blocks without a receipt before replacing with bumped fees.
        bump_after_blocks: u64,
    },
}

impl Default for FeeStrategy {
    fn default() -> Self {
        FeeStrategy::L2MinPriority
    }
}

impl FeeStrategy {
    /// The fixed priority-fee override, if this strategy uses one.
    pub fn fixed_priority_fee(&self) -> Option<u128> {
        match self {
            FeeStrategy::L2MinPriority => Some(L2_MIN_PRIORITY_FEE_WEI),
            FeeStrategy::Eip1559 { .. } => None,
        }
    }

    /// Resolve a strategy from loosely-typed config/env values.
    ///
    /// `name` is `l2-min-priority`/`l2` (default when `None`) or `eip1559`/`l1`;
    /// the remaining parameters apply to the EIP-1559 strategy and fall back to the
    /// documented defaults when absent.
    pub fn resolve(
        name: Option<&str>,
        max_fee_cap_gwei: Option<u64>,
        bump_percent: Option<u8>,
        bump_after_blocks: Option<u64>,
    ) -> anyhow::Result<Self> {
        match name.map(str::to_ascii_lowercase).as_deref() {
            None | Some("l2-min-priority") | Some("l2") => Ok(FeeStrategy::L2MinPriority),
            Some("eip1559") | Some("eip-1559") | Some("l1") => Ok(FeeStrategy::Eip1559 {
                max_fee_cap_gwei,
                bump_percent: bump_percent.unwrap_or(DEFAULT_FEE_BUMP_PERCENT),
                bump_after_blocks: bump_after_blocks.unwrap_or(DEFAULT_FEE_BUMP_AFTER_BLOCKS),
            }),
            Some(other) => anyhow::bail!(
                "unknown fee strategy '{other}' (expected 'l2-min-priority' or 'eip1559')"
            ),
        }
    }
}

/// Per-deployment chain behaviour profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainProfile {
    /// Fee selection/replacement strategy.
    #[serde(default)]
    pub fee_strategy: FeeStrategy,
    /// Historical event query lookback, sized to the chain's reorg depth.
    #[serde(default = "default_lookback")]
    pub lookback_blocks: u64,
    /// Whether to connect over WebSocket (subscriptions) or plain HTTP.
    #[serde(default = "default_ws")]
    pub ws: bool,
}

fn default_lookback() -> u64 {
    DEFAULT_LOOKBACK_BLOCKS
}

fn default_ws() -> bool {
    true
}

impl Default for ChainProfile {
    fn default() -> Self {
        Self {
            fee_strategy: FeeStrategy::default(),
            lookback_blocks: DEFAULT_LOOKBACK_BLOCKS,
            ws: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_matches_pre_profile_constants() {
        // The L2 regression gate: a config with no new keys resolves to exactly the
        // behaviour that was hard-coded before N7.
        let profile = ChainProfile::default();
        assert_eq!(profile.fee_strategy, FeeStrategy::L2MinPriority);
        assert_eq!(profile.fee_strategy.fixed_priority_fee(), Some(1u128));
        assert_eq!(profile.lookback_blocks, 50);
        assert!(profile.ws);
    }

    #[test]
    fn resolve_defaults_to_l2() {
        assert_eq!(
            FeeStrategy::resolve(None, None, None, None).unwrap(),
            FeeStrategy::L2MinPriority
        );
        assert_eq!(
            FeeStrategy::resolve(Some("l2"), None, None, None).unwrap(),
            FeeStrategy::L2MinPriority
        );
    }

    #[test]
    fn resolve_eip1559_with_defaults_and_overrides() {
        assert_eq!(
            FeeStrategy::resolve(Some("eip1559"), None, None, None).unwrap(),
            FeeStrategy::Eip1559 {
                max_fee_cap_gwei: None,
                bump_percent: DEFAULT_FEE_BUMP_PERCENT,
                bump_after_blocks: DEFAULT_FEE_BUMP_AFTER_BLOCKS,
            }
        );
        assert_eq!(
            FeeStrategy::resolve(Some("L1"), Some(100), Some(20), Some(5)).unwrap(),
            FeeStrategy::Eip1559 {
                max_fee_cap_gwei: Some(100),
                bump_percent: 20,
                bump_after_blocks: 5,
            }
        );
        assert!(FeeStrategy::resolve(Some("bogus"), None, None, None).is_err());
    }

    #[test]
    fn serde_round_trip() {
        let profile = ChainProfile {
            fee_strategy: FeeStrategy::Eip1559 {
                max_fee_cap_gwei: Some(200),
                bump_percent: 12,
                bump_after_blocks: 4,
            },
            lookback_blocks: 128,
            ws: false,
        };
        let json = serde_json::to_string(&profile).unwrap();
        let back: ChainProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(profile, back);

        // empty object -> defaults (bit-identical L2 behaviour)
        let empty: ChainProfile = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ChainProfile::default());
    }
}
