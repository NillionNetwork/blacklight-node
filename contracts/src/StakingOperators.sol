// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";

import "./Interfaces.sol";

/// @title StakingOperators
/// @notice Hardened ERC20 staking + operator registry for AV operators.
contract StakingOperators is IStakingOperators, AccessControl, ReentrancyGuard {
    using SafeERC20 for IERC20;

    // Errors
    error ZeroAddress();
    error ZeroAmount();
    error PendingUnbonding();
    error DifferentStaker();
    error NotStaker();
    error UnbondingExists();
    error InsufficientStake();
    error InsufficientStakeForActivation();
    error OperatorJailed();
    error NoUnbonding();
    error NotReady();
    error NoStake();
    error NotActive();

    bytes32 public constant SLASHER_ROLE = keccak256("SLASHER_ROLE");

    IERC20 private immutable _stakingToken;
    uint256 public override unstakeDelay;

    /// @notice Optional protocol config registry used to fetch the minimum operator stake.
    IProtocolConfig public protocolConfig;

    mapping(address => uint256) private _operatorStake;
    uint256 private _totalStaked;

    mapping(address => address) public override operatorStaker;

    struct Unbonding {
        address staker;
        uint256 amount;
        uint64  releaseTime;
    }
    mapping(address => Unbonding) public unbondings;

    mapping(address => uint64) private _jailedUntil;

    struct OperatorData {
        bool active;
        string metadataURI;
        bool exists;
        uint256 index;
    }

    mapping(address => OperatorData) private _operators;
    address[] private _allOperators;

    event StakedTo(address indexed staker, address indexed operator, uint256 amount);
    event UnstakeRequested(address indexed staker, address indexed operator, uint256 amount, uint64 releaseTime);
    event UnstakedWithdrawn(address indexed staker, address indexed operator, uint256 amount);
    event Slashed(address indexed operator, uint256 amount);
    event Jailed(address indexed operator, uint64 until);
    event OperatorRegistered(address indexed operator, string metadataURI);
    event OperatorDeactivated(address indexed operator);
    event UnstakeDelayUpdated(uint256 oldDelay, uint256 newDelay);
    event ProtocolConfigUpdated(address oldConfig, address newConfig);

    constructor(IERC20 token_, address admin, uint256 initialUnstakeDelay) {
        if (address(token_) == address(0)) revert ZeroAddress();
        if (admin == address(0)) revert ZeroAddress();

        _stakingToken = token_;
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        unstakeDelay = initialUnstakeDelay;
    }

    /// @notice Sets the protocol config contract used to read the minimum operator stake.
    /// @dev Callable by the admin; allows late wiring and future governance-controlled updates.
    function setProtocolConfig(IProtocolConfig newConfig) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (address(newConfig) == address(0)) revert ZeroAddress();
        address old = address(protocolConfig);
        protocolConfig = newConfig;
        emit ProtocolConfigUpdated(old, address(newConfig));
    }

    function _hasMinStake(address operator) internal view returns (bool) {
        uint256 bal = _operatorStake[operator];
        // If no config is set, fall back to simple non-zero check to preserve old behavior.
        IProtocolConfig cfg = protocolConfig;
        if (address(cfg) == address(0)) {
            return bal > 0;
        }
        uint256 minStake = cfg.minOperatorStake();
        if (minStake == 0) {
            return bal > 0;
        }
        return bal >= minStake;
    }

    // Views

    function stakingToken() external view override returns (address) {
        return address(_stakingToken);
    }

    function stakeOf(address operator) external view override returns (uint256) {
        return _operatorStake[operator];
    }

    function totalStaked() external view override returns (uint256) {
        return _totalStaked;
    }

    function isJailed(address operator) external view override returns (bool) {
        return block.timestamp < _jailedUntil[operator];
    }

    // Admin

    function setUnstakeDelay(uint256 newDelay) external onlyRole(DEFAULT_ADMIN_ROLE) {
        emit UnstakeDelayUpdated(unstakeDelay, newDelay);
        unstakeDelay = newDelay;
    }

    // Staking

    function stakeTo(address operator, uint256 amount) external override nonReentrant {
        if (operator == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();

        Unbonding storage u = unbondings[operator];
        address currentStaker = operatorStaker[operator];

        if (currentStaker == address(0)) {
            if (u.amount != 0) revert PendingUnbonding();
            operatorStaker[operator] = msg.sender;
        } else {
            if (currentStaker != msg.sender) revert DifferentStaker();
        }

        _stakingToken.safeTransferFrom(msg.sender, address(this), amount);

        _operatorStake[operator] += amount;
        _totalStaked += amount;

        emit StakedTo(msg.sender, operator, amount);
    }

    function requestUnstake(address operator, uint256 amount)
        external
        override
        nonReentrant
    {
        if (operator == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();

        if (operatorStaker[operator] != msg.sender) revert NotStaker();

        Unbonding storage u = unbondings[operator];
        if (u.amount != 0) revert UnbondingExists();

        uint256 bal = _operatorStake[operator];
        if (bal < amount) revert InsufficientStake();
        if (block.timestamp < _jailedUntil[operator]) revert OperatorJailed();

        _operatorStake[operator] = bal - amount;
        _totalStaked -= amount;

        uint64 releaseTime = uint64(block.timestamp + unstakeDelay);
        u.staker = msg.sender;
        u.amount = amount;
        u.releaseTime = releaseTime;

        if (_operatorStake[operator] == 0) {
            operatorStaker[operator] = address(0);
        }

        emit UnstakeRequested(msg.sender, operator, amount, releaseTime);
    }

    function withdrawUnstaked(address operator) external override nonReentrant {
        if (operator == address(0)) revert ZeroAddress();

        Unbonding storage u = unbondings[operator];
        uint256 amt = u.amount;
        if (amt == 0) revert NoUnbonding();
        if (msg.sender != u.staker) revert NotStaker();
        if (block.timestamp < u.releaseTime) revert NotReady();

        address staker = u.staker;
        delete unbondings[operator];

        _stakingToken.safeTransfer(staker, amt);

        emit UnstakedWithdrawn(staker, operator, amt);
    }

    // Operator registry

    function registerOperator(string calldata metadataURI) external override {
        if (!_hasMinStake(msg.sender)) revert InsufficientStakeForActivation();

        OperatorData storage data = _operators[msg.sender];
        if (!data.exists) {
            data.exists = true;
            data.index = _allOperators.length;
            _allOperators.push(msg.sender);
        }

        data.active = true;
        data.metadataURI = metadataURI;

        emit OperatorRegistered(msg.sender, metadataURI);
    }

    function deactivateOperator() external override {
        OperatorData storage data = _operators[msg.sender];
        if (!(data.exists && data.active)) revert NotActive();
        data.active = false;
        emit OperatorDeactivated(msg.sender);
    }

    function getOperatorInfo(address operator)
        external
        view
        override
        returns (OperatorInfo memory)
    {
        OperatorData storage data = _operators[operator];
        return OperatorInfo({active: data.active, metadataURI: data.metadataURI});
    }

    function isActiveOperator(address operator) public view override returns (bool) {
        OperatorData storage data = _operators[operator];
        if (!data.active) return false;
        if (!_hasMinStake(operator)) return false;
        return block.timestamp >= _jailedUntil[operator];
    }

    function getActiveOperators() external view override returns (address[] memory) {
        uint256 len = _allOperators.length;
        uint256 count;

        for (uint256 i = 0; i < len; ) {
            if (isActiveOperator(_allOperators[i])) {
                unchecked { ++count; }
            }
            unchecked { ++i; }
        }

        address[] memory active = new address[](count);
        uint256 idx;
        for (uint256 i = 0; i < len; ) {
            if (isActiveOperator(_allOperators[i])) {
                active[idx] = _allOperators[i];
                unchecked { ++idx; }
            }
            unchecked { ++i; }
        }
        return active;
    }

    // Slashing / jailing

    function slash(address operator, uint256 amount)
        external
        override
        onlyRole(SLASHER_ROLE)
        nonReentrant
    {
        uint256 bal = _operatorStake[operator];
        uint256 toSlash = amount > bal ? bal : amount;
        if (toSlash == 0) return;

        _operatorStake[operator] = bal - toSlash;
        _totalStaked -= toSlash;

        emit Slashed(operator, toSlash);
    }

    function jail(address operator, uint64 untilTimestamp)
        external
        override
        onlyRole(SLASHER_ROLE)
    {
        if (untilTimestamp > _jailedUntil[operator]) {
            _jailedUntil[operator] = untilTimestamp;
            emit Jailed(operator, untilTimestamp);
        }
    }
}