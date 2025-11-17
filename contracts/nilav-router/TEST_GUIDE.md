# NilAVRouter Testing Guide with Foundry

Complete guide for running the comprehensive test suite for NilAVRouter.

## Quick Start

```bash
# Install dependencies (forge-std)
forge install foundry-rs/forge-std

# Run all tests
forge test

# Run with verbose output
forge test -vv

# Run with detailed traces
forge test -vvvv
```

## Test File Overview

**NilAVRouter.t.sol** contains comprehensive tests organized into categories:

### Test Categories

1. **Node Registration Tests** (7 tests)
   - Basic registration
   - Multiple node registration
   - Zero address rejection
   - Duplicate prevention
   - Event emission

2. **Node Deregistration Tests** (5 tests)
   - Basic deregistration
   - Deregistration from multiple nodes
   - Unregistered node rejection
   - Event emission

3. **HTX Submission Tests** (8 tests)
   - Basic submission
   - Multi-node assignment
   - No nodes error
   - Duplicate prevention
   - Event emission
   - Deterministic ID generation
   - Different sender handling

4. **HTX Response Tests** (6 tests)
   - True/false responses
   - Unknown HTX rejection
   - Non-assigned node rejection
   - Double response prevention
   - Event emission

5. **View Function Tests** (3 tests)
   - Assignment retrieval
   - Node list retrieval
   - Index-based access

6. **Complex Workflow Tests** (2 tests)
   - Complete end-to-end workflow
   - Multiple HTX submissions

7. **Fuzz Tests** (3 tests)
   - Random address registration
   - Random HTX data
   - Random response values

8. **Edge Case Tests** (5 tests)
   - Empty HTX data
   - Large HTX data
   - Last node deregistration
   - Re-registration after deregistration

## Running Tests

### Basic Test Execution

```bash
# Run all tests
forge test

# Expected output:
# [PASS] testRegisterNode() (gas: ...)
# [PASS] testSubmitHTX() (gas: ...)
# ...
# Test result: ok. XX passed; 0 failed; finished in Xs
```

### Verbose Output Levels

```bash
# Level 1: Show test names
forge test -v

# Level 2: Show test names and logs
forge test -vv

# Level 3: Show test names, logs, and stack traces for failing tests
forge test -vvv

# Level 4: Show test names, logs, stack traces, and setup traces
forge test -vvvv

# Level 5: Show everything including storage changes
forge test -vvvvv
```

### Run Specific Tests

```bash
# Run tests matching a pattern
forge test --match-test testRegisterNode

# Run tests in a specific contract
forge test --match-contract NilAVRouterTest

# Run tests matching a path
forge test --match-path src/smart_contract/solidity/NilAVRouter.t.sol
```

### Filter by Test Category

```bash
# Run only node registration tests
forge test --match-test "testRegister"

# Run only HTX submission tests
forge test --match-test "testSubmitHTX"

# Run only response tests
forge test --match-test "testRespondHTX"

# Run only fuzz tests
forge test --match-test "testFuzz"
```

## Gas Reporting

```bash
# Generate gas report for all tests
forge test --gas-report

# Expected output:
# ╭───────────────────┬─────────────────┬───────┬────────┬───────┬─────────╮
# │ Contract          ┆ Function        ┆ min   ┆ avg    ┆ max   ┆ calls   │
# ├───────────────────┼─────────────────┼───────┼────────┼───────┼─────────┤
# │ NilAVRouter       ┆ registerNode    ┆ ...   ┆ ...    ┆ ...   ┆ ...     │
# │ NilAVRouter       ┆ submitHTX       ┆ ...   ┆ ...    ┆ ...   ┆ ...     │
# ...

# Save gas report to file
forge test --gas-report > gas-report.txt
```

## Coverage Analysis

```bash
# Generate coverage report
forge coverage

# Generate detailed coverage report
forge coverage --report lcov

# View coverage in terminal
forge coverage --report summary

# Expected output showing coverage percentages for each function
```

## Fuzz Testing

The test suite includes fuzz tests that automatically generate random inputs:

```bash
# Run only fuzz tests
forge test --match-test testFuzz

# Increase number of fuzz runs (default is 256)
forge test --match-test testFuzz --fuzz-runs 10000

# Set fuzz seed for reproducible tests
forge test --match-test testFuzz --fuzz-seed 12345
```

### Fuzz Test Details

- **testFuzzRegisterNode**: Tests registration with random addresses
- **testFuzzSubmitHTX**: Tests submission with random HTX data
- **testFuzzRespondHTX**: Tests responses with random boolean values

## Debugging Failed Tests

If a test fails, use verbose output to see what went wrong:

```bash
# Run with maximum verbosity
forge test --match-test testFailingTest -vvvvv

# This will show:
# - Setup state
# - Function calls
# - State changes
# - Revert reasons
# - Gas usage
```

## Test Configuration

Edit `foundry.toml` to customize test behavior:

```toml
[profile.default]
src = "."
out = "out"
libs = ["lib"]
verbosity = 2
fuzz_runs = 256

[profile.ci]
fuzz_runs = 10000
verbosity = 3
```

Run with specific profile:
```bash
forge test --profile ci
```

## Continuous Integration

Example GitHub Actions workflow:

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          submodules: recursive

      - name: Install Foundry
        uses: foundry-rs/foundry-toolchain@v1

      - name: Install dependencies
        run: forge install

      - name: Run tests
        run: forge test -vvv

      - name: Run coverage
        run: forge coverage --report summary
```

## Test Results Interpretation

### Passing Tests
```
[PASS] testRegisterNode() (gas: 85234)
```
- ✅ Test passed
- Gas usage: 85,234 units

### Failing Tests
```
[FAIL. Reason: NilAV: zero address] testCannotRegisterZeroAddress()
```
- ❌ Test failed
- Reason: Expected revert message matched

### Skipped Tests
```
[SKIP] testExpensiveOperation()
```
- ⏭️ Test skipped (if marked with `skip` modifier)

## Common Test Patterns

### Expecting Reverts
```solidity
vm.expectRevert("Error message");
contract.functionThatReverts();
```

### Testing Events
```solidity
vm.expectEmit(true, true, false, true);
emit EventName(param1, param2);
contract.functionThatEmits();
```

### Pranking (Changing msg.sender)
```solidity
vm.prank(address(0x123));
contract.functionAsUser();
```

### Time Manipulation
```solidity
vm.warp(block.timestamp + 1 days);
vm.roll(block.number + 100);
```

## Snapshot Testing

Create gas snapshots to track gas usage changes:

```bash
# Create snapshot
forge snapshot

# Compare with snapshot
forge snapshot --diff

# Update snapshot
forge snapshot --check
```

## Interactive Testing

Use Forge's interactive debugger:

```bash
# Run test in debug mode
forge test --match-test testRegisterNode --debug

# This opens an interactive debugger where you can:
# - Step through execution
# - Inspect state
# - View memory/storage
# - Analyze gas usage
```

## Performance Benchmarks

Run performance benchmarks:

```bash
# Run with gas reporting
forge test --gas-report

# Compare gas usage between versions
git checkout main
forge snapshot --snap main.snap
git checkout feature-branch
forge snapshot --diff main.snap
```

## Test Statistics

After running all tests, you should see:

```
Ran 41 tests for src/smart_contract/solidity/NilAVRouter.t.sol:NilAVRouterTest
[PASS] testCannotDeregisterUnregisteredNode() (gas: ...)
[PASS] testCannotRegisterDuplicateNode() (gas: ...)
[PASS] testCannotRegisterZeroAddress() (gas: ...)
[PASS] testCannotRespondIfNotAssignedNode() (gas: ...)
[PASS] testCannotRespondToUnknownHTX() (gas: ...)
[PASS] testCannotRespondTwice() (gas: ...)
[PASS] testCannotSubmitHTXWithNoNodes() (gas: ...)
[PASS] testCannotSubmitDuplicateHTX() (gas: ...)
[PASS] testCompleteWorkflow() (gas: ...)
[PASS] testDeregisterLastNode() (gas: ...)
[PASS] testDeregisterNode() (gas: ...)
[PASS] testDeregisterNodeEmitsEvent() (gas: ...)
[PASS] testDeregisterNodeFromMultiple() (gas: ...)
[PASS] testEmptyHTXData() (gas: ...)
[PASS] testFuzzRegisterNode(...) (runs: 256, μ: ..., ~: ...)
[PASS] testFuzzRespondHTX(...) (runs: 256, μ: ..., ~: ...)
[PASS] testFuzzSubmitHTX(...) (runs: 256, μ: ..., ~: ...)
[PASS] testGetAssignment() (gas: ...)
[PASS] testGetNodesReturnsCorrectList() (gas: ...)
[PASS] testHTXIDIsDeterministic() (gas: ...)
[PASS] testLargeHTXData() (gas: ...)
[PASS] testMultipleHTXSubmissions() (gas: ...)
[PASS] testNodeAtIndex() (gas: ...)
[PASS] testRegisterAfterDeregister() (gas: ...)
[PASS] testRegisterMultipleNodes() (gas: ...)
[PASS] testRegisterNode() (gas: ...)
[PASS] testRegisterNodeEmitsEvent() (gas: ...)
[PASS] testRespondHTXEmitsEvent() (gas: ...)
[PASS] testRespondHTXFalse() (gas: ...)
[PASS] testRespondHTXTrue() (gas: ...)
[PASS] testSubmitHTXEmitsEvents() (gas: ...)
[PASS] testSubmitHTXFromDifferentSenders() (gas: ...)
[PASS] testSubmitHTX() (gas: ...)
[PASS] testSubmitHTXWithMultipleNodes() (gas: ...)

Test result: ok. 41 passed; 0 failed; 0 ignored; finished in XXms
```

## Troubleshooting

### forge-std Not Found
```bash
# Install forge-std library
forge install foundry-rs/forge-std --no-commit
```

### Test Compilation Errors
```bash
# Clean and rebuild
forge clean
forge build
forge test
```

### Outdated Foundry
```bash
# Update Foundry
foundryup
```

## Additional Resources

- [Foundry Book - Testing](https://book.getfoundry.sh/forge/tests)
- [Foundry Book - Cheatcodes](https://book.getfoundry.sh/cheatcodes/)
- [Foundry Book - Invariant Testing](https://book.getfoundry.sh/forge/invariant-testing)
- [Forge Std Library](https://github.com/foundry-rs/forge-std)

## Next Steps

After running tests successfully:

1. **Generate Gas Report**: `forge test --gas-report` to optimize gas usage
2. **Create Snapshots**: `forge snapshot` to track gas changes over time
3. **Run Coverage**: `forge coverage` to ensure all code paths are tested
4. **Add More Tests**: Extend the test suite for additional edge cases
5. **Set up CI/CD**: Add tests to your continuous integration pipeline
