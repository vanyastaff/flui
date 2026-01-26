# Quick Start Guide: flui-platform

**Last Updated**: 2026-01-26

## Installation

Add flui-platform to your `Cargo.toml`:

```toml
[dependencies]
flui-platform = { path = "../../crates/flui-platform" }
```

## Basic Usage

### 1. Create a Window

```rust
use flui_platform::{current_platform, WindowOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get platform implementation (auto-detects Windows/macOS/Headless)
    let platform = current_platform();

    // Configure window
    let options = WindowOptions {
        title: "My FLUI App".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        ..Default::default()
    };

    // Create window
    let window = platform.open_window(options)?;

    // Run event loop
    platform.run(Box::new(|| {
        println!("Application ready!");
    }));

    Ok(())
}
```

### 2. Handle Window Events

```rust
use flui_platform::{current_platform, WindowEvent};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let platform = current_platform();

    // Register event handler
    platform.on_window_event(Box::new(|event| {
        match event {
            WindowEvent::CloseRequested { window_id } => {
                println!("Window {} close requested", window_id);
                // Handle cleanup, destroy window
            }
            WindowEvent::Resized { window_id, size } => {
                println!("Window {} resized to {:?}", window_id, size);
            }
            WindowEvent::FocusChanged { window_id, focused } => {
                println!("Window {} focus: {}", window_id, focused);
            }
            _ => {}
        }
    }));

    // Create window and run
    let window = platform.open_window(WindowOptions::default())?;
    platform.run(Box::new(|| {}));

    Ok(())
}
```

### 3. Measure Text (After Text System MVP Complete)

```rust
use flui_platform::current_platform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let platform = current_platform();
    let text_system = platform.text_system();

    // Load default font
    let font_family = text_system.default_font_family();

    // Measure text (returns bounding box in logical pixels)
    let text = "Hello, FLUI!";
    let font_size = 16.0;
    let bounds = text_system.measure_text(text, font_family, font_size)?;

    println!("Text bounds: {:?}", bounds);

    Ok(())
}
```

### 4. Use Background and Foreground Executors

The platform provides two executor types for different use cases:

#### Background Executor (Multi-threaded Tokio Runtime)

Use for CPU-intensive or I/O-bound work that should not block the UI thread:

```rust
use flui_platform::executor::BackgroundExecutor;
use flui_platform::PlatformExecutor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executor = BackgroundExecutor::new();
    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = Arc::clone(&completed);

    // Spawn background task (runs on worker thread pool)
    executor.spawn(Box::new(move || {
        // CPU-intensive work (image processing, data analysis, etc.)
        let result = expensive_computation();
        println!("Background task complete: {}", result);
        completed_clone.store(true, Ordering::SeqCst);
    }));

    // Main thread continues immediately (non-blocking)
    println!("UI thread continues...");

    Ok(())
}

fn expensive_computation() -> u64 {
    (0..10_000_000).fold(0u64, |acc, x| acc.wrapping_add(x))
}
```

**Key Features:**
- **Multi-threaded**: Uses `num_cpus::get()` worker threads
- **Send+Sync**: Can be shared across threads
- **Tokio runtime**: Supports async/await
- **Use cases**: File I/O, network requests, image processing, data analysis

#### Foreground Executor (UI Thread Queue)

Use for tasks that must run on the UI thread (layout, rendering, state updates):

```rust
use flui_platform::executor::ForegroundExecutor;
use flui_platform::PlatformExecutor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executor = ForegroundExecutor::new();

    // Queue tasks for UI thread execution
    executor.spawn(Box::new(|| {
        println!("This runs on UI thread");
    }));

    executor.spawn(Box::new(|| {
        println!("This also runs on UI thread");
    }));

    // In your message loop, drain all pending tasks
    executor.drain_tasks(); // Executes ALL queued tasks in FIFO order

    Ok(())
}
```

**Key Features:**
- **FIFO execution**: Tasks execute in order they were queued
- **Flume channels**: Lock-free, high-performance queue
- **drain_tasks()**: Process all pending tasks in message loop
- **Use cases**: UI updates, layout recalculation, event dispatch

#### Pattern: Background Work with Foreground Callback

Common pattern for loading data in background and updating UI:

```rust
use flui_platform::executor::{BackgroundExecutor, ForegroundExecutor};
use flui_platform::PlatformExecutor;
use std::sync::{Arc, Mutex};

fn load_data_example() {
    let background = BackgroundExecutor::new();
    let foreground = ForegroundExecutor::new();

    let ui_state = Arc::new(Mutex::new(String::from("Loading...")));
    let ui_state_bg = Arc::clone(&ui_state);
    let foreground_clone = foreground.clone();

    // Background: Load data from disk/network
    background.spawn(Box::new(move || {
        let data = load_data_from_disk(); // Blocking I/O

        // Foreground: Update UI with loaded data
        foreground_clone.spawn(Box::new(move || {
            *ui_state_bg.lock().unwrap() = data;
        }));
    }));

    // In message loop: drain foreground tasks
    foreground.drain_tasks();
}

fn load_data_from_disk() -> String {
    std::thread::sleep(std::time::Duration::from_millis(100));
    "Loaded data".to_string()
}
```

#### Async/Await Integration

Background executor provides Tokio runtime access for native async code:

```rust
use flui_platform::executor::BackgroundExecutor;
use std::time::Duration;

async fn async_task() {
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!("Async work complete");
}

fn main() {
    let executor = BackgroundExecutor::new();
    let handle = executor.handle();

    // Spawn native async task
    handle.spawn(async {
        async_task().await;
    });
}
```

#### Performance Characteristics

- **Spawn overhead**: <100µs for both executors
- **Background concurrency**: Up to `num_cpus::get()` parallel tasks
- **Foreground batching**: drain_tasks() processes all tasks efficiently
- **Zero allocation**: Foreground executor uses pre-allocated channel

**See also**: `examples/executor.rs` for comprehensive usage examples

### 5. Enumerate Displays

```rust
use flui_platform::current_platform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let platform = current_platform();

    // Get all connected displays
    let displays = platform.displays();

    for display in displays {
        println!("Display: {}", display.name());
        println!("  Bounds: {:?}", display.bounds());
        println!("  Scale Factor: {}", display.scale_factor());
        println!("  Refresh Rate: {} Hz", display.refresh_rate());
        println!("  Primary: {}", display.is_primary());
    }

    Ok(())
}
```

### 6. Use Clipboard

```rust
use flui_platform::current_platform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let platform = current_platform();
    let clipboard = platform.clipboard();

    // Write text to clipboard
    clipboard.write_text("Hello from FLUI!")?;

    // Read text from clipboard
    if let Some(text) = clipboard.read_text()? {
        println!("Clipboard content: {}", text);
    }

    // Check if clipboard has text
    if clipboard.has_text() {
        println!("Clipboard has text available");
    }

    Ok(())
}
```

## Running Examples

```bash
# Simple window creation
cargo run --example simple_window

# Multi-window application
cargo run --example multi_window

# Event handling
cargo run --example event_handling

# Text measurement (after text system complete)
cargo run --example text_measurement
```

## Running Tests

```bash
# Run all tests
cargo test -p flui-platform

# Run in headless mode (CI-friendly)
FLUI_HEADLESS=1 cargo test -p flui-platform

# Run contract tests only
cargo test -p flui-platform --test contract

# Run with logging
RUST_LOG=debug cargo test -p flui-platform
```

## Platform-Specific Notes

### Windows
- Requires Windows 10 or later
- Uses native Win32 API
- Supports Windows 11 Mica backdrop automatically
- DPI-aware (per-monitor v2)

### macOS
- Requires macOS 11 (Big Sur) or later
- Uses native AppKit/Cocoa API
- Supports Retina displays automatically
- Respects system appearance (light/dark mode)

### Headless (Testing)
- No GPU or display server required
- Perfect for CI/CD pipelines
- Mock implementations of all platform APIs
- Enable with `FLUI_HEADLESS=1` environment variable

## Troubleshooting

### Window not appearing
- Check if `visible: true` in WindowOptions
- Ensure `platform.run()` is called (starts event loop)
- Verify no panic in ready callback

### Events not firing
- Ensure `on_window_event` registered before `platform.run()`
- Check event filter in handler (match all events during debugging)
- Enable tracing: `RUST_LOG=flui_platform=debug`

### Text measurement errors (after MVP)
- Verify font family exists on system
- Check font size is reasonable (8-72pt typical range)
- Ensure text is valid UTF-8

### Clipboard issues
- On Windows: Ensure application has clipboard access
- On macOS: Check entitlements for clipboard access
- Test with simple ASCII text first before Unicode

## Next Steps

- Read [ARCHITECTURE.md](../../crates/flui-platform/ARCHITECTURE.md) for design overview
- Check [IMPLEMENTATION_STATUS.md](../../crates/flui-platform/IMPLEMENTATION_STATUS.md) for feature completeness
- See [examples/](../../crates/flui-platform/examples/) for more usage patterns
- Review constitution principles in [.specify/memory/constitution.md](../../.specify/memory/constitution.md)

## API Reference

Full API documentation:
```bash
cargo doc -p flui-platform --no-deps --open
```

## Contributing

See [CLAUDE.md](../../CLAUDE.md) for development workflow and coding standards.
