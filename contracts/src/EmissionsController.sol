// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

interface IL1StandardBridge {
    function depositERC20To(
        address _l1Token,
        address _l2Token,
        address _to,
        uint256 _amount,
        uint32 _l2Gas,
        bytes calldata _data
    ) external payable;
}

interface IERC20Mintable is IERC20 {
    function mint(address to, uint256 amount) external;
}

/// @title EmissionsController
/// @notice Mints token emissions on L1 on a fixed schedule and bridges them to a fixed L2 recipient.
/// @dev The schedule is immutable after deployment. Anyone can trigger minting once an epoch is ready.
contract EmissionsController is ReentrancyGuard, Ownable {
    using SafeERC20 for IERC20;

    error ZeroAddress();
    error ZeroEpochDuration();
    error EmptySchedule();
    error EpochNotElapsed(uint256 currentTime, uint256 readyAt);
    error NoRemainingEpochs();
    /// @notice Raised when a mint would exceed the global cap.
    /// @param requested The epoch emission amount.
    /// @param remaining Remaining mintable amount under the cap.
    error GlobalCapExceeded(uint256 requested, uint256 remaining);
    error InvalidEpoch(uint256 epochId);

    event EpochMinted(
        uint256 indexed epoch,
        uint256 amount,
        address indexed caller,
        address indexed l2Recipient,
        bytes bridgeData,
        uint256 timestamp
    );
    event BridgeApprovalSet(address indexed token, address indexed bridge, uint256 allowance);
    event L2GasLimitUpdated(uint32 oldLimit, uint32 newLimit);

    IERC20Mintable public immutable token;
    IL1StandardBridge public immutable bridge;
    address public immutable l2Token;
    address public immutable l2Recipient;
    uint256 public immutable startTime;
    uint256 public immutable epochDuration;
    uint256 public immutable globalMintCap;

    uint32 public l2GasLimit;

    uint256 public mintedEpochs;
    uint256 public mintedTotal;

    uint256[] private _emissionsPerEpoch;

    constructor(
        IERC20Mintable _token,
        IL1StandardBridge _bridge,
        address _l2Token,
        address _l2Recipient,
        uint256 _startTime,
        uint256 _epochDuration,
        uint32 _l2GasLimit,
        uint256 _globalMintCap,
        uint256[] memory emissionsSchedule,
        address _owner
    ) Ownable(_owner) {
        if (address(_token) == address(0)) revert ZeroAddress();
        if (address(_bridge) == address(0)) revert ZeroAddress();
        if (_l2Token == address(0)) revert ZeroAddress();
        if (_l2Recipient == address(0)) revert ZeroAddress();
        if (_epochDuration == 0) revert ZeroEpochDuration();
        if (emissionsSchedule.length == 0) revert EmptySchedule();

        token = _token;
        bridge = _bridge;
        l2Token = _l2Token;
        l2Recipient = _l2Recipient;
        startTime = _startTime;
        epochDuration = _epochDuration;
        l2GasLimit = _l2GasLimit;
        globalMintCap = _globalMintCap;
        _emissionsPerEpoch = emissionsSchedule;

        _setBridgeApproval();
    }

    function setL2GasLimit(uint32 newLimit) external onlyOwner {
        emit L2GasLimitUpdated(l2GasLimit, newLimit);
        l2GasLimit = newLimit;
    }

    function epochs() external view returns (uint256) {
        return _emissionsPerEpoch.length;
    }

    function emissionForEpoch(uint256 epochId) external view returns (uint256) {
        if (epochId == 0 || epochId > _emissionsPerEpoch.length) revert InvalidEpoch(epochId);
        return _emissionsPerEpoch[epochId - 1];
    }

    function nextEpochReadyAt() public view returns (uint256) {
        if (mintedEpochs >= _emissionsPerEpoch.length) return type(uint256).max;
        return startTime + epochDuration * mintedEpochs;
    }

    function mintAndBridgeNextEpoch() external payable nonReentrant returns (uint256 epochId, uint256 amount) {
        return _mintAndBridgeNextEpoch("", msg.value);
    }

    function mintAndBridgeNextEpoch(bytes calldata bridgeData)
        external
        payable
        nonReentrant
        returns (uint256 epochId, uint256 amount)
    {
        return _mintAndBridgeNextEpoch(bridgeData, msg.value);
    }

    function _mintAndBridgeNextEpoch(bytes memory bridgeData, uint256 value)
        internal
        returns (uint256 epochId, uint256 amount)
    {
        uint256 mintedSoFar = mintedEpochs;
        if (mintedSoFar >= _emissionsPerEpoch.length) revert NoRemainingEpochs();

        epochId = mintedSoFar + 1;
        uint256 readyAt = nextEpochReadyAt();
        if (block.timestamp < readyAt) revert EpochNotElapsed(block.timestamp, readyAt);

        amount = _emissionsPerEpoch[epochId - 1];

        if (globalMintCap != 0) {
            if (mintedTotal >= globalMintCap) revert GlobalCapExceeded(amount, 0);
            uint256 remaining = globalMintCap - mintedTotal;
            if (amount > remaining) revert GlobalCapExceeded(amount, remaining);
        }

        mintedEpochs = mintedSoFar + 1;
        mintedTotal += amount;

        if (amount != 0) {
            token.mint(address(this), amount);
            bridge.depositERC20To{value: value}(
                address(token),
                l2Token,
                l2Recipient,
                amount,
                l2GasLimit,
                bridgeData
            );
        }

        emit EpochMinted(epochId, amount, msg.sender, l2Recipient, bridgeData, block.timestamp);
    }

    function ensureBridgeApproval() external {
        _setBridgeApproval();
    }

    function _setBridgeApproval() internal {
        IERC20 erc20 = IERC20(address(token));
        uint256 currentAllowance = erc20.allowance(address(this), address(bridge));
        if (currentAllowance != type(uint256).max) {
            SafeERC20.forceApprove(erc20, address(bridge), type(uint256).max);
            emit BridgeApprovalSet(address(token), address(bridge), type(uint256).max);
        }
    }
}
