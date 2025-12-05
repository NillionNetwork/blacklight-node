// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import "src/core/TESTToken.sol";
import "src/core/StakingOperators.sol";

/// @title FundOperator
/// @notice Script to mint TEST tokens, stake them for an operator, and transfer ETH for gas fees
/// @dev Usage: forge script script/FundOperator.s.sol:FundOperator --rpc-url $RPC_URL --broadcast
///      Set PRIVATE_KEY, OPERATOR_ADDRESS, and TOKEN_AMOUNT as environment variables
contract FundOperator is Script {
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

        console.log("=== Fund Operator Script ===");
        console.log("Deployer:", deployer);
        console.log("Operator:", operatorAddress);
        console.log("Token Amount:", tokenAmount);
        console.log("ETH Amount:", ethAmount, "ether");
        console.log("TEST Token:", testTokenAddress);
        console.log("StakingOperators:", stakingOperatorsAddress);

        TESTToken token = TESTToken(testTokenAddress);
        StakingOperators staking = StakingOperators(stakingOperatorsAddress);

        vm.startBroadcast(deployerPrivateKey);

        // 1. Mint TEST tokens to deployer (who is the owner)
        console.log("\n1. Minting", tokenAmount, "TEST tokens to deployer...");
        token.mint(deployer, tokenAmount);
        console.log("   Minted successfully");

        // 2. Approve StakingOperators to spend tokens
        console.log("\n2. Approving StakingOperators to spend tokens...");
        token.approve(address(staking), tokenAmount);
        console.log("   Approved successfully");

        // 3. Stake tokens for the operator address
        console.log("\n3. Staking", tokenAmount, "tokens for operator", operatorAddress);
        staking.stakeTo(operatorAddress, tokenAmount);
        console.log("   Staked successfully");

        // 4. Transfer ETH to operator for gas fees
        uint256 ethAmountWei = ethAmount * 1 ether;
        console.log("\n4. Transferring", ethAmount, "ETH to operator for gas fees...");
        (bool success,) = payable(operatorAddress).call{value: ethAmountWei}("");
        require(success, "ETH transfer failed");
        console.log("   Transferred successfully");

        vm.stopBroadcast();

        console.log("\n=== Summary ===");
        console.log("Tokens minted:", tokenAmount);
        console.log("Tokens staked for:", operatorAddress);
        console.log("ETH transferred:", ethAmount, "ETH");
        console.log("Operator stake:", staking.stakeOf(operatorAddress));
        console.log("Operator staker:", staking.operatorStaker(operatorAddress));
    }
}

