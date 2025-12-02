// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import "forge-std/Script.sol";
import "../src/core/NilAVRouter.sol";

contract DeployRouter is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");

        // Get staking contract address from environment
        address stakingOperators = vm.envOr("STAKING_ADDRESS", address(0));
        require(stakingOperators != address(0), "STAKING_ADDRESS environment variable not set");

        vm.startBroadcast(deployerPrivateKey);

        NilAVRouter router = new NilAVRouter(stakingOperators);

        vm.stopBroadcast();

        console.log("NilAVRouter deployed to:", address(router));
        console.log("Using StakingOperators at:", stakingOperators);
    }
}
