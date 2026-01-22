use alloy::sol;

sol! {
    #[sol(rpc)]
    contract EmissionsController {
        function mintedEpochs() external view returns (uint256);
        function epochs() external view returns (uint256);
        function nextEpochReadyAt() external view returns (uint256);
        function mintAndBridgeNextEpoch() external payable returns (uint256 epochId, uint256 amount);
    }
}
