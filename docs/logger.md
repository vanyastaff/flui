# FLUI Log - Cross-Platform Logging

## Architecture (based on Bevy's approach)

### Core Components

1. **LogPlugin** - Main plugin that configures tracing
2. **Platform-specific layers**:
   - Desktop: `tracing_subscriber::fmt` (stdout)
   - Android: Custom `AndroidLayer` using `android_log-sys`
   - iOS: `tracing-oslog` using Apple's os_log
   - WASM: `tracing-wasm` for browser console

### Crate Structure

```
crates/flui_log/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Main plugin & re-exports
│   ├── android_layer.rs    # Android tracing layer
│   └── plugin.rs           # LogPlugin implementation
```

## Implementation

### 1. Cargo.toml

```toml
[package]
name = "flui_log"
version = "0.1.0"
edition = "2021"

[dependencies]
# Core tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["registry", "env-filter"] }

# Android support
[target.'cfg(target_os = "android")'.dependencies]
android_log-sys = "0.3"

# WASM support
[target.'cfg(target_arch = "wasm32")'.dependencies]
tracing-wasm = "0.2"

# iOS support
[target.'cfg(target_os = "ios")'.dependencies]
tracing-oslog = "0.3"

[features]
default = []
```

### 2. LogPlugin API (similar to Bevy)

```rust
// src/plugin.rs

pub struct LogPlugin {
    /// Log filter string (e.g. "info,wgpu=warn,flui=debug")
    pub filter: String,
    /// Global log level
    pub level: tracing::Level,
}

impl Default for LogPlugin {
    fn default() -> Self {
        Self {
            filter: "info,wgpu=warn".to_string(),
            level: tracing::Level::INFO,
        }
    }
}

impl LogPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = filter.into();
        self
    }

    pub fn with_level(mut self, level: tracing::Level) -> Self {
        self.level = level;
        self
    }

    /// Initialize logging (called automatically by run_app)
    pub fn init(&self) {
        use tracing_subscriber::{layer::SubscriberExt, Registry};

        // Create filter layer
        let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
            .or_else(|_| tracing_subscriber::EnvFilter::try_new(&self.filter))
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

        // Platform-specific setup
        #[cfg(target_os = "android")]
        {
            let subscriber = Registry::default()
                .with(filter_layer)
                .with(crate::android_layer::AndroidLayer::default());
            
            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }

        #[cfg(target_arch = "wasm32")]
        {
            use tracing_wasm::WASMLayerConfigBuilder;
            
            let wasm_layer = tracing_wasm::WASMLayer::new(
                WASMLayerConfigBuilder::new()
                    .set_max_level(self.level)
                    .build()
            );
            
            let subscriber = Registry::default()
                .with(filter_layer)
                .with(wasm_layer);
            
            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }

        #[cfg(target_os = "ios")]
        {
            use tracing_oslog::OsLogger;
            
            let os_logger = OsLogger::new("com.flui.app", "default");
            
            let subscriber = Registry::default()
                .with(filter_layer)
                .with(os_logger);
            
            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }

        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        {
            // Desktop: use fmt layer
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(true)
                .with_line_number(true);

            let subscriber = Registry::default()
                .with(filter_layer)
                .with(fmt_layer);

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }

        tracing::info!("Logging initialized for platform");
    }
}
```

### 3. Android Layer (from Bevy)

```rust
// src/android_layer.rs

use core::fmt::{Debug, Write};
use tracing::{
    field::Field,
    span::{Attributes, Record},
    Event, Id, Level, Subscriber,
};
use tracing_subscriber::{field::Visit, layer::Context, registry::LookupSpan, Layer};

#[derive(Default)]
pub struct AndroidLayer;

struct StringRecorder(String, bool);

impl StringRecorder {
    fn new() -> Self {
        StringRecorder(String::new(), false)
    }
}

impl Visit for StringRecorder {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            if !self.0.is_empty() {
                self.0 = format!("{:?}\n{}", value, self.0)
            } else {
                self.0 = format!("{:?}", value)
            }
        } else {
            if self.1 {
                write!(self.0, " ").unwrap();
            } else {
                self.1 = true;
            }
            write!(self.0, "{} = {:?}", field.name(), value).unwrap();
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for AndroidLayer {
    fn on_new_span(&self, _attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {}

    fn on_record(&self, _span: &Id, _values: &Record<'_>, _ctx: Context<'_, S>) {}

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut recorder = StringRecorder::new();
        event.record(&mut recorder);

        let metadata = event.metadata();
        let level = metadata.level();

        // Convert tracing level to android_log level
        let priority = match *level {
            Level::TRACE => android_log_sys::LogPriority::VERBOSE,
            Level::DEBUG => android_log_sys::LogPriority::DEBUG,
            Level::INFO => android_log_sys::LogPriority::INFO,
            Level::WARN => android_log_sys::LogPriority::WARN,
            Level::ERROR => android_log_sys::LogPriority::ERROR,
        };

        let tag = std::ffi::CString::new(metadata.target()).unwrap();
        let message = std::ffi::CString::new(recorder.0).unwrap();

        unsafe {
            android_log_sys::__android_log_write(
                priority as i32,
                tag.as_ptr(),
                message.as_ptr(),
            );
        }
    }
}
```

### 4. Main lib.rs

```rust
// src/lib.rs

//! Cross-platform logging for FLUI
//!
//! Provides automatic logging configuration for:
//! - Desktop (stdout via tracing_subscriber::fmt)
//! - Android (logcat via android_log-sys)
//! - WASM (browser console via tracing-wasm)
//! - iOS (TODO: os_log)

mod plugin;

#[cfg(target_os = "android")]
pub mod android_layer;

pub use plugin::LogPlugin;

// Re-export tracing macros
pub use tracing::{debug, error, info, trace, warn, Level};
```

## Usage in flui_app

### Update flui_app/Cargo.toml

```toml
[dependencies]
# ... existing deps ...
flui_log = { path = "../flui_log" }
```

### Update run_app() in flui_app/src/lib.rs

```rust
pub fn run_app<V>(app: V) -> !
where
    V: View + 'static,
{
    // Initialize logging with sensible defaults
    let log_plugin = flui_log::LogPlugin::default()
        .with_filter("info,wgpu=warn,flui_core=debug,flui_app=info");
    
    log_plugin.init();

    flui_log::info!("Starting FLUI app");

    // Rest of initialization...
    let binding = AppBinding::ensure_initialized();
    binding.pipeline.attach_root_widget(app);
    
    // ...
}
```

### Advanced Usage (custom config)

```rust
use flui_log::{LogPlugin, Level};

fn main() {
    // Custom log configuration
    let log = LogPlugin::new()
        .with_level(Level::DEBUG)
        .with_filter("debug,wgpu=error,flui_core=trace");
    
    log.init();

    run_app(MyApp);
}
```

## Benefits

1. **Zero-cost on desktop**: same as before, just better organized
2. **Android support**: logs visible in `adb logcat`
3. **iOS support**: logs visible in Xcode Console and Console.app (using Apple's os_log)
4. **WASM support**: logs visible in browser console
5. **Consistent API**: use `flui_log::info!()` everywhere
6. **Configurable**: filter by module, set levels, etc.
7. **Production-ready**: all platforms supported out of the box

## Testing on Different Platforms

### Desktop
```bash
cargo run
# Logs appear in terminal
```

### Android
```bash
cargo apk run
adb logcat | grep -i flui
# Logs appear in logcat
```

### iOS
```bash
# Build and run on iOS simulator/device
cargo build --target aarch64-apple-ios
# Open Xcode Console or Console.app - logs appear there
# Filter by subsystem: com.flui.app
```

### WASM
```bash
trunk serve
# Open browser console - logs appear there
```

## Comparison with current approach

**Before:**
```rust
tracing_subscriber::fmt()
    .with_target(false)
    .with_level(true)
    .with_line_number(true)
    .init();
```
❌ Only works on desktop
❌ No Android/WASM support
❌ Hard to configure

**After:**
```rust
flui_log::LogPlugin::default().init();
```
✅ Works on all platforms automatically (Desktop, Android, iOS, WASM)
✅ Configurable filters
✅ Follows Bevy's proven approach
✅ Uses native logging systems on each platform

## Next Steps

1. Create `crates/flui_log` directory
2. Implement the basic structure above
3. Test on desktop first
4. Add to `Workspace.toml` dependencies
5. Update `flui_app` to use it
6. Test on Android/iOS/WASM when ready