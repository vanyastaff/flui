# flui_log

Cross-platform logging system for the FLUI framework with automatic platform detection and zero-configuration setup.

## Features

- 🚀 **Zero-config** - Automatically selects the right logging backend for each platform
- 🌲 **Pretty logging** - Optional hierarchical logs with `tracing-forest` (desktop development)
- 📱 **Native integration** - Uses platform-native logging APIs on all targets
- ⚡ **High performance** - Built on `tracing` with minimal overhead
- 🎯 **Flexible filtering** - Fine-grained control over log levels per module
- 🔒 **Production-ready** - Battle-tested architecture based on Bevy engine

## Platform Support

| Platform | Backend | Output Destination | Viewing Tools |
|----------|---------|-------------------|---------------|
| **Desktop** (Windows, Linux, macOS) | `tracing-subscriber::fmt` or `tracing-forest` | Terminal stdout/stderr | Terminal |
| **Android** | `android_log-sys` | logcat | `adb logcat`, Android Studio Logcat |
| **iOS** | `tracing-oslog` | os_log | Xcode Console, Console.app, `log stream` |
| **WASM** | `tracing-wasm` | Browser console | Browser DevTools (F12) |

### Platform-Specific Features

#### Android
- Native FFI integration with Android's logcat system
- Automatic tag assignment from module path
- Priority mapping: TRACE→VERBOSE, DEBUG→DEBUG, INFO→INFO, WARN→WARN, ERROR→ERROR
- View logs: `adb logcat`, `adb logcat -s flui:*`, or Android Studio Logcat panel

#### iOS
- Unified logging system (`os_log`) integration via `tracing-oslog`
- Subsystem: "com.flui.app" (configurable in source)
- Privacy-preserving structured logging
- View logs: Xcode Console, Console.app, or `log stream --predicate 'subsystem == "com.flui.app"'`

#### WASM
- Browser DevTools console integration with color coding
- Performance timeline integration via `window.performance` API
- **Requirement**: Browser environment (Chrome, Firefox, Safari, Edge)
- **Not supported**: Node.js, Cloudflare Workers (no `window.performance`)
- View Performance tab for span timing visualizations

#### Desktop
- Standard output with configurable formatting
- Optional hierarchical logging with `tracing-forest` (enable `pretty` feature)
- Automatic debug/release mode detection

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_log = { path = "../flui_log" }
```

For development with pretty hierarchical logging:

```toml
[dependencies]
flui_log = { path = "../flui_log", features = ["pretty"] }
```

## Quick Start

### Basic Usage

```rust
use flui_log::Logger;

fn main() {
    // Initialize with defaults (INFO level, wgpu=WARN filter)
    Logger::default().init();

    // Start logging
    tracing::info!("Application started!");
    tracing::debug!("This won't appear (default is INFO)");
    tracing::warn!("Warning message");
    tracing::error!("Error message");
}
```

### Custom Configuration

```rust
use flui_log::{Logger, Level};

fn main() {
    Logger::new()
        .with_app_name("my_game")  // Custom app name for platform-specific logging
        .with_filter("debug,wgpu=error,flui_core=trace")
        .with_level(Level::DEBUG)
        .init();

    tracing::debug!("Now debug messages appear!");
    tracing::trace!("Trace from flui_core module");
}
```

### Application Name Configuration

The application name is used for platform-specific logging identification:

```rust
use flui_log::Logger;

fn main() {
    Logger::new()
        .with_app_name("space_shooter")
        .init();

    // Results in:
    // - Android: logcat tag "space_shooter" (when module unavailable)
    // - iOS: subsystem "com.space_shooter.app"
    // - WASM: Reserved for future features
    // - Desktop: Reserved for future features
}
```

### Convenience Methods

```rust
use flui_log::Logger;

// Quick initialization with custom filter
Logger::init_with_filter("trace,wgpu=warn");

// Or use default settings
Logger::init_default();
```

## Pretty Logging (Development)

Enable beautiful hierarchical logs for desktop development:

```toml
[dependencies]
flui_log = { path = "../flui_log", features = ["pretty"] }
```

```rust
use flui_log::Logger;

fn main() {
    Logger::new()
        .with_pretty(true)  // Enable hierarchical output
        .init();

    // Your application code...
}
```

**Example Output:**

```
INFO    main_logic [ 151ms | 100.00% ]
├─ ℹ️  Running main application logic
├─ process_item [ 50.1ms | 33.22% ] id: 0
│  └─ ℹ️  Processing item | item_id: 0
├─ process_item [ 50.5ms | 33.52% ] id: 1
│  └─ ℹ️  Processing item | item_id: 1
└─ 🚧 Some items need attention
```

**Note:** Pretty logging is automatically enabled in debug builds when the feature is present.

## Environment Variables

Override default configuration using the `RUST_LOG` environment variable:

```bash
# Set global log level
RUST_LOG=debug cargo run

# Filter specific modules
RUST_LOG=info,wgpu=warn,flui_core=trace cargo run

# Disable all logging except errors
RUST_LOG=error cargo run
```

The environment variable takes precedence over programmatic configuration.

## Examples

Run the provided examples to see the logger in action:

```bash
# Basic usage example
cargo run --example basic_usage -p flui_log

# Pretty logging example (requires feature)
cargo run --example pretty_logging -p flui_log --features pretty
```

## Architecture

`flui_log` is built on the robust `tracing` ecosystem:

- **`tracing`** - Structured, composable logging framework
- **`tracing-subscriber`** - Layer-based subscriber composition
- **Platform backends** - Native integration for each target platform
- **`tracing-forest`** (optional) - Hierarchical tree-based output for development

The design is inspired by [Bevy's logging system](https://github.com/bevyengine/bevy/tree/main/crates/bevy_log), providing a proven architecture for cross-platform logging.

## API

### Logger Methods

```rust
// Construction
Logger::new()                              // Create new logger
Logger::default()                          // Create with defaults (app_name="flui")

// Configuration (builder pattern)
.with_app_name(name: impl Into<String>)    // Set application name
.with_filter(filter: impl Into<String>)    // Set log filter
.with_level(level: Level)                  // Set global level
.with_pretty(pretty: bool)                 // Enable pretty logging (feature: "pretty")
.init()                                     // Initialize logging

// Convenience methods
Logger::init_default()                     // Quick init with defaults
Logger::init_with_filter(filter)           // Quick init with custom filter

// Getters
.app_name() -> &str                        // Get application name
.filter() -> &str                          // Get current filter
.level() -> &Level                         // Get current level
.use_pretty() -> bool                      // Check if pretty enabled (feature: "pretty")
```

## Minimum Supported Rust Version (MSRV)

This crate requires Rust 1.91 or later (workspace MSRV).

## Development

### Running Tests

```bash
# Run all tests
cargo test -p flui_log

# Run with all features
cargo test -p flui_log --all-features

# Run doc tests only
cargo test -p flui_log --doc
```

### Code Quality

```bash
# Check code
cargo check -p flui_log

# Run clippy
cargo clippy -p flui_log --all-features -- -D warnings

# Format code
cargo fmt -p flui_log

# Generate documentation
cargo doc -p flui_log --no-deps --open
```

### Tooling Checks

```bash
# Security audit
cargo audit

# Check for unused dependencies (requires nightly)
cargo +nightly udeps -p flui_log --backend=depinfo

# Test all feature combinations
cargo hack check -p flui_log --feature-powerset

# Check for outdated dependencies
cargo outdated -p flui_log
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Related Crates

Part of the FLUI framework:

- [`flui_core`](../flui_core) - Core framework functionality
- [`flui_app`](../flui_app) - Application runtime
- [`flui_engine`](../flui_engine) - Rendering engine
- [`flui_types`](../flui_types) - Common types
- [`flui-foundation`](../flui-foundation) - Foundation utilities

## Contributing

See the [FLUI project guidelines](../../CLAUDE.md) for development setup and coding standards.

---

**Version:** 0.1.0 | **Status:** Production-ready ✅
