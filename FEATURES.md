# FLUI Feature Flags Guide

This document explains all available feature flags in FLUI and how to use them.

## Quick Start

For most users, the default features are perfect:

```toml
[dependencies]
flui = "0.1"
```

This includes egui backend and state persistence. All widgets and app framework are always included.

## Backend Selection

FLUI supports multiple rendering backends. Choose **ONE** of these:

### egui Backend (Default, Recommended)

```toml
[dependencies]
flui = "0.1"  # egui is default
```

- **Best for**: Desktop applications, rapid prototyping
- **Pros**: Mature, immediate mode, great debugging
- **Cons**: Not GPU-optimized for complex scenes

### wgpu Backend (Experimental)

```toml
[dependencies]
flui = { version = "0.1", default-features = false, features = ["wgpu"] }
```

- **Best for**: GPU-accelerated applications, games
- **Pros**: Modern GPU API, cross-platform
- **Cons**: Still experimental, more complexity

## Performance Features

### parallel - Parallel Processing

```toml
features = ["parallel"]
```

**Status:** ✅ Stable - Thread-safe parallel processing

Enables rayon-based parallel processing for build pipeline. All thread-safety issues have been resolved through comprehensive Arc/Mutex refactoring.

### profiling - Puffin Profiler

```toml
features = ["profiling"]
```

Enables puffin profiling for performance analysis.

**Usage:**
```rust
puffin::profile_scope!("my_function");
// Your code here
```

View profiling data with puffin_viewer or puffin_egui.

### tracy - Tracy Profiler Integration

```toml
features = ["tracy"]
```

Enables Tracy profiler integration for advanced performance analysis.

### full-profiling - All Profiling Tools

```toml
features = ["full-profiling"]
```

Enables both puffin and tracy profiling.

## Optional Features

### persistence - State Persistence

```toml
features = ["persistence"]  # Enabled by default
```

Enables saving/loading application state between sessions.

### serde - Serialization Support

```toml
features = ["serde"]
```

Adds serde serialization support to core types.

**Example:**
```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
struct MyState {
    count: i32,
}
```

### devtools - Developer Tools

```toml
features = ["devtools"]
```

Enables developer tools integration for debugging.

### memory-profiler - Memory Profiling

```toml
features = ["memory-profiler"]
```

Enables memory profiling. Requires `devtools` feature.

```toml
features = ["devtools", "memory-profiler"]
```

## Feature Combinations

### Minimal Setup

```toml
# Default: egui backend with persistence
flui = "0.1"
```

### wgpu Backend

```toml
# wgpu without persistence
flui = { version = "0.1", default-features = false, features = ["wgpu"] }
```

### Production with Profiling

```toml
[dependencies]
flui = { version = "0.1", features = ["profiling"] }
```

### Development with All Tools

```toml
[dev-dependencies]
flui = { version = "0.1", features = ["full-profiling", "devtools", "memory-profiler"] }
```

## Usage Examples

### Basic Application (egui)

```toml
[dependencies]
flui = "0.1"
```

```rust
use flui::prelude::*;

#[derive(Clone)]
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Container::builder()
            .padding(EdgeInsets::all(32.0))
            .child(Text::new("Hello, FLUI!"))
            .build()
    }
}

fn main() {
    run_app("My App", MyApp);
}
```

### wgpu Application

```toml
[dependencies]
flui = { version = "0.1", default-features = false, features = ["wgpu"] }
```

```rust
use flui::prelude::*;

#[derive(Clone)]
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Running on wgpu!")
    }
}

fn main() {
    run_app("wgpu App", MyApp);
}
```

### With State Management

```toml
[dependencies]
flui = { version = "0.1", features = ["serde"] }
serde = { version = "1.0", features = ["derive"] }
```

```rust
use flui::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
struct Counter {
    count: i32,
}

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, self.count);

        Column::builder()
            .children(vec![
                Box::new(Text::new(format!("Count: {}", count.get()))),
                Box::new(Button::new("Increment")
                    .on_pressed(move || count.set(count.get() + 1))),
            ])
            .build()
    }
}
```

## Feature Status

| Feature | Status | Notes |
|---------|--------|-------|
| `egui` | ✅ Stable | Recommended for production |
| `wgpu` | 🚧 Experimental | Still in development |
| `parallel` | ✅ Stable | Thread-safe parallel processing |
| `profiling` | ✅ Stable | Works well |
| `tracy` | ✅ Stable | Advanced profiling |
| `persistence` | ✅ Stable | Default feature |
| `serde` | ✅ Stable | Serialization support |
| `devtools` | ✅ Stable | Developer tools |
| `memory-profiler` | ✅ Stable | Memory analysis |

## Default Features

When you specify just:
```toml
flui = "0.1"
```

You get these default features:
- `egui` - egui backend
- `persistence` - state persistence

All widgets and application framework are always included.

## Troubleshooting

### "wgpu backend not working"

The wgpu backend is still experimental. For production use, stick with the egui backend:
```toml
flui = "0.1"  # Uses egui by default
```

## Getting Help

- Documentation: https://docs.rs/flui
- Examples: `cargo run --example profile_card`
- Issues: https://github.com/yourusername/flui/issues
