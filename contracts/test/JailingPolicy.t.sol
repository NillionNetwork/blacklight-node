// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./helpers/RCFixture.sol";

contract JailingPolicyTest is RCFixture {
    function setUp() public {
        uint256[] memory stakes = new uint256[](12);
        for (uint256 i = 0; i < stakes.length; i++) stakes[i] = 2e18;
        _deploySystem(
            12,
            stakes,
            10,   // baseCommitteeSize
            10,   // maxCommitteeSize
            5000, // quorumBps
            5000, // verificationBps
            1 days,
            7 days,
            1
        );
    }

    function test_roundIsRecordedOnFinalize() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // finalize valid threshold
        for (uint256 i = 0; i < 5; i++) _vote(wk, round, members, members[i], 1);

        (bool set, ISlashingPolicy.Outcome outcome, bytes32 root, address stakingAddr, uint64 jailDur, uint32 committeeSize) =
            jailingPolicy.roundRecord(wk, round);

        assertTrue(set);
        assertEq(uint8(outcome), uint8(ISlashingPolicy.Outcome.ValidThreshold));
        assertTrue(root != bytes32(0));
        assertEq(stakingAddr, address(stakingOps));
        assertEq(jailDur, uint64(7 days));
        assertEq(committeeSize, 10);
    }

    function test_enforceJailFromMembers_jailsNonvoters_and_incorrectVoters() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // 4 valid votes, 1 invalid, then a final valid vote to reach threshold.
        for (uint256 i = 0; i < 4; i++) _vote(wk, round, members, members[i], 1);
        _vote(wk, round, members, members[5], 2); // incorrect voter
        _vote(wk, round, members, members[4], 1); // pushes valid stake to threshold

        // should be finalized valid threshold
        assertEq(uint8(manager.roundOutcome(wk, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

        // enforce jailing for entire committee
        jailingPolicy.enforceJailFromMembers(wk, round, members);

        // incorrect voter jailed
        assertTrue(stakingOps.isJailed(members[5]));
        assertTrue(jailingPolicy.enforced(wk, round, members[5]));

        // nonvoters jailed
        for (uint256 i = 6; i < members.length; i++) {
            assertTrue(stakingOps.isJailed(members[i]), "nonvoter not jailed");
            assertTrue(jailingPolicy.enforced(wk, round, members[i]));
        }

        // valid voters not jailed
        for (uint256 i = 0; i < 5; i++) {
            assertFalse(stakingOps.isJailed(members[i]), "valid voter jailed");
            assertFalse(jailingPolicy.enforced(wk, round, members[i]));
        }
    }

    function test_enforceJailFromMembers_revertsOnRootMismatchOrUnsorted() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(wk, round, members, members[i], 1);

        // unsorted list
        address[] memory unsorted = new address[](members.length);
        for (uint256 i = 0; i < members.length; i++) unsorted[i] = members[members.length - 1 - i];

        vm.expectRevert(JailingPolicy.UnsortedMembers.selector);
        jailingPolicy.enforceJailFromMembers(wk, round, unsorted);

        // root mismatch
        address[] memory wrong = new address[](members.length);
        for (uint256 i = 0; i < members.length; i++) wrong[i] = members[i];
        uint160 first = uint160(members[0]);
        address smaller = address(first > 1 ? first - 1 : 1);
        wrong[0] = smaller;

        vm.expectRevert(JailingPolicy.CommitteeRootMismatch.selector);
        jailingPolicy.enforceJailFromMembers(wk, round, wrong);
    }

    function test_enforceJail_individualWithProof() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // only 1 vote => no quorum -> inconclusive after deadline
        _vote(wk, round, members, members[0], 1);

        (, , , , , , , , , , uint64 deadline, , , , , , , , , ) = manager.rounds(wk, round);
        vm.warp(uint256(deadline) + 1);
        manager.escalateOrExpire(wk, _defaultRawHTX(1));

        assertEq(uint8(manager.roundOutcome(wk, round)), uint8(ISlashingPolicy.Outcome.Inconclusive));

        // pick nonvoter member[1]
        address target = members[1];
        bytes32[] memory proof = _proofForMember(wk, round, members, target);

        jailingPolicy.enforceJail(wk, round, target, proof);
        assertTrue(stakingOps.isJailed(target));

        vm.expectRevert(JailingPolicy.AlreadyEnforced.selector);
        jailingPolicy.enforceJail(wk, round, target, proof);

        // responded voter should NOT be jailable in inconclusive
        bytes32[] memory proof0 = _proofForMember(wk, round, members, members[0]);
        vm.expectRevert(JailingPolicy.NotJailable.selector);
        jailingPolicy.enforceJail(wk, round, members[0], proof0);
    }
}
