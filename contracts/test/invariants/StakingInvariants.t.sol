// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "forge-std/Test.sol";
import "forge-std/StdInvariant.sol";

import "../../src/mocks/MockERC20.sol";
import "../../src/ProtocolConfig.sol";
import "../../src/StakingOperators.sol";

contract StakingHandler is Test {
    MockERC20 public token;
    StakingOperators public stakingOps;
    ProtocolConfig public config;

    address public admin;

    address[] public operators;
    mapping(address => bool) public registered;

    constructor(MockERC20 _token, StakingOperators _stakingOps, ProtocolConfig _config, address _admin, address[] memory _ops) {
        token = _token;
        stakingOps = _stakingOps;
        config = _config;
        admin = _admin;
        operators = _ops;

        // Pre-approve staking for each operator (operator is staker)
        for (uint256 i = 0; i < operators.length; i++) {
            vm.startPrank(operators[i]);
            token.approve(address(stakingOps), type(uint256).max);
            vm.stopPrank();
        }
    }

    function _op(uint256 idx) internal view returns (address) {
        return operators[idx % operators.length];
    }

    function warp(uint256 secs) external {
        uint256 dt = bound(secs, 0, 14 days);
        vm.warp(block.timestamp + dt);
        vm.roll(block.number + 1);
    }

    function stake(uint256 idx, uint256 amount) external {
        address op = _op(idx);
        uint256 bal = token.balanceOf(op);
        if (bal == 0) return;
        uint256 a = bound(amount, 0, bal);
        if (a == 0) return;

        vm.prank(op);
        stakingOps.stakeTo(op, a);
    }

    function register(uint256 idx) external {
        address op = _op(idx);
        uint256 minStake = config.minOperatorStake();
        if (stakingOps.stakeOf(op) < minStake) return;

        vm.prank(op);
        stakingOps.registerOperator("ipfs://x");
        registered[op] = true;
    }

    function deactivate(uint256 idx) external {
        address op = _op(idx);
        IStakingOperators.OperatorInfo memory info = stakingOps.getOperatorInfo(op);
        if (!info.active) return;

        vm.prank(op);
        stakingOps.deactivateOperator();
    }

    function reactivate(uint256 idx) external {
        address op = _op(idx);
        if (!registered[op]) return;
        if (stakingOps.isJailed(op)) return;
        uint256 minStake = config.minOperatorStake();
        if (stakingOps.stakeOf(op) < minStake) return;

        IStakingOperators.OperatorInfo memory info = stakingOps.getOperatorInfo(op);
        if (info.active) return;

        vm.prank(op);
        stakingOps.reactivateOperator();
    }

    function requestUnstake(uint256 idx, uint256 amount) external {
        address op = _op(idx);
        if (stakingOps.isJailed(op)) return;

        uint256 bal = stakingOps.stakeOf(op);
        if (bal == 0) return;

        uint256 a = bound(amount, 0, bal);
        if (a == 0) return;

        vm.prank(op);
        stakingOps.requestUnstake(op, a);
    }

    function withdraw(uint256 idx) external {
        address op = _op(idx);

        // only attempt if there is at least one matured tranche
        IStakingOperators.Tranche[] memory tr = stakingOps.getUnbondingTranches(op);
        if (tr.length == 0) return;

        bool ready;
        for (uint256 i = 0; i < tr.length; i++) {
            if (block.timestamp >= tr[i].releaseTime) { ready = true; break; }
        }
        if (!ready) return;

        vm.prank(op);
        stakingOps.withdrawUnstaked(op);
    }

    function jail(uint256 idx, uint256 duration) external {
        address op = _op(idx);
        uint64 until = uint64(block.timestamp + bound(duration, 0, 14 days));
        stakingOps.jail(op, until);
    }

    function slash(uint256 idx, uint256 amount) external {
        address op = _op(idx);
        uint256 a = bound(amount, 0, 20e18);
        stakingOps.slash(op, a);
    }

    function poke(uint256 idx) external {
        address op = _op(idx);
        stakingOps.pokeActive(op);
    }
}

contract StakingInvariants is StdInvariant, Test {
    MockERC20 token;
    ProtocolConfig config;
    StakingOperators stakingOps;
    StakingHandler handler;

    address admin = address(0xA11CE);
    address[] ops;

    function setUp() public {
        token = new MockERC20("STAKE", "STK");

        vm.startPrank(admin);
        stakingOps = new StakingOperators(IERC20(address(token)), admin, 1 days);
        vm.stopPrank();

        config = new ProtocolConfig(
            address(this),
            address(stakingOps),
            address(this),
            address(this),
            address(this),
            2,
            0,
            10,
            0,
            1,
            1,
            10,
            10,
            100,
            1e18,
            1e18,
            0
        );

        vm.prank(admin);
        stakingOps.setProtocolConfig(config);

        // Create operators with initial balances
        uint256 n = 20;
        ops = new address[](n);
        for (uint256 i = 0; i < n; i++) {
            address op = address(uint160(uint256(keccak256(abi.encodePacked("op", i + 1)))));
            ops[i] = op;
            token.mint(op, 100e18);
        }

        handler = new StakingHandler(token, stakingOps, config, admin, ops);
        vm.startPrank(admin);
        stakingOps.grantRole(stakingOps.SLASHER_ROLE(), address(handler));
        vm.stopPrank();
        targetContract(address(handler));
    }

    function invariant_totalStakedMatchesTokenBalance() public {
        assertEq(token.balanceOf(address(stakingOps)), stakingOps.totalStaked());
    }

    function invariant_totalStakedEqualsSumOfActiveAndUnbondingAcrossTrackedOperators() public {
        uint256 sum;
        for (uint256 i = 0; i < ops.length; i++) {
            address op = ops[i];
            sum += stakingOps.stakeOf(op);
            IStakingOperators.Tranche[] memory tr = stakingOps.getUnbondingTranches(op);
            for (uint256 j = 0; j < tr.length; j++) sum += tr[j].amount;
        }
        assertEq(sum, stakingOps.totalStaked());
    }

    function invariant_activeSetHasNoDuplicatesAndIsConsistent() public {
        address[] memory active = stakingOps.getActiveOperators();
        // no duplicates
        for (uint256 i = 0; i < active.length; i++) {
            assertTrue(stakingOps.isActiveOperator(active[i]), "active set contains inactive");
            for (uint256 j = i + 1; j < active.length; j++) {
                assertTrue(active[i] != active[j], "duplicate in active set");
            }
        }
    }

    function invariant_operatorStakerZeroImpliesNoStakeAndNoUnbonding() public {
        for (uint256 i = 0; i < ops.length; i++) {
            address op = ops[i];
            address staker = stakingOps.operatorStaker(op);
            if (staker == address(0)) {
                assertEq(stakingOps.stakeOf(op), 0);
                assertEq(stakingOps.getUnbondingTranches(op).length, 0);
            }
        }
    }
}
