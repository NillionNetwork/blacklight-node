// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "./Interfaces.sol";

/// @title WeightedCommitteeSelector
/// @notice Weighted committee selection based on operator stake (VP).
contract WeightedCommitteeSelector is ICommitteeSelector {
    error ZeroAddress();
    error ZeroMaxSize();
    error NoOperators();
    error NotAdmin();
    error EmptyCommitteeRequested();

    IStakingOperators public immutable stakingOps;
    address public immutable admin;

    uint256 public minCommitteeVP;
    uint32 public maxCommitteeSize;

    event MinCommitteeVPUpdated(uint256 oldVP, uint256 newVP);
    event MaxCommitteeSizeUpdated(uint32 oldSize, uint32 newSize);

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
        if (_maxCommitteeSize == 0) revert ZeroMaxSize();

        stakingOps = _stakingOps;
        admin = _admin;
        minCommitteeVP = _minCommitteeVP;
        maxCommitteeSize = _maxCommitteeSize;
    }

    function setMinCommitteeVP(uint256 newVP) external onlyAdmin {
        emit MinCommitteeVPUpdated(minCommitteeVP, newVP);
        minCommitteeVP = newVP;
    }

    function setMaxCommitteeSize(uint32 newSize) external onlyAdmin {
        if (newSize == 0) revert ZeroMaxSize();
        emit MaxCommitteeSizeUpdated(maxCommitteeSize, newSize);
        maxCommitteeSize = newSize;
    }

    function selectCommittee(
        bytes32 workloadKey,
        uint8 round,
        uint32 committeeSize
    ) external view override returns (address[] memory members) {
        address[] memory active = stakingOps.getActiveOperators();
        uint256 n = active.length;
        if (n == 0) revert NoOperators();

        uint32 maxSize = committeeSize;
        if (maxSize == 0) revert EmptyCommitteeRequested();
        if (maxSize > maxCommitteeSize) {
            maxSize = maxCommitteeSize;
        }
        if (maxSize > n) {
            maxSize = uint32(n);
        }

        uint256[] memory stakes = new uint256[](n);
        uint256 totalVP;
        for (uint256 i = 0; i < n; ) {
            uint256 s = stakingOps.stakeOf(active[i]);
            stakes[i] = s;
            totalVP += s;
            unchecked { ++i; }
        }

        // Fallback: if no stake, just random sample without weights.
        if (totalVP == 0) {
            members = new address[](maxSize);
            bool[] memory chosen = new bool[](n);
            uint32 count;
            uint256 saltNoStake;
            while (count < maxSize && count < n) {
                bytes32 seed = keccak256(abi.encodePacked(workloadKey, round, saltNoStake));
                uint256 idx = uint256(seed) % n;
                if (!chosen[idx]) {
                    chosen[idx] = true;
                    members[count] = active[idx];
                    unchecked { ++count; }
                }
                unchecked { ++saltNoStake; }
            }
            return members;
        }

        members = new address[](maxSize);
        bool[] memory selected = new bool[](n);
        uint32 selectedCount;
        uint256 selectedVP;
        uint256 remainingVP = totalVP;
        uint256 salt;

        while (selectedCount < maxSize && selectedCount < n && (minCommitteeVP == 0 || selectedVP < minCommitteeVP)) {
            bytes32 seed = keccak256(abi.encodePacked(workloadKey, round, salt));
            uint256 r = uint256(seed) % remainingVP;

            uint256 cumulative;
            uint256 chosenIndex;
            for (uint256 i = 0; i < n; ) {
                if (!selected[i]) {
                    uint256 s = stakes[i];
                    if (s != 0) {
                        cumulative += s;
                        if (r < cumulative) {
                            chosenIndex = i;
                            break;
                        }
                    }
                }
                unchecked { ++i; }
            }

            if (selected[chosenIndex] || stakes[chosenIndex] == 0) {
                bool found;
                for (uint256 i = 0; i < n; ) {
                    if (!selected[i] && stakes[i] > 0) {
                        chosenIndex = i;
                        found = true;
                        break;
                    }
                    unchecked { ++i; }
                }
                if (!found) {
                    break;
                }
            }

            selected[chosenIndex] = true;
            members[selectedCount] = active[chosenIndex];
            selectedVP += stakes[chosenIndex];
            remainingVP -= stakes[chosenIndex];
            unchecked {
                ++selectedCount;
                ++salt;
            }
        }

        if (selectedCount < maxSize) {
            address[] memory trimmed = new address[](selectedCount);
            for (uint32 i = 0; i < selectedCount; ) {
                trimmed[i] = members[i];
                unchecked { ++i; }
            }
            members = trimmed;
        }

        return members;
    }
}