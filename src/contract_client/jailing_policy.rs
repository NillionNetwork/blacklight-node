use alloy::sol;

sol! {
    #[sol(rpc)]
    contract JailingPolicy {
        function recordRound(bytes32 heartbeatKey, uint8 round) external;
        function enforceJailFromMembers(bytes32 heartbeatKey, uint8 round, address[] calldata sortedMembers) external;
    }
}
