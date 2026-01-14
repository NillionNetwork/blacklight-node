// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/math/Math.sol";
import "./Interfaces.sol";

/// @title RewardPolicy
/// @notice Streaming-budget reward allocator with stake-weighted distribution.
/// @dev Assumes rewardToken is a standard ERC20 (no transfer fees, rebasing, or balance-modifying hooks).
contract RewardPolicy is IRewardPolicy, Ownable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    error ZeroAddress();
    error NotHeartbeatManager();
    error NothingToClaim();
    error AlreadyProcessed();
    error LengthMismatch();
    error UnsortedRecipients();
    error CommitmentMismatch();
    error InsufficientBudget();
    error ZeroEpochDuration();
    error InsufficientWithdrawable();
    error AccountingFrozen();
    error Insolvent(uint256 balance, uint256 reserved);

    IERC20 public immutable rewardToken;
    address public immutable heartbeatManager;

    uint256 public accountedBalance;
    uint256 private _spendableBudget;
    uint64 public lastUpdate;

    uint256 public streamRemaining;
    uint256 public streamRatePerSecondWad;
    uint64  public streamEnd;

    uint256 public epochDuration;
    uint256 public maxPayoutPerFinalize;
    bool public accountingFrozen;

    mapping(address => uint256) public rewards;
    mapping(bytes32 => mapping(uint8 => bool)) public processed;
    mapping(bytes32 => mapping(uint8 => bytes32)) public commitment;
    uint256 public totalOutstandingRewards;

    event RewardsAccrued(bytes32 indexed heartbeatKey, uint8 round, address indexed recipient, uint256 amount);
    event RewardClaimed(address indexed recipient, uint256 amount);
    event Synced(uint256 newAmount, uint256 newAccountedBalance);
    event AccountingUnderflow(uint256 observedBalance, uint256 previousAccountedBalance);
    event AccountingFreezeCleared(uint256 balance, uint256 accountedBalance);
    event EpochDurationUpdated(uint256 oldDuration, uint256 newDuration);
    event MaxPayoutPerFinalizeUpdated(uint256 oldCap, uint256 newCap);
    event StreamUpdated(uint256 streamRemaining, uint256 ratePerSecondWad, uint64 streamEnd);
    event BudgetUsed(bytes32 indexed heartbeatKey, uint8 round, uint256 budget, uint256 distributed);

    modifier whenAccountingHealthy() {
        if (accountingFrozen) revert AccountingFrozen();
        _;
    }

    constructor(
        IERC20 _rewardToken,
        address _manager,
        address _owner,
        uint256 _epochDuration,
        uint256 _maxPayoutPerFinalize
    ) Ownable(_owner) {
        if (address(_rewardToken) == address(0)) revert ZeroAddress();
        if (_manager == address(0)) revert ZeroAddress();
        if (_epochDuration == 0) revert ZeroEpochDuration();

        rewardToken = _rewardToken;
        heartbeatManager = _manager;

        epochDuration = _epochDuration;
        maxPayoutPerFinalize = _maxPayoutPerFinalize;

        sync();
    }

    function spendableBudget() external view override returns (uint256) { return _spendableBudget; }

    function reservedBalance() external view returns (uint256) {
        return totalOutstandingRewards + streamRemaining + _spendableBudget;
    }

    function withdrawableBalance() external view returns (uint256) {
        uint256 bal = rewardToken.balanceOf(address(this));
        uint256 reserved = totalOutstandingRewards + streamRemaining + _spendableBudget;
        if (bal <= reserved) return 0;
        unchecked { return bal - reserved; }
    }

    function setEpochDuration(uint256 newDuration) external onlyOwner {
        if (newDuration == 0) revert ZeroEpochDuration();
        sync();
        emit EpochDurationUpdated(epochDuration, newDuration);
        epochDuration = newDuration;
        _recomputeStreamRate();
    }

    function setMaxPayoutPerFinalize(uint256 newCap) external onlyOwner {
        emit MaxPayoutPerFinalizeUpdated(maxPayoutPerFinalize, newCap);
        maxPayoutPerFinalize = newCap;
    }

    /// @notice Clears freeze iff the contract is solvent w.r.t. obligations.
    function clearAccountingFreeze() external onlyOwner {
        _updateUnlock();
        uint256 bal = rewardToken.balanceOf(address(this));
        uint256 reserved = totalOutstandingRewards + streamRemaining + _spendableBudget;
        if (bal < reserved) revert Insolvent(bal, reserved);

        accountingFrozen = false;
        accountedBalance = bal;
        emit AccountingFreezeCleared(bal, accountedBalance);
    }

    function fund(uint256 amount) external onlyOwner {
        rewardToken.safeTransferFrom(msg.sender, address(this), amount);
        sync();
    }

    function withdraw(uint256 amount, address to) external onlyOwner whenAccountingHealthy {
        if (to == address(0)) revert ZeroAddress();
        sync();
        if (accountingFrozen) revert AccountingFrozen();

        uint256 bal = rewardToken.balanceOf(address(this));
        uint256 reserved = totalOutstandingRewards + streamRemaining + _spendableBudget;
        if (bal <= reserved) revert InsufficientWithdrawable();

        uint256 withdrawable = bal - reserved;
        if (amount > withdrawable) revert InsufficientWithdrawable();

        rewardToken.safeTransfer(to, amount);
        accountedBalance = rewardToken.balanceOf(address(this));
    }

    function sync() public {
        _updateUnlock();

        uint256 bal = rewardToken.balanceOf(address(this));

        if (bal < accountedBalance) {
            emit AccountingUnderflow(bal, accountedBalance);
            uint256 reserved = totalOutstandingRewards + streamRemaining + _spendableBudget;
            if (bal < reserved) {
                accountingFrozen = true;
                return;
            }
            accountedBalance = bal;
            return;
        }

        if (bal == accountedBalance) return;

        uint256 delta = bal - accountedBalance;
        accountedBalance = bal;
        _onNewDeposit(delta);
        emit Synced(delta, accountedBalance);
    }

    function _updateUnlock() internal {
        uint64 nowTs = uint64(block.timestamp);
        uint64 last = lastUpdate;
        if (last == 0) { lastUpdate = nowTs; return; }
        if (nowTs <= last) return;
        if (streamRemaining == 0) { lastUpdate = nowTs; return; }

        uint256 elapsed = uint256(nowTs - last);
        if (streamRatePerSecondWad != 0) {
            uint256 unlock = Math.mulDiv(elapsed, streamRatePerSecondWad, 1e18);
            if (unlock > streamRemaining) unlock = streamRemaining;
            if (unlock != 0) {
                streamRemaining -= unlock;
                _spendableBudget += unlock;
            }
        }

        if (nowTs >= streamEnd && streamRemaining != 0) {
            _spendableBudget += streamRemaining;
            streamRemaining = 0;
            streamRatePerSecondWad = 0;
        }

        lastUpdate = nowTs;
    }

    function _onNewDeposit(uint256 amount) internal {
        if (amount == 0) return;
        streamRemaining += amount;
        _recomputeStreamRate();
    }

    function _recomputeStreamRate() internal {
        if (streamRemaining == 0) {
            streamRatePerSecondWad = 0;
            streamEnd = uint64(block.timestamp);
            emit StreamUpdated(streamRemaining, streamRatePerSecondWad, streamEnd);
            return;
        }

        uint256 dur = epochDuration;
        uint64 nowTs = uint64(block.timestamp);
        if (nowTs < streamEnd && streamEnd != 0) {
            uint256 remainingTime = uint256(streamEnd - nowTs);
            if (remainingTime == 0) remainingTime = 1;
            streamRatePerSecondWad = Math.mulDiv(streamRemaining, 1e18, remainingTime);
        } else {
            streamEnd = uint64(nowTs + dur);
            streamRatePerSecondWad = Math.mulDiv(streamRemaining, 1e18, dur);
        }

        emit StreamUpdated(streamRemaining, streamRatePerSecondWad, streamEnd);
    }

    function accrueWeights(
        bytes32 heartbeatKey,
        uint8 round,
        address[] calldata recipients,
        uint256[] calldata weights
    ) external override whenAccountingHealthy {
        if (msg.sender != heartbeatManager) revert NotHeartbeatManager();
        if (processed[heartbeatKey][round]) revert AlreadyProcessed();
        if (recipients.length != weights.length) revert LengthMismatch();

        address last = address(0);
        uint256 totalWeight;
        uint256 bestWeight;
        address bestRecipient;
        for (uint256 i = 0; i < recipients.length; i++) {
            address recipient = recipients[i];
            uint256 weight = weights[i];
            if (recipient <= last) revert UnsortedRecipients();
            last = recipient;
            totalWeight += weight;
            if (weight > bestWeight || (weight == bestWeight && uint160(recipient) < uint160(bestRecipient))) {
                bestWeight = weight;
                bestRecipient = recipient;
            }
        }

        bytes32 h = keccak256(abi.encode(recipients, weights));
        bytes32 prev = commitment[heartbeatKey][round];
        if (prev == bytes32(0)) commitment[heartbeatKey][round] = h;
        else if (prev != h) revert CommitmentMismatch();

        sync();
        if (accountingFrozen) revert AccountingFrozen();

        if (totalWeight == 0) {
            processed[heartbeatKey][round] = true;
            return;
        }

        uint256 budget = _spendableBudget;
        if (budget == 0) revert InsufficientBudget();

        uint256 cap = maxPayoutPerFinalize;
        if (cap != 0 && budget > cap) budget = cap;

        uint256 distributed;
        for (uint256 i = 0; i < recipients.length; i++) {
            uint256 w = weights[i];
            if (w != 0) {
                uint256 amt = Math.mulDiv(budget, w, totalWeight);
                if (amt != 0) {
                    rewards[recipients[i]] += amt;
                    totalOutstandingRewards += amt;
                    distributed += amt;
                    emit RewardsAccrued(heartbeatKey, round, recipients[i], amt);
                }
            }
        }

        if (distributed == 0) {
            if (bestWeight == 0) revert InsufficientBudget();
            rewards[bestRecipient] += 1;
            totalOutstandingRewards += 1;
            distributed = 1;
            emit RewardsAccrued(heartbeatKey, round, bestRecipient, 1);
        }

        _spendableBudget -= distributed;
        processed[heartbeatKey][round] = true;
        emit BudgetUsed(heartbeatKey, round, budget, distributed);
    }

    function claim() external override nonReentrant whenAccountingHealthy {
        sync();
        if (accountingFrozen) revert AccountingFrozen();

        uint256 amount = rewards[msg.sender];
        if (amount == 0) revert NothingToClaim();

        rewards[msg.sender] = 0;
        totalOutstandingRewards -= amount;

        rewardToken.safeTransfer(msg.sender, amount);
        accountedBalance = rewardToken.balanceOf(address(this));
        emit RewardClaimed(msg.sender, amount);
    }
}
