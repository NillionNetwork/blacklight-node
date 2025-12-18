// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "forge-std/console2.sol";

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "../src/mocks/TESTToken.sol";
import "../src/ProtocolConfig.sol";
import "../src/StakingOperators.sol";
import "../src/WeightedCommitteeSelector.sol";
import "../src/WorkloadManager.sol";
import "../src/RewardPolicy.sol";
import "../src/JailingPolicy.sol";

/// @notice Deploys and wires the full RC contract suite (staking, selector, config, manager, slashing, rewards).
/// @dev Configure via env vars when running `forge script`:
///      - PRIVATE_KEY (required)
///      - GOVERNANCE (defaults to deployer)
///      - ADMIN (defaults to deployer) — receives StakingOperators admin role
///      - USE_MOCK_TOKENS (bool, default false) — deploy TEST tokens when token addrs are unset
///      - STAKE_TOKEN / REWARD_TOKEN (addresses, optional when using mocks)
///      - MINT_RECIPIENT (defaults to GOVERNANCE)
///      - MOCK_STAKE_MINT / MOCK_REWARD_MINT (uints, only used when deploying TEST tokens)
///      - ProtocolConfig params (uint): BASE_COMMITTEE_SIZE, COMMITTEE_GROWTH_BPS, MAX_COMMITTEE_SIZE,
///        MAX_ESCALATIONS, QUORUM_BPS, VERIFICATION_BPS, RESPONSE_WINDOW_SEC, JAIL_DURATION_SEC,
///        MAX_VOTE_BATCH, MIN_OPERATOR_STAKE
///      - Staking params: UNSTAKE_DELAY_SEC
///      - Selector params: MIN_COMMITTEE_VP
///      - Reward params: REWARD_EPOCH_DURATION, REWARD_MAX_PAYOUT_PER_FINALIZE
contract DeployRCSystem is Script {
    struct Params {
        uint256 deployerKey;
        address deployer;
        address governance;
        address admin;

        bool useMockTokens;
        address stakeToken;
        address rewardToken;
        address mintRecipient;
        uint256 mockStakeMint;
        uint256 mockRewardMint;

        uint32 baseCommitteeSize;
        uint32 committeeGrowthBps;
        uint32 maxCommitteeSize;
        uint8 maxEscalations;
        uint16 quorumBps;
        uint16 verificationBps;
        uint256 responseWindow;
        uint256 jailDuration;
        uint256 maxVoteBatchSize;
        uint256 minOperatorStake;

        uint256 unstakeDelay;
        uint256 minCommitteeVP;

        uint256 rewardEpochDuration;
        uint256 rewardMaxPayoutPerFinalize;
    }

    function run() external {
        Params memory p = _readParams();

        vm.startBroadcast(p.deployerKey);

        address stakeToken = p.stakeToken;
        address rewardToken = p.rewardToken;

        if (p.useMockTokens && stakeToken == address(0)) {
            TESTToken mock = new TESTToken(p.governance);
            stakeToken = address(mock);
            if (p.mockStakeMint != 0) mock.mint(p.mintRecipient, p.mockStakeMint);
            console2.log("Deployed TEST stake token:", stakeToken);
        }
        if (p.useMockTokens && rewardToken == address(0)) {
            TESTToken mock = new TESTToken(p.governance);
            rewardToken = address(mock);
            if (p.mockRewardMint != 0) mock.mint(p.mintRecipient, p.mockRewardMint);
            console2.log("Deployed TEST reward token:", rewardToken);
        }

        require(stakeToken != address(0), "stake token required");
        require(rewardToken != address(0), "reward token required");

        StakingOperators stakingOps = new StakingOperators(
            IERC20(stakeToken),
            p.admin,
            p.unstakeDelay
        );
        console2.log("StakingOperators:", address(stakingOps));

        WeightedCommitteeSelector selector = new WeightedCommitteeSelector(
            stakingOps,
            p.admin,
            p.minCommitteeVP,
            p.maxCommitteeSize
        );
        console2.log("WeightedCommitteeSelector:", address(selector));

        // Temporary placeholders for slashing/reward modules are updated after deploy.
        address placeholder = p.governance;
        ProtocolConfig config = new ProtocolConfig(
            p.governance,
            address(stakingOps),
            address(selector),
            placeholder,
            placeholder,
            p.baseCommitteeSize,
            p.committeeGrowthBps,
            p.maxCommitteeSize,
            p.maxEscalations,
            p.quorumBps,
            p.verificationBps,
            p.responseWindow,
            p.jailDuration,
            p.maxVoteBatchSize,
            p.minOperatorStake
        );
        console2.log("ProtocolConfig:", address(config));

        WorkloadManager manager = new WorkloadManager(config, p.governance);
        console2.log("WorkloadManager:", address(manager));

        RewardPolicy rewardPolicy = new RewardPolicy(
            IERC20(rewardToken),
            address(manager),
            p.governance,
            p.rewardEpochDuration,
            p.rewardMaxPayoutPerFinalize
        );
        console2.log("RewardPolicy:", address(rewardPolicy));

        JailingPolicy jailingPolicy = new JailingPolicy(address(manager));
        console2.log("JailingPolicy:", address(jailingPolicy));

        config.setModules(address(stakingOps), address(selector), address(jailingPolicy), address(rewardPolicy));

        stakingOps.setProtocolConfig(config);
        stakingOps.setWorkloadManager(address(manager));
        stakingOps.setSnapshotter(address(manager));
        stakingOps.grantRole(stakingOps.SLASHER_ROLE(), address(jailingPolicy));
        if (p.admin != p.governance) {
            stakingOps.grantRole(stakingOps.DEFAULT_ADMIN_ROLE(), p.governance);
        }

        vm.stopBroadcast();

        console2.log("--- Deployment complete ---");
        console2.log("Stake token:", stakeToken);
        console2.log("Reward token:", rewardToken);
        console2.log("Config owner (governance):", p.governance);
        console2.log("Staking admin:", p.admin);
    }

    function _readParams() internal view returns (Params memory p) {
        p.deployerKey = vm.envUint("PRIVATE_KEY");
        p.deployer = vm.addr(p.deployerKey);

        p.governance = vm.envOr("GOVERNANCE", p.deployer);
        p.admin = vm.envOr("ADMIN", p.deployer);

        p.useMockTokens = vm.envOr("USE_MOCK_TOKENS", false);
        p.stakeToken = vm.envOr("STAKE_TOKEN", address(0));
        p.rewardToken = vm.envOr("REWARD_TOKEN", address(0));
        p.mintRecipient = vm.envOr("MINT_RECIPIENT", p.governance);
        p.mockStakeMint = vm.envOr("MOCK_STAKE_MINT", uint256(0));
        p.mockRewardMint = vm.envOr("MOCK_REWARD_MINT", uint256(0));

        p.baseCommitteeSize = uint32(vm.envOr("BASE_COMMITTEE_SIZE", uint256(10)));
        p.committeeGrowthBps = uint32(vm.envOr("COMMITTEE_GROWTH_BPS", uint256(0)));
        p.maxCommitteeSize = uint32(vm.envOr("MAX_COMMITTEE_SIZE", uint256(200)));
        p.maxEscalations = uint8(vm.envOr("MAX_ESCALATIONS", uint256(0)));
        p.quorumBps = uint16(vm.envOr("QUORUM_BPS", uint256(5_000)));
        p.verificationBps = uint16(vm.envOr("VERIFICATION_BPS", uint256(5_000)));
        p.responseWindow = vm.envOr("RESPONSE_WINDOW_SEC", uint256(1 days));
        p.jailDuration = vm.envOr("JAIL_DURATION_SEC", uint256(7 days));
        p.maxVoteBatchSize = vm.envOr("MAX_VOTE_BATCH", uint256(100));
        p.minOperatorStake = vm.envOr("MIN_OPERATOR_STAKE", uint256(1e18));

        p.unstakeDelay = vm.envOr("UNSTAKE_DELAY_SEC", uint256(7 days));
        p.minCommitteeVP = vm.envOr("MIN_COMMITTEE_VP", uint256(0));

        p.rewardEpochDuration = vm.envOr("REWARD_EPOCH_DURATION", uint256(1 days));
        p.rewardMaxPayoutPerFinalize = vm.envOr("REWARD_MAX_PAYOUT_PER_FINALIZE", uint256(0));
    }
}
