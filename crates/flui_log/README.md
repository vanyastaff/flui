# flui_log

Cross-platform logging system for FLUI framework with automatic platform detection.

## Features

- 🚀 **Zero-config**: Automatically selects the right backend for each platform
- 🌲 **Pretty logging**: Optional hierarchical logs with `tracing-forest` (desktop only)
- 📱 **Native integration**: Uses platform-native logging on each target
- ⚡ **Performance**: Based on the proven approach from Bevy engine
- 🎯 **Flexible filtering**: Fine-grained control over log levels per module

## Platform Support

| Platform | Backend | Output |
|----------|---------|--------|
| Desktop  | `tracing-subscriber::fmt` or `tracing-forest` | stdout/stderr |
| Android  | `android_log-sys` | `adb logcat` |
| iOS      | `tracing-oslog` | Xcode Console, Console.app |
| WASM     | `tracing-wasm` | Browser DevTools |

## Quick Start

```rust
use flui_log::Logger;

fn main() {
    // Initialize with defaults
    Logger::default().init();

    // Start logging
    tracing::info!("Application started!");
    tracing::debug!("Debug info");
}
```

## Custom Configuration

```rust
use flui_log::{Logger, Level};

fn main() {
    Logger::new()
        .with_filter("debug,wgpu=error,flui_core=trace")
        .with_level(Level::DEBUG)
        .init();
}
```

## Pretty Logging (Desktop Development)

Enable the `"pretty"` feature for beautiful hierarchical logs:

```toml
[dependencies]
flui_log = { path = "../flui_log", features = ["pretty"] }
```

```rust
use flui_log::Logger;

fn main() {
    Logger::new()
        .with_pretty(true)  // Requires "pretty" feature
        .init();
}
```

**Output example:**
```
INFO    main_logic [ 151ms | 100.00% ]
┝━ ｉ Running main application logic
┝━ process_item [ 50.1ms | 33.22% ] id: 0
│  ┕━ ｉ Processing item | item_id: 0
┝━ process_item [ 50.5ms | 33.52% ] id: 1
│  ┕━ ｉ Processing item | item_id: 1
┕━ 🚧 Some items need attention
```

## Environment Variables

Override the default filter with `RUST_LOG`:

```bash
# Set global level
RUST_LOG=debug cargo run

# Filter specific modules
RUST_LOG=info,wgpu=warn,flui_core=trace cargo run
```

## Examples

```bash
# Basic usage
cargo run -p flui_log --example basic_usage

# Pretty logging (requires feature)
cargo run -p flui_log --example pretty_logging --features pretty
```

## Architecture

Based on Bevy's logging system, using:
- `tracing` for structured logging
- `tracing-subscriber` for composable layers
- Platform-specific backends for native integration
- Optional `tracing-forest` for development

## License

MIT OR Apache-2.0
