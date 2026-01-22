use alloy::sol;

sol! {
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
}

sol! {
    #[sol(rpc)]
    contract ERC20 {
        function balanceOf(address account) external view returns (uint256);
        function decimals() external view returns (uint8);
    }
}
