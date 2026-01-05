// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

import "../src/mocks/MockERC20.sol";
import "../src/StakingOperators.sol";
import "../src/WeightedCommitteeSelector.sol";

contract WeightedCommitteeSelectorTest is Test {
    MockERC20 stakeToken;
    StakingOperators stakingOps;
    WeightedCommitteeSelector selector;

    address admin = address(0xA11CE);

    function setUp() public {
        stakeToken = new MockERC20("STAKE", "STK");

        vm.startPrank(admin);
        stakingOps = new StakingOperators(IERC20(address(stakeToken)), admin, 1 days);
        selector = new WeightedCommitteeSelector(stakingOps, admin, 0, 1000);
        stakingOps.setSnapshotter(address(this));
        vm.stopPrank();
    }

    function _makeOperators(uint256 n, uint256 baseStake) internal returns (address[] memory ops) {
        ops = new address[](n);
        for (uint256 i = 0; i < n; i++) {
            address op = address(uint160(uint256(keccak256(abi.encodePacked("op", i + 1)))));
            ops[i] = op;
            stakeToken.mint(op, baseStake);
            vm.startPrank(op);
            stakeToken.approve(address(stakingOps), type(uint256).max);
            stakingOps.stakeTo(op, baseStake);
            stakingOps.registerOperator("ipfs://x");
            vm.stopPrank();
        }
    }

    function test_selectCommittee_unique_and_bounded() public {
        _makeOperators(20, 2e18);

        vm.roll(block.number + 1);
        uint64 snap = stakingOps.snapshot();

        address[] memory members = selector.selectCommittee(bytes32("hbKey"), 1, 10, snap);

        assertLe(members.length, 10);
        // uniqueness
        for (uint256 i = 0; i < members.length; i++) {
            for (uint256 j = i + 1; j < members.length; j++) {
                assertTrue(members[i] != members[j], "duplicate member");
            }
            assertTrue(stakingOps.isActiveOperator(members[i]), "inactive selected");
        }
    }

    function test_selectCommittee_respects_maxActiveOperators_pool() public {
        // 20 ops with increasing stake, cap pool to top 8
        address[] memory ops = new address[](20);
        for (uint256 i = 0; i < 20; i++) {
            address op = address(uint160(uint256(keccak256(abi.encodePacked("op", i + 1)))));
            ops[i] = op;
            uint256 stake = (i + 1) * 1e18;
            stakeToken.mint(op, stake);
            vm.startPrank(op);
            stakeToken.approve(address(stakingOps), type(uint256).max);
            stakingOps.stakeTo(op, stake);
            stakingOps.registerOperator("ipfs://x");
            vm.stopPrank();
        }

        vm.prank(admin);
        selector.setMaxActiveOperators(8);

        vm.roll(block.number + 1);
        uint64 snap = stakingOps.snapshot();

        address[] memory members = selector.selectCommittee(bytes32("hbKey"), 1, 8, snap);

        // Determine the 8 highest-stake operators
        bool[20] memory isTop;
        // Top are ops[12..19] because stake increases linearly
        for (uint256 i = 12; i < 20; i++) isTop[i] = true;

        for (uint256 i = 0; i < members.length; i++) {
            bool found;
            for (uint256 j = 0; j < 20; j++) {
                if (ops[j] == members[i]) {
                    found = true;
                    assertTrue(isTop[j], "selected outside top pool");
                    break;
                }
            }
            assertTrue(found, "selected unknown");
        }
    }

    function test_largeCommitteeSelection() public {
        _makeOperators(220, 1e18);

        vm.roll(block.number + 1);
        uint64 snap = stakingOps.snapshot();

        address[] memory members = selector.selectCommittee(bytes32("hbKey"), 1, 150, snap);
        assertEq(members.length, 150);

        // uniqueness check
        for (uint256 i = 0; i < members.length; i++) {
            for (uint256 j = i + 1; j < members.length; j++) {
                assertTrue(members[i] != members[j], "duplicate member");
            }
        }
    }

    function test_selectCommittee_fallsBackToPrevrandao() public {
        _makeOperators(5, 1e18);

        vm.roll(block.number + 1);
        uint64 snap = stakingOps.snapshot();

        // Move far enough that the snapshot blockhash is unavailable
        vm.roll(block.number + 300);
        assertEq(blockhash(uint256(snap)), bytes32(0));
        vm.prevrandao(bytes32(uint256(1234)));

        address[] memory members = selector.selectCommittee(bytes32("hbKey"), 1, 3, snap);
        assertEq(members.length, 3);
        for (uint256 i = 0; i < members.length; i++) {
            assertTrue(stakingOps.isActiveOperator(members[i]), "inactive selected");
        }
    }

    function test_selectCommittee_capsToMaxActiveOperators_whenPoolIsHuge() public {
        // 1,200 operators with increasing stake (op i has stake i+1).
        address[] memory ops = new address[](1200);
        for (uint256 i = 0; i < ops.length; i++) {
            address op = address(uint160(uint256(keccak256(abi.encodePacked("op", i + 1)))));
            ops[i] = op;
            uint256 stake = (i + 1) * 1e18;
            stakeToken.mint(op, stake);
            vm.startPrank(op);
            stakeToken.approve(address(stakingOps), type(uint256).max);
            stakingOps.stakeTo(op, stake);
            stakingOps.registerOperator("ipfs://x");
            vm.stopPrank();
        }

        // Leave default maxActiveOperators (1000) and request an oversized committee.
        vm.roll(block.number + 1);
        uint64 snap = stakingOps.snapshot();

        address[] memory members = selector.selectCommittee(bytes32("hbKey"), 1, 1100, snap);
        // Should cap at 1000 from maxActiveOperators and maxCommitteeSize
        assertEq(members.length, 1000);

        // Only the top 1000 stakers (highest indexes) should be eligible.
        bool[1200] memory isTop;
        for (uint256 i = 200; i < 1200; i++) isTop[i] = true; // indexes 200..1199 are the 1000 highest stakes

        for (uint256 i = 0; i < members.length; i++) {
            bool found;
            for (uint256 j = 0; j < ops.length; j++) {
                if (members[i] == ops[j]) {
                    found = true;
                    assertTrue(isTop[j], "selected below top-1000 pool");
                    break;
                }
            }
            assertTrue(found, "unknown member returned");
        }
    }
}
