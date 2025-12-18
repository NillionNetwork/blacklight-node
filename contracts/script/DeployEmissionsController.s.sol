// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "forge-std/console2.sol";

import "../src/EmissionsController.sol";

/// @notice Deploys EmissionsController with a provided emissions schedule and bridge settings.
/// @dev Configure via env vars when running `forge script`:
///      - PRIVATE_KEY (required)
///      - OWNER (defaults to deployer)
///      - TOKEN (IERC20Mintable, required)
///      - L1_BRIDGE (IL1StandardBridge, required)
///      - L2_TOKEN (required)
///      - L2_RECIPIENT (required)
///      - EPOCH_START (defaults to current block.timestamp)
///      - EPOCH_DURATION (seconds, default 7 days)
///      - L2_GAS_LIMIT (default 200_000)
///      - GLOBAL_MINT_CAP (uint, default 0 for unlimited)
///      - EMISSIONS_SCHEDULE (uint[], comma delimited, default single zero entry)
contract DeployEmissionsController is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);

        address owner = vm.envOr("OWNER", deployer);
        address token = vm.envAddress("TOKEN");
        address bridge = vm.envAddress("L1_BRIDGE");
        address l2Token = vm.envAddress("L2_TOKEN");
        address l2Recipient = vm.envAddress("L2_RECIPIENT");

        uint256 startTime = vm.envOr("EPOCH_START", block.timestamp);
        uint256 epochDuration = vm.envOr("EPOCH_DURATION", uint256(7 days));
        uint32 l2GasLimit = uint32(vm.envOr("L2_GAS_LIMIT", uint256(200_000)));
        uint256 globalCap = vm.envOr("GLOBAL_MINT_CAP", uint256(0));

        uint256[] memory schedule = vm.envOr("EMISSIONS_SCHEDULE", ",", _defaultSchedule());
        require(schedule.length > 0, "empty schedule");

        vm.startBroadcast(deployerKey);

        EmissionsController controller = new EmissionsController(
            IERC20Mintable(token),
            IL1StandardBridge(bridge),
            l2Token,
            l2Recipient,
            startTime,
            epochDuration,
            l2GasLimit,
            globalCap,
            schedule,
            owner
        );

        vm.stopBroadcast();

        console2.log("EmissionsController:", address(controller));
        console2.log("Owner:", owner);
    }

    function _defaultSchedule() internal pure returns (uint256[] memory arr) {
        arr = new uint256[](1);
        arr[0] = 0;
    }
}
