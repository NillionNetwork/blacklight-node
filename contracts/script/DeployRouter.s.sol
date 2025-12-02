// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import "forge-std/Script.sol";
import "../src/core/NilAVRouter.sol";

contract DeployRouter is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerPrivateKey);

        NilAVRouter router = new NilAVRouter();

        vm.stopBroadcast();

        console.log("NilAVRouter deployed to:", address(router));
    }
}
