# flui_app

[![Crates.io](https://img.shields.io/crates/v/flui_app)](https://crates.io/crates/flui_app)
[![Documentation](https://docs.rs/flui_app/badge.svg)](https://docs.rs/flui_app)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Cross-platform application framework for FLUI - Build native apps with Flutter-like development experience.**

FLUI App provides the application layer that bridges your UI components with the underlying platform. It handles window management, event loops, platform integration, and provides a unified API across desktop, mobile, and web platforms.

## Features

- 🖥️ **Desktop Support** - Windows, macOS, Linux with native performance
- 📱 **Mobile Support** - iOS and Android with platform-specific optimizations
- 🌐 **Web Support** - WebAssembly deployment with full feature parity
- ⚡ **GPU Accelerated** - Hardware-accelerated rendering on all platforms
- 🎯 **Event Handling** - Unified input handling (touch, mouse, keyboard, gamepad)
- 🪟 **Window Management** - Multi-window support with flexible configurations
- 🔧 **Developer Tools** - Built-in debugging and profiling capabilities
- 📦 **Asset Management** - Integrated asset loading and caching
- 🚀 **Hot Reload** - Fast development iteration (development builds)

## Quick Start

### Desktop Application

```rust
use flui_app::{App, WindowBuilder};
use flui_widgets::{Text, Center};
use flui_core::View;

#[derive(Debug)]
struct HelloWorld;

impl View for HelloWorld {
    fn build(self, _ctx: &flui_core::BuildContext) -> impl flui_core::IntoElement {
        Center::new(
            Text::new("Hello, FLUI!")
                .size(24.0)
                .color(flui_types::Color::BLUE)
        )
    }
}

fn main() {
    App::new()
        .window(
            WindowBuilder::new()
                .title("My FLUI App")
                .size((800, 600))
                .resizable(true)
        )
        .run(HelloWorld);
}
```

### Mobile Application

```rust
use flui_app::{App, MobileConfig};

fn main() {
    App::new()
        .mobile(
            MobileConfig::new()
                .orientation_lock(flui_app::Orientation::Portrait)
                .fullscreen(true)
        )
        .run(MyMobileApp);
}
```

### Web Application

```rust
use flui_app::{App, WebConfig};

#[cfg(target_arch = "wasm32")]
fn main() {
    App::new()
        .web(
            WebConfig::new()
                .canvas_id("flui-canvas")
                .prevent_default(true)
        )
        .run(MyWebApp);
}
```

## Architecture

FLUI App sits at the top of the framework stack:

```text
┌─────────────────────────────────────────────────────┐
│                   flui_app                          │
│         (Application Framework)                     │
├─────────────────────────────────────────────────────┤
│  flui_widgets │ flui_interaction │ flui_animation   │
│            (UI Components Layer)                    │
├─────────────────────────────────────────────────────┤
│     flui_rendering │ flui_assets │ flui_log         │
│              (Rendering Layer)                      │
├─────────────────────────────────────────────────────┤
│ flui_core │ flui-reactivity │ flui-pipeline        │
│              (Framework Layer)                      │
├─────────────────────────────────────────────────────┤
│  flui_types │ flui-foundation │ flui-tree          │
│              (Foundation Layer)                     │
└─────────────────────────────────────────────────────┘
                        │
            ┌───────────┴───────────┐
            │                       │
┌─────────────────┐    ┌─────────────────┐
│   flui_engine   │    │    Platform     │
│ (GPU Rendering) │    │   (winit/web)   │
└─────────────────┘    └─────────────────┘
```

## Platform Configuration

### Desktop Platforms

```toml
[dependencies]
flui_app = { version = "0.1", features = ["desktop", "widgets"] }
```

**Features:**
- **Multiple Windows** - Create and manage multiple application windows
- **Native Menus** - Platform-native menu bars and context menus  
- **File Dialogs** - Open/save file dialogs with platform integration
- **System Tray** - System tray integration with notifications
- **Native Styling** - Respect system themes and accessibility settings

```rust
use flui_app::{App, WindowBuilder, MenuBuilder};

App::new()
    .window(
        WindowBuilder::new()
            .title("Main Window")
            .size((1200, 800))
            .min_size((600, 400))
            .resizable(true)
            .maximized(false)
    )
    .menu(
        MenuBuilder::new()
            .item("File", |menu| {
                menu.item("New", |_| println!("New file"))
                   .item("Open", |_| println!("Open file"))
                   .separator()
                   .item("Exit", |_| std::process::exit(0))
            })
    )
    .run(MyDesktopApp);
```

### Mobile Platforms

```toml
[dependencies]
flui_app = { version = "0.1", features = ["mobile", "widgets"] }
```

**Android Features:**
- **Activity Lifecycle** - Proper Android activity management
- **Permissions** - Runtime permission handling
- **Intents** - Intent handling for deep linking
- **Hardware Back** - Back button handling
- **Status Bar** - Status bar styling and visibility control

**iOS Features:**
- **View Controller** - Proper iOS view controller integration
- **Safe Areas** - Automatic safe area handling
- **Orientation** - Device orientation support
- **App Lifecycle** - Background/foreground state management

```rust
use flui_app::{App, MobileConfig, Permission};

App::new()
    .mobile(
        MobileConfig::new()
            .orientation_lock(flui_app::Orientation::Portrait)
            .status_bar_style(flui_app::StatusBarStyle::Light)
            .request_permission(Permission::Camera)
            .deep_linking("myapp://")
    )
    .run(MyMobileApp);
```

### Web Platform

```toml
[dependencies]
flui_app = { version = "0.1", features = ["web", "widgets"] }
```

**Web Features:**
- **Canvas Rendering** - Hardware-accelerated WebGL rendering
- **Responsive Design** - Automatic viewport handling
- **Browser Events** - Full browser event integration
- **URL Routing** - Client-side routing with history API
- **Progressive Web App** - PWA support with service workers

```rust
use flui_app::{App, WebConfig};

#[cfg(target_arch = "wasm32")]
fn main() {
    App::new()
        .web(
            WebConfig::new()
                .canvas_id("app-canvas")
                .prevent_default(true)
                .enable_pwa(true)
                .route("/", |_| HomePage)
                .route("/about", |_| AboutPage)
        )
        .run(MyWebApp);
}
```

## Event Handling

### Input Events

FLUI App provides unified input handling across all platforms:

```rust
use flui_app::{App, InputHandler};
use flui_interaction::{PointerEvent, KeyEvent};

struct MyInputHandler;

impl InputHandler for MyInputHandler {
    fn on_pointer_event(&mut self, event: &PointerEvent) -> bool {
        match event {
            PointerEvent::Down { position, .. } => {
                println!("Touch/click at {:?}", position);
                true // Event handled
            }
            PointerEvent::Move { position, .. } => {
                println!("Drag to {:?}", position);
                true
            }
            _ => false // Event not handled
        }
    }
    
    fn on_key_event(&mut self, event: &KeyEvent) -> bool {
        if event.key == "Escape" {
            std::process::exit(0);
        }
        false
    }
}

App::new()
    .input_handler(MyInputHandler)
    .run(MyApp);
```

### Window Events

```rust
use flui_app::{App, WindowEventHandler, WindowEvent};

struct MyWindowHandler;

impl WindowEventHandler for MyWindowHandler {
    fn on_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Resized(size) => {
                println!("Window resized to {:?}", size);
            }
            WindowEvent::CloseRequested => {
                println!("Close requested");
                // Custom close handling
            }
            WindowEvent::Minimized => {
                println!("Window minimized");
            }
            _ => {}
        }
    }
}
```

## Asset Integration

FLUI App integrates with the asset system for seamless resource loading:

```rust
use flui_app::{App, AssetConfig};
use flui_assets::{ImageAsset, FontAsset};

App::new()
    .assets(
        AssetConfig::new()
            .root_dir("assets/")
            .preload(&[
                ImageAsset::file("logo.png"),
                FontAsset::file("fonts/Roboto-Regular.ttf"),
            ])
            .cache_size(100 * 1024 * 1024) // 100MB cache
    )
    .run(MyApp);
```

## State Management

FLUI App integrates with the reactive system:

```rust
use flui_app::{App, AppState};
use flui_reactivity::{Signal, use_signal};

#[derive(Debug, Clone)]
struct GlobalState {
    user_id: Option<u32>,
    theme: Theme,
    settings: Settings,
}

impl AppState for GlobalState {
    fn default() -> Self {
        GlobalState {
            user_id: None,
            theme: Theme::Light,
            settings: Settings::default(),
        }
    }
}

App::new()
    .state(GlobalState::default())
    .run(MyApp);

// In your components:
impl View for MyComponent {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let app_state = ctx.app_state::<GlobalState>();
        
        if app_state.user_id.is_some() {
            UserDashboard
        } else {
            LoginScreen
        }
    }
}
```

## Performance Optimization

### Production Configuration

```rust
use flui_app::{App, PerformanceConfig};

App::new()
    .performance(
        PerformanceConfig::production()
            .target_fps(60)
            .vsync(true)
            .gpu_preference(flui_app::GPUPreference::HighPerformance)
            .memory_limit(512 * 1024 * 1024) // 512MB
    )
    .run(MyApp);
```

### Development Configuration

```rust
App::new()
    .performance(
        PerformanceConfig::development()
            .hot_reload(true)
            .debug_overlay(true)
            .performance_monitor(true)
            .memory_profiler(true)
    )
    .run(MyApp);
```

## Multi-Window Applications

```rust
use flui_app::{App, WindowBuilder, WindowId};

struct MultiWindowApp;

impl flui_core::View for MultiWindowApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let app = ctx.app();
        
        Column::new()
            .children(vec![
                Button::new("Open Settings")
                    .on_press(move || {
                        app.create_window(
                            WindowBuilder::new()
                                .title("Settings")
                                .size((400, 300))
                                .resizable(false),
                            SettingsWindow
                        );
                    }),
                Button::new("Open About")
                    .on_press(move || {
                        app.create_window(
                            WindowBuilder::new()
                                .title("About")
                                .size((300, 200)),
                            AboutWindow
                        );
                    }),
            ])
    }
}
```

## Platform-Specific Code

```rust
#[cfg(target_os = "windows")]
fn setup_windows_specific() {
    // Windows-specific setup
}

#[cfg(target_os = "macos")]
fn setup_macos_specific() {
    // macOS-specific setup
}

#[cfg(target_os = "linux")]
fn setup_linux_specific() {
    // Linux-specific setup
}

#[cfg(target_os = "android")]
fn setup_android_specific() {
    // Android-specific setup
}

#[cfg(target_os = "ios")]
fn setup_ios_specific() {
    // iOS-specific setup
}

fn main() {
    #[cfg(target_os = "windows")]
    setup_windows_specific();
    
    #[cfg(target_os = "macos")]
    setup_macos_specific();
    
    // ... other platforms
    
    App::new().run(MyApp);
}
```

## Feature Flags

### Core Features

```toml
[dependencies]
flui_app = { version = "0.1", default-features = false, features = [
    "desktop",    # Desktop platform support
    "mobile",     # Mobile platform support  
    "web",        # Web platform support
    "widgets",    # Include widget library
    "images",     # Image loading support
] }
```

### Optional Features

```toml
flui_app = { version = "0.1", features = [
    "pretty-logs",  # Pretty hierarchical logging
    "devtools",     # Developer tools integration
    "android",      # Android-specific features
    "ios",          # iOS-specific features
] }
```

## Examples

### Hello World

```rust
use flui_app::App;
use flui_widgets::{Text, Center};

fn main() {
    App::new().run(
        Center::new(Text::new("Hello, World!"))
    );
}
```

### Counter App

```rust
use flui_app::App;
use flui_widgets::{Column, Text, Button};
use flui_reactivity::use_signal;

#[derive(Debug)]
struct CounterApp;

impl flui_core::View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);
        
        Column::new()
            .spacing(20.0)
            .main_axis_alignment(MainAxisAlignment::Center)
            .children(vec![
                Text::new(format!("Count: {}", count.get(ctx)))
                    .size(24.0),
                Button::new("Increment")
                    .on_press(move || count.update(|n| *n + 1)),
            ])
    }
}

fn main() {
    App::new().run(CounterApp);
}
```

### Todo App

```rust
use flui_app::{App, AppState};
use flui_widgets::*;
use flui_reactivity::*;

#[derive(Debug, Clone)]
struct Todo {
    id: u32,
    text: String,
    completed: bool,
}

#[derive(Debug, Clone)]
struct TodoAppState {
    todos: Vec<Todo>,
    next_id: u32,
}

impl AppState for TodoAppState {
    fn default() -> Self {
        TodoAppState {
            todos: vec![],
            next_id: 1,
        }
    }
}

#[derive(Debug)]
struct TodoApp;

impl View for TodoApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let state = use_signal(ctx, TodoAppState::default());
        let input_text = use_signal(ctx, String::new());
        
        Column::new()
            .padding(EdgeInsets::all(20.0))
            .children(vec![
                // Title
                Text::new("Todo App")
                    .size(32.0)
                    .weight(FontWeight::Bold),
                
                // Input section
                Row::new()
                    .spacing(10.0)
                    .children(vec![
                        Expanded::new(
                            TextField::new()
                                .placeholder("Enter todo...")
                                .value(input_text.get(ctx))
                                .on_changed(move |text| input_text.set(text))
                        ),
                        Button::new("Add")
                            .on_press(move || {
                                let text = input_text.get(ctx);
                                if !text.trim().is_empty() {
                                    state.update(|s| {
                                        s.todos.push(Todo {
                                            id: s.next_id,
                                            text: text.clone(),
                                            completed: false,
                                        });
                                        s.next_id += 1;
                                    });
                                    input_text.set(String::new());
                                }
                            }),
                    ]),
                
                // Todo list
                Expanded::new(
                    ListView::builder()
                        .item_count(state.get(ctx).todos.len())
                        .item_builder(move |ctx, index| {
                            let todo = &state.get(ctx).todos[index];
                            TodoItem::new(todo.clone(), state.clone())
                        })
                ),
            ])
    }
}

fn main() {
    App::new()
        .state(TodoAppState::default())
        .run(TodoApp);
}
```

## Build and Deployment

### Desktop Build

```bash
# Development build
cargo run

# Release build
cargo build --release

# Platform-specific optimizations
cargo build --release --target x86_64-pc-windows-msvc    # Windows
cargo build --release --target x86_64-apple-darwin       # macOS
cargo build --release --target x86_64-unknown-linux-gnu  # Linux
```

### Mobile Build

```bash
# Android
cargo install cargo-ndk
rustup target add aarch64-linux-android
cargo ndk --target aarch64-linux-android build --release

# iOS
cargo install cargo-lipo
rustup target add aarch64-apple-ios
cargo lipo --release
```

### Web Build

```bash
# Install wasm tools
cargo install wasm-pack
rustup target add wasm32-unknown-unknown

# Build for web
wasm-pack build --target web --out-dir pkg

# Serve locally
python -m http.server 8000
```

## Performance Characteristics

### Memory Usage
- **Minimal overhead:** ~2MB base memory usage
- **Efficient caching:** Asset cache with LRU eviction
- **GPU memory:** Automatic management with frame budgets

### Rendering Performance
- **60fps target:** Optimized for smooth 60fps rendering
- **GPU acceleration:** Hardware-accelerated on all platforms
- **Adaptive quality:** Dynamic quality adjustment under load

### Build Times
- **Incremental builds:** Only recompile changed crates
- **Parallel compilation:** Multi-core build optimization
- **Feature-gated:** Only compile needed platform code

## Thread Safety

FLUI App is designed for multi-threaded environments:

- **UI Thread:** Main application and rendering thread
- **Worker Threads:** Background asset loading and processing
- **Platform Threads:** Platform-specific event handling
- **Async Runtime:** Built-in tokio integration for async operations

```rust
use flui_app::{App, AsyncConfig};

App::new()
    .async_runtime(
        AsyncConfig::new()
            .worker_threads(4)
            .enable_io(true)
            .enable_time(true)
    )
    .run(MyAsyncApp);
```

## Error Handling

```rust
use flui_app::{App, ErrorHandler, AppError};

struct MyErrorHandler;

impl ErrorHandler for MyErrorHandler {
    fn handle_error(&mut self, error: &AppError) -> bool {
        match error {
            AppError::RenderingError(e) => {
                eprintln!("Rendering error: {}", e);
                false // Continue running
            }
            AppError::AssetLoadError(path, e) => {
                eprintln!("Failed to load asset {}: {}", path, e);
                false // Continue running
            }
            AppError::PlatformError(e) => {
                eprintln!("Platform error: {}", e);
                true // Terminate app
            }
            _ => false
        }
    }
}

App::new()
    .error_handler(MyErrorHandler)
    .run(MyApp);
```

## Testing

FLUI App provides testing utilities for application-level tests:

```rust
use flui_app::testing::{AppTester, SimulateInput};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_app() {
        let mut tester = AppTester::new(CounterApp);
        
        // Initial state
        tester.expect_text("Count: 0");
        
        // Click increment button
        tester.tap_button("Increment");
        tester.expect_text("Count: 1");
        
        // Multiple clicks
        for i in 2..=5 {
            tester.tap_button("Increment");
            tester.expect_text(&format!("Count: {}", i));
        }
    }
}
```

## Contributing

We welcome contributions to FLUI App! See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Clone repository
git clone https://github.com/flui-org/flui.git
cd flui

# Install dependencies
cargo build --workspace

# Run tests
cargo test -p flui_app

# Run examples
cargo run --example counter
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui_core`](../flui_core) - Core framework implementation
- [`flui_widgets`](../flui_widgets) - Widget library
- [`flui_engine`](../flui_engine) - GPU rendering engine
- [`flui-reactivity`](../flui-reactivity) - Reactive state management
- [`flui_interaction`](../flui_interaction) - Input handling and gestures

---

**FLUI App** - Write once, run everywhere. Native performance, web reach, mobile touch.