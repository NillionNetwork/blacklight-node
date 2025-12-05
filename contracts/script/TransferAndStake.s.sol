// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import "src/core/TESTToken.sol";
import "src/core/StakingOperators.sol";

/// @title TransferAndStake
/// @notice Script to transfer TEST tokens from deployer, stake them for an operator, and transfer ETH for gas fees
/// @dev Usage: forge script script/TransferAndStake.s.sol:TransferAndStake --rpc-url $RPC_URL --broadcast
///      Set PRIVATE_KEY, OPERATOR_ADDRESS, and TOKEN_AMOUNT as environment variables
///      NOTE: Deployer must already have sufficient TEST tokens in their wallet
contract TransferAndStake is Script {
    function run() external {
        // Get deployer private key from environment
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        // Get operator address, token amount, and ETH amount from environment
        address operatorAddress = vm.envAddress("OPERATOR_ADDRESS");
        uint256 tokenAmount = vm.envUint("TOKEN_AMOUNT") * (10 ** 18);
        uint256 ethAmount = vm.envUint("ETH_AMOUNT");

        // Get deployed contract addresses from environment
        address testTokenAddress = vm.envAddress("TEST_TOKEN_ADDRESS");
        address stakingOperatorsAddress = vm.envAddress("STAKING_OPERATORS_ADDRESS");

        console.log("=== Transfer and Stake Script ===");
        console.log("Deployer:", deployer);
        console.log("Operator:", operatorAddress);
        console.log("Token Amount:", tokenAmount);
        console.log("ETH Amount:", ethAmount, "ether");
        console.log("TEST Token:", testTokenAddress);
        console.log("StakingOperators:", stakingOperatorsAddress);

        TESTToken token = TESTToken(testTokenAddress);
        StakingOperators staking = StakingOperators(stakingOperatorsAddress);

        // Check deployer has sufficient balance
        uint256 deployerBalance = token.balanceOf(deployer);
        console.log("\nDeployer TEST balance:", deployerBalance);
        require(deployerBalance >= tokenAmount, "Insufficient TEST token balance");

        vm.startBroadcast(deployerPrivateKey);

        // 1. Approve StakingOperators to spend tokens
        console.log("\n1. Approving StakingOperators to spend tokens...");
        token.approve(address(staking), tokenAmount);
        console.log("   Approved successfully");

        // 2. Stake tokens for the operator address
        console.log("\n2. Staking", tokenAmount, "tokens for operator", operatorAddress);
        staking.stakeTo(operatorAddress, tokenAmount);
        console.log("   Staked successfully");

        // 3. Transfer ETH to operator for gas fees
        uint256 ethAmountWei = ethAmount * 1 ether;
        console.log("\n3. Transferring", ethAmount, "ETH to operator for gas fees...");
        (bool success,) = payable(operatorAddress).call{value: ethAmountWei}("");
        require(success, "ETH transfer failed");
        console.log("   Transferred successfully");

        vm.stopBroadcast();

        console.log("\n=== Summary ===");
        console.log("Tokens staked for:", operatorAddress);
        console.log("ETH transferred:", ethAmount, "ETH");
        console.log("Operator stake:", staking.stakeOf(operatorAddress));
        console.log("Operator staker:", staking.operatorStaker(operatorAddress));
        console.log("Deployer remaining balance:", token.balanceOf(deployer));
    }
}

