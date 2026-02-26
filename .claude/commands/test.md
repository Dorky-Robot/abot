Run the test suite for abot and report results.

## Instructions

Run abot's Rust test suite and report results clearly.

### Step 1: Run cargo test

```bash
cargo test 2>&1
```

### Step 2: Analyze results

Parse the test output:
- Count passing tests, failing tests, and ignored tests
- For any failures, identify the test name, file location, and failure reason
- Check for compilation errors vs runtime test failures

### Step 3: Report

If all tests pass, report:
```
All N tests passed.
```

If any tests fail, for each failure:
1. Read the source file containing the failing test
2. Analyze the failure reason (assertion mismatch, panic, timeout, etc.)
3. Suggest a fix if the cause is clear

### Step 4: Build check (if tests pass)

If all tests pass, also run a release build check to catch warnings:

```bash
cargo check 2>&1
```

Report any compiler warnings that should be addressed.
