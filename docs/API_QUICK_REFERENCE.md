# FLUI API Quick Reference

> **Цель**: Быстрая справка по ключевым API flui-platform и flui_app  
> **Дата**: 2026-01-24

---

## flui-platform API

### Получение текущей платформы

```rust
use flui_platform::{current_platform, Platform};

// Автоматический выбор платформы
let platform = current_platform();
println!("Platform: {}", platform.name());

// Явно headless (для тестов)
let platform = flui_platform::headless_platform();
```

### Запуск приложения

```rust
use flui_platform::{current_platform, Platform};

fn main() {
    let platform = current_platform();
    
    platform.run(Box::new(|| {
        println!("Platform ready!");
        
        // Создать окно
        let window = platform.open_window(WindowOptions::default())
            .expect("Failed to create window");
    }));
}
```

### Создание окна

```rust
use flui_platform::{WindowOptions, Platform};
use flui_types::geometry::px;

let options = WindowOptions {
    title: "My App".to_string(),
    size: Size::new(px(1024.0), px(768.0)),
    resizable: true,
    visible: true,
    decorated: true,
    min_size: Some(Size::new(px(400.0), px(300.0))),
    max_size: None,
};

let window = platform.open_window(options)?;
```

### Регистрация callbacks

```rust
use flui_platform::{Platform, WindowEvent};

let platform = current_platform();

// Quit handler
platform.on_quit(Box::new(|| {
    println!("Application quitting...");
    // Cleanup code
}));

// Window events
platform.on_window_event(Box::new(|event| {
    match event {
        WindowEvent::Created(id) => {
            println!("Window {:?} created", id);
        }
        WindowEvent::CloseRequested { window_id } => {
            println!("Window {:?} close requested", window_id);
        }
        WindowEvent::Resized { window_id, size } => {
            println!("Window {:?} resized to {:?}", window_id, size);
        }
        WindowEvent::ScaleFactorChanged { window_id, scale_factor } => {
            println!("Window {:?} scale changed to {}", window_id, scale_factor);
        }
        _ => {}
    }
}));
```

### Работа с окном

```rust
use flui_platform::PlatformWindow;

// Размеры окна
let physical_size = window.physical_size(); // DevicePixels
let logical_size = window.logical_size();   // Pixels
let scale_factor = window.scale_factor();   // f64

// DPI calculation
// logical_size * scale_factor = physical_size

// Запросить перерисовку
window.request_redraw();

// Состояние окна
let is_focused = window.is_focused();
let is_visible = window.is_visible();
```

### Clipboard

```rust
use flui_platform::Clipboard;

let clipboard = platform.clipboard();

// Read
if let Some(text) = clipboard.read_text() {
    println!("Clipboard: {}", text);
}

// Write
clipboard.write_text("Hello from FLUI!".to_string());

// Check
if clipboard.has_text() {
    println!("Clipboard has text");
}
```

### Executors (async tasks)

```rust
use flui_platform::PlatformExecutor;

// Background executor (blocking tasks OK)
let bg_executor = platform.background_executor();
bg_executor.spawn(Box::new(|| {
    // Heavy computation
    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("Background task complete");
}));

// Foreground executor (UI thread, must not block)
let fg_executor = platform.foreground_executor();
fg_executor.spawn(Box::new(|| {
    println!("UI task");
}));
```

### Displays (мониторы)

```rust
// All displays
let displays = platform.displays();
for display in displays {
    println!("Display: {:?}", display);
}

// Primary display
if let Some(primary) = platform.primary_display() {
    println!("Primary: {:?}", primary);
}
```

### Platform capabilities

```rust
let caps = platform.capabilities();

// Desktop
if let Some(desktop) = caps.as_desktop() {
    println!("Desktop platform");
}

// Mobile
if let Some(mobile) = caps.as_mobile() {
    if mobile.supports_touch() {
        println!("Touch support");
    }
}
```

---

## flui_app API

### Простейшее приложение

```rust
use flui_app::run_app;
use flui_view::{StatelessView, BuildContext, View, ElementBase, StatelessElement};

#[derive(Clone)]
struct MyApp;

impl StatelessView for MyApp {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        // TODO: Build your UI
        Box::new(MyApp)
    }
}

impl View for MyApp {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
    }
}

fn main() {
    run_app(MyApp);
}
```

### С конфигурацией

```rust
use flui_app::{run_app_with_config, AppConfig};

let config = AppConfig::new()
    .with_title("My FLUI App")
    .with_size(1280, 720)
    .with_target_fps(60)
    .with_resizable(true)
    .with_vsync(true);

run_app_with_config(MyApp, config);
```

### Доступ к AppBinding

```rust
use flui_app::AppBinding;

fn some_function() {
    let binding = AppBinding::instance();
    
    // Check if initialized
    if binding.is_initialized() {
        println!("Binding ready");
    }
    
    // Request redraw
    binding.request_redraw();
    
    // Check if redraw needed
    if binding.needs_redraw() {
        println!("Redraw needed");
    }
}
```

### Работа с Widgets Binding

```rust
use flui_app::AppBinding;

let binding = AppBinding::instance();

// Attach root widget
binding.attach_root_widget(&MyApp);

// Access widgets binding (read-only)
{
    let widgets = binding.widgets();
    let has_builds = widgets.has_pending_builds();
    println!("Pending builds: {}", has_builds);
}

// Access widgets binding (write)
{
    let mut widgets = binding.widgets_mut();
    widgets.draw_frame();
}
```

### Работа с Renderer Binding

```rust
let binding = AppBinding::instance();

// Access renderer (read-only)
{
    let renderer = binding.renderer();
    let is_init = renderer.is_initialized();
}

// Access renderer (write)
{
    let mut renderer = binding.renderer_mut();
    // Renderer operations
}
```

### Работа с PipelineOwner

```rust
let binding = AppBinding::instance();

// For Elements: get Arc wrapper
let pipeline_arc = binding.render_pipeline_arc();
// element.set_pipeline_owner(pipeline_arc);

// Direct access (read)
{
    let pipeline = binding.render_pipeline();
    let has_dirty = pipeline.has_dirty_nodes();
}

// Direct access (write)
{
    let mut pipeline = binding.render_pipeline_mut();
    pipeline.flush_layout();
    pipeline.flush_paint();
}
```

### Gesture Binding

```rust
let binding = AppBinding::instance();
let gestures = binding.gestures();

// Gestures already set up automatically
// Events routed through AppBinding.handle_*() methods
```

### Event Handling

```rust
use flui_app::AppBinding;
use flui_types::Offset;
use flui_interaction::events::{PointerType, PointerButton};

let binding = AppBinding::instance();

// Pointer move (coalesced)
binding.handle_pointer_move(
    Offset::new(100.0, 200.0),
    PointerType::Mouse
);

// Pointer button (click)
binding.handle_pointer_button(
    Offset::new(100.0, 200.0),
    PointerType::Mouse,
    PointerButton::Primary,
    true  // is_down
);

// Keyboard
use flui_interaction::events::KeyboardEvent;
binding.handle_key_event(keyboard_event);

// Scroll
use flui_interaction::events::ScrollEventData;
binding.handle_scroll_event(scroll_event);
```

### Frame rendering

```rust
use flui_app::AppBinding;
use flui_rendering::constraints::BoxConstraints;
use flui_types::Size;

let binding = AppBinding::instance();

// Draw frame (returns Scene)
let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
if let Some(scene) = binding.draw_frame(constraints) {
    println!("Frame {} rendered", scene.frame_number());
}

// Full render (with GPU renderer)
use flui_engine::wgpu::SceneRenderer;
let mut renderer = SceneRenderer::new(/* ... */);

if let Some(scene) = binding.render_frame(&mut renderer) {
    println!("Rendered to GPU");
}
```

### Root element management

```rust
use flui_app::AppBinding;

let binding = AppBinding::instance();

// Store root element (called by runner)
binding.set_root_element(element);

// Rebuild root
binding.rebuild_root();

// Take root element
if let Some(element) = binding.take_root_element() {
    // Use element
}
```

### Scheduler access

```rust
use flui_app::AppBinding;

let binding = AppBinding::instance();
let scheduler = binding.scheduler();

// Scheduler is singleton (same as Scheduler::instance())
```

---

## Типы и единицы измерения

### Pixels types

```rust
use flui_types::geometry::{px, device_px, Pixels, DevicePixels};

// Logical pixels (density-independent)
let logical = px(100.0);  // Pixels

// Physical pixels (actual screen pixels)
let physical = device_px(200);  // DevicePixels (i32)

// Conversion
let scale_factor = 2.0;
let physical_from_logical = logical.0 * scale_factor; // 200.0
```

### Geometry types

```rust
use flui_types::geometry::{Point, Size, Offset, Rect, px};

// Point (position in coordinate space)
let point = Point::new(px(10.0), px(20.0));

// Size (width x height)
let size = Size::new(px(100.0), px(50.0));

// Offset (delta/displacement)
let offset = Offset::new(5.0, 10.0);

// Rect (position + size)
let rect = Rect::new(point, size);
```

### Constraints

```rust
use flui_rendering::constraints::BoxConstraints;
use flui_types::Size;

// Tight (exact size)
let tight = BoxConstraints::tight(Size::new(100.0, 200.0));

// Loose (max size, min = 0)
let loose = BoxConstraints::loose(Size::new(300.0, 400.0));

// Custom
let custom = BoxConstraints::new(
    min_width: 50.0,
    max_width: 200.0,
    min_height: 100.0,
    max_height: 300.0,
);

// Constrain a size
let constrained = constraints.constrain(Size::new(150.0, 250.0));
```

---

## Частые паттерны

### Singleton access

```rust
// Platform (from function)
let platform = flui_platform::current_platform();

// AppBinding
let binding = flui_app::AppBinding::instance();

// Scheduler
let scheduler = flui_scheduler::Scheduler::instance();
```

### Arc<RwLock<T>> pattern

```rust
use parking_lot::RwLock;
use std::sync::Arc;

// Read
{
    let guard = arc.read();
    let value = *guard;
}

// Write
{
    let mut guard = arc.write();
    *guard = new_value;
}

// Clone Arc (cheap, shares data)
let arc2 = Arc::clone(&arc);
```

### Interior mutability в Platform

```rust
// Platform trait требует &self
pub trait Platform {
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
}

// Реализация использует Mutex внутри
pub struct WindowsPlatform {
    windows: Arc<Mutex<HashMap<...>>>,
}

impl Platform for WindowsPlatform {
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        let window = WindowsWindow::new(options)?;
        self.windows.lock().insert(window.id(), window.clone());
        Ok(Box::new(window))
    }
}
```

### Callback registration

```rust
// Platform callbacks - Box<dyn FnMut() + Send>
platform.on_quit(Box::new(|| {
    println!("Quitting");
}));

// With capture
let counter = Arc::new(AtomicUsize::new(0));
let counter_clone = Arc::clone(&counter);

platform.on_window_event(Box::new(move |event| {
    counter_clone.fetch_add(1, Ordering::Relaxed);
    println!("Event #{}: {:?}", counter_clone.load(Ordering::Relaxed), event);
}));
```

---

## Debugging

### Logging

```rust
use flui_log::{info, debug, warn, error, trace};

// Initialize logging
flui_log::Logger::new()
    .with_filter("info,flui_app=debug")
    .with_level(flui_log::Level::DEBUG)
    .init();

// Log
info!("Application started");
debug!(count = 5, "Processing items");
warn!("Slow operation detected");
error!(error = ?err, "Failed to load");
```

### Environment variables

```bash
# Force headless mode
FLUI_HEADLESS=1 cargo run

# Logging
RUST_LOG=debug cargo run
RUST_LOG=flui_app=trace,flui_view=debug cargo run

# See all logs
RUST_LOG=trace cargo run 2>&1 | less
```

### Assertions

```rust
// Debug-only assertions (from GPUI pattern)
#[cfg(debug_assertions)]
{
    debug_assert!(phase == PipelinePhase::Layout, 
        "Can only layout during Layout phase");
}
```

---

## Примеры кода

### Минимальное окно

```rust
use flui_platform::{current_platform, Platform, WindowOptions};

fn main() {
    let platform = current_platform();
    
    platform.run(Box::new(move || {
        let window = platform.open_window(WindowOptions::default())
            .expect("Failed to create window");
        
        println!("Window created: {:?}", window.logical_size());
    }));
}
```

### Минимальное приложение

```rust
use flui_app::{run_app, AppConfig};
use flui_view::{StatelessView, BuildContext, View, ElementBase, StatelessElement};

#[derive(Clone)]
struct App;

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(App)
    }
}

impl View for App {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
    }
}

fn main() {
    let config = AppConfig::new().with_title("Hello FLUI");
    run_app_with_config(App, config);
}
```

---

## Дальнейшее чтение

- `ARCHITECTURE_OVERVIEW.md` - Полная архитектура
- `CLAUDE.md` - Project guidelines
- `docs/plans/MIGRATION_STRATEGY.md` - Roadmap
- `.flutter/` - Flutter source для reference

**Версия**: FLUI 0.1.0 (Phase 1)  
**Дата**: 2026-01-24
