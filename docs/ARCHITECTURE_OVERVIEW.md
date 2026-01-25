# FLUI Architecture Overview

> **Дата создания**: 2026-01-24  
> **Автор**: Claude (анализ на основе кодовой базы)  
> **Цель**: Полное описание архитектуры flui-platform и flui_app для понимания системы

---

## 📋 Содержание

1. [Общая архитектура](#общая-архитектура)
2. [flui-platform - Platform Abstraction](#flui-platform---platform-abstraction)
3. [flui_app - Application Framework](#flui_app---application-framework)
4. [Поток данных](#поток-данных)
5. [Сравнение с Flutter](#сравнение-с-flutter)
6. [Текущее состояние](#текущее-состояние)

---

## Общая архитектура

### Высокоуровневая структура

```
┌─────────────────────────────────────────────────────────────┐
│                      FLUI Application                        │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                    flui_app                           │  │
│  │  ┌──────────────────────────────────────────────┐    │  │
│  │  │         AppBinding (Singleton)               │    │  │
│  │  │  ┌────────────┐  ┌───────────────────────┐  │    │  │
│  │  │  │ Widgets    │  │ Renderer              │  │    │  │
│  │  │  │ Binding    │  │ Binding               │  │    │  │
│  │  │  │(Build)     │  │(Layout/Paint)         │  │    │  │
│  │  │  └────────────┘  └───────────────────────┘  │    │  │
│  │  │  ┌────────────┐  ┌───────────────────────┐  │    │  │
│  │  │  │ Gesture    │  │ Scheduler             │  │    │  │
│  │  │  │ Binding    │  │ (Frame callbacks)     │  │    │  │
│  │  │  └────────────┘  └───────────────────────┘  │    │  │
│  │  └──────────────────────────────────────────────┘    │  │
│  └───────────────────────────────────────────────────────┘  │
│                            ↕                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                  flui-platform                        │  │
│  │  ┌──────────────────────────────────────────────┐    │  │
│  │  │         Platform Trait                       │    │  │
│  │  │  • Lifecycle (run, quit)                     │    │  │
│  │  │  • Windows (create, manage)                  │    │  │
│  │  │  • Display (monitors)                        │    │  │
│  │  │  • Executors (async tasks)                   │    │  │
│  │  │  • Text System (fonts)                       │    │  │
│  │  │  • Clipboard                                 │    │  │
│  │  └──────────────────────────────────────────────┘    │  │
│  │                                                        │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌────────────┐  │  │
│  │  │ Windows      │  │ Winit        │  │ Headless   │  │  │
│  │  │ Platform     │  │ Platform     │  │ Platform   │  │  │
│  │  │ (Win32 API)  │  │ (Cross-plat) │  │ (Testing)  │  │  │
│  │  └──────────────┘  └──────────────┘  └────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## flui-platform - Platform Abstraction

### Цель и назначение

**flui-platform** предоставляет абстракцию над платформенными API (Windows, macOS, Linux, mobile, web). Это позволяет:

1. **Единый API** для всех платформ
2. **Тестирование** через HeadlessPlatform
3. **Расширяемость** - легко добавить новую платформу
4. **Изоляция** - фреймворк не зависит от конкретной платформы

### Структура модулей

```
flui-platform/
├── traits/                    # Абстрактные трейты
│   ├── platform.rs           # Platform trait (центральный)
│   ├── window.rs             # PlatformWindow trait
│   ├── display.rs            # PlatformDisplay trait
│   ├── capabilities.rs       # PlatformCapabilities
│   ├── lifecycle.rs          # Lifecycle events
│   ├── input.rs              # Input events
│   └── embedder.rs           # PlatformEmbedder
│
├── platforms/                 # Конкретные реализации
│   ├── windows/              # Native Win32 (ACTIVE)
│   │   ├── platform.rs       # WindowsPlatform
│   │   ├── window.rs         # WindowsWindow
│   │   ├── events.rs         # Event handling
│   │   └── util.rs           # Utilities
│   │
│   ├── winit/                # Cross-platform via winit (ACTIVE)
│   │   ├── platform.rs       # WinitPlatform
│   │   ├── window_requests.rs
│   │   ├── clipboard.rs
│   │   └── display.rs
│   │
│   └── headless/             # Testing platform (ACTIVE)
│       └── platform.rs       # HeadlessPlatform
│
└── shared/                    # Общая инфраструктура
    └── handlers.rs           # PlatformHandlers (callback registry)
```

### Platform Trait - Центральный контракт

```rust
pub trait Platform: Send + Sync + 'static {
    // ===== Core System =====
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;

    // ===== Lifecycle =====
    fn run(&self, on_ready: Box<dyn FnOnce()>);
    fn quit(&self);
    fn request_frame(&self);

    // ===== Window Management =====
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
    fn active_window(&self) -> Option<WindowId>;
    fn window_stack(&self) -> Option<Vec<WindowId>>;

    // ===== Display Management =====
    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>>;
    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>>;

    // ===== Input & Clipboard =====
    fn clipboard(&self) -> Arc<dyn Clipboard>;

    // ===== Capabilities =====
    fn capabilities(&self) -> &dyn PlatformCapabilities;
    fn name(&self) -> &'static str;

    // ===== Callbacks (GPUI pattern) =====
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);
    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>);
    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>);

    // ===== File System =====
    fn app_path(&self) -> Result<PathBuf>;
    fn reveal_path(&self, path: &Path);
    fn open_path(&self, path: &Path);
}
```

### Ключевые особенности Platform trait

#### 1. **Callback Registry (GPUI pattern)**

Фреймворк может регистрировать обработчики без жесткой связи:

```rust
let platform = current_platform();

platform.on_quit(Box::new(|| {
    println!("Application is quitting");
}));

platform.on_window_event(Box::new(|event| {
    match event {
        WindowEvent::Resized { window_id, size } => { /* ... */ }
        WindowEvent::CloseRequested { window_id } => { /* ... */ }
        _ => {}
    }
}));
```

#### 2. **Type Erasure через Box<dyn Trait>**

```rust
// Platform возвращает trait objects для гибкости
let window: Box<dyn PlatformWindow> = platform.open_window(options)?;
let executor: Arc<dyn PlatformExecutor> = platform.background_executor();
let clipboard: Arc<dyn Clipboard> = platform.clipboard();
```

#### 3. **Interior Mutability**

```rust
// Platform использует &self (не &mut self)
// Реализации используют Arc<Mutex<T>> внутри:

pub struct WindowsPlatform {
    windows: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
    handlers: Arc<Mutex<PlatformHandlers>>,
}

impl Platform for WindowsPlatform {
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        let window = WindowsWindow::new(options)?;
        self.windows.lock().insert(window.hwnd(), window.clone());
        Ok(Box::new(window))
    }
}
```

### Текущие реализации Platform

#### 1. **WindowsPlatform** (ACTIVE - Phase 1.2 Complete)

**Технологии**: Native Win32 API (windows-rs)

**Особенности**:
- ✅ Direct Win32 window creation (CreateWindowExW)
- ✅ DPI awareness (SetProcessDpiAwarenessContext)
- ✅ Thread-safe (Arc + Mutex)
- ✅ Raw window handle для wgpu
- ✅ Windows message loop

**Статус**: Production-ready для Phase 1

**TODOs**:
- ⏳ Display enumeration (EnumDisplayMonitors)
- ⏳ DirectWrite text system
- ⏳ Windows clipboard (OpenClipboard/GetClipboardData)

#### 2. **WinitPlatform** (ACTIVE, но не приоритетна)

**Технологии**: winit crate (cross-platform)

**Особенности**:
- ✅ Works on Windows, macOS, Linux
- ✅ Simpler than native platforms
- ✅ Good for prototyping

**Когда использовать**:
- Разработка на macOS/Linux
- Быстрое прототипирование
- Когда нет времени на native реализацию

#### 3. **HeadlessPlatform** (ACTIVE - для тестов)

**Особенности**:
- ✅ No-op implementation
- ✅ Perfect for unit tests
- ✅ No dependencies on windowing systems

**Использование**:
```bash
FLUI_HEADLESS=1 cargo test
```

### Platform Selection Logic

```rust
pub fn current_platform() -> Arc<dyn Platform> {
    // 1. Check for headless mode (CI/testing)
    if std::env::var("FLUI_HEADLESS").unwrap_or_default() == "1" {
        return Arc::new(HeadlessPlatform::new());
    }

    // 2. Windows: Native Win32 platform (приоритет!)
    #[cfg(windows)]
    {
        return Arc::new(WindowsPlatform::new()
            .expect("Failed to create Windows platform"));
    }

    // 3. Winit backend (cross-platform fallback)
    #[cfg(all(feature = "winit-backend", not(windows)))]
    {
        return Arc::new(WinitPlatform::new());
    }

    // 4. Fallback to headless
    Arc::new(HeadlessPlatform::new())
}
```

**Важно**: На Windows по умолчанию используется **WindowsPlatform** (native Win32), НЕ winit!

---

## flui_app - Application Framework

### Цель и назначение

**flui_app** - это Application Layer, который объединяет все bindings и управляет lifecycle приложения.

### Паттерн "Binding" (из Flutter)

Flutter использует mixins для комбинирования bindings:

```dart
// Flutter
class WidgetsFlutterBinding extends BindingBase
    with GestureBinding, SchedulerBinding, ServicesBinding,
         SemanticsBinding, PaintingBinding, RendererBinding,
         WidgetsBinding { }
```

FLUI использует **композицию через owned fields**:

```rust
// FLUI
pub struct AppBinding {
    renderer: RwLock<RenderingFlutterBinding>,
    widgets: RwLock<WidgetsBinding>,
    gestures: GestureBinding,
    frame_coordinator: RwLock<FrameCoordinator>,
    pointer_state: RwLock<PointerState>,
    shared_pipeline_owner: Arc<RwLock<PipelineOwner>>,
    root_element: Mutex<Option<Box<dyn ElementBase>>>,
    // ...
}
```

### AppBinding - Центральный координатор

**AppBinding** - это **singleton**, который координирует все части фреймворка:

```rust
impl AppBinding {
    /// Singleton instance
    pub fn instance() -> &'static Self { /* ... */ }

    // ===== Renderer Binding (Layout/Paint) =====
    pub fn renderer(&self) -> RwLockReadGuard<'_, RenderingFlutterBinding>;
    pub fn renderer_mut(&self) -> RwLockWriteGuard<'_, RenderingFlutterBinding>;

    // ===== Widgets Binding (Build) =====
    pub fn widgets(&self) -> RwLockReadGuard<'_, WidgetsBinding>;
    pub fn widgets_mut(&self) -> RwLockWriteGuard<'_, WidgetsBinding>;
    pub fn attach_root_widget<V: View>(&self, view: &V);

    // ===== Render Pipeline (для Elements) =====
    pub fn render_pipeline_arc(&self) -> Arc<RwLock<PipelineOwner>>;
    pub fn render_pipeline(&self) -> RwLockReadGuard<'_, PipelineOwner>;
    pub fn render_pipeline_mut(&self) -> RwLockWriteGuard<'_, PipelineOwner>;

    // ===== Gesture Binding (Input) =====
    pub fn gestures(&self) -> &GestureBinding;

    // ===== Frame Management =====
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Option<Arc<Scene>>;
    pub fn render_frame(&self, renderer: &mut SceneRenderer) -> Option<Arc<Scene>>;
    pub fn request_redraw(&self);
    pub fn needs_redraw(&self) -> bool;

    // ===== Event Handling =====
    pub fn handle_pointer_move(&self, position: Offset, device: PointerType);
    pub fn handle_pointer_button(&self, position: Offset, ...);
    pub fn handle_key_event(&self, key_event: KeyboardEvent);
    pub fn handle_scroll_event(&self, scroll_event: ScrollEventData);

    // ===== Root Element Management =====
    pub fn set_root_element(&self, element: Box<dyn ElementBase>);
    pub fn rebuild_root(&self);
}
```

### Структура модулей flui_app

```
flui_app/
├── app/
│   ├── binding.rs            # AppBinding (singleton)
│   ├── config.rs             # AppConfig (window title, size, etc.)
│   ├── lifecycle.rs          # AppLifecycle (states)
│   └── runner.rs             # run_app(), platform-specific event loops
│
├── bindings/
│   ├── renderer_binding.rs   # RenderingFlutterBinding
│   └── traits.rs             # Binding traits
│
├── embedder/                  # Desktop embedder (wgpu + winit)
│   ├── desktop.rs            # DesktopEmbedder
│   ├── frame_coordinator.rs  # Frame statistics
│   ├── pointer_state.rs      # Event coalescing
│   └── scene_cache.rs        # Scene caching
│
├── overlay/                   # Overlay system (tooltips, etc.)
├── theme/                     # Theme system
└── debug/
    └── flags.rs              # DebugFlags
```

### Application Entry Point

#### Simple usage:

```rust
use flui_app::run_app;
use flui_view::{StatelessView, BuildContext, View};

#[derive(Clone)]
struct MyApp;

impl StatelessView for MyApp {
    fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> {
        // Build your UI
        Box::new(MyApp) // Placeholder
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

#### With config:

```rust
use flui_app::{run_app_with_config, AppConfig};

let config = AppConfig::new()
    .with_title("My FLUI App")
    .with_size(1024, 768)
    .with_target_fps(60);

run_app_with_config(MyApp, config);
```

### Desktop Runner Implementation

**flui_app/src/app/runner.rs** содержит platform-specific event loops:

```rust
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    use winit::{event_loop::EventLoop, application::ApplicationHandler};

    struct DesktopApp<V: View> {
        root_widget: V,
        embedder: Option<DesktopEmbedder>,
    }

    impl<V: View> ApplicationHandler for DesktopApp<V> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            // 1. Create embedder (wgpu + winit window)
            let embedder = DesktopEmbedder::new(event_loop).await;

            // 2. Mount root element (wraps in RootRenderView)
            self.mount_root(width, height);

            // 3. Request initial redraw
            embedder.request_redraw();
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
            match event {
                WindowEvent::RedrawRequested => {
                    // Render frame via AppBinding
                    embedder.render_frame();
                }
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                }
                _ => {
                    embedder.handle_window_event(event, event_loop);
                }
            }
        }
    }

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait); // On-demand rendering
    event_loop.run_app(&mut DesktopApp::new(root, config));
}
```

**Ключевые моменты**:

1. **On-demand rendering** (`ControlFlow::Wait`):
   - Frames отрисовываются только когда нужно (state changes, animations, resize)
   - НЕ постоянный loop на 60 FPS (это расточительно)

2. **RootRenderView wrapper**:
   - User widget оборачивается в `RootRenderView`
   - `RootRenderView` создает `RenderViewObject` (root render object)
   - `RenderViewObject` управляет child render objects

3. **Pipeline owner sharing**:
   - `AppBinding` владеет `Arc<RwLock<PipelineOwner>>`
   - `RootRenderElement` получает clone этого Arc
   - Все используют один и тот же PipelineOwner!

---

## Поток данных

### Frame Rendering Flow

```
User action (click, type, etc.)
    ↓
WindowEvent → DesktopEmbedder.handle_window_event()
    ↓
AppBinding.handle_pointer_button() / handle_key_event()
    ↓
GestureBinding.handle_pointer_event()
    ↓
Widget state changes → mark_needs_build()
    ↓
AppBinding.request_redraw()
    ↓
WindowEvent::RedrawRequested
    ↓
AppBinding.render_frame()
    ├─→ Phase 1: Process pending events
    ├─→ Phase 2: draw_frame(constraints)
    │   ├─→ WidgetsBinding.draw_frame() [BUILD]
    │   │   └─→ Rebuild dirty elements
    │   ├─→ PipelineOwner.flush_layout() [LAYOUT]
    │   │   └─→ Compute sizes
    │   ├─→ PipelineOwner.flush_paint() [PAINT]
    │   │   └─→ Generate display lists
    │   └─→ Create Scene from LayerTree
    ├─→ Phase 3: SceneRenderer.render(scene) [GPU]
    │   └─→ wgpu commands
    └─→ Phase 4: mark_rendered()
```

### Three-Tree Architecture

FLUI использует three-tree architecture Flutter:

```
┌──────────────────────────────────────────────────────────┐
│                      VIEW TREE                           │
│  Immutable widget configurations (user code)             │
│  Example: Container(padding: 10, child: Text("Hi"))     │
└──────────────────────────────────────────────────────────┘
                        ↓ build()
┌──────────────────────────────────────────────────────────┐
│                    ELEMENT TREE                          │
│  Mutable state, lifecycle, build coordination            │
│  Example: StatelessElement, StatefulElement             │
│  Storage: Slab in BuildOwner                            │
└──────────────────────────────────────────────────────────┘
                        ↓ createRenderObject()
┌──────────────────────────────────────────────────────────┐
│                    RENDER TREE                           │
│  Layout, paint, hit testing                             │
│  Example: RenderPadding, RenderFlex, RenderText         │
│  Storage: Slab in PipelineOwner                         │
└──────────────────────────────────────────────────────────┘
```

**Важные детали**:

1. **View Tree** (immutable):
   - User code: `Container::new().padding(10).child(Text::new("Hi"))`
   - Implements `View` trait with `create_element()`
   - Cloned when rebuilding

2. **Element Tree** (mutable):
   - Created from Views via `create_element()`
   - Stored in `Slab` in `BuildOwner`
   - Has lifecycle: `mount()`, `update()`, `unmount()`
   - Manages state for StatefulWidgets

3. **Render Tree** (layout/paint):
   - Created from Elements via `create_render_object()`
   - Stored in `Slab` in `PipelineOwner`
   - Implements `RenderObject` trait
   - Type-safe arity: `Leaf`, `Single`, `Optional`, `Variable`

### Pipeline Phases (как в Flutter)

```rust
// AppBinding.draw_frame()

// Phase 1: BUILD
{
    let mut widgets = self.widgets.write();
    widgets.draw_frame(); // Rebuilds dirty elements
}

// Phase 2: LAYOUT
{
    let mut pipeline = self.shared_pipeline_owner.write();
    pipeline.flush_layout(); // Computes sizes bottom-up
}

// Phase 3: COMPOSITING
{
    let mut pipeline = self.shared_pipeline_owner.write();
    pipeline.flush_compositing_bits(); // Updates layer tree
}

// Phase 4: PAINT
{
    let mut pipeline = self.shared_pipeline_owner.write();
    pipeline.flush_paint(); // Generates display lists
}

// Phase 5: SEMANTICS (accessibility)
{
    let mut pipeline = self.shared_pipeline_owner.write();
    pipeline.flush_semantics();
}

// Phase 6: Create Scene
let scene = Scene::new(size, layer_tree, root, frame_number);
```

---

## Сравнение с Flutter

### Platform abstraction

| Flutter | FLUI |
|---------|------|
| `dart:ui` (embedder API) | `flui-platform` trait |
| Platform channels | Callback registry |
| `WindowPlatform`, etc. | `Platform` trait |
| Impeller/Skia | wgpu |

### Application binding

| Flutter | FLUI |
|---------|------|
| `WidgetsFlutterBinding` (mixins) | `AppBinding` (composition) |
| `runApp(MyApp())` | `run_app(MyApp)` |
| `BindingBase.instance` | `AppBinding::instance()` |

### Three trees

| Flutter | FLUI |
|---------|------|
| Widget tree | View tree |
| Element tree | Element tree (same!) |
| RenderObject tree | RenderObject tree (same!) |

### Pipeline phases

| Flutter | FLUI |
|---------|------|
| Build | Build (WidgetsBinding) |
| Layout | Layout (PipelineOwner) |
| Compositing | Compositing (PipelineOwner) |
| Paint | Paint (PipelineOwner) |
| Semantics | Semantics (PipelineOwner) |

### Event handling

| Flutter | FLUI |
|---------|------|
| GestureBinding | GestureBinding (same!) |
| HitTestResult | HitTestResult (same!) |
| PointerEvent | PointerEvent (ui-events crate) |

---

## Текущее состояние

### ✅ Что работает (Phase 1 Complete)

**flui-platform**:
- ✅ Platform trait определен
- ✅ WindowsPlatform (native Win32)
  - ✅ Window creation
  - ✅ Thread-safe Arc/Mutex
  - ✅ DPI awareness
  - ✅ Raw window handle для wgpu
  - ✅ Basic event loop
- ✅ WinitPlatform (cross-platform)
- ✅ HeadlessPlatform (testing)
- ✅ current_platform() selection logic

**flui_app**:
- ✅ AppBinding singleton
- ✅ run_app() / run_app_with_config()
- ✅ Desktop runner (winit event loop)
- ✅ DesktopEmbedder (wgpu rendering)
- ✅ Three-tree pipeline (build → layout → paint)
- ✅ On-demand rendering (ControlFlow::Wait)
- ✅ RootRenderView wrapper
- ✅ Event routing (pointer, keyboard, scroll)

### ⏳ В процессе (Week 1)

**Re-enabling crates**:
- ✅ flui-foundation (Day 1 complete)
- ✅ flui-tree (Day 1 complete)
- ✅ flui_log (Day 1 complete)
- ✅ flui_animation (Day 1 complete)
- ✅ flui_painting (Day 1 complete)
- ⏳ flui_interaction (BLOCKED - architecture decision)
- ⏳ flui-layer (Day 2 planned)
- ⏳ flui-semantics (Day 2 planned)

### ❌ TODO (Future)

**flui-platform**:
- ❌ Display enumeration (monitors)
- ❌ DirectWrite text system
- ❌ Windows clipboard integration
- ❌ macOS native platform
- ❌ Linux native platform
- ❌ Android platform
- ❌ iOS platform
- ❌ Web platform

**flui_app**:
- ❌ Multi-window support
- ❌ Overlay system (tooltips, popups)
- ❌ Theme system
- ❌ Hot reload
- ❌ DevTools integration

---

## Ключевые архитектурные решения

### 1. **Singleton Pattern для AppBinding**

**Почему**:
- Единая точка координации
- Простой доступ из любого места: `AppBinding::instance()`
- Thread-safe через OnceLock

**Альтернативы**:
- Dependency injection (сложнее)
- Global state (менее type-safe)

### 2. **Arc<RwLock<PipelineOwner>> sharing**

**Почему**:
- Elements нужен доступ к PipelineOwner для insert/remove RenderObjects
- AppBinding владеет PipelineOwner
- Arc позволяет sharing без передачи ownership

**Детали**:
```rust
// AppBinding creates Arc wrapper
let shared_pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

// Elements get clone of Arc
let pipeline_arc = binding.render_pipeline_arc(); // Arc::clone()
element.set_pipeline_owner(pipeline_arc);

// Everyone uses same PipelineOwner!
```

### 3. **Callback Registry (GPUI pattern)**

**Почему**:
- Decoupling: platform не знает о framework
- Flexibility: framework может регистрировать любые handlers
- Testability: mock callbacks в тестах

**Пример**:
```rust
platform.on_window_event(Box::new(|event| {
    match event {
        WindowEvent::Resized { size, .. } => {
            // Framework handles resize
        }
        _ => {}
    }
}));
```

### 4. **Interior Mutability через RwLock**

**Почему**:
- Platform trait требует `&self` (не `&mut self`)
- Bindings могут быть вызваны из разных мест
- RwLock позволяет multiple readers, exclusive writer

**Trade-offs**:
- Performance overhead (lock contention)
- Runtime panics если deadlock
- Но: проще чем Cell/RefCell для multi-threading

### 5. **On-demand Rendering (ControlFlow::Wait)**

**Почему**:
- Экономия CPU/battery (не рисуем постоянно)
- Flutter-style (UI framework, не game engine)
- Frames только когда нужно (state change, animation, resize)

**Когда рисуем**:
- Widget state changes → `mark_needs_build()` → `request_redraw()`
- Animations running → scheduler callbacks
- Window resize/expose events

---

## Следующие шаги

### Week 1 Day 2 (ближайшее)

1. **Решить flui_interaction architecture**:
   - Option C: Mixed (Pixels для позиций, f32 для дельт)
   - Исправить 592 ошибки

2. **Re-enable rendering stack**:
   - flui-layer
   - flui-semantics
   - Verify compilation

3. **Cleanup diagnostics**:
   - Unused imports
   - Dead code

### Week 1 Day 3-5

- Re-enable flui_engine
- Re-enable flui_rendering
- Re-enable flui-view
- Re-enable flui-scheduler
- Re-enable flui_app dependencies

### Week 2-3 (V2 Enhancements)

- Apply GPUI patterns to flui-view (associated types, 3-phase)
- Apply GPUI patterns to flui_rendering (pipeline phase tracking)

---

## Глоссарий

- **Platform** - абстракция над OS-specific API
- **Binding** - координатор части фреймворка (widgets, renderer, gestures)
- **Element** - mutable instance of widget в element tree
- **RenderObject** - объект в render tree (layout/paint)
- **PipelineOwner** - владелец render tree, управляет layout/paint phases
- **BuildOwner** - владелец element tree, управляет build phase
- **Scene** - финальный результат рендеринга для GPU
- **LayerTree** - дерево слоев для композитинга
- **Embedder** - интеграция с platform (window + GPU)

---

**Документация актуальна на**: 2026-01-24  
**Версия FLUI**: 0.1.0 (Phase 1 в процессе)  
**Автор**: Claude (анализ кодовой базы)
