// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "./Interfaces.sol";

/// @title WeightedCommitteeSelector
/// @notice Stake-weighted selection without replacement using snapshot voting power.
/// @dev Randomness seed prefers `blockhash(snapshotId)` and falls back to future `prevrandao` if unavailable; this is
/// grindable by submitters, accepted for this RC.
contract WeightedCommitteeSelector is ICommitteeSelector {
    error ZeroAddress();
    error ZeroMaxSize();
    error NoOperators();
    error NotAdmin();
    error EmptyCommitteeRequested();
    error InsufficientCommitteeVP(uint256 selectedVP, uint256 requiredVP);
    error SnapshotBlockUnavailable(uint64 snapshotId);
    error ZeroTotalVotingPower();
    error ZeroMinCommitteeVP();

    uint32 private constant DEFAULT_MAX_ACTIVE_OPERATORS = 1000;

    IStakingOperators public immutable stakingOps;
    address public immutable admin;

    uint256 public minCommitteeVP;
    uint32 public maxCommitteeSize;
    uint32 public maxActiveOperators;

    event MinCommitteeVPUpdated(uint256 oldVP, uint256 newVP);
    event MaxCommitteeSizeUpdated(uint32 oldSize, uint32 newSize);
    event MaxActiveOperatorsUpdated(uint32 oldCap, uint32 newCap);

    modifier onlyAdmin() {
        if (msg.sender != admin) revert NotAdmin();
        _;
    }

    constructor(
        IStakingOperators _stakingOps,
        address _admin,
        uint256 _minCommitteeVP,
        uint32 _maxCommitteeSize
    ) {
        if (address(_stakingOps) == address(0)) revert ZeroAddress();
        if (_admin == address(0)) revert ZeroAddress();
        if (_minCommitteeVP == 0) revert ZeroMinCommitteeVP();
        if (_maxCommitteeSize == 0) revert ZeroMaxSize();

        stakingOps = _stakingOps;
        admin = _admin;
        minCommitteeVP = _minCommitteeVP;
        maxCommitteeSize = _maxCommitteeSize;

        maxActiveOperators = DEFAULT_MAX_ACTIVE_OPERATORS;
        emit MaxActiveOperatorsUpdated(0, DEFAULT_MAX_ACTIVE_OPERATORS);
    }

    function setMinCommitteeVP(uint256 newVP) external onlyAdmin {
        if (newVP == 0) revert ZeroMinCommitteeVP();
        emit MinCommitteeVPUpdated(minCommitteeVP, newVP);
        minCommitteeVP = newVP;
    }

    function setMaxCommitteeSize(uint32 newSize) external onlyAdmin {
        if (newSize == 0) revert ZeroMaxSize();
        emit MaxCommitteeSizeUpdated(maxCommitteeSize, newSize);
        maxCommitteeSize = newSize;
    }

    function setMaxActiveOperators(uint32 newCap) external onlyAdmin {
        if (newCap == 0) newCap = DEFAULT_MAX_ACTIVE_OPERATORS;
        emit MaxActiveOperatorsUpdated(maxActiveOperators, newCap);
        maxActiveOperators = newCap;
    }

    function selectCommittee(
        bytes32 heartbeatKey,
        uint8 round,
        uint32 committeeSize,
        uint64 snapshotId
    ) external view override returns (address[] memory members) {
        bytes32 bh = _randomnessSeed(snapshotId);

        address[] memory active = stakingOps.getActiveOperators();
        uint256 n = active.length;
        if (n == 0) revert NoOperators();

        uint32 cap = maxActiveOperators;
        if (cap == 0) cap = DEFAULT_MAX_ACTIVE_OPERATORS;

        address[] memory pool;
        uint256[] memory stakes;

        if (n > cap) {
            (pool, stakes) = _topByStake(active, snapshotId, cap);
            n = pool.length;
        } else {
            pool = active;
            stakes = new uint256[](n);
            for (uint256 i = 0; i < n; ) {
                stakes[i] = stakingOps.stakeAt(pool[i], snapshotId);
                unchecked { ++i; }
            }
        }

        uint32 k = committeeSize;
        if (k == 0) revert EmptyCommitteeRequested();
        if (k > maxCommitteeSize) k = maxCommitteeSize;
        if (k > n) k = uint32(n);

        uint256 totalVP;
        for (uint256 i = 0; i < n; ) {
            totalVP += stakes[i];
            unchecked { ++i; }
        }
        if (totalVP == 0) revert ZeroTotalVotingPower();

        // Fenwick tree build (O(n))
        uint256[] memory bit = new uint256[](n + 1);
        for (uint256 i = 1; i <= n; ) {
            bit[i] = stakes[i - 1];
            unchecked { ++i; }
        }
        for (uint256 i = 1; i <= n; ) {
            uint256 j = i + _lsb(i);
            if (j <= n) bit[j] += bit[i];
            unchecked { ++i; }
        }

        members = new address[](k);

        uint256 remainingVP = totalVP;
        uint256 selectedVP;
        uint32 picked;

        for (; picked < k; ) {
            if (remainingVP == 0) break;

            bytes32 seed = keccak256(abi.encodePacked(
                bh, address(this), heartbeatKey, round, snapshotId, picked
            ));
            uint256 r = uint256(seed) % remainingVP;

            uint256 idx1 = _bitFind(bit, r); // 1-based
            uint256 idx0 = idx1 - 1;

            uint256 w = stakes[idx0];
            if (w == 0) break; // defensive

            members[picked] = pool[idx0];
            selectedVP += w;

            // remove this weight (without replacement)
            stakes[idx0] = 0;
            _bitSub(bit, idx1, w);
            remainingVP -= w;

            unchecked { ++picked; }
        }

        if (picked < k) {
            address[] memory trimmed = new address[](picked);
            for (uint32 i = 0; i < picked; ) {
                trimmed[i] = members[i];
                unchecked { ++i; }
            }
            members = trimmed;
        }

        if (minCommitteeVP != 0 && selectedVP < minCommitteeVP) revert InsufficientCommitteeVP(selectedVP, minCommitteeVP);
        return members;
    }

    function _randomnessSeed(uint64 snapshotId) internal view returns (bytes32) {
        bytes32 bh = blockhash(uint256(snapshotId));
        if (bh == bytes32(0)) bh = bytes32(block.prevrandao);
        if (bh == bytes32(0)) revert SnapshotBlockUnavailable(snapshotId);
        return bh;
    }

    function _topByStake(address[] memory active, uint64 snapshotId, uint32 cap)
        internal
        view
        returns (address[] memory top, uint256[] memory topStakes)
    {
        uint256 n = active.length;
        uint256 k = uint256(cap);

        uint256[] memory heapStake = new uint256[](k);
        address[] memory heapAddr = new address[](k);
        uint256 heapSize;

        for (uint256 i = 0; i < n; ) {
            address op = active[i];
            uint256 s = stakingOps.stakeAt(op, snapshotId);

            if (heapSize < k) {
                heapStake[heapSize] = s;
                heapAddr[heapSize] = op;
                _heapSiftUp(heapStake, heapAddr, heapSize);
                unchecked { ++heapSize; }
            } else {
                if (_isBetter(s, op, heapStake[0], heapAddr[0])) {
                    heapStake[0] = s;
                    heapAddr[0] = op;
                    _heapSiftDown(heapStake, heapAddr, heapSize, 0);
                }
            }
            unchecked { ++i; }
        }

        top = new address[](heapSize);
        topStakes = new uint256[](heapSize);
        for (uint256 i = 0; i < heapSize; ) {
            top[i] = heapAddr[i];
            topStakes[i] = heapStake[i];
            unchecked { ++i; }
        }
    }

    function _isBetter(uint256 sA, address aA, uint256 sB, address aB) internal pure returns (bool) {
        if (sA > sB) return true;
        if (sA < sB) return false;
        return uint160(aA) < uint160(aB);
    }

    function _heapLess(uint256 sA, address aA, uint256 sB, address aB) internal pure returns (bool) {
        if (sA < sB) return true;
        if (sA > sB) return false;
        return uint160(aA) > uint160(aB);
    }

    function _heapSiftUp(uint256[] memory heapStake, address[] memory heapAddr, uint256 idx) internal pure {
        while (idx != 0) {
            uint256 parent = (idx - 1) / 2;
            if (!_heapLess(heapStake[idx], heapAddr[idx], heapStake[parent], heapAddr[parent])) break;
            (heapStake[idx], heapStake[parent]) = (heapStake[parent], heapStake[idx]);
            (heapAddr[idx], heapAddr[parent]) = (heapAddr[parent], heapAddr[idx]);
            idx = parent;
        }
    }

    function _heapSiftDown(uint256[] memory heapStake, address[] memory heapAddr, uint256 heapSize, uint256 idx) internal pure {
        while (true) {
            uint256 left = idx * 2 + 1;
            if (left >= heapSize) break;

            uint256 right = left + 1;
            uint256 smallest = left;
            if (right < heapSize && _heapLess(heapStake[right], heapAddr[right], heapStake[left], heapAddr[left])) {
                smallest = right;
            }
            if (!_heapLess(heapStake[smallest], heapAddr[smallest], heapStake[idx], heapAddr[idx])) break;

            (heapStake[idx], heapStake[smallest]) = (heapStake[smallest], heapStake[idx]);
            (heapAddr[idx], heapAddr[smallest]) = (heapAddr[smallest], heapAddr[idx]);
            idx = smallest;
        }
    }

    // Fenwick helpers
    function _lsb(uint256 x) internal pure returns (uint256) {
        return x & (~x + 1);
    }

    function _bitSub(uint256[] memory bit, uint256 idx, uint256 delta) internal pure {
        uint256 n = bit.length - 1;
        while (idx <= n) {
            bit[idx] -= delta;
            idx += _lsb(idx);
        }
    }

    // Finds smallest idx such that prefixSum(idx) > r (1-based)
    function _bitFind(uint256[] memory bit, uint256 r) internal pure returns (uint256) {
        uint256 n = bit.length - 1;
        uint256 idx = 0;
        uint256 bitMask = 1;
        while (bitMask <= n) bitMask <<= 1;

        for (uint256 step = bitMask; step != 0; step >>= 1) {
            uint256 next = idx + step;
            if (next <= n && bit[next] <= r) {
                idx = next;
                r -= bit[next];
            }
        }
        return idx + 1;
    }
}
