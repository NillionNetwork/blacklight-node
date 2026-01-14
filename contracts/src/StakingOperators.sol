// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";
import "@openzeppelin/contracts/utils/math/Math.sol";
import "./Interfaces.sol";

/// @title StakingOperators
/// @notice ERC20 staking + operator registry with stake snapshots for committee selection.
contract StakingOperators is IStakingOperators, AccessControl, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    error ZeroAddress();
    error ZeroAmount();
    error DifferentStaker();
    error NotStaker();
    error InsufficientStake();
    error InsufficientStakeForActivation();
    error OperatorJailed();
    error NoUnbonding();
    error NotReady();
    error NotActive();
    error NotSnapshotter();
    error TooManyTranches();
    error InvalidAddress();
    error CannotReactivateWhileJailed();
    error OperatorDoesNotExist();
    error StakeOverflow();
    error BatchTooLarge();
    error InvalidUnstakeDelay();
    error UnauthorizedStaker();
    error StakerAlreadyBound();
    error InvalidMaxActiveOperators();
    error TooManyActiveOperators();

    struct StakeCheckpoint { uint64 fromBlock; uint224 stake; }
    struct Unbonding { address staker; IStakingOperators.Tranche[] tranches; }
    struct OperatorData { bool active; string metadataURI; bool exists; }

    bytes32 public constant SLASHER_ROLE = keccak256("SLASHER_ROLE");

    uint256 private constant MAX_BATCH_POKE = 50;
    uint256 private constant MIN_DELAY = 1 days;
    uint256 private constant MAX_DELAY = 365 days;

    uint256 private constant DEFAULT_MAX_ACTIVE_OPERATORS = 1000;
    IERC20 private immutable _stakingToken;
    uint256 public override unstakeDelay;
    uint256 public constant MAX_TRANCHES_PER_OPERATOR = 32;

    IProtocolConfig public protocolConfig;
    uint256 public maxActiveOperators;

    mapping(address => uint256) private _operatorStake;
    uint256 private _totalStaked;
    mapping(address => StakeCheckpoint[]) private _stakeCheckpoints;

    uint64 public currentSnapshotId;
    address public snapshotter;
    address public override heartbeatManager;

    mapping(address => address) public override operatorStaker;
    mapping(address => Unbonding) private _unbondings;

    mapping(address => uint64) private _jailedUntil;

    mapping(address => OperatorData) private _operators;

    mapping(address => address) public approvedStaker;
    address[] private _activeOperators;
    mapping(address => uint256) private _activeIndexPlus1;

    event StakedTo(address indexed staker, address indexed operator, uint256 amount);
    event UnstakeRequested(address indexed staker, address indexed operator, uint256 amount, uint64 releaseTime);
    event UnstakedWithdrawn(address indexed staker, address indexed operator, uint256 amount);
    event Slashed(address indexed operator, uint256 amount);
    event Jailed(address indexed operator, uint64 until);
    event OperatorRegistered(address indexed operator, string metadataURI);
    event OperatorDeactivated(address indexed operator);
    event OperatorDeactivatedByPolicy(address indexed operator, uint64 until);
    event UnstakeDelayUpdated(uint256 oldDelay, uint256 newDelay);
    event ProtocolConfigUpdated(address oldConfig, address newConfig);
    event SnapshotterUpdated(address oldSnapshotter, address newSnapshotter);
    event HeartbeatManagerUpdated(address oldHeartbeatManager, address newHeartbeatManager);
    event ActiveStatusUpdated(address indexed operator, bool isActive);

    constructor(IERC20 token_, address admin, uint256 initialUnstakeDelay) {
        if (address(token_) == address(0)) revert ZeroAddress();
    event MaxActiveOperatorsUpdated(uint256 oldCap, uint256 newCap);
        if (admin == address(0)) revert ZeroAddress();
        _stakingToken = token_;
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    event StakerApproved(address indexed operator, address indexed staker);
        if (initialUnstakeDelay < MIN_DELAY || initialUnstakeDelay > MAX_DELAY) revert InvalidUnstakeDelay();
        unstakeDelay = initialUnstakeDelay;
    }

    function pause() external onlyRole(DEFAULT_ADMIN_ROLE) { _pause(); }
        maxActiveOperators = DEFAULT_MAX_ACTIVE_OPERATORS;
        emit MaxActiveOperatorsUpdated(0, DEFAULT_MAX_ACTIVE_OPERATORS);
    function unpause() external onlyRole(DEFAULT_ADMIN_ROLE) { _unpause(); }

    function setProtocolConfig(IProtocolConfig newConfig) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (address(newConfig) == address(0)) revert ZeroAddress();
        address old = address(protocolConfig);
        protocolConfig = newConfig;
        emit ProtocolConfigUpdated(old, address(newConfig));
    }

    function setUnstakeDelay(uint256 newDelay) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (newDelay < MIN_DELAY || newDelay > MAX_DELAY) revert InvalidUnstakeDelay();
        emit UnstakeDelayUpdated(unstakeDelay, newDelay);
        unstakeDelay = newDelay;
    }

    function setSnapshotter(address newSnapshotter) external override onlyRole(DEFAULT_ADMIN_ROLE) {
        if (newSnapshotter == address(0)) revert ZeroAddress();
        emit SnapshotterUpdated(snapshotter, newSnapshotter);
        snapshotter = newSnapshotter;
    function setMaxActiveOperators(uint256 newCap) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (newCap == 0) revert InvalidMaxActiveOperators();
        emit MaxActiveOperatorsUpdated(maxActiveOperators, newCap);
        maxActiveOperators = newCap;
    }

    }

    function setHeartbeatManager(address newHeartbeatManager) external override onlyRole(DEFAULT_ADMIN_ROLE) {
        if (newHeartbeatManager == address(0)) revert InvalidAddress();
        emit HeartbeatManagerUpdated(heartbeatManager, newHeartbeatManager);
        heartbeatManager = newHeartbeatManager;
    }


    function snapshot() external override returns (uint64 snapshotId) {
        if (msg.sender != snapshotter && msg.sender != heartbeatManager) revert NotSnapshotter();
        if (block.number <= 1) revert NotReady();
        snapshotId = uint64(block.number - 1);
        currentSnapshotId = snapshotId;
    }

    function stakeAt(address operator, uint64 snapshotId) public view override returns (uint256) {
        StakeCheckpoint[] storage ckpts = _stakeCheckpoints[operator];
        uint256 len = ckpts.length;
        if (len == 0) return 0;
        if (ckpts[0].fromBlock > snapshotId) return 0;

        uint256 high = len - 1;
        if (ckpts[high].fromBlock <= snapshotId) return ckpts[high].stake;

        uint256 low = 0;
        while (high > low) {
            uint256 mid = Math.ceilDiv(high + low, 2);
            if (ckpts[mid].fromBlock <= snapshotId) low = mid;
            else high = mid - 1;
        }
        return ckpts[low].stake;
    }

    function stakingToken() external view override returns (address) { return address(_stakingToken); }
    function stakeOf(address operator) external view override returns (uint256) { return _operatorStake[operator]; }
    function totalStaked() external view override returns (uint256) { return _totalStaked; }
    function isJailed(address operator) external view override returns (bool) { return block.timestamp < _jailedUntil[operator]; }
    function getUnbondingTranches(address operator) external view returns (IStakingOperators.Tranche[] memory) { return _unbondings[operator].tranches; }
    function unbondingStaker(address operator) external view returns (address) { return _unbondings[operator].staker; }
    function isActiveOperator(address operator) public view override returns (bool) { return _computeIsActive(operator); }

    function getOperatorInfo(address operator) external view override returns (OperatorInfo memory) {
        OperatorData storage data = _operators[operator];
        return OperatorInfo({active: data.active, metadataURI: data.metadataURI});
    }

    function _hasMinStake(address operator) internal view returns (bool) {
        uint256 bal = _operatorStake[operator];
        IProtocolConfig cfg = protocolConfig;
        if (address(cfg) == address(0)) return bal > 0;
        uint256 minStake = cfg.minOperatorStake();
        if (minStake == 0) return bal > 0;
        return bal >= minStake;
    }

    function _computeIsActive(address operator) internal view returns (bool) {
        OperatorData storage data = _operators[operator];
        if (!data.active) return false;
        if (!_hasMinStake(operator)) return false;
        return block.timestamp >= _jailedUntil[operator];
    }

    function _setActiveInSet(address operator, bool shouldBeActive) internal {
        uint256 idxPlus1 = _activeIndexPlus1[operator];
        bool isInSet = idxPlus1 != 0;

        if (shouldBeActive && !isInSet) {
            _activeOperators.push(operator);
            _activeIndexPlus1[operator] = _activeOperators.length;
            emit ActiveStatusUpdated(operator, true);
        } else if (!shouldBeActive && isInSet) {
            uint256 idx = idxPlus1 - 1;
            if (_activeOperators.length >= maxActiveOperators) revert TooManyActiveOperators();
            uint256 last = _activeOperators.length - 1;
            if (idx != last) {
                address swapped = _activeOperators[last];
                _activeOperators[idx] = swapped;
                _activeIndexPlus1[swapped] = idx + 1;
            }
            _activeOperators.pop();
            _activeIndexPlus1[operator] = 0;
            emit ActiveStatusUpdated(operator, false);
        }
    }

    function pokeActive(address operator) external {
        _setActiveInSet(operator, _computeIsActive(operator));
    }

    function pokeActiveMany(address[] calldata operators) external {
        if (operators.length > MAX_BATCH_POKE) revert BatchTooLarge();
        for (uint256 i = 0; i < operators.length; ) {
            _setActiveInSet(operators[i], _computeIsActive(operators[i]));
            unchecked { ++i; }
        }
    }

    function getActiveOperators() external view override returns (address[] memory) {
        uint256 n = _activeOperators.length;
        if (n == 0) return new address[](0);
        return _activeOperators;
    }

    function approveStaker(address staker) external whenNotPaused {
        if (staker == address(0)) revert ZeroAddress();
        if (operatorStaker[msg.sender] != address(0)) revert StakerAlreadyBound();
        approvedStaker[msg.sender] = staker;
        emit StakerApproved(msg.sender, staker);
    }

    function stakeTo(address operator, uint256 amount) external override nonReentrant whenNotPaused {
        if (operator == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();

        address currentStaker = operatorStaker[operator];
        if (currentStaker == address(0)) {
            IProtocolConfig cfg = protocolConfig;
            if (address(cfg) != address(0)) {
                uint256 minStake = cfg.minOperatorStake();
                if (minStake != 0 && amount < minStake) revert InsufficientStakeForActivation();
            }
            address approved = approvedStaker[operator];
            if (approved != address(0) && msg.sender != operator && msg.sender != approved) revert UnauthorizedStaker();
            operatorStaker[operator] = msg.sender;
            _unbondings[operator].staker = msg.sender;
            if (approved != address(0)) approvedStaker[operator] = address(0);
        } else if (currentStaker != msg.sender) revert DifferentStaker();

        _stakingToken.safeTransferFrom(msg.sender, address(this), amount);
        _operatorStake[operator] += amount;
        _totalStaked += amount;

        _writeCheckpoint(operator, _operatorStake[operator]);
        _setActiveInSet(operator, _computeIsActive(operator));

        emit StakedTo(msg.sender, operator, amount);
    }

    function requestUnstake(address operator, uint256 amount) external override nonReentrant whenNotPaused {
        if (operator == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();
        if (operatorStaker[operator] != msg.sender) revert NotStaker();
        // If MAX_TRANCHES_PER_OPERATOR is reached, callers must withdraw matured tranches before requesting more.
        uint256 bal = _operatorStake[operator];
        if (bal < amount) revert InsufficientStake();
        if (block.timestamp < _jailedUntil[operator]) revert OperatorJailed();

        _operatorStake[operator] = bal - amount;
        _writeCheckpoint(operator, _operatorStake[operator]);

        Unbonding storage u = _unbondings[operator];
        uint64 releaseTime = uint64(block.timestamp + unstakeDelay);
        _pushTranche(u, amount, releaseTime);

        _setActiveInSet(operator, _computeIsActive(operator));
        emit UnstakeRequested(msg.sender, operator, amount, releaseTime);
    }

    function withdrawUnstaked(address operator) external override nonReentrant whenNotPaused {
        if (operator == address(0)) revert ZeroAddress();

        Unbonding storage u = _unbondings[operator];
        uint256 len = u.tranches.length;
        if (len == 0) revert NoUnbonding();
        if (msg.sender != u.staker) revert NotStaker();

        uint256 payout;
        uint256 writeIndex;

        for (uint256 i = 0; i < len; ) {
            IStakingOperators.Tranche memory t = u.tranches[i];
            if (block.timestamp >= t.releaseTime) payout += t.amount;
            else { u.tranches[writeIndex] = t; unchecked { ++writeIndex; } }
            unchecked { ++i; }
        }
        while (u.tranches.length > writeIndex) u.tranches.pop();
        if (payout == 0) revert NotReady();

        address staker = u.staker;
        if (u.tranches.length == 0 && _operatorStake[operator] == 0) {
            operatorStaker[operator] = address(0);
            u.staker = address(0);
        }

        _totalStaked -= payout;
        _stakingToken.safeTransfer(staker, payout);

        _setActiveInSet(operator, _computeIsActive(operator));
        emit UnstakedWithdrawn(staker, operator, payout);
    }

    function registerOperator(string calldata metadataURI) external override whenNotPaused {
        if (!_hasMinStake(msg.sender)) revert InsufficientStakeForActivation();
        OperatorData storage data = _operators[msg.sender];
        if (!data.exists) { data.exists = true; }
        data.active = true;
        data.metadataURI = metadataURI;

        _setActiveInSet(msg.sender, _computeIsActive(msg.sender));
        emit OperatorRegistered(msg.sender, metadataURI);
    }

    function deactivateOperator() external override whenNotPaused {
        OperatorData storage data = _operators[msg.sender];
        if (!(data.exists && data.active)) revert NotActive();
        data.active = false;
        _setActiveInSet(msg.sender, false);
        emit OperatorDeactivated(msg.sender);
    }

    function reactivateOperator() external override whenNotPaused {
        OperatorData storage data = _operators[msg.sender];
        if (!data.exists) revert OperatorDoesNotExist();
        if (block.timestamp < _jailedUntil[msg.sender]) revert CannotReactivateWhileJailed();
        if (!_hasMinStake(msg.sender)) revert InsufficientStakeForActivation();
        data.active = true;
        _setActiveInSet(msg.sender, _computeIsActive(msg.sender));
    }

    function slash(address operator, uint256 amount) external override onlyRole(SLASHER_ROLE) nonReentrant {
        uint256 remaining = _slashActiveStake(operator, amount);
        remaining = _slashUnbonding(operator, remaining);

        uint256 slashed = amount - remaining;
        if (slashed != 0) {
            _totalStaked -= slashed;
            _stakingToken.safeTransfer(address(0xdead), slashed);
            emit Slashed(operator, slashed);
        }
        _setActiveInSet(operator, _computeIsActive(operator));
    }

    function jail(address operator, uint64 untilTimestamp) external override onlyRole(SLASHER_ROLE) {
        if (untilTimestamp > _jailedUntil[operator]) {
            _jailedUntil[operator] = untilTimestamp;
            emit Jailed(operator, untilTimestamp);
        }

        OperatorData storage data = _operators[operator];
        if (data.exists && data.active) {
            data.active = false;
            emit OperatorDeactivatedByPolicy(operator, _jailedUntil[operator]);
        }

        _setActiveInSet(operator, _computeIsActive(operator));
    }

    function _slashActiveStake(address operator, uint256 remaining) internal returns (uint256) {
        uint256 bal = _operatorStake[operator];
        if (bal == 0) return remaining;

        uint256 toSlash = remaining > bal ? bal : remaining;
        if (toSlash == 0) return remaining;

        _operatorStake[operator] = bal - toSlash;
        _writeCheckpoint(operator, _operatorStake[operator]);
        return remaining - toSlash;
    }

    function _slashUnbonding(address operator, uint256 remaining) internal returns (uint256) {
        if (remaining == 0) return remaining;

        Unbonding storage u = _unbondings[operator];
        uint256 len = u.tranches.length;
        uint256 writeIndex;

        for (uint256 i = 0; i < len; ) {
            IStakingOperators.Tranche memory t = u.tranches[i];
            if (remaining != 0 && t.amount <= remaining) {
                remaining -= t.amount;
            } else {
                if (remaining != 0) {
                    uint256 newAmount = t.amount - remaining;
                    remaining = 0;
                    u.tranches[writeIndex] = IStakingOperators.Tranche({amount: newAmount, releaseTime: t.releaseTime});
                } else {
                    u.tranches[writeIndex] = t;
                }
                unchecked { ++writeIndex; }
            }
            unchecked { ++i; }
        }

        while (u.tranches.length > writeIndex) u.tranches.pop();
        return remaining;
    }

    function _pushTranche(Unbonding storage u, uint256 amount, uint64 releaseTime) internal {
        uint256 len = u.tranches.length;

        if (len != 0) {
            IStakingOperators.Tranche storage last = u.tranches[len - 1];
            if (last.releaseTime == releaseTime) { last.amount += amount; return; }
        }

        if (len >= MAX_TRANCHES_PER_OPERATOR) revert TooManyTranches();
        u.tranches.push(IStakingOperators.Tranche({amount: amount, releaseTime: releaseTime}));
    }

    function _writeCheckpoint(address operator, uint256 newStake) internal {
        if (newStake > type(uint224).max) revert StakeOverflow();

        StakeCheckpoint[] storage ckpts = _stakeCheckpoints[operator];
        uint64 blockNum = uint64(block.number);
        uint224 boundedStake = uint224(newStake);
        uint256 len = ckpts.length;

        if (len != 0 && ckpts[len - 1].fromBlock == blockNum) {
            ckpts[len - 1].stake = boundedStake;
        } else {
            ckpts.push(StakeCheckpoint({fromBlock: blockNum, stake: boundedStake}));
        }
    }
}
