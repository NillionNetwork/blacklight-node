// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";

import "src/interfaces/Interfaces.sol";

/// @title StakingOperators
/// @notice Hardened ERC20 staking + operator registry for AV operators.
/// @dev Implements staking, unbonding, operator registration, and slashing mechanics.
///      Inherits from AccessControl for role management and ReentrancyGuard for security.
contract StakingOperators is IStakingOperators, AccessControl, ReentrancyGuard {
    using SafeERC20 for IERC20;

    // Errors
    /// @notice Thrown when an address is the zero address.
    error ZeroAddress();
    /// @notice Thrown when an amount is zero.
    error ZeroAmount();
    /// @notice Thrown when an operation is attempted while an unbonding is pending.
    error PendingUnbonding();
    /// @notice Thrown when the caller is not the current staker for the operator.
    error DifferentStaker();
    /// @notice Thrown when the caller is not the staker.
    error NotStaker();
    /// @notice Thrown when an unbonding request already exists.
    error UnbondingExists();
    /// @notice Thrown when the stake is insufficient for the requested operation.
    error InsufficientStake();
    /// @notice Thrown when the operator is currently jailed.
    error OperatorJailed();
    /// @notice Thrown when there is no unbonding request to withdraw.
    error NoUnbonding();
    /// @notice Thrown when the unbonding period has not yet passed.
    error NotReady();
    /// @notice Thrown when the operator has no stake.
    error NoStake();
    /// @notice Thrown when the operator is not active.
    error NotActive();

    /// @notice Role identifier for the slasher role, capable of slashing and jailing operators.
    bytes32 public constant SLASHER_ROLE = keccak256("SLASHER_ROLE");

    IERC20 private immutable _stakingToken;

    /// @notice The delay in seconds before unstaked funds can be withdrawn.
    uint256 public override unstakeDelay;

    mapping(address => uint256) private _operatorStake;
    uint256 private _totalStaked;

    /// @notice Mapping from operator address to their current staker's address.
    mapping(address => address) public override operatorStaker;

    struct Unbonding {
        address staker;
        uint256 amount;
        uint64 releaseTime;
    }

    /// @notice Mapping from operator address to their pending unbonding request.
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

    /// @notice Emitted when a staker stakes tokens to an operator.
    /// @param staker The address of the staker.
    /// @param operator The address of the operator.
    /// @param amount The amount of tokens staked.
    event StakedTo(address indexed staker, address indexed operator, uint256 amount);

    /// @notice Emitted when an unstake request is made.
    /// @param staker The address of the staker.
    /// @param operator The address of the operator.
    /// @param amount The amount of tokens requested to unstake.
    /// @param releaseTime The timestamp when the funds will be available for withdrawal.
    event UnstakeRequested(address indexed staker, address indexed operator, uint256 amount, uint64 releaseTime);

    /// @notice Emitted when unstaked tokens are withdrawn.
    /// @param staker The address of the staker.
    /// @param operator The address of the operator.
    /// @param amount The amount of tokens withdrawn.
    event UnstakedWithdrawn(address indexed staker, address indexed operator, uint256 amount);

    /// @notice Emitted when an operator is slashed.
    /// @param operator The address of the slashed operator.
    /// @param amount The amount of tokens slashed.
    event Slashed(address indexed operator, uint256 amount);

    /// @notice Emitted when an operator is jailed.
    /// @param operator The address of the jailed operator.
    /// @param until The timestamp until which the operator is jailed.
    event Jailed(address indexed operator, uint64 until);

    /// @notice Emitted when a new operator is registered or updates their metadata.
    /// @param operator The address of the operator.
    /// @param metadataURI The URI containing operator metadata.
    event OperatorRegistered(address indexed operator, string metadataURI);

    /// @notice Emitted when an operator is deactivated.
    /// @param operator The address of the deactivated operator.
    event OperatorDeactivated(address indexed operator);

    /// @notice Emitted when the unstake delay is updated.
    /// @param oldDelay The previous unstake delay.
    /// @param newDelay The new unstake delay.
    event UnstakeDelayUpdated(uint256 oldDelay, uint256 newDelay);

    /// @notice Initializes the contract with the staking token, admin, and initial unstake delay.
    /// @param token_ The ERC20 token used for staking.
    /// @param admin The address to be granted the DEFAULT_ADMIN_ROLE.
    /// @param initialUnstakeDelay The initial delay in seconds for unstaking.
    constructor(IERC20 token_, address admin, uint256 initialUnstakeDelay) {
        if (address(token_) == address(0)) revert ZeroAddress();
        if (admin == address(0)) revert ZeroAddress();

        _stakingToken = token_;
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        unstakeDelay = initialUnstakeDelay;
    }

    // Views

    /// @notice Returns the address of the staking token.
    /// @return The address of the ERC20 staking token.
    function stakingToken() external view override returns (address) {
        return address(_stakingToken);
    }

    /// @notice Returns the total stake amount for a specific operator.
    /// @param operator The address of the operator.
    /// @return The amount of tokens staked for the operator.
    function stakeOf(address operator) external view override returns (uint256) {
        return _operatorStake[operator];
    }

    /// @notice Returns the total amount of tokens staked in the contract.
    /// @return The total staked amount.
    function totalStaked() external view override returns (uint256) {
        return _totalStaked;
    }

    /// @notice Checks if an operator is currently jailed.
    /// @param operator The address of the operator.
    /// @return True if the operator is jailed, false otherwise.
    function isJailed(address operator) external view override returns (bool) {
        return block.timestamp < _jailedUntil[operator];
    }

    // Admin

    /// @notice Updates the unstake delay.
    /// @dev Only callable by accounts with DEFAULT_ADMIN_ROLE.
    /// @param newDelay The new delay in seconds.
    function setUnstakeDelay(uint256 newDelay) external onlyRole(DEFAULT_ADMIN_ROLE) {
        emit UnstakeDelayUpdated(unstakeDelay, newDelay);
        unstakeDelay = newDelay;
    }

    // Staking

    /// @notice Stakes tokens to a specific operator.
    /// @dev Transfers tokens from msg.sender to the contract.
    ///      If the operator has no current staker, msg.sender becomes the staker.
    ///      If the operator already has a staker, msg.sender must be that staker.
    /// @param operator The address of the operator to stake to.
    /// @param amount The amount of tokens to stake.
    function stakeTo(address operator, uint256 amount) external override nonReentrant {
        if (operator == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();
        if (block.timestamp < _jailedUntil[operator]) revert OperatorJailed();

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

    /// @notice Requests to unstake tokens from an operator.
    /// @dev Starts the unbonding period. Tokens are locked until releaseTime.
    ///      Only the current staker can request unstaking.
    ///      Cannot request unstake if an unbonding is already in progress or if the operator is jailed.
    /// @param operator The address of the operator to unstake from.
    /// @param amount The amount of tokens to unstake.
    function requestUnstake(address operator, uint256 amount) external override nonReentrant {
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

    /// @notice Withdraws unstaked tokens after the unbonding period has passed.
    /// @dev Transfers tokens to the staker and clears the unbonding record.
    /// @param operator The address of the operator associated with the unbonding.
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

    /// @notice Registers the caller as an operator or updates their metadata.
    /// @dev Requires the caller to have a non-zero stake.
    /// @param metadataURI The URI containing the operator's metadata.
    function registerOperator(string calldata metadataURI) external override {
        if (_operatorStake[msg.sender] == 0) revert NoStake();

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

    /// @notice Deactivates the caller as an operator.
    /// @dev The operator must exist and be currently active.
    function deactivateOperator() external override {
        OperatorData storage data = _operators[msg.sender];
        if (!(data.exists && data.active)) revert NotActive();
        data.active = false;
        emit OperatorDeactivated(msg.sender);
    }

    /// @notice Returns information about a specific operator.
    /// @param operator The address of the operator.
    /// @return An OperatorInfo struct containing the operator's active status and metadata URI.
    function getOperatorInfo(address operator) external view override returns (OperatorInfo memory) {
        OperatorData storage data = _operators[operator];
        return OperatorInfo({active: data.active, metadataURI: data.metadataURI});
    }

    /// @notice Checks if an operator is active.
    /// @dev An operator is active if they have registered, have stake > 0, and are not jailed.
    /// @param operator The address of the operator.
    /// @return True if the operator is active, false otherwise.
    function isActiveOperator(address operator) public view override returns (bool) {
        OperatorData storage data = _operators[operator];
        if (!data.active) return false;
        if (_operatorStake[operator] == 0) return false;
        return block.timestamp >= _jailedUntil[operator];
    }

    /// @notice Returns a list of all currently active operators.
    /// @dev Iterates through all registered operators to filter for active ones.
    ///      Warning: This function may be gas-intensive if there are many operators.
    /// @return An array of addresses of active operators.
    function getActiveOperators() external view override returns (address[] memory) {
        uint256 len = _allOperators.length;
        uint256 count;

        for (uint256 i = 0; i < len;) {
            if (isActiveOperator(_allOperators[i])) {
                unchecked {
                    ++count;
                }
            }
            unchecked {
                ++i;
            }
        }

        address[] memory active = new address[](count);
        uint256 idx;
        for (uint256 i = 0; i < len;) {
            if (isActiveOperator(_allOperators[i])) {
                active[idx] = _allOperators[i];
                unchecked {
                    ++idx;
                }
            }
            unchecked {
                ++i;
            }
        }
        return active;
    }

    /// @notice Returns a list of all registered operators (active and inactive).
    /// @dev Returns the complete list of operators who have ever registered.
    ///      Warning: This function may be gas-intensive if there are many operators.
    /// @return An array of addresses of all registered operators.
    function getAllOperators() external view returns (address[] memory) {
        return _allOperators;
    }

    // Slashing / jailing

    /// @notice Slashes an operator's stake.
    /// @dev Only callable by accounts with SLASHER_ROLE.
    ///      If the amount to slash exceeds the operator's stake, the entire stake is slashed.
    /// @param operator The address of the operator to slash.
    /// @param amount The amount of tokens to slash.
    function slash(address operator, uint256 amount) external override onlyRole(SLASHER_ROLE) nonReentrant {
        uint256 bal = _operatorStake[operator];
        uint256 toSlash = amount > bal ? bal : amount;
        if (toSlash == 0) return;

        _operatorStake[operator] = bal - toSlash;
        _totalStaked -= toSlash;

        emit Slashed(operator, toSlash);
    }

    /// @notice Jails an operator until a specific timestamp.
    /// @dev Only callable by accounts with SLASHER_ROLE.
    ///      Can only extend the jail time, not shorten it.
    /// @param operator The address of the operator to jail.
    /// @param untilTimestamp The timestamp until which the operator is jailed.
    function jail(address operator, uint64 untilTimestamp) external override onlyRole(SLASHER_ROLE) {
        if (untilTimestamp > _jailedUntil[operator]) {
            _jailedUntil[operator] = untilTimestamp;
            emit Jailed(operator, untilTimestamp);
        }
    }
}
