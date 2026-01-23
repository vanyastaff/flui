# flui-platform Architecture

Modern platform abstraction layer for FLUI, inspired by GPUI's architecture.

## Overview

The `flui-platform` crate provides a complete, type-safe platform abstraction that enables FLUI to run on multiple platforms (Windows, macOS, Linux) with a unified API.

## Architecture Principles

### 1. **Central Platform Trait** (inspired by GPUI)

All platform implementations must provide:
- **Executors**: Background and foreground task execution
- **Lifecycle**: Event loop, quit, frame requests
- **Windows**: Creation, management, events
- **Displays**: Monitor enumeration and information
- **Text System**: Font loading and text rendering
- **Clipboard**: Read/write operations
- **Callbacks**: Event handler registration

### 2. **Type-Safe Units** (GPUI pattern)

Following GPUI's approach, we use type-safe coordinate systems:

```rust
// Logical pixels for UI layout (f32)
Size<Pixels>
Point<Pixels>

// Physical pixels for rendering (i32)
Size<DevicePixels>
Point<DevicePixels>
```

This prevents accidental mixing of coordinate systems at compile time.

### 3. **Callback Registry Pattern**

The `PlatformHandlers` struct decouples the framework from platform implementations:

```rust
pub struct PlatformHandlers {
    pub quit: Option<Box<dyn FnMut() + Send>>,
    pub window_event: Option<Box<dyn FnMut(WindowEvent) + Send>>,
    // ...
}
```

Framework registers handlers, platform invokes them when events occur.

### 4. **Interior Mutability**

Platform implementations use `Arc<Mutex<State>>` for thread-safe `&self` methods:

```rust
pub struct WinitPlatform {
    state: Arc<Mutex<WinitPlatformState>>,
}

impl Platform for WinitPlatform {
    fn quit(&self) {
        self.state.lock().is_running = false;
    }
}
```

## Project Structure

```
flui-platform/
├── src/
│   ├── traits/              # Core abstractions
│   │   ├── platform.rs      # Platform trait (primary interface)
│   │   ├── window.rs        # PlatformWindow trait
│   │   ├── display.rs       # PlatformDisplay trait
│   │   ├── capabilities.rs  # PlatformCapabilities (dyn-safe)
│   │   ├── lifecycle.rs     # PlatformLifecycle
│   │   └── embedder.rs      # PlatformEmbedder (legacy)
│   │
│   ├── shared/              # Shared infrastructure
│   │   └── handlers.rs      # PlatformHandlers callback registry
│   │
│   ├── platforms/           # Concrete implementations
│   │   ├── winit/           # Cross-platform (Windows/macOS/Linux)
│   │   │   ├── platform.rs  # WinitPlatform implementation
│   │   │   └── mod.rs
│   │   │
│   │   ├── headless/        # Testing implementation
│   │   │   ├── platform.rs  # HeadlessPlatform (no-op)
│   │   │   └── mod.rs
│   │   │
│   │   └── mod.rs
│   │
│   └── lib.rs               # Public API + current_platform()
│
├── Cargo.toml
└── ARCHITECTURE.md          # This file
```

## Key Traits

### Platform

Central trait that all platforms must implement:

```rust
pub trait Platform: Send + Sync + 'static {
    // Core system
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
    
    // Lifecycle
    fn run(&self, on_ready: Box<dyn FnOnce()>);
    fn quit(&self);
    fn request_frame(&self);
    
    // Windows
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
    fn active_window(&self) -> Option<WindowId>;
    
    // Displays
    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>>;
    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>>;
    
    // Input
    fn clipboard(&self) -> Arc<dyn Clipboard>;
    
    // Metadata
    fn capabilities(&self) -> &dyn PlatformCapabilities;
    fn name(&self) -> &'static str;
    
    // Callbacks
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);
    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>);
    
    // File system
    fn reveal_path(&self, path: &Path);
    fn open_path(&self, path: &Path);
    fn app_path(&self) -> Result<PathBuf>;
}
```

### PlatformWindow

Window abstraction with type-safe coordinates:

```rust
pub trait PlatformWindow: Send + Sync {
    fn physical_size(&self) -> Size<DevicePixels>;  // Physical pixels (i32)
    fn logical_size(&self) -> Size<Pixels>;         // Logical pixels (f32)
    fn scale_factor(&self) -> f64;                  // DPI scale
    fn request_redraw(&self);
    fn is_focused(&self) -> bool;
    fn is_visible(&self) -> bool;
}
```

### PlatformDisplay

Display/monitor information:

```rust
pub trait PlatformDisplay: Send + Sync {
    fn id(&self) -> DisplayId;
    fn name(&self) -> String;
    fn bounds(&self) -> Bounds<DevicePixels>;        // Physical bounds (type-safe)
    fn usable_bounds(&self) -> Bounds<DevicePixels>; // Excluding taskbars
    fn scale_factor(&self) -> f64;
    fn refresh_rate(&self) -> f64;
    fn is_primary(&self) -> bool;
    fn logical_size(&self) -> Size<Pixels>;          // Type-safe logical size
}
```

**Note**: Uses `Bounds<DevicePixels>` instead of `Rect` to follow GPUI's type-safe approach and prevent mixing coordinate systems.

## Platform Implementations

### WinitPlatform

Cross-platform implementation using `winit`:

- **Target**: Windows, macOS, Linux desktop
- **Windowing**: winit 0.30 with `ApplicationHandler` trait
- **Clipboard**: arboard 3.4 (cross-platform clipboard access)
- **Event Loop**: Proper integration using `ApplicationHandler` pattern
- **Displays**: Monitor enumeration and information
- **Status**: Core functionality complete
  - ✅ Event loop with ApplicationHandler
  - ✅ Display/monitor enumeration
  - ✅ Clipboard read/write
  - ✅ Platform capabilities
  - 🚧 Window creation (requires ActiveEventLoop access)
  - 🚧 Text system (using simple fallback)

**Architecture Decision**: Unlike GPUI which uses native APIs (Win32, Cocoa, Wayland/X11), 
FLUI uses winit as a cross-platform abstraction. This trade-off provides simpler 
implementation and faster multi-platform support at the cost of some native features.

### HeadlessPlatform

No-op implementation for testing:

- **Target**: Unit tests, CI environments
- **Features**: Mock windows, in-memory clipboard, immediate task execution
- **Status**: Complete and tested

## Usage

### Basic Usage

```rust
use flui_platform::current_platform;

let platform = current_platform();
platform.run(Box::new(|| {
    println!("Platform: {}", platform.name());
}));
```

### Environment Variables

- `FLUI_HEADLESS=1` - Force headless mode (useful for CI)

### Testing

```rust
use flui_platform::{HeadlessPlatform, Platform};

let platform = HeadlessPlatform::new();
let window = platform.open_window(Default::default())?;
assert_eq!(window.logical_size(), Size::new(px(800.0), px(600.0)));
```

## Comparison with GPUI

| Feature | GPUI | FLUI Platform |
|---------|------|---------------|
| Central trait | ✅ `Platform` | ✅ `Platform` |
| Type-safe units | ✅ `Pixels`, `DevicePixels` | ✅ Same types from flui_types |
| Callback registry | ✅ Handlers in state | ✅ `PlatformHandlers` |
| Winit support | ✅ Per-platform | ✅ `WinitPlatform` |
| Headless testing | ✅ `TestPlatform` | ✅ `HeadlessPlatform` |
| Native backends | ✅ Mac/Windows/Linux | 🚧 Planned |

## Future Roadmap

### Phase 1: Core Functionality ✅
- ✅ Platform trait design
- ✅ Type-safe coordinates (Pixels/DevicePixels)
- ✅ WinitPlatform structure
- ✅ HeadlessPlatform for testing
- ✅ Callback registry

### Phase 2: Event Loop Implementation 🚧
- Event loop handling in WinitPlatform
- Window creation and management
- Input event routing
- Frame scheduling

### Phase 3: Native Extensions 📋
- macOS-specific features (dock menu, traffic lights)
- Windows-specific features (taskbar, jump lists)
- Linux-specific features (different compositors)

### Phase 4: Advanced Features 📋
- Multi-window support
- Drag & drop
- System tray/status item
- File dialogs
- Accessibility

## Dependencies

- `flui_types` - Geometry types (Pixels, DevicePixels, Size, Point)
- `winit 0.30` - Cross-platform windowing
- `anyhow` - Error handling
- `parking_lot` - High-performance synchronization
- `tracing` - Structured logging

## References

- **GPUI Architecture**: `.gpui/src/platform.rs` - Studied for patterns and best practices
- **Flutter Platform Layer**: Inspiration for embedder concepts
- **winit Documentation**: Cross-platform window management
