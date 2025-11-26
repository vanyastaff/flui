# Rust Tools Cheatsheet 🦀

Quick reference for FLUI development workflow.

## 🚀 Daily Commands

```bash
# Start development session
cargo watch -x "nextest run"    # Terminal 1: Auto-test
bacon                           # Terminal 2: Interactive builds

# Quick checks
cargo nextest run               # Fast testing
cargo check                     # Quick compilation check
cargo +nightly udeps --backend=depinfo  # Find unused deps
```

## 🔒 Security (Weekly)

```bash
cargo audit                     # Vulnerability scan
cargo deny check               # License/policy compliance
cargo geiger                   # Unsafe code detection
```

## 📦 Dependencies

```bash
# Analysis
cargo outdated                 # Check for updates
cargo +nightly udeps --all-features --all-targets  # Unused deps (accurate)
cargo hack check --feature-powerset  # Test all feature combinations

# Maintenance  
cargo update                   # Update dependencies
cargo install-update -a       # Update tools
```

## 🛠 Build & Clean

```bash
# Optimization
cargo sweep --time 30          # Remove old artifacts
cargo cache --autoclean        # Clean cargo cache

# Testing
cargo nextest run --all-features   # Comprehensive testing
cargo minimal-versions check   # Test minimal versions
```

## 🚀 Release

```bash
# Pre-release checks
cargo semver-checks check-release  # Breaking changes
cargo msrv verify              # MSRV compatibility
cargo hack check --feature-powerset  # Feature validation

# Release
cargo release --dry-run patch  # Preview release
cargo release patch           # Patch release (0.1.0 → 0.1.1)
git cliff                     # Generate changelog
```

## 🔍 Debug & Analysis

```bash
cargo expand                   # Show macro expansions
cargo expand --lib geometry::matrix4  # Specific module
hyperfine "cargo build"        # Benchmark commands
```

## ⚡ Power Commands

```bash
# Comprehensive health check
cargo audit && cargo deny check && cargo +nightly udeps --backend=depinfo

# Feature matrix testing
cargo hack --feature-powerset -- +nightly udeps --backend=depinfo

# Release readiness
cargo semver-checks check-release && cargo msrv verify && cargo nextest run --all-features
```

## 🛠 Installation

```bash
# Install all tools (run once)
./install-rust-tools.bat      # Windows
# or manually:
cargo install cargo-binstall
cargo binstall cargo-nextest cargo-watch bacon cargo-audit cargo-deny cargo-udeps cargo-hack --no-confirm
```

## 📋 Tool Status

Run `cargo install-update -l` to check which tools need updates.

**Nightly Required:** `cargo-udeps` only  
**All other tools work on stable Rust.**

---
*See RUST_TOOLS.md for detailed documentation*