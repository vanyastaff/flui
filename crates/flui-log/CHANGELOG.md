# Changelog

All notable changes to `flui_log` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Configurable application name via `with_app_name()` builder method
- Platform-specific application name usage:
  - **Android**: `app_name` used as fallback logcat tag when module path unavailable
  - **iOS**: `app_name` formatted as subsystem identifier (`"com.{app_name}.app"`)
  - **WASM/Desktop**: Reserved for future features
- `app_name()` getter method to retrieve current application name
- `AndroidLayer::new(app_name)` constructor for custom app names
- Default application name: `"flui"`

### Enhanced

- **Android Layer**: Complete rewrite with comprehensive improvements
  - Enhanced error handling for null bytes in strings
  - Comprehensive safety documentation for FFI calls
  - Unit tests for `StringRecorder` and thread-safety
  - Detailed module-level documentation with examples
- **iOS Documentation**: Added detailed os_log integration guide
  - Viewing tools: Xcode Console, Console.app, `log stream`
  - Subsystem and category concepts explained
  - Privacy and structured logging benefits documented
- **WASM Documentation**: Added browser requirements and limitations
  - Performance timeline integration details
  - Compatibility notes (browser-only, not Node.js/Cloudflare Workers)
  - DevTools console integration explained
- **README**: Platform-specific features and viewing tools table
- **API Documentation**: Application name usage examples for all platforms

## [0.1.0] - 2025-11-28

### Added

- Initial implementation of cross-platform logging system
- Automatic platform detection and backend selection
- Zero-configuration setup with sensible defaults
- Support for Desktop (Windows, Linux, macOS), Android, iOS, and WASM platforms
- Optional pretty logging with `tracing-forest` for desktop development
- Flexible log filtering per module using `tracing-subscriber::EnvFilter`
- Environment variable override support via `RUST_LOG`
- Builder pattern API with `Logger` struct
- Convenience methods: `init_default()` and `init_with_filter()`
- Comprehensive documentation with examples
- Two working examples: `basic_usage` and `pretty_logging`

### Platform Backends

- **Desktop**: `tracing-subscriber::fmt` with optional `tracing-forest`
- **Android**: `android_log-sys` for logcat integration
- **iOS**: `tracing-oslog` for native os_log
- **WASM**: `tracing-wasm` for browser console

### API

Core `Logger` struct with:
- `new()` / `default()` - Construction
- `with_filter(filter)` - Set log filter string
- `with_level(level)` - Set global log level
- `with_pretty(bool)` - Enable pretty logging (requires `pretty` feature)
- `init()` - Initialize the logging system
- `init_default()` - Quick initialization with defaults
- `init_with_filter(filter)` - Quick initialization with custom filter
- `filter()` - Get current filter
- `level()` - Get current level
- `use_pretty()` - Check if pretty logging enabled

### Features

- `pretty` - Enable hierarchical logging with `tracing-forest` (optional, desktop only)

### Quality Improvements

- Full Rust API Guidelines compliance
- All workspace lints applied (zero clippy warnings)
- No unused dependencies (verified with cargo-udeps)
- All feature combinations tested (cargo-hack)
- Security audited (cargo-audit)
- Private struct fields with getter methods
- `#[must_use]` attributes on builder methods
- `Debug` and `Clone` traits implemented
- Complete API documentation with examples

### Documentation

- Comprehensive README with usage examples
- Full API documentation for all public items
- Architecture section explaining design
- Development section with tooling commands
- Two working examples with documentation

---

**Internal Use Only** - This crate is part of the FLUI framework and is not published to crates.io.
