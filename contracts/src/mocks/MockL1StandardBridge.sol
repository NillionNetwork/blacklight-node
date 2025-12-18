// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

contract MockL1StandardBridge {
    event Deposit(address indexed l1Token, address indexed to, uint256 amount);

    function depositERC20To(
        address l1Token,
        address, /* l2Token */
        address to,
        uint256 amount,
        uint32, /* l2Gas */
        bytes calldata /* data */
    ) external payable {
        IERC20(l1Token).transferFrom(msg.sender, address(this), amount);
        emit Deposit(l1Token, to, amount);
    }
}
