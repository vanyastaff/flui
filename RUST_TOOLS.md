# Rust Development Tools for FLUI

This document lists all the essential Rust cargo tools installed for the FLUI project development workflow. These tools dramatically improve development speed, security, and code quality.

## Installation Summary

All tools were installed using `cargo-binstall` for faster pre-compiled binary installation:

```bash
# Install binary installer first (much faster than compiling from source)
cargo install cargo-binstall

# Then install all tools at once
cargo binstall cargo-nextest cargo-watch bacon cargo-audit cargo-deny cargo-geiger cargo-outdated cargo-semver-checks cargo-update cargo-cache cargo-sweep cargo-release git-cliff cargo-msrv cargo-expand hyperfine cargo-hack cargo-minimal-versions cargo-udeps --no-confirm
```

**Note:** `cargo-udeps` requires nightly toolchain: `rustup toolchain install nightly`

## 🧪 Core Development Tools

### **cargo-nextest** `v0.9.114` - Next-generation Test Runner
**6.9M downloads** - Up to 60% faster than `cargo test` with better output formatting.

```bash
# Drop-in replacement for cargo test
cargo nextest run

# Run specific package tests
cargo nextest run --package flui_types

# Run with coverage (requires cargo-llvm-cov)
cargo nextest run --coverage

# Continuous testing with cargo-watch
cargo watch -x "nextest run"
```

**Key Features:**
- ✅ Parallel test execution with better isolation
- ✅ Flaky test detection and retries  
- ✅ Rich terminal output with timing information
- ✅ JSON output for CI integration

### **cargo-watch** `v8.5.3` - Auto-rebuild on File Changes  
**1.5M downloads** - Monitor source files and run commands automatically.

```bash
# Continuous compilation checking
cargo watch -x check

# Auto-run tests on changes  
cargo watch -x test

# Auto-run nextest (recommended)
cargo watch -x "nextest run"

# Multiple commands
cargo watch -x check -x test -x run

# Watch specific files/directories
cargo watch -w src -w tests -x check
```

### **bacon** `v3.20.1` - Modern TUI Build Tool
Terminal UI showing real-time compilation feedback with better ergonomics than cargo-watch.

```bash
# Start bacon (interactive TUI)
bacon

# Different modes
bacon test    # Run tests continuously
bacon check   # Check compilation
bacon clippy  # Run clippy continuously
```

## 🔒 Security & Supply Chain

### **cargo-audit** `v0.22.0` - Vulnerability Scanner
**4.17M downloads** - Scans dependencies against RustSec Advisory Database.

```bash
# Scan for vulnerabilities
cargo audit

# Generate JSON report for CI
cargo audit --json

# Auto-fix vulnerable dependencies (experimental)
cargo audit fix --features=fix
```

### **cargo-deny** `v0.18.6` - Comprehensive Dependency Linting
**2.17M downloads** from Embark Studios - License compliance, bans, and security.

```bash
# Initialize configuration
cargo deny init

# Check all policies
cargo deny check

# Check specific areas
cargo deny check licenses    # License compliance
cargo deny check bans        # Banned/duplicate crates  
cargo deny check advisories  # Security vulnerabilities
cargo deny check sources     # Trusted registries only
```

**Configuration in `deny.toml`:**
```toml
[licenses]
allow = ["MIT", "Apache-2.0", "BSD-3-Clause"]

[bans]
multiple-versions = "deny"
deny = [
    { name = "openssl", reason = "Use rustls instead" }
]

[advisories]
ignore = ["RUSTSEC-2020-0001"]  # Ignore specific advisories
```

### **cargo-geiger** `v0.13.0` - Unsafe Code Detector
Detects unsafe Rust usage across dependency tree with intuitive symbols.

```bash
# Analyze unsafe usage
cargo geiger

# Generate report
cargo geiger --format json > unsafe-report.json

# Check specific package
cargo geiger --package flui_types
```

**Output Symbols:**
- 🔒 forbid unsafe
- ❓ no unsafe found  
- ☢️ unsafe detected

## 📦 Smart Dependency Management

### **cargo-udeps** `v0.1.60` - Find Truly Unused Dependencies
**Most accurate** unused dependency detection using compilation analysis.

```bash
# Analyze with all features (most comprehensive)
cargo +nightly udeps --all-features --all-targets --backend=depinfo

# Check specific package
cargo +nightly udeps --package flui_types --all-features

# Find dependencies not used by any feature  
cargo +nightly udeps --no-default-features

# For projects with compilation issues, use depinfo backend
cargo +nightly udeps --backend=depinfo
```

**Why cargo-udeps > cargo-machete:**
- ✅ Analyzes actual compilation usage, not just source code
- ✅ Understands feature flags and conditional compilation
- ✅ Detects dependencies used only in specific feature combinations
- ❌ Requires nightly toolchain and longer analysis time

### **cargo-hack** `v0.6.39` - Feature Flag Testing
Tests all feature flag combinations to ensure each compiles independently.

```bash
# Test all feature combinations (powerset)
cargo hack check --feature-powerset

# Test each feature individually
cargo hack check --each-feature

# Exclude dev-dependencies for cleaner results
cargo hack check --feature-powerset --no-dev-deps

# Combined with other tools
cargo hack --feature-powerset -- +nightly udeps --backend=depinfo
```

**Perfect for library authors** - ensures users can enable any feature combination.

### **cargo-outdated** `v0.17.0` - Check for Updates
Shows when dependencies have newer versions available.

```bash
# Check outdated dependencies
cargo outdated

# Exit with error code if outdated (for CI)
cargo outdated --exit-code 1

# Check only direct dependencies
cargo outdated --depth 1

# Workspace mode
cargo outdated --workspace
```

### **cargo-semver-checks** `v0.45.0` - SemVer Violation Detection  
**318K downloads** - Prevents accidental breaking changes before publishing.

```bash
# Check for breaking changes
cargo semver-checks check-release

# Compare against specific version
cargo semver-checks check-release --baseline-version 0.1.0

# Generate detailed report
cargo semver-checks check-release --output-format json
```

**150+ lints** catch breaking changes like:
- Public API removals
- Function signature changes  
- Trait requirement additions
- Public field removals

## 🛠 Build Optimization & Maintenance

### **cargo-cache** `v0.8.3` - Cache Management
**2.52M downloads** - Manage local cargo cache directories efficiently.

```bash
# Show cache statistics
cargo cache --info

# Auto-clean (keeps archives, removes extracted sources)
cargo cache --autoclean

# Clean specific targets
cargo cache --remove-dir all-targets

# Clean everything older than 30 days
cargo cache --autoclean-expensive --keep-duplicate-crates 1
```

### **cargo-sweep** `v0.8.0` - Build Artifact Cleanup  
Remove unused build artifacts to reclaim disk space.

```bash
# Remove artifacts older than 30 days
cargo sweep --time 30

# Remove all artifacts (nuclear option)
cargo sweep --maxsize 0

# Dry run to see what would be removed
cargo sweep --dry-run --time 30

# Workspace mode
cargo sweep --recursive --time 7
```

### **cargo-minimal-versions** `v0.1.33` - MSRV Testing
Tests that minimum dependency versions in Cargo.toml actually work.

```bash
# Check minimal versions compile
cargo minimal-versions check

# Run tests with minimal versions
cargo minimal-versions test

# Generate minimal Cargo.lock  
cargo minimal-versions generate-lockfile
```

### **cargo-update** `v18.0.0` - Keep Tools Updated
Manages updates for cargo-installed binaries.

```bash
# Update all installed tools
cargo install-update -a

# List tools that need updates
cargo install-update -l

# Update specific tool
cargo install-update cargo-nextest

# Install missing tools from list
cargo install-update --install-new-git
```

## 🚀 Release Management

### **cargo-release** `v0.25.22` - Automated Release Workflow
**1.4K stars** - Automates version bumping, tagging, and publishing.

```bash
# Patch release (0.1.0 -> 0.1.1)
cargo release patch

# Minor release (0.1.0 -> 0.2.0)  
cargo release minor

# Major release (0.1.0 -> 1.0.0)
cargo release major

# Dry run (recommended first)
cargo release --dry-run patch

# Custom version
cargo release --version 2.0.0
```

**Workflow:**
1. Updates version in Cargo.toml
2. Updates changelog 
3. Commits changes
4. Creates git tag
5. Pushes to repository
6. Publishes to crates.io

### **git-cliff** `v2.10.1` - Changelog Generation
**5.6K stars** - Generates beautiful changelogs from git history.

```bash
# Generate changelog
git cliff

# Update existing CHANGELOG.md
git cliff --output CHANGELOG.md

# Generate for specific version range
git cliff --tag v0.1.0..v0.2.0

# Custom template
git cliff --config cliff.toml
```

### **cargo-msrv** `v0.18.4` - MSRV Discovery & Verification  
Find and verify Minimum Supported Rust Version.

```bash
# Find MSRV automatically
cargo msrv find

# Verify declared MSRV  
cargo msrv verify

# Check if current Rust version satisfies MSRV
cargo msrv list
```

## 🔍 Development Utilities

### **cargo-expand** `v1.0.118` - Macro Expansion
**2.6K stars** - Show what macros actually generate (invaluable for debugging).

```bash
# Expand all macros
cargo expand

# Expand specific module
cargo expand --lib geometry::matrix4

# Expand with specific features
cargo expand --features serde

# Output to file
cargo expand > expanded.rs
```

**Note:** Works on stable but produces better results with nightly installed.

### **hyperfine** `v1.20.0` - Command-line Benchmarking  
**23K stars** - Statistical benchmarking with warmup and outlier detection.

```bash
# Benchmark single command
hyperfine "cargo build"

# Compare multiple commands
hyperfine "cargo build" "cargo build --release"

# With warmup runs
hyperfine --warmup 3 "cargo test"

# Export results
hyperfine --export-json results.json "cargo build"

# Parameter sweeps
hyperfine --parameter-list threads 1,2,4,8 "cargo build --jobs {threads}"
```

## 📋 Daily Development Workflow

### Quick Security Check (Weekly)
```bash
# Comprehensive security audit
cargo audit && cargo deny check
```

### Feature Flag Validation (Before PR)
```bash
# Ensure all features compile independently
cargo hack check --feature-powerset --no-dev-deps
```

### Dependency Cleanup (Monthly)  
```bash
# Find truly unused dependencies
cargo +nightly udeps --all-features --all-targets --backend=depinfo

# Check for updates
cargo outdated

# Clean old artifacts
cargo sweep --time 30 && cargo cache --autoclean
```

### Pre-Release Checklist
```bash
# 1. Check for breaking changes
cargo semver-checks check-release

# 2. Verify MSRV  
cargo msrv verify

# 3. Run comprehensive tests
cargo nextest run --all-features

# 4. Security audit
cargo audit && cargo deny check

# 5. Release (dry run first)
cargo release --dry-run patch
cargo release patch
```

### Continuous Development
```bash
# Terminal 1: Continuous testing
cargo watch -x "nextest run"

# Terminal 2: Interactive build feedback  
bacon

# Terminal 3: Development work
# ... coding ...
```

## 🛠 Tool Update Maintenance

Keep all tools up to date:

```bash
# Check which tools need updates
cargo install-update -l

# Update all tools at once  
cargo install-update -a

# Update nightly toolchain (for cargo-udeps)
rustup update nightly
```

## 📝 Notes

- **Performance**: Using `cargo-binstall` reduces installation time from minutes to seconds
- **CI Integration**: Most tools support `--json` output for automated pipelines  
- **Nightly Requirement**: Only `cargo-udeps` requires nightly; everything else works on stable
- **Windows Compatibility**: All tools are fully tested and supported on Windows
- **Memory Usage**: Tools like `cargo-hack` can be memory-intensive on large workspaces

## 🔗 Resources  

- [cargo-binstall GitHub](https://github.com/cargo-bins/cargo-binstall)
- [Rust Tools Documentation](https://forge.rust-lang.org/infra/cargo.html)  
- [RustSec Advisory Database](https://rustsec.org/)
- [FLUI Project Guidelines](./CLAUDE.md)

---

**Total Download Count**: 20M+ combined downloads across all tools  
**Installation Time**: ~2 minutes with cargo-binstall vs 30+ minutes compiling from source  
**Productivity Impact**: Estimated 40-60% improvement in development workflow efficiency