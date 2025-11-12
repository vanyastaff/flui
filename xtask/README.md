# xtask - FLUI Development Tasks

**xtask** provides workspace maintenance and development automation tasks for FLUI using the [xtask pattern](https://github.com/matklad/cargo-xtask).

## Purpose

xtask is for **FLUI framework development tasks**:
- ✅ Code formatting (`fmt`)
- ✅ Linting (`lint`, `clippy`)
- ✅ Testing (`test`)
- ✅ Quality checks (`check`)
- ✅ Pre-commit validation (`validate`)
- ✅ Benchmarks (`bench`)
- ✅ Documentation (`docs`)
- ✅ CI workflows (`ci`)

**NOT for:**
- ❌ Building user applications (use `flui_cli` instead)
- ❌ Project scaffolding (use `flui create`)

---

## Quick Start

```bash
# Most common commands
cargo fmt-fix        # Format code
cargo lint           # Run clippy
cargo check          # Format + clippy + check
cargo test-all       # Run all tests
cargo validate       # Pre-commit validation
```

---

## Commands

### `cargo xtask fmt`

Format all workspace code with rustfmt.

```bash
# Format code (modify files)
cargo xtask fmt
cargo fmt-fix                    # Alias

# Check only (no modifications)
cargo xtask fmt --check
cargo fmt-check                  # Alias
```

---

### `cargo xtask lint`

Run clippy linter with project configuration.

```bash
# Lint (warnings as errors)
cargo xtask lint
cargo lint                       # Alias

# Auto-fix clippy warnings
cargo xtask lint --fix
cargo lint-fix                   # Alias

# Allow warnings (no -D warnings)
cargo xtask lint --allow-warnings
```

**What it checks:**
- All workspace crates
- All targets (lib, bins, tests, benchmarks)
- All features enabled
- Treats warnings as errors by default

---

### `cargo xtask check`

Run all code quality checks: **fmt** + **clippy** + **cargo check**.

```bash
cargo xtask check
cargo check                      # Alias (⚠️ shadows cargo check)
```

**What it does:**
1. ✓ **Format check** - `cargo fmt --all -- --check`
2. ✓ **Clippy** - `cargo clippy --workspace --all-targets -- -D warnings`
3. ✓ **Cargo check** - `cargo check --workspace --all-targets`

**Options:**
- `--skip-fmt` - Skip formatting check
- `--skip-clippy` - Skip clippy

**Use before every commit!**

---

### `cargo xtask test`

Run tests across the workspace.

```bash
# All tests
cargo xtask test
cargo test-all                   # Alias

# Specific package
cargo xtask test -p flui_core

# Test filter (by name)
cargo xtask test layout_test

# Library tests only
cargo xtask test --lib
cargo test-lib                   # Alias

# Documentation tests only
cargo xtask test --doc
cargo test-doc                   # Alias

# With specific features
cargo xtask test --all-features
cargo xtask test --no-default-features
```

**Options:**
- `-p, --package <PACKAGE>` - Test specific package
- `--filter <FILTER>` - Test name filter
- `--all-features` - Enable all features
- `--no-default-features` - Disable default features
- `--lib` - Library tests only
- `--doc` - Documentation tests only
- `--bench` - Run benchmark tests

---

### `cargo xtask validate`

**Pre-commit validation**: Runs `check` + `test`.

```bash
cargo xtask validate
cargo validate                   # Alias
```

**What it does:**
```
━━━ Step 1/2: Code Quality Checks ━━━
- Format check
- Clippy
- Cargo check

━━━ Step 2/2: Tests ━━━
- All workspace tests
```

**Options:**
- `--skip-tests` - Skip tests (only run checks)

**Use this before:**
- Creating a commit
- Opening a pull request
- Pushing to CI

---

### `cargo xtask bench`

Run benchmarks across workspace.

```bash
# All benchmarks (flui_core + flui_types)
cargo xtask bench
cargo bench-all                  # Alias

# Specific package
cargo xtask bench -p flui_core

# Benchmark name filter
cargo xtask bench my_bench_name
```

**Packages with benchmarks:**
- `flui_core` - Pipeline, element tree, hooks
- `flui_types` - Geometry, color, typography

---

### `cargo xtask examples`

Build all examples to ensure they compile.

```bash
# Debug builds
cargo xtask examples
cargo examples-all               # Alias

# Release builds (faster)
cargo xtask examples --release
cargo examples-release           # Alias

# Specific package
cargo xtask examples -p flui_core
```

**Why:** FLUI has 40+ examples. This ensures API changes don't break examples.

---

### `cargo xtask docs`

Generate documentation with all features.

```bash
cargo xtask docs
```

**What it does:**
- Generates docs for entire workspace
- Includes all features
- Opens in browser automatically

---

### `cargo xtask ci`

Run full CI suite locally.

```bash
cargo xtask ci
cargo ci                         # Alias
```

**What it does:**
1. Code quality checks (`check`)
2. All tests (`test`)
3. Benchmarks (`bench`)

**Use to test CI before pushing.**

---

## Cargo Aliases

All commands have convenient aliases in `.cargo/config.toml`:

```bash
# Code quality
cargo fmt-fix                    # Format code
cargo fmt-check                  # Check format
cargo lint                       # Run clippy
cargo lint-fix                   # Fix clippy warnings
cargo check                      # All checks (fmt + clippy + check)
cargo validate                   # Pre-commit (check + test)
cargo ci                         # Full CI suite

# Testing
cargo test-all                   # All tests
cargo test-lib                   # Library tests
cargo test-doc                   # Doc tests

# Quality
cargo bench-all                  # All benchmarks
cargo examples-all               # Build all examples
cargo examples-release           # Build examples (release)
```

---

## Common Workflows

### 🔨 Before Committing

```bash
# Quick check (~10-30 seconds)
cargo check

# Full validation (~1-3 minutes)
cargo validate
```

### 🧹 Format All Code

```bash
cargo fmt-fix
```

### 🔍 Find Clippy Issues

```bash
# Check
cargo lint

# Auto-fix
cargo lint-fix
```

### 🧪 Run Specific Tests

```bash
# All tests in flui_core
cargo xtask test -p flui_core

# Tests matching "layout"
cargo xtask test layout

# With verbose output
cargo xtask test -v layout
```

### 📊 Run Benchmarks

```bash
# All benchmarks
cargo bench-all

# Specific package
cargo xtask bench -p flui_types

# Specific benchmark
cargo xtask bench "color_conv"
```

### ✅ Pre-Release Checklist

```bash
# 1. Full validation
cargo validate

# 2. Build all examples
cargo examples-release

# 3. Run benchmarks
cargo bench-all

# 4. Generate docs
cargo xtask docs
```

---

## Architecture

### Command Structure

```
xtask/src/
├── main.rs              # CLI entry point (clap)
├── commands/            # Command implementations
│   ├── fmt.rs           # Format command
│   ├── lint.rs          # Lint command
│   ├── check.rs         # Quality checks
│   ├── test.rs          # Test orchestration
│   ├── validate.rs      # Pre-commit validation
│   ├── bench.rs         # Benchmarks
│   ├── examples.rs      # Example builds
│   ├── docs.rs          # Documentation
│   └── ci.rs            # CI suite
└── util/
    └── process.rs       # Async command execution
```

### Design Principles

1. **Single Responsibility**: Each command does one thing well
2. **Composability**: Commands can be combined (`validate` = `check` + `test`)
3. **Async Execution**: Uses `tokio` for async command execution
4. **Error Context**: Uses `anyhow` for actionable error messages
5. **Workspace-Aware**: Operates on entire workspace by default

---

## Requirements

- **Rust toolchain** (1.90+)
- **cargo** (comes with Rust)
- **rustfmt** - `rustup component add rustfmt`
- **clippy** - `rustup component add clippy`

**Optional:**
- **wasm-pack** (for web examples) - `cargo install wasm-pack`
- **cargo-llvm-cov** (for coverage) - `cargo install cargo-llvm-cov`

---

## Troubleshooting

### `cargo check` alias shadows built-in command

**Issue:** Cargo's built-in `check` command conflicts with our alias.

**Solution:**
```bash
# Use explicit xtask command
cargo xtask check

# Or use workspace check
cargo check --workspace
```

### Tests failing

```bash
# Run with verbose output
cargo xtask test -v

# Run specific test with output
cargo xtask test "my_test" -- --nocapture
```

### Clippy warnings

```bash
# View warnings
cargo lint

# Auto-fix (when possible)
cargo lint-fix

# Allow warnings temporarily
cargo xtask lint --allow-warnings
```

---

## Contributing

When contributing to FLUI:

1. **Before coding:**
   ```bash
   cargo validate  # Ensure clean baseline
   ```

2. **While coding:**
   ```bash
   cargo check  # Run frequently
   ```

3. **Before committing:**
   ```bash
   cargo validate  # Final check
   ```

4. **For API changes:**
   ```bash
   cargo examples-all  # Verify examples compile
   ```

---

## Resources

- **xtask Pattern:** https://github.com/matklad/cargo-xtask
- **FLUI Documentation:** `../README.md`
- **Architecture Docs:** `../docs/arch/`
- **CLAUDE.md:** `../CLAUDE.md` (AI assistant guidelines)

---

**For FLUI development** - Workspace maintenance automation 🛠️
