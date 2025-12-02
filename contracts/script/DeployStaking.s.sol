// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import "forge-std/Script.sol";
import "src/core/StakingOperators.sol";
import "src/core/TESTToken.sol";

contract DeployStaking is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);
        uint256 unstakeDelay = 7 days;

        vm.startBroadcast(deployerPrivateKey);

        // Deploy TEST token
        TESTToken token = new TESTToken(deployer);
        console.log("TESTToken deployed to:", address(token));

        // Deploy StakingOperators with 7 day unstake delay
        StakingOperators staking = new StakingOperators(IERC20(address(token)), deployer, unstakeDelay);

        vm.stopBroadcast();

        console.log("StakingOperators deployed to:", address(staking));
        console.log("Admin:", deployer);
        console.logUint(unstakeDelay);
    }
}
