// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import "src/core/TESTToken.sol";
import "src/core/StakingOperators.sol";
import "src/core/NilAVRouter.sol";

contract DeployAll is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        // Allow overriding the unstake delay, defaulting to 7 days
        uint256 unstakeDelay = vm.envOr("UNSTAKE_DELAY", uint256(7 days));

        vm.startBroadcast(deployerPrivateKey);

        TESTToken token = new TESTToken(deployer);
        StakingOperators staking =
            new StakingOperators(IERC20(address(token)), deployer, unstakeDelay);
        NilAVRouter router = new NilAVRouter(address(staking));

        vm.stopBroadcast();

        console.log("TESTToken deployed to:", address(token));
        console.log("StakingOperators deployed to:", address(staking));
        console.log("NilAVRouter deployed to:", address(router));
        console.log("Admin (token recipient):", deployer);
        console.log("Unstake delay (seconds):", unstakeDelay);
    }
}

