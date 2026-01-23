# Phase 1: Foundation Layer - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Референсы**: `.gpui/`, `.flutter/`, winit 0.30 документация  
> **Цель**: Завершить фундаментальный слой (flui_types + flui-platform) с 90%+ покрытием тестами

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui_types
- ✅ Структура модулей: `geometry/`, `layout/`, `styling/`, `typography/`, `painting/`, `gestures/`, `physics/`, `platform/`
- ✅ Geometry типы: `Point`, `Size`, `Rect`, `Offset`, `Vector`, `Matrix4`, `RRect`, `Bezier`, `Circle`, `Line`
- ✅ Generic Unit system: `units.rs` с `Unit` trait
- ✅ Layout типы: `Axis`, `EdgeInsets`, `Alignment`
- ✅ Color система: `Color`, `Color32`, `HSLColor`, `HSVColor`
- ✅ Cargo.toml с правильными зависимостями (num-traits, thiserror, serde optional)

#### flui-platform  
- ✅ Trait структура: `Platform`, `PlatformWindow`, `PlatformDisplay`, `PlatformCapabilities`, `PlatformLifecycle`
- ✅ Модули: `traits/`, `platforms/`, `shared/`
- ✅ `HeadlessPlatform` для тестирования
- ✅ `current_platform()` функция с platform selection
- ✅ Cargo.toml с platform-specific зависимостями (windows, cocoa, x11rb, wayland)

### ❌ Что Нужно Доделать / Улучшить

#### flui_types
1. **Generic Unit System** - доработать типы для использования с разными единицами измерения
2. **Geometry типы** - привести к единому generic стилю с Unit параметром
3. **Тесты** - добавить comprehensive unit tests (цель: 575+ тестов как в плане)
4. **SIMD оптимизации** - добавить feature flag и SIMD версии для Matrix4, Vector
5. **Документация** - добавить примеры для всех публичных API
6. **Интеграция с mint/glam** - добавить конверсии (feature-gated)

#### flui-platform
1. **Winit Platform** - реализовать WinitPlatform с winit 0.30
2. **Platform Executors** - реализовать `PlatformExecutor` trait для async tasks
3. **Text System** - базовый `PlatformTextSystem` trait
4. **Clipboard** - реализовать `Clipboard` trait
5. **Event Handlers** - доработать `PlatformHandlers` callback registry
6. **Тесты** - comprehensive тесты с HeadlessPlatform
7. **Документация** - примеры использования для каждого trait

---

## Детальный План Реализации

### Этап 1.1: Улучшение flui_types (Неделя 1, Дни 1-4)

#### День 1: Generic Unit System Refinement

**Цель**: Привести все geometry типы к единому generic стилю

**Референсы**:
- `.gpui/src/geometry.rs` - GPUI's generic approach
- План `3.1.2 Core Type Design` - спецификация Generic Unit System

**Задачи**:

1. **Обновить `geometry/units.rs`**
   ```rust
   // Добавить строгую типизацию для Unit конверсий
   pub trait Unit: Copy + Clone + Debug + 'static {
       const NAME: &'static str;
   }
   
   // Уже есть LogicalPixels, PhysicalPixels, DevicePixels
   // Добавить Scale factor conversions
   pub struct ScaleFactor<Src: Unit, Dst: Unit>(pub f64, PhantomData<(Src, Dst)>);
   ```

2. **Обновить `geometry/point.rs`, `size.rs`, `rect.rs`, `offset.rs`**
   - Добавить generic Unit parameter: `Point<T, U: Unit = LogicalPixels>`
   - Реализовать `cast_unit<V: Unit>()` методы
   - Добавить scale conversion: `to_physical(scale)`, `to_logical(scale)`

3. **Тесты**
   ```rust
   #[test]
   fn test_unit_type_safety() {
       let logical = Point::<f32, LogicalPixels>::new(100.0, 200.0);
       let physical = logical.to_physical(2.0);
       
       // Compile error - cannot add different units:
       // let _ = logical + physical; // ❌
       
       assert_eq!(physical.x, 200.0);
   }
   ```

**Критерии завершения**:
- [ ] Все geometry типы используют generic Unit
- [ ] Type-safe конверсии между units
- [ ] 30+ unit tests
- [ ] Zero runtime overhead (verify with cargo asm)

---

#### День 2: Color System & Mathematical Types

**Цель**: Финализировать color типы и math utilities

**Референсы**:
- `.gpui/src/color.rs` - GPUI color implementation
- `.flutter/src/material/colors.dart` - Flutter Material colors

**Задачи**:

1. **Обновить `styling/color.rs`**
   ```rust
   // Добавить SIMD-friendly layout
   #[repr(C)]
   #[derive(Copy, Clone, Debug, PartialEq)]
   pub struct Color {
       pub r: f32,
       pub g: f32,
       pub b: f32,
       pub a: f32,
   }
   
   impl Color {
       // const fn constructors
       pub const fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Self { ... }
       pub const fn from_hex(hex: u32) -> Self { ... }
       
       // Conversions
       pub fn to_linear(&self) -> Color { ... }
       pub fn to_srgb(&self) -> Color { ... }
       
       // Operations
       pub fn mix(&self, other: &Color, t: f32) -> Color { ... }
   }
   ```

2. **Добавить `geometry/transform.rs`**
   ```rust
   // Generic 2D transform
   pub struct Transform2D<T, Src: Unit, Dst: Unit> {
       pub m11: T, pub m12: T, pub m13: T,
       pub m21: T, pub m22: T, pub m23: T,
       _units: PhantomData<(Src, Dst)>,
   }
   
   impl<T, Src, Dst> Transform2D<T, Src, Dst> {
       pub fn identity() -> Self { ... }
       pub fn translate(offset: Offset<T, Src>) -> Self { ... }
       pub fn scale(sx: T, sy: T) -> Self { ... }
       pub fn rotate(angle: T) -> Self where T: Float { ... }
   }
   ```

3. **Тесты**
   - Color space conversions (sRGB ↔ Linear)
   - Color mixing/interpolation
   - Transform composition
   - Inverse transforms

**Критерии завершения**:
- [ ] Color operations корректны (epsilon-based comparisons)
- [ ] Transform2D работает с generic units
- [ ] 40+ color tests, 30+ transform tests
- [ ] SIMD feature flag ready (но пока без SIMD impl)

---

#### День 3: Layout & Typography Types

**Цель**: Финализировать layout constraints и text types

**Референсы**:
- `.flutter/src/rendering/box.dart` - BoxConstraints
- `.flutter/src/painting/text_style.dart` - TextStyle

**Задачи**:

1. **Обновить `layout/constraints.rs`** (если нужно создать)
   ```rust
   // NOTE: Moved from flui_rendering per plan
   #[derive(Copy, Clone, Debug, PartialEq)]
   pub struct BoxConstraints<U: Unit = LogicalPixels> {
       pub min_width: f32,
       pub max_width: f32,
       pub min_height: f32,
       pub max_height: f32,
       _unit: PhantomData<U>,
   }
   
   impl<U: Unit> BoxConstraints<U> {
       pub fn tight(size: Size<f32, U>) -> Self { ... }
       pub fn loose(size: Size<f32, U>) -> Self { ... }
       pub fn constrain(&self, size: Size<f32, U>) -> Size<f32, U> { ... }
       pub fn is_tight(&self) -> bool { ... }
   }
   ```

2. **Обновить `typography/text_style.rs`**
   ```rust
   // Убедиться что типы совместимы с cosmic-text/glyphon
   pub struct TextStyle {
       pub font_family: String,
       pub font_size: f32,
       pub font_weight: FontWeight,
       pub font_style: FontStyle,
       pub color: Color,
       pub letter_spacing: Option<f32>,
       pub word_spacing: Option<f32>,
       pub height: Option<f32>,
       pub decoration: Option<TextDecoration>,
   }
   ```

3. **Тесты**
   - BoxConstraints tight/loose
   - Constrain operations
   - TextStyle serialization (if serde enabled)

**Критерии завершения**:
- [ ] BoxConstraints API polished
- [ ] TextStyle compatible with text renderers
- [ ] 25+ layout tests, 20+ typography tests
- [ ] Documentation examples

---

#### День 4: Testing & Documentation Sprint

**Цель**: Достичь 90%+ test coverage и полная документация

**Задачи**:

1. **Comprehensive Testing**
   - [ ] Property-based testing (proptest) для geometry
   - [ ] Edge case tests (NaN, Infinity, zero-size)
   - [ ] Integration tests для cross-module usage
   - [ ] Benchmark tests (criterion) для hot paths

2. **Documentation**
   - [ ] Doc comments для всех pub items
   - [ ] Examples в doc comments
   - [ ] Module-level docs (`//!` comments)
   - [ ] README.md для flui_types

3. **CI/CD**
   - [ ] cargo test --all-features
   - [ ] cargo clippy -- -D warnings
   - [ ] cargo fmt --check
   - [ ] cargo doc --no-deps

**Критерии завершения**:
- [ ] `cargo test --all-features` passes
- [ ] `cargo tarpaulin` shows 90%+ coverage
- [ ] `cargo doc` builds without warnings
- [ ] All public APIs have examples

---

### Этап 1.2: Реализация flui-platform (Неделя 1-2, Дни 5-10)

#### День 5: Winit Platform Foundation

**Цель**: Базовая интеграция с winit 0.30

**Референсы**:
- `.gpui/src/platform/` - GPUI platform implementations
- Winit docs (fetched via MCP earlier)

**Задачи**:

1. **Создать `platforms/winit/platform.rs`**
   ```rust
   pub struct WinitPlatform {
       event_loop: RefCell<Option<EventLoop<UserEvent>>>,
       windows: Arc<DashMap<WindowId, Arc<WinitWindow>>>,
       handlers: Arc<PlatformHandlers>,
       capabilities: WinitCapabilities,
   }
   
   impl Platform for WinitPlatform {
       fn name(&self) -> &str { "Winit" }
       
       fn run(&self, on_ready: Box<dyn FnOnce() + Send>) {
           let event_loop = self.event_loop.borrow_mut().take()
               .expect("Event loop already started");
           
           on_ready();
           
           event_loop.run(move |event, elwt| {
               // Event dispatch logic
           }).expect("Event loop error");
       }
       
       fn create_window(&self, options: WindowOptions) 
           -> Result<Arc<dyn PlatformWindow>, PlatformError> 
       {
           // Winit window creation
       }
   }
   ```

2. **Создать `platforms/winit/window.rs`**
   ```rust
   pub struct WinitWindow {
       winit_window: Arc<winit::window::Window>,
       handlers: Arc<PlatformHandlers>,
       state: Arc<RwLock<WindowState>>,
   }
   
   impl PlatformWindow for WinitWindow {
       fn id(&self) -> WindowId { ... }
       fn title(&self) -> String { ... }
       fn set_title(&self, title: &str) { ... }
       // ... остальные методы
   }
   ```

3. **Тесты** (с HeadlessPlatform)
   ```rust
   #[test]
   fn test_platform_selection() {
       std::env::set_var("FLUI_HEADLESS", "1");
       let platform = current_platform();
       assert_eq!(platform.name(), "Headless");
   }
   ```

**Критерии завершения**:
- [ ] WinitPlatform создает event loop
- [ ] Базовая обработка событий
- [ ] Window creation работает
- [ ] Integration test с winit

---

#### День 6: Event Handling & Callbacks

**Цель**: Полноценная система событий

**Референсы**:
- `.gpui/src/platform/events.rs`
- Winit event handling examples

**Задачи**:

1. **Доработать `shared/handlers.rs`**
   ```rust
   pub struct PlatformHandlers {
       resize_handlers: Arc<DashMap<WindowId, Vec<ResizeHandler>>>,
       close_handlers: Arc<DashMap<WindowId, Vec<CloseHandler>>>,
       frame_requested: Arc<AtomicBool>,
   }
   
   impl PlatformHandlers {
       pub fn register_resize(
           &self,
           window_id: WindowId,
           handler: Box<dyn Fn(Size<f32, PhysicalPixels>) + Send + Sync>,
       ) -> HandlerId {
           // ... registration logic
       }
       
       pub fn trigger_resize(&self, window_id: WindowId, size: Size<f32, PhysicalPixels>) {
           if let Some(handlers) = self.resize_handlers.get(&window_id) {
               for handler in handlers.iter() {
                   handler(size);
               }
           }
       }
   }
   ```

2. **Event Routing в WinitPlatform**
   ```rust
   event_loop.run(move |event, elwt| {
       match event {
           Event::WindowEvent { window_id, event } => {
               match event {
                   WindowEvent::Resized(size) => {
                       handlers.trigger_resize(window_id, size.into());
                   }
                   WindowEvent::CloseRequested => {
                       if handlers.trigger_close_requested(window_id) {
                           windows.remove(&window_id);
                       }
                   }
                   // ... other events
               }
           }
           Event::AboutToWait => {
               if handlers.should_request_frame() {
                   handlers.trigger_frame_requested();
               }
           }
           _ => {}
       }
   });
   ```

3. **Тесты**
   - Handler registration/unregistration
   - Event triggering
   - Multiple handlers per event

**Критерии завершения**:
- [ ] All window events mapped to Platform callbacks
- [ ] Handler registry thread-safe
- [ ] 30+ event handling tests

---

#### День 7: Platform Capabilities

**Цель**: Query system для platform features

**Референсы**:
- `.gpui/src/platform/capabilities.rs`

**Задачи**:

1. **Финализировать `traits/capabilities.rs`**
   ```rust
   pub trait PlatformCapabilities: Send + Sync {
       fn platform_type(&self) -> PlatformType;
       fn supports_transparency(&self) -> bool;
       fn supports_blur(&self) -> bool;
       fn supports_shadows(&self) -> bool;
       fn supports_touch(&self) -> bool;
       fn supports_stylus(&self) -> bool;
       fn max_texture_size(&self) -> u32;
   }
   
   pub struct DesktopCapabilities {
       transparency: bool,
       blur: bool,
       shadows: bool,
       max_texture_size: u32,
   }
   
   impl PlatformCapabilities for DesktopCapabilities { ... }
   ```

2. **Platform-specific Capabilities**
   ```rust
   // Windows
   pub struct WindowsCapabilities {
       dwm_enabled: bool,
       compositor_enabled: bool,
   }
   
   // Query from Windows API
   impl WindowsCapabilities {
       pub fn new() -> Self {
           // Use windows crate to query DWM state
       }
   }
   ```

3. **Тесты**
   ```rust
   #[test]
   fn test_headless_capabilities() {
       let platform = HeadlessPlatform::new();
       let caps = platform.capabilities();
       assert_eq!(caps.platform_type(), PlatformType::Headless);
       assert!(!caps.supports_touch());
   }
   ```

**Критерии завершения**:
- [ ] Capabilities query для всех платформ
- [ ] Runtime feature detection
- [ ] Documentation для каждой capability

---

#### День 8: Display & Monitor Abstraction

**Цель**: Multi-monitor support

**Референсы**:
- `.gpui/src/platform/display.rs`
- Winit monitor APIs

**Задачи**:

1. **Реализовать `traits/display.rs`**
   ```rust
   pub trait PlatformDisplay: Send + Sync {
       fn id(&self) -> DisplayId;
       fn name(&self) -> String;
       fn bounds(&self) -> Rect<f32, PhysicalPixels>;
       fn work_area(&self) -> Rect<f32, PhysicalPixels>;
       fn scale_factor(&self) -> f64;
       fn refresh_rate(&self) -> f32;
       fn is_primary(&self) -> bool;
   }
   
   pub struct WinitDisplay {
       monitor_handle: winit::monitor::MonitorHandle,
   }
   
   impl PlatformDisplay for WinitDisplay { ... }
   ```

2. **Display Enumeration в Platform**
   ```rust
   impl Platform for WinitPlatform {
       fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
           self.event_loop.available_monitors()
               .map(|handle| {
                   Arc::new(WinitDisplay { monitor_handle: handle }) 
                       as Arc<dyn PlatformDisplay>
               })
               .collect()
       }
       
       fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
           self.event_loop.primary_monitor()
               .map(|handle| Arc::new(WinitDisplay { monitor_handle: handle }))
       }
   }
   ```

3. **Тесты**
   - Display enumeration
   - Primary display selection
   - Scale factor correctness

**Критерии завершения**:
- [ ] Multi-monitor support works
- [ ] Correct scale factors per display
- [ ] 20+ display tests

---

#### День 9: Executors & Async Support

**Цель**: Platform-aware task execution

**Референсы**:
- `.gpui/src/executor.rs`
- Tokio runtime integration

**Задачи**:

1. **Создать `traits/executor.rs`**
   ```rust
   pub trait PlatformExecutor: Send + Sync {
       fn spawn(&self, task: Box<dyn Future<Output = ()> + Send>);
       fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send>);
       fn yield_now(&self) -> impl Future<Output = ()>;
   }
   
   pub struct TokioExecutor {
       runtime: Arc<tokio::runtime::Runtime>,
   }
   
   impl PlatformExecutor for TokioExecutor {
       fn spawn(&self, task: Box<dyn Future<Output = ()> + Send>) {
           self.runtime.spawn(task);
       }
   }
   ```

2. **Foreground vs Background Executors**
   ```rust
   impl Platform for WinitPlatform {
       fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
           // Multi-threaded tokio runtime
           Arc::clone(&self.background_executor)
       }
       
       fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
           // Current-thread executor (main thread only)
           Arc::clone(&self.foreground_executor)
       }
   }
   ```

3. **Тесты**
   ```rust
   #[tokio::test]
   async fn test_background_execution() {
       let platform = headless_platform();
       let executor = platform.background_executor();
       
       let result = Arc::new(AtomicBool::new(false));
       let result_clone = Arc::clone(&result);
       
       executor.spawn(Box::new(async move {
           result_clone.store(true, Ordering::Relaxed);
       }));
       
       tokio::time::sleep(Duration::from_millis(100)).await;
       assert!(result.load(Ordering::Relaxed));
   }
   ```

**Критерии завершения**:
- [ ] Background executor works
- [ ] Foreground executor main-thread safe
- [ ] Async tests pass

---

#### День 10: Polish, Documentation & Integration Tests

**Цель**: Production-ready flui-platform

**Задачи**:

1. **Finalize Public APIs**
   - [ ] Review all trait methods
   - [ ] Ensure consistent naming
   - [ ] Remove deprecated methods
   - [ ] Add #[must_use] where appropriate

2. **Comprehensive Documentation**
   - [ ] README.md with architecture diagram
   - [ ] Doc examples for every trait
   - [ ] Platform selection guide
   - [ ] Migration guide (if applicable)

3. **Integration Tests**
   ```rust
   #[test]
   fn test_full_platform_lifecycle() {
       let platform = current_platform();
       
       // Create window
       let window = platform.create_window(WindowOptions {
           title: "Test".into(),
           size: Size::new(800.0, 600.0),
       }).unwrap();
       
       // Register callback
       window.on_resize(Box::new(|size| {
           println!("Resized to: {:?}", size);
       }));
       
       // Platform is ready
       assert_eq!(platform.windows().len(), 1);
   }
   ```

4. **CI Configuration**
   - [ ] GitHub Actions для всех платформ
   - [ ] Coverage reporting (tarpaulin)
   - [ ] Cargo publish dry-run

**Критерии завершения**:
- [ ] cargo test --all-features passes на всех платформах
- [ ] cargo doc builds без warnings
- [ ] 90%+ test coverage
- [ ] All examples run

---

## Критерии Завершения Phase 1

### Обязательные Требования

- [ ] **flui_types 0.1.0**
  - [ ] Generic Unit system работает
  - [ ] Все geometry types immutable and Copy
  - [ ] 575+ unit tests
  - [ ] 90%+ test coverage
  - [ ] Zero unsafe code (кроме SIMD feature)
  - [ ] Документация на всех pub APIs

- [ ] **flui-platform 0.1.0**
  - [ ] WinitPlatform работает на Windows/macOS/Linux
  - [ ] HeadlessPlatform для тестов
  - [ ] All traits documented
  - [ ] 200+ platform tests
  - [ ] 90%+ test coverage

### Бонусные Цели (если успеем)

- [ ] SIMD feature flag implementation для Matrix4
- [ ] Native Windows platform (Win32 API) начало
- [ ] Text rendering trait integration с cosmic-text

---

## Примеры Использования (для Тестирования)

### Example 1: Basic Platform Setup

```rust
use flui_platform::{current_platform, WindowOptions};
use flui_types::{Size, LogicalPixels};

fn main() {
    let platform = current_platform();
    println!("Running on: {}", platform.name());
    
    platform.run(Box::new(move || {
        let window = platform.create_window(WindowOptions {
            title: "Hello FLUI".into(),
            size: Size::<f32, LogicalPixels>::new(800.0, 600.0),
        }).unwrap();
        
        println!("Window created: {}", window.title());
    }));
}
```

### Example 2: Unit Type Safety

```rust
use flui_types::{Point, LogicalPixels, PhysicalPixels};

fn main() {
    let logical = Point::<f32, LogicalPixels>::new(100.0, 100.0);
    let physical = logical.to_physical(2.0); // 2x scale factor
    
    assert_eq!(physical.x, 200.0);
    assert_eq!(physical.y, 200.0);
    
    // Compile error - cannot mix units:
    // let bad = logical + physical; // ❌
}
```

### Example 3: Multi-Monitor

```rust
use flui_platform::current_platform;

fn main() {
    let platform = current_platform();
    
    for display in platform.displays() {
        println!("Display: {}", display.name());
        println!("  Bounds: {:?}", display.bounds());
        println!("  Scale: {}", display.scale_factor());
        println!("  Primary: {}", display.is_primary());
    }
}
```

---

## Troubleshooting Guide

### Issue: Generic Unit конверсии не компилируются

**Solution**: Убедитесь, что используете правильный метод:
```rust
// ✅ Correct
let physical = logical.to_physical(scale_factor);

// ❌ Wrong - type mismatch
let physical: Point<f32, PhysicalPixels> = logical.into();
```

### Issue: WinitPlatform event loop не запускается

**Solution**: Проверьте, что event_loop не был stolen:
```rust
// Event loop должен быть взят только один раз
let event_loop = self.event_loop.borrow_mut().take()
    .expect("Event loop already started");
```

### Issue: Тесты падают с "Event loop not available"

**Solution**: Используйте HeadlessPlatform для unit tests:
```rust
#[test]
fn test_something() {
    std::env::set_var("FLUI_HEADLESS", "1");
    let platform = current_platform();
    // ...
}
```

---

## Следующие Шаги (Phase 2 Preview)

После завершения Phase 1:

1. **flui_engine** - wgpu integration, scene graph
2. **flui_interaction** - event routing, hit testing
3. **flui_app** - application lifecycle

Референсы для Phase 2:
- `.gpui/src/scene.rs` - Scene graph design
- `.gpui/src/app.rs` - Application lifecycle
- `.flutter/src/rendering/` - Render pipeline

---

## Вопросы для Обсуждения

1. Нужен ли нам сразу native Windows platform или достаточно winit?
2. SIMD оптимизации - делать сразу или отложить?
3. Clipboard integration - делать в Phase 1 или отложить на Phase 3?
4. Text system trait - насколько детальный API нужен в Phase 1?

---

**Статус**: 🟡 Ready for Implementation  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базируется на**: docs/plans/2026-01-22-core-architecture-design.md
