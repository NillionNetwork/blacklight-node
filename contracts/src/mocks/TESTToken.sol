// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/// @notice Simple mintable ERC20 used for deploying test staking contracts.
contract TESTToken is ERC20, Ownable {
    constructor(address owner_) ERC20("TEST Token", "TEST") Ownable(owner_) {}

    function mint(address to, uint256 amount) external onlyOwner {
        _mint(to, amount);
    }

    function decimals() public pure override returns (uint8) {
        return 6;
    }
}

