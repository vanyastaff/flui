# Migration from winit to Native Platform APIs

## Цель

Полностью отказаться от winit и перейти на прямые нативные API каждой платформы, как это делает GPUI.

## Причины миграции

### Проблемы winit:
1. ❌ **Монолитный дизайн** - нельзя выбрать только нужные части
2. ❌ **Нет async support** - wgpu требует async, winit не поддерживает
3. ❌ **Проблемы с потоками** - всё идёт обратно в event loop thread
4. ❌ **Ограниченный контроль** - нельзя делать платформо-специфичные фичи
5. ❌ **API redesign в процессе** - нестабильный API

### Преимущества нативных API:
1. ✅ **Максимальный контроль** над каждой платформой
2. ✅ **Платформо-специфичные фичи** (macOS Force Touch, Windows Acrylic, etc.)
3. ✅ **Лучшая интеграция с OS** (меню, трей, нотификации)
4. ✅ **Нет промежуточных абстракций** - прямой доступ к OS
5. ✅ **Проверенный подход** - GPUI использует это в production

## Архитектура GPUI (референс)

### Структура платформ:

```
.gpui/src/platform/
├── windows/          # Win32 API + DirectX 11
│   ├── platform.rs   # WindowsPlatform
│   ├── window.rs     # WindowsWindow (HWND)
│   ├── events.rs     # WM_* message handling
│   ├── dispatcher.rs # Message queue
│   ├── display.rs    # Monitor management
│   ├── clipboard.rs  # Clipboard API
│   └── directx_renderer.rs
│
├── mac/              # AppKit + Metal
│   ├── platform.rs   # MacPlatform
│   ├── window.rs     # NSWindow wrapper
│   ├── events.rs     # NSEvent handling
│   ├── dispatcher.rs # Dispatch queue
│   ├── display.rs    # NSScreen
│   ├── text_system.rs # Core Text
│   └── metal_renderer.rs
│
├── linux/            # Wayland + X11 + Blade
│   ├── platform.rs   # LinuxPlatform (enum)
│   ├── wayland/      # smithay-client-toolkit
│   │   ├── client.rs
│   │   ├── window.rs
│   │   ├── display.rs
│   │   └── clipboard.rs
│   ├── x11/          # x11rb
│   │   ├── client.rs
│   │   ├── window.rs
│   │   ├── display.rs
│   │   └── clipboard.rs
│   └── dispatcher.rs
│
└── test/             # Headless для тестов
    ├── platform.rs
    └── window.rs
```

### Ключевые паттерны GPUI:

#### 1. Platform Trait (общий интерфейс)

```rust
pub trait Platform {
    fn open_window(&self, handle: AnyWindowHandle, params: WindowParams) 
        -> Result<Box<dyn PlatformWindow>>;
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>>;
    fn clipboard(&self) -> Rc<dyn PlatformClipboard>;
    fn run(&self); // Event loop
}
```

#### 2. PlatformWindow Trait

```rust
pub trait PlatformWindow {
    fn bounds(&self) -> Bounds<Pixels>;
    fn set_size(&self, size: Size<Pixels>);
    fn set_title(&self, title: &str);
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput)>);
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>);
    // ... callbacks для событий
}
```

#### 3. Platform-specific implementations

**Windows:**
```rust
pub struct WindowsPlatform {
    hwnd: HWND,  // Message-only window
    directx_devices: DirectXDevices,
    dispatcher: WindowsDispatcher,
    windows: HashMap<HWND, WindowsWindow>,
}

impl Platform for WindowsPlatform {
    fn run(&self) {
        // WM_* message loop
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
```

**macOS:**
```rust
pub struct MacPlatform {
    app: *mut Object,  // NSApplication
    dispatcher: MacDispatcher,
    windows: HashMap<*mut Object, MacWindow>,
}

impl Platform for MacPlatform {
    fn run(&self) {
        unsafe {
            let _: () = msg_send![self.app, run];
        }
    }
}
```

**Linux Wayland:**
```rust
pub struct WaylandClient {
    conn: wayland_client::Connection,
    event_queue: EventQueue<WaylandClientState>,
    compositor: wl_compositor::WlCompositor,
    xdg_wm_base: xdg_wm_base::XdgWmBase,
}

impl LinuxClient for WaylandClient {
    fn run(&self) {
        loop {
            self.event_queue.dispatch_pending().unwrap();
            // Process events
        }
    }
}
```

## План миграции FLUI

### Фаза 1: Подготовка архитектуры ✅ (DONE)

- ✅ Trait-based абстракция уже есть (`Platform`, `PlatformWindow`)
- ✅ Изолированный winit за traits
- ✅ Готовая структура для замены

### Фаза 2: Windows Platform (Win32 API)

#### Зависимости:
```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_Graphics_Dwm",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Dxgi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_Shell",
    "Win32_System_Com",
    "Win32_System_Ole",
] }
raw-window-handle = "0.6"
```

#### Файлы для создания:
```
crates/flui-platform/src/platforms/windows/
├── platform.rs       # WindowsPlatform
├── window.rs         # WindowsWindow (HWND wrapper)
├── events.rs         # WM_* to PlatformInput conversion
├── dispatcher.rs     # Message queue
├── display.rs        # Monitor info
├── clipboard.rs      # Windows clipboard
├── util.rs           # Helper functions
└── mod.rs
```

#### Ключевые компоненты:

**1. Window Class Registration:**
```rust
const WINDOW_CLASS_NAME: PCWSTR = w!("FluiWindowClass");

unsafe fn register_window_class() {
    let wc = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: GetModuleHandleW(None).unwrap().into(),
        lpszClassName: WINDOW_CLASS_NAME,
        style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
        // ...
    };
    RegisterClassW(&wc);
}
```

**2. Window Creation:**
```rust
unsafe fn create_window(options: &WindowOptions) -> Result<HWND> {
    CreateWindowExW(
        WS_EX_APPWINDOW,
        WINDOW_CLASS_NAME,
        &HSTRING::from(&options.title),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT,
        width, height,
        None,  // parent
        None,  // menu
        GetModuleHandleW(None).unwrap(),
        Some(/* user data */),
    )
}
```

**3. Message Loop:**
```rust
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => handle_mouse_move(hwnd, lparam),
        WM_LBUTTONDOWN => handle_mouse_down(hwnd, MouseButton::Left),
        WM_KEYDOWN => handle_key_down(hwnd, wparam, lparam),
        WM_SIZE => handle_resize(hwnd, lparam),
        WM_PAINT => handle_paint(hwnd),
        WM_CLOSE => handle_close(hwnd),
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
```

**4. Event Conversion:**
```rust
fn convert_mouse_event(hwnd: HWND, button: MouseButton, lparam: LPARAM) -> PointerEvent {
    let x = GET_X_LPARAM(lparam);
    let y = GET_Y_LPARAM(lparam);
    
    PointerEvent {
        pointer_id: 0,  // Mouse always ID 0
        device_id: 0,
        kind: PointerKind::Mouse(button),
        position: Point::new(px(x as f32), px(y as f32)),
        delta: Point::zero(),  // Calculate from previous
        phase: PointerPhase::Down,
        timestamp: Instant::now(),
        modifiers: get_current_modifiers(),
        // ...
    }
}
```

### Фаза 3: macOS Platform (AppKit/Cocoa)

#### Зависимости:
```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-foundation = { version = "0.2", features = ["NSString", "NSArray"] }
objc2-app-kit = { version = "0.2", features = [
    "NSApplication",
    "NSWindow",
    "NSView",
    "NSEvent",
    "NSScreen",
] }
cocoa = "0.26"
core-foundation = "0.10"
core-graphics = "0.24"
raw-window-handle = "0.6"
```

#### Файлы:
```
crates/flui-platform/src/platforms/mac/
├── platform.rs       # MacPlatform (NSApplication)
├── window.rs         # MacWindow (NSWindow)
├── events.rs         # NSEvent conversion
├── dispatcher.rs     # Dispatch queue
├── display.rs        # NSScreen
├── text_system.rs    # Core Text
└── mod.rs
```

#### NSApplication setup:
```rust
unsafe fn create_app() -> *mut Object {
    let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
    let _: () = msg_send![app, setActivationPolicy: NSApplicationActivationPolicyRegular];
    
    // Create app delegate
    let delegate = create_app_delegate();
    let _: () = msg_send![app, setDelegate: delegate];
    
    app
}
```

### Фаза 4: Linux Wayland (smithay-client-toolkit)

#### Зависимости:
```toml
[target.'cfg(target_os = "linux")'.dependencies]
wayland-client = "0.31"
wayland-protocols = { version = "0.32", features = ["client"] }
wayland-protocols-wlr = { version = "0.3", features = ["client"] }
smithay-client-toolkit = "0.19"
calloop = "0.14"
xkbcommon = "0.8"
raw-window-handle = "0.6"
```

#### Файлы:
```
crates/flui-platform/src/platforms/linux/wayland/
├── client.rs         # WaylandClient
├── window.rs         # WaylandWindow
├── display.rs        # Output management
├── clipboard.rs      # wl_data_device
├── cursor.rs         # Cursor theming
└── mod.rs
```

#### Wayland setup:
```rust
pub struct WaylandClient {
    conn: Connection,
    event_queue: EventQueue<ClientState>,
    qh: QueueHandle<ClientState>,
    compositor: wl_compositor::WlCompositor,
    xdg_wm_base: xdg_wm_base::XdgWmBase,
    seat: Option<wl_seat::WlSeat>,
}

impl WaylandClient {
    pub fn new() -> Result<Self> {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init(&conn)?;
        
        // Get required globals
        let compositor = globals.bind(&qh, 1..=6, ())?;
        let xdg_wm_base = globals.bind(&qh, 1..=6, ())?;
        
        Ok(Self { conn, event_queue, compositor, xdg_wm_base, ... })
    }
}
```

### Фаза 5: Linux X11 (x11rb - fallback)

#### Зависимости:
```toml
x11rb = { version = "0.13", features = ["all-extensions"] }
x11-clipboard = "0.9"
```

#### Файлы:
```
crates/flui-platform/src/platforms/linux/x11/
├── client.rs         # X11Client
├── window.rs         # X11Window
├── display.rs        # Screen info
├── clipboard.rs      # X11 clipboard
├── event.rs          # XEvent conversion
└── mod.rs
```

### Фаза 6: Android (NDK)

#### Зависимости:
```toml
[target.'cfg(target_os = "android")'.dependencies]
ndk = "0.9"
ndk-glue = "0.7"
jni = "0.21"
raw-window-handle = "0.6"
```

### Фаза 7: iOS (UIKit)

#### Зависимости:
```toml
[target.'cfg(target_os = "ios")'.dependencies]
objc2 = "0.5"
objc2-foundation = "0.2"
objc2-ui-kit = "0.2"
raw-window-handle = "0.6"
```

## Общая структура Cargo.toml

```toml
[package]
name = "flui-platform"
version = "0.1.0"
edition = "2021"

[features]
default = []
# Platform features
windows = ["dep:windows"]
macos = ["dep:objc2", "dep:cocoa"]
wayland = ["dep:wayland-client", "dep:smithay-client-toolkit"]
x11 = ["dep:x11rb"]
android = ["dep:ndk"]
ios = ["dep:objc2-ui-kit"]

[dependencies]
flui_types = { path = "../flui_types" }
anyhow = "1.0"
tracing = "0.1"
parking_lot = "0.12"
raw-window-handle = "0.6"

# Windows
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [...], optional = true }

# macOS
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = { version = "0.5", optional = true }
objc2-app-kit = { version = "0.2", optional = true }
cocoa = { version = "0.26", optional = true }
core-foundation = "0.10"

# Linux
[target.'cfg(target_os = "linux")'.dependencies]
wayland-client = { version = "0.31", optional = true }
smithay-client-toolkit = { version = "0.19", optional = true }
x11rb = { version = "0.13", optional = true }
calloop = "0.14"

# Android
[target.'cfg(target_os = "android")'.dependencies]
ndk = { version = "0.9", optional = true }

# iOS
[target.'cfg(target_os = "ios")'.dependencies]
objc2-ui-kit = { version = "0.2", optional = true }
```

## Runtime Platform Selection

```rust
// crates/flui-platform/src/lib.rs

#[cfg(target_os = "windows")]
mod platforms {
    pub use super::windows::WindowsPlatform as NativePlatform;
}

#[cfg(target_os = "macos")]
mod platforms {
    pub use super::mac::MacPlatform as NativePlatform;
}

#[cfg(target_os = "linux")]
mod platforms {
    pub enum NativePlatform {
        Wayland(WaylandClient),
        X11(X11Client),
    }
    
    impl NativePlatform {
        pub fn new() -> Result<Self> {
            // Try Wayland first
            if let Ok(wayland) = WaylandClient::new() {
                return Ok(Self::Wayland(wayland));
            }
            
            // Fallback to X11
            Ok(Self::X11(X11Client::new()?))
        }
    }
}

#[cfg(target_os = "android")]
mod platforms {
    pub use super::android::AndroidPlatform as NativePlatform;
}

#[cfg(target_os = "ios")]
mod platforms {
    pub use super::ios::IosPlatform as NativePlatform;
}

pub fn current_platform() -> Result<Box<dyn Platform>> {
    Ok(Box::new(platforms::NativePlatform::new()?))
}
```

## Порядок реализации

### Приоритет 1: Windows (текущая платформа разработки)
1. ✅ Создать базовую структуру `WindowsPlatform`
2. ✅ Реализовать `WindowsWindow` с HWND
3. ✅ Message loop и WM_* обработка
4. ✅ Конвертация событий в `PlatformInput`
5. ✅ Integration с wgpu через raw-window-handle
6. ✅ Тестирование

### Приоритет 2: Linux Wayland
1. Smithay-client-toolkit интеграция
2. WaylandWindow с xdg_surface
3. Keyboard/Mouse events
4. Integration с wgpu
5. Тестирование

### Приоритет 3: macOS
1. NSApplication setup
2. NSWindow wrapper
3. NSEvent handling
4. Metal integration
5. Тестирование

### Приоритет 4: Linux X11 (fallback)
### Приоритет 5: Android
### Приоритет 6: iOS

## Удаление winit

После успешной реализации всех платформ:

1. ✅ Убрать `winit` из `Cargo.toml`
2. ✅ Удалить `src/platforms/winit/`
3. ✅ Обновить документацию
4. ✅ Обновить тесты

## Тестирование

Для каждой платформы:

1. **Unit tests** - Тесты отдельных компонентов
2. **Integration tests** - Создание окна, события, размеры
3. **Manual testing** - Реальное приложение на каждой платформе

## Риски и митигация

| Риск | Вероятность | Митигация |
|------|-------------|-----------|
| Сложность native API | Высокая | Использовать GPUI как референс |
| Больше кода для поддержки | Высокая | Хорошая архитектура и тесты |
| Platform-specific баги | Средняя | CI/CD на всех платформах |
| Регрессии | Средняя | Comprehensive test suite |

## Преимущества после миграции

1. ✅ **Полный контроль** над каждой платформой
2. ✅ **Нет winit ограничений** (async, threading)
3. ✅ **Platform-specific features** (macOS Force Touch, Windows Snap Assist)
4. ✅ **Лучшая производительность** (нет промежуточных абстракций)
5. ✅ **Проверенный подход** (GPUI в production)

## Временные затраты (оценка)

- **Windows**: 2-3 недели
- **Linux Wayland**: 2 недели
- **macOS**: 2 недели
- **Linux X11**: 1 неделя
- **Android**: 2 недели
- **iOS**: 2 недели

**Итого**: ~3 месяца для полной поддержки всех платформ

## Начало работы

Следующий шаг: **Реализация Windows Platform**

См. `WINDOWS_PLATFORM_IMPL.md` для детального плана.
