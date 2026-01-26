# CI Configuration Examples for Headless Testing

This document provides example CI configurations for running flui-platform tests in headless mode.

## Overview

The `FLUI_HEADLESS=1` environment variable forces `current_platform()` to return `HeadlessPlatform`, enabling tests to run without a display server, GPU, or OS windowing system.

**Benefits:**
- ✅ Fast execution (<100ms overhead)
- ✅ No display server required
- ✅ Works in Docker containers
- ✅ Parallel test execution safe
- ✅ Cross-platform CI support

## GitHub Actions

### Basic Configuration

```yaml
name: CI

on:
  push:
    branches: [main, dev]
  pull_request:
    branches: [main, dev]

jobs:
  test:
    name: Test flui-platform
    runs-on: ubuntu-latest
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Cache dependencies
        uses: Swatinem/rust-cache@v2
      
      - name: Run tests (headless)
        run: cargo test -p flui-platform
        env:
          FLUI_HEADLESS: 1
      
      - name: Run all workspace tests (headless)
        run: cargo test --workspace
        env:
          FLUI_HEADLESS: 1
```

### Multi-Platform Matrix

```yaml
name: Cross-Platform CI

on: [push, pull_request]

jobs:
  test:
    name: Test on ${{ matrix.os }}
    runs-on: ${{ matrix.os }}
    
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Run tests (headless on Linux/macOS)
        if: runner.os != 'Windows'
        run: cargo test -p flui-platform
        env:
          FLUI_HEADLESS: 1
      
      - name: Run tests (Windows native)
        if: runner.os == 'Windows'
        run: cargo test -p flui-platform
```

### Performance Monitoring

```yaml
name: CI with Performance Tracking

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Run tests with timing
        run: |
          START=$(date +%s)
          cargo test -p flui-platform -- --test-threads=1
          END=$(date +%s)
          DURATION=$((END - START))
          echo "Test suite completed in ${DURATION}s"
          if [ $DURATION -gt 30 ]; then
            echo "::warning::Test suite took longer than 30s"
          fi
        env:
          FLUI_HEADLESS: 1
```

## GitLab CI

```yaml
# .gitlab-ci.yml
test:
  image: rust:latest
  stage: test
  
  variables:
    FLUI_HEADLESS: "1"
  
  script:
    - cargo test -p flui-platform
    - cargo test --workspace
  
  cache:
    paths:
      - target/
      - .cargo/
```

## CircleCI

```yaml
# .circleci/config.yml
version: 2.1

jobs:
  test:
    docker:
      - image: cimg/rust:1.75
    
    environment:
      FLUI_HEADLESS: "1"
    
    steps:
      - checkout
      - restore_cache:
          keys:
            - cargo-cache-{{ checksum "Cargo.lock" }}
      
      - run:
          name: Run tests
          command: cargo test -p flui-platform
      
      - save_cache:
          key: cargo-cache-{{ checksum "Cargo.lock" }}
          paths:
            - target
            - ~/.cargo

workflows:
  test:
    jobs:
      - test
```

## Jenkins

```groovy
// Jenkinsfile
pipeline {
    agent any
    
    environment {
        FLUI_HEADLESS = '1'
    }
    
    stages {
        stage('Test') {
            steps {
                sh 'cargo test -p flui-platform'
            }
        }
    }
}
```

## Docker

### Dockerfile for Testing

```dockerfile
FROM rust:1.75-slim

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# Set headless mode
ENV FLUI_HEADLESS=1

# Run tests
RUN cargo test -p flui-platform --release
```

### Docker Compose

```yaml
# docker-compose.yml
version: '3.8'

services:
  test:
    build:
      context: .
      dockerfile: Dockerfile
    
    environment:
      - FLUI_HEADLESS=1
      - RUST_LOG=debug
    
    command: cargo test -p flui-platform
```

## Local Development

### Running Tests Locally in Headless Mode

```bash
# Single test file
FLUI_HEADLESS=1 cargo test -p flui-platform --test headless

# All platform tests
FLUI_HEADLESS=1 cargo test -p flui-platform

# Specific test
FLUI_HEADLESS=1 cargo test -p flui-platform test_window_creation

# With output
FLUI_HEADLESS=1 cargo test -p flui-platform -- --nocapture

# With logging
FLUI_HEADLESS=1 RUST_LOG=debug cargo test -p flui-platform
```

### Windows PowerShell

```powershell
# Set environment variable for session
$env:FLUI_HEADLESS = "1"
cargo test -p flui-platform

# Single command
$env:FLUI_HEADLESS = "1"; cargo test -p flui-platform
```

### Windows CMD

```cmd
set FLUI_HEADLESS=1
cargo test -p flui-platform
```

## Performance Expectations

With headless mode enabled:

- **Test suite execution**: <30 seconds (target)
- **Per-test overhead**: <1ms
- **Startup time**: <10ms
- **Memory usage**: <10MB baseline
- **Parallel execution**: Full support, no race conditions

## Troubleshooting

### Tests Fail Without FLUI_HEADLESS

**Problem**: Tests try to create real windows and fail in CI.

**Solution**: Ensure `FLUI_HEADLESS=1` is set in environment variables.

### Tests Are Slow

**Problem**: Test suite takes >30 seconds.

**Solution**: 
1. Check if headless mode is actually enabled (look for "Headless" in logs)
2. Run with `--test-threads=1` to identify slow tests
3. Use `RUST_LOG=debug` to see where time is spent

### Platform Detection Not Working

**Problem**: `current_platform()` returns wrong platform.

**Solution**: 
1. Check environment variable is set correctly
2. Use `headless_platform()` directly instead of `current_platform()`
3. Verify no other code is caching platform instances

## Integration with Coverage Tools

### Tarpaulin

```yaml
- name: Generate coverage
  run: |
    cargo install cargo-tarpaulin
    cargo tarpaulin -p flui-platform --out xml
  env:
    FLUI_HEADLESS: 1
```

### Llvm-cov

```yaml
- name: Generate coverage
  run: |
    cargo install cargo-llvm-cov
    cargo llvm-cov test -p flui-platform --lcov --output-path lcov.info
  env:
    FLUI_HEADLESS: 1
```

## Best Practices

1. **Always set FLUI_HEADLESS=1 in CI** - Prevents CI failures due to missing display
2. **Test locally with headless mode** - Ensures CI will pass
3. **Monitor test execution time** - Keep below 30s for rapid feedback
4. **Use parallel execution** - Headless mode is thread-safe
5. **Check logs for "Headless"** - Verifies headless mode is active
6. **Cache dependencies** - Speeds up CI builds significantly

## See Also

- [flui-platform API documentation](https://docs.rs/flui-platform)
- [Test suite documentation](./tests/README.md)
- [Platform abstraction guide](./ARCHITECTURE.md)
