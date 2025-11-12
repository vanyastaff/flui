# FLUI App Documentation

> **Application framework for FLUI - Platform integration, lifecycle management, and app initialization**

Version: 0.7.0  
Status: Production Ready

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Getting Started](#getting-started)
4. [Core Concepts](#core-concepts)
5. [Platform Support](#platform-support)
6. [API Reference](#api-reference)
7. [Examples](#examples)
8. [Advanced Topics](#advanced-topics)
9. [Troubleshooting](#troubleshooting)

---

## Overview

### What is flui_app?

`flui_app` is the application framework layer for FLUI that provides:

- **Application initialization** - `run_app()` entry point
- **Platform integration** - Window creation, event loops, input handling
- **Multi-platform support** - Desktop (Windows/Linux/macOS), Mobile (Android/iOS), Web
- **Binding system** - Flutter-inspired binding architecture
- **Hot reload** - Development workflow optimization

### Key Features

✅ **Cross-platform** - Write once, run everywhere  
✅ **Zero-cost abstractions** - No runtime overhead  
✅ **Type-safe** - Full Rust type safety  
✅ **GPU-accelerated** - wgpu-based rendering  
✅ **Hot reload ready** - Fast iteration cycles  
✅ **Production tested** - Battle-tested architecture

---

## Architecture

### Three-Layer System

```
┌─────────────────────────────────────────┐
│         Framework Layer                 │
│  (flui_app, flui_widgets, flui_core)    │
│  • App initialization                   │
│  • Widget tree management               │
│  • State management                     │
└─────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────┐
│          Engine Layer                   │
│  (flui_engine, flui_rendering)          │
│  • GPU rendering (wgpu)                 │
│  • Layout engine                        │
│  • Animation system                     │
└─────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────┐
│         Embedder Layer                  │
│  (platform-specific in flui_app)        │
│  • Window creation (winit)              │
│  • Event loop                           │
│  • Input handling                       │
└─────────────────────────────────────────┘
```

### Initialization Flow

```
main()
  ↓
run_app(root_view)
  ↓
AppBinding::ensure_initialized()
  ↓
Initialize Bindings:
  1. GestureBinding (input)
  2. SchedulerBinding (frames)
  3. RendererBinding (rendering)
  4. WidgetsBinding (widgets)
  ↓
Create Platform Embedder
  ↓
Create Window (winit)
  ↓
Attach Root Widget
  ↓
Schedule First Frame
  ↓
Run Event Loop ♻️
```

---

## Getting Started

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_app = "0.7"
flui_core = "0.7"
flui_widgets = "0.7"
```

### Hello World

```rust
use flui_app::run_app;
use flui_core::prelude::*;
use flui_widgets::Text;

#[derive(Debug)]
struct HelloWorld;

impl View for HelloWorld {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Hello, FLUI!")
    }
}

fn main() {
    run_app(HelloWorld);
}
```

### Counter Example

```rust
use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::hooks::use_signal;
use flui_widgets::*;

#[derive(Debug)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);
        
        Column::new()
            .spacing(20.0)
            .children(vec![
                Box::new(Text::new(format!("Count: {}", count.get(ctx)))
                    .size(32.0)),
                
                Box::new(Button::new("Increment")
                    .on_pressed(move || count.update(|n| *n += 1))),
                
                Box::new(Button::new("Decrement")
                    .on_pressed(move || count.update(|n| *n -= 1))),
                
                Box::new(Button::new("Reset")
                    .on_pressed(move || count.set(0))),
            ])
    }
}

fn main() {
    run_app(CounterApp);
}
```

---

## Core Concepts

### 1. Application Entry Point

#### `run_app()`

The main entry point for FLUI applications.

```rust
pub fn run_app<V>(app: V) -> !
where
    V: View + 'static
```

**Example:**

```rust
fn main() {
    run_app(MyApp);
}
```

**What it does:**

1. Initializes tracing/logging
2. Creates `AppBinding` singleton
3. Attaches root widget
4. Creates platform embedder (WgpuEmbedder)
5. Enters event loop (never returns)

### 2. Binding System

Inspired by Flutter's binding architecture, FLUI uses a layered binding system:

#### AppBinding

Central coordinator that combines all bindings:

```rust
pub struct AppBinding {
    pub gesture: GestureBinding,
    pub scheduler: SchedulerBinding,
    pub renderer: RendererBinding,
    pub widgets: WidgetsBinding,
    pub pipeline: Arc<PipelineOwner>,
}
```

**Lifecycle:**

```rust
// Singleton pattern
let binding = AppBinding::ensure_initialized();

// Attach root widget
binding.pipeline.attach_root_widget(app);

// Schedule frame
binding.scheduler.schedule_frame();
```

#### GestureBinding

Handles pointer events (mouse, touch, pen):

```rust
pub struct GestureBinding {
    hit_test_result: Arc<RwLock<HitTestResult>>,
    pointer_router: Arc<PointerRouter>,
}
```

#### SchedulerBinding

Manages frame callbacks and scheduling:

```rust
pub struct SchedulerBinding {
    frame_callbacks: Vec<FrameCallback>,
    frame_number: AtomicU64,
}
```

#### RendererBinding

Bridges to rendering engine:

```rust
pub struct RendererBinding {
    pipeline: Arc<PipelineOwner>,
    surface: Arc<RwLock<Surface>>,
}
```

#### WidgetsBinding

Manages widget tree lifecycle:

```rust
pub struct WidgetsBinding {
    root_element: Arc<RwLock<Option<AnyElement>>>,
    dirty_elements: Arc<RwLock<Vec<ElementId>>>,
}
```

### 3. Platform Embedder

Platform-specific window and event loop management.

#### WgpuEmbedder

The production embedder using wgpu + winit:

```rust
pub struct WgpuEmbedder {
    binding: Arc<AppBinding>,
    window: Arc<Window>,
    surface: Surface,
    device: Arc<Device>,
    queue: Arc<Queue>,
}
```

**Responsibilities:**

- Window creation via winit
- GPU initialization via wgpu
- Event loop management
- Frame rendering
- Input event dispatch

### 4. Event Loop

The heart of the application that processes events and renders frames:

```rust
// Simplified event loop
loop {
    // 1. Process window events
    for event in events {
        match event {
            WindowEvent::Resized(size) => handle_resize(size),
            WindowEvent::CursorMoved(pos) => handle_pointer_move(pos),
            WindowEvent::MouseInput(button) => handle_pointer_down(button),
            WindowEvent::CloseRequested => break,
            _ => {}
        }
    }
    
    // 2. Run scheduled tasks
    scheduler.run_tasks();
    
    // 3. Build frame (if dirty)
    if needs_rebuild {
        pipeline.flush_build();
    }
    
    // 4. Layout (if needed)
    if needs_layout {
        pipeline.flush_layout(constraints);
    }
    
    // 5. Paint
    let layer = pipeline.flush_paint();
    
    // 6. Composite to GPU
    render_to_surface(layer);
    
    // 7. Present
    surface.present();
}
```

---

## Platform Support

### Desktop

#### Windows

**Requirements:**
- Rust 1.70+
- MSVC or GNU toolchain

**Running:**
```bash
cargo run --release
```

**Features:**
- Native window decorations
- High-DPI support
- Multi-monitor support
- Hardware acceleration (DirectX 12)

#### Linux

**Requirements:**
- Rust 1.70+
- X11 or Wayland
- Vulkan drivers

**Running:**
```bash
cargo run --release
```

**Features:**
- X11/Wayland support
- High-DPI support (via env vars)
- Hardware acceleration (Vulkan)

#### macOS

**Requirements:**
- Rust 1.70+
- macOS 10.15+
- Xcode Command Line Tools

**Running:**
```bash
cargo run --release
```

**Features:**
- Native window decorations
- Retina display support
- Hardware acceleration (Metal)

### Mobile

#### Android

**Requirements:**
- Android SDK (API 26+)
- Android NDK (r25+)
- cargo-ndk

**Setup:**
```bash
rustup target add aarch64-linux-android
cargo install cargo-ndk
```

**Building:**
```bash
cargo ndk -t arm64-v8a \
  -o platforms/android/app/src/main/jniLibs \
  build --release
```

**Entry Point:**
```rust
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
    );
    
    run_app(MyApp);
}
```

#### iOS

**Requirements:**
- macOS with Xcode
- iOS 13.0+
- Rust iOS targets

**Setup:**
```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

**Building:**
```bash
cargo build --target aarch64-apple-ios --release
```

**Entry Point:**
```rust
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn start_flui_app() {
    run_app(MyApp);
}
```

### Web

**Requirements:**
- wasm-pack
- Web browser with WebGPU support

**Setup:**
```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

**Building:**
```bash
wasm-pack build \
  --target web \
  --out-dir platforms/web/pkg \
  --release
```

**Entry Point:**
```rust
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());
    
    run_app(MyApp);
}
```

---

## API Reference

### Core Functions

#### `run_app<V>(app: V) -> !`

Main entry point for FLUI applications.

**Type Parameters:**
- `V: View + 'static` - Root view of the application

**Panics:**
- If window creation fails
- If GPU initialization fails
- If root widget is already attached

**Example:**
```rust
#[derive(Debug)]
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Hello!")
    }
}

fn main() {
    run_app(MyApp);
}
```

### AppBinding

#### `AppBinding::ensure_initialized() -> Arc<AppBinding>`

Returns the global AppBinding singleton, initializing it if needed.

**Thread Safety:** Thread-safe, uses `Once` for initialization.

**Example:**
```rust
let binding = AppBinding::ensure_initialized();
println!("Frame number: {}", binding.scheduler.current_frame());
```

#### `binding.pipeline.attach_root_widget<V: View>(widget: V)`

Attaches the root widget to the pipeline.

**Panics:** If root widget is already attached.

### WgpuEmbedder

#### `WgpuEmbedder::new(binding, event_loop) -> WgpuEmbedder`

Creates a new embedder with GPU initialization.

**Async:** Requires async runtime (uses `pollster::block_on`).

#### `embedder.run(event_loop) -> !`

Enters the event loop (never returns).

### Platform-Specific APIs

#### Window Configuration

```rust
use winit::window::WindowBuilder;

// Custom window (advanced usage)
let window = WindowBuilder::new()
    .with_title("My FLUI App")
    .with_inner_size(winit::dpi::LogicalSize::new(800, 600))
    .with_resizable(true)
    .build(&event_loop)?;
```

---

## Examples

### Example 1: Multi-Page App

```rust
use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::hooks::{use_signal, use_memo};
use flui_widgets::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Page {
    Home,
    Profile,
    Settings,
}

#[derive(Debug)]
struct MultiPageApp;

impl View for MultiPageApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let current_page = use_signal(ctx, Page::Home);
        
        Column::new()
            .children(vec![
                // Navigation bar
                Box::new(Row::new()
                    .spacing(10.0)
                    .children(vec![
                        Box::new(Button::new("Home")
                            .on_pressed(move || current_page.set(Page::Home))),
                        Box::new(Button::new("Profile")
                            .on_pressed(move || current_page.set(Page::Profile))),
                        Box::new(Button::new("Settings")
                            .on_pressed(move || current_page.set(Page::Settings))),
                    ])),
                
                // Content
                Box::new(match current_page.get(ctx) {
                    Page::Home => HomePage.into_element(),
                    Page::Profile => ProfilePage.into_element(),
                    Page::Settings => SettingsPage.into_element(),
                }),
            ])
    }
}

#[derive(Debug)]
struct HomePage;

impl View for HomePage {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Welcome Home!")
    }
}

// ... ProfilePage, SettingsPage ...

fn main() {
    run_app(MultiPageApp);
}
```

### Example 2: Theme Provider

```rust
use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::hooks::use_signal;
use flui_widgets::*;

#[derive(Debug, Clone, Copy)]
struct Theme {
    background: (u8, u8, u8),
    text: (u8, u8, u8),
    primary: (u8, u8, u8),
}

impl Theme {
    fn light() -> Self {
        Self {
            background: (255, 255, 255),
            text: (0, 0, 0),
            primary: (100, 150, 255),
        }
    }
    
    fn dark() -> Self {
        Self {
            background: (30, 30, 30),
            text: (255, 255, 255),
            primary: (255, 150, 100),
        }
    }
}

#[derive(Debug)]
struct ThemedApp;

impl View for ThemedApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let is_dark = use_signal(ctx, false);
        let theme = if is_dark.get(ctx) {
            Theme::dark()
        } else {
            Theme::light()
        };
        
        Container::new(
            Column::new()
                .children(vec![
                    Box::new(Text::new("Themed App")
                        .color(theme.text)),
                    
                    Box::new(Button::new("Toggle Theme")
                        .on_pressed(move || is_dark.update(|d| *d = !*d))),
                ])
        )
        .background(theme.background)
    }
}

fn main() {
    run_app(ThemedApp);
}
```

### Example 3: Async Data Loading

```rust
use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::hooks::{use_signal, use_effect};
use flui_widgets::*;

#[derive(Debug, Clone)]
enum LoadState<T> {
    Loading,
    Loaded(T),
    Error(String),
}

#[derive(Debug)]
struct AsyncApp;

impl View for AsyncApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let data = use_signal(ctx, LoadState::Loading);
        
        // Load data on mount
        use_effect(ctx, move |_| {
            std::thread::spawn(move || {
                // Simulate async load
                std::thread::sleep(std::time::Duration::from_secs(2));
                data.set(LoadState::Loaded("Hello from async!".to_string()));
            });
            None // No cleanup
        });
        
        match data.get(ctx) {
            LoadState::Loading => Text::new("Loading...").into_element(),
            LoadState::Loaded(text) => Text::new(text).into_element(),
            LoadState::Error(err) => Text::new(format!("Error: {}", err)).into_element(),
        }
    }
}

fn main() {
    run_app(AsyncApp);
}
```

---

## Advanced Topics

### Hot Reload

Hot reload allows you to see changes instantly without restarting the app.

#### Development Setup

```bash
# Terminal 1: Watch for changes
cargo watch -x 'build --release'

# Terminal 2: Run app
cargo run --release
```

#### Implementation

```rust
#[cfg(debug_assertions)]
pub fn enable_hot_reload() {
    use notify::{Watcher, RecursiveMode};
    
    let (tx, rx) = std::sync::mpsc::channel();
    
    let mut watcher = notify::watcher(tx, Duration::from_secs(1)).unwrap();
    watcher.watch("./src", RecursiveMode::Recursive).unwrap();
    
    std::thread::spawn(move || {
        loop {
            match rx.recv() {
                Ok(_) => {
                    // Trigger rebuild
                    AppBinding::ensure_initialized()
                        .pipeline
                        .mark_needs_rebuild();
                }
                Err(e) => eprintln!("Watch error: {}", e),
            }
        }
    });
}
```

### Custom Embedders

Create custom embedders for specialized platforms:

```rust
pub trait PlatformEmbedder {
    fn create_window(&self) -> Result<Window>;
    fn run_event_loop(&mut self) -> !;
    fn render_frame(&mut self, layer: BoxedLayer);
}

pub struct CustomEmbedder {
    // Your custom implementation
}

impl PlatformEmbedder for CustomEmbedder {
    // Implement trait methods
}
```

### Performance Optimization

#### Frame Budget

Control frame timing:

```rust
let scheduler = binding.scheduler();
scheduler.set_frame_budget(Duration::from_millis(16)); // 60 FPS
```

#### Batch Updates

Reduce rebuilds:

```rust
// Bad: Multiple updates = multiple rebuilds
count.set(1);
count.set(2);
count.set(3);

// Good: Single update
count.update(|n| *n = 3);
```

#### Layout Caching

```rust
// Render objects cache layout automatically
impl LeafRender for MyRender {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Expensive computation cached by framework
        expensive_calculation()
    }
}
```

---

## Troubleshooting

### Common Issues

#### "Failed to create window"

**Cause:** Missing graphics drivers or unsupported platform.

**Solution:**
```bash
# Linux: Install Vulkan drivers
sudo apt install vulkan-tools

# Check GPU support
vulkaninfo
```

#### "wgpu initialization failed"

**Cause:** No compatible GPU backend.

**Solution:**
```rust
// Use software renderer as fallback
use wgpu::Backend;

let backends = wgpu::Backends::VULKAN 
    | wgpu::Backends::DX12 
    | wgpu::Backends::METAL
    | wgpu::Backends::GL; // Software fallback
```

#### "Root widget already attached"

**Cause:** Called `run_app()` twice.

**Solution:**
```rust
// Only call run_app once
fn main() {
    run_app(MyApp); // ✓
    // run_app(OtherApp); // ✗ Error!
}
```

#### High CPU Usage

**Cause:** Continuous redraw loop.

**Solution:**
```rust
// Use on-demand rendering
scheduler.set_render_mode(RenderMode::OnDemand);

// Only render when needed
if needs_redraw {
    scheduler.schedule_frame();
}
```

### Platform-Specific Issues

#### Android: "JNI_OnLoad not found"

**Cause:** Library not built correctly.

**Solution:**
```bash
# Ensure correct library name in AndroidManifest.xml
<meta-data android:name="android.app.lib_name" android:value="your_crate_name" />
```

#### iOS: "dyld: Library not loaded"

**Cause:** Library not copied to app bundle.

**Solution:**
- Check Xcode build phases
- Ensure library is in "Link Binary With Libraries"

#### Web: "WebGPU not supported"

**Cause:** Browser doesn't support WebGPU.

**Solution:**
- Use Chrome 113+ or Edge 113+
- Enable WebGPU in `chrome://flags`

---

## Best Practices

### 1. Application Structure

```rust
// ✓ Good: Modular structure
mod ui {
    pub mod home;
    pub mod profile;
    pub mod settings;
}

fn main() {
    run_app(ui::home::HomePage);
}
```

### 2. Error Handling

```rust
// ✓ Good: Graceful error handling
match load_data() {
    Ok(data) => render_content(data),
    Err(e) => render_error(e),
}
```

### 3. State Management

```rust
// ✓ Good: Centralized state
#[derive(Debug, Clone)]
struct AppState {
    user: Option<User>,
    theme: Theme,
}

let state = use_signal(ctx, AppState::default());
```

### 4. Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_app_builds() {
        let app = MyApp;
        // Test view building logic
    }
}
```

---

## Resources

### Documentation
- [FLUI Core Docs](../flui_core/README.md)
- [Widget Library](../flui_widgets/README.md)
- [Rendering Engine](../flui_engine/README.md)

### Examples
- [Hello World](examples/hello_world.rs)
- [Counter App](examples/counter_demo.rs)
- [Todo List](examples/todo_app.rs)

### External Resources
- [wgpu Documentation](https://wgpu.rs/)
- [winit Documentation](https://docs.rs/winit/)
- [Flutter Architecture](https://docs.flutter.dev/resources/architectural-overview)

---

## Contributing

We welcome contributions! Areas for improvement:

- Additional platform support
- Performance optimizations
- Documentation improvements
- Example applications

See [CLAUDE.md](../../CLAUDE.md) for development guidelines.

---

## License

MIT OR Apache-2.0

---

## Changelog

### v0.7.0 (Current)
- ✨ Unified View API
- ✨ Thread-safe hooks
- ✨ Multi-platform support
- 🐛 Fixed memory leaks
- 📚 Complete documentation

### v0.6.0
- ✨ Initial release
- ✅ Desktop support
- ✅ Basic embedder

---

**Built with ❤️ in Rust**

*"Flutter's architecture meets Rust's performance and safety"*
