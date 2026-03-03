Run the test suite for abot and report results.

## Instructions

Run abot's test suites and report results clearly. $ARGUMENTS can be `rust`, `flutter`, `e2e`, or `all` (default: `rust`).

### Step 1: Run cargo test

```bash
cargo test 2>&1
```

### Step 2: Analyze Rust results

Parse the test output:
- Count passing tests, failing tests, and ignored tests
- For any failures, identify the test name, file location, and failure reason
- Check for compilation errors vs runtime test failures

### Step 3: Report Rust results

If all tests pass, report:
```
Rust: All N tests passed.
```

If any tests fail, for each failure:
1. Read the source file containing the failing test
2. Analyze the failure reason (assertion mismatch, panic, timeout, etc.)
3. Suggest a fix if the cause is clear

### Step 4: Build check (if Rust tests pass)

If all Rust tests pass, also run a release build check to catch warnings:

```bash
cargo check 2>&1
```

Report any compiler warnings that should be addressed.

### Step 5: Flutter tests (if requested)

If `$ARGUMENTS` is `flutter` or `all`, run the Flutter tests:

```bash
cd flutter_client && flutter test 2>&1
```

Also run Dart static analysis:

```bash
cd flutter_client && dart analyze 2>&1
```

Parse the output:
- Count passing and failing tests
- For failures, identify the test name, file, and failure reason
- For analysis warnings/errors, list them with file and line

Report:
```
Flutter: N tests passed, M failed.
Dart analyze: N issues (E errors, W warnings, I infos).
```

### Step 6: E2E tests (if requested)

If `$ARGUMENTS` is `e2e` or `all`, run the Playwright e2e tests:

```bash
npx playwright test 2>&1
```

Parse the Playwright output:
- Count passing, failing, and skipped tests
- For failures, identify the test name, file, and failure reason (assertion, timeout, element not found, etc.)

Report:
```
E2E: N passed, M failed, K skipped.
```

**Note**: E2E tests require a running abot server (`cargo run -- start`). If the server is not running, tell the user to start it first.

### Step 7: Summary

Print a final summary:
```
## Test Results for abot
- Rust: N passed, M failed, K ignored
- Flutter: N passed, M failed (if run)
- Dart analyze: clean / N issues (if run)
- E2E: N passed, M failed, K skipped (if run)
- Build check: clean / N warnings
```
