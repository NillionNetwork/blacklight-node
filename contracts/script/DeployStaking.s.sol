// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "forge-std/console2.sol";

import "../src/mocks/TESTToken.sol";
import "../src/StakingOperators.sol";

/// @notice Deploys TEST token + staking contract.
///         Uses PRIVATE_KEY as deployer/admin/minter.
contract DeployStaking is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        console2.log("Deploying from:", deployer);

        vm.startBroadcast(deployerPrivateKey);

        TESTToken test = new TESTToken(deployer);
        console2.log("TEST token deployed at:", address(test));

        StakingOperators staking = new StakingOperators(
            test,
            deployer,
            7 days
        );
        console2.log("StakingOperators deployed at:", address(staking));

        uint256 initialMint = 1_000_000 ether;
        test.mint(deployer, initialMint);
        console2.log("Minted TEST to deployer:", initialMint);

        vm.stopBroadcast();
    }
}