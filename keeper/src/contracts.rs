use alloy::sol;

sol! {
    #[sol(rpc)]
    contract JailingPolicy {
        function recordRound(bytes32 heartbeatKey, uint8 round) external;
        function enforceJailFromMembers(bytes32 heartbeatKey, uint8 round, address[] calldata sortedMembers) external;
    }

    #[sol(rpc)]
    contract RewardPolicy {
        function sync() external;
        function spendableBudget() external view returns (uint256);
        function streamRemaining() external view returns (uint256);
        function rewardToken() external view returns (address);
        function accountedBalance() external view returns (uint256);
        function lastUpdate() external view returns (uint64);
        function streamRatePerSecondWad() external view returns (uint256);
        function streamEnd() external view returns (uint64);
    }

    #[sol(rpc)]
    contract Erc20 {
        function balanceOf(address account) external view returns (uint256);
        function decimals() external view returns (uint8);
    }

    #[sol(rpc)]
    contract EmissionsController {
        function mintedEpochs() external view returns (uint256);
        function epochs() external view returns (uint256);
        function nextEpochReadyAt() external view returns (uint256);
        function mintAndBridgeNextEpoch() external payable returns (uint256 epochId, uint256 amount);
    }
}
