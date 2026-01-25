# Dependency Recommendations for flui-platform

Based on GPUI analysis, here are recommended dependencies to add for production-quality platform layer.

## High Priority (Add Soon)

### 1. flume = "0.11"
**Purpose:** Better MPSC channels for foreground executor  
**Why:** Faster and simpler than `tokio::sync::mpsc`, used by GPUI on all platforms  
**Impact:** Better UI thread communication performance

**Migration:**
```rust
// Replace tokio::sync::mpsc with flume
use flume::{Sender, Receiver};

pub struct ForegroundExecutor {
    sender: Sender<Box<dyn FnOnce() + Send>>,
    receiver: Arc<Mutex<Receiver<Box<dyn FnOnce() + Send>>>>,
}
```

### 2. raw-window-handle = "0.6"
**Purpose:** Cross-platform window handle abstraction  
**Why:** Standard for passing window handles to renderers (wgpu, vulkan, etc.)  
**Impact:** Better renderer integration

**Usage:**
```rust
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

impl HasRawWindowHandle for WindowsWindow {
    fn raw_window_handle(&self) -> RawWindowHandle {
        RawWindowHandle::Win32(Win32WindowHandle::new(self.hwnd.0))
    }
}
```

### 3. waker-fn = "1.2.0"
**Purpose:** Simplify async waker creation  
**Why:** Makes executor implementation cleaner  
**Impact:** Cleaner async code

## Medium Priority (Phase 8+)

### 4. calloop = "0.14.3"
**Purpose:** Event loop for Linux  
**When:** Phase 9 - Linux native implementation  
**Why:** Native Wayland/X11 event loop without winit

### 5. xkbcommon = { version = "0.8.0", features = ["wayland", "x11"] }
**Purpose:** Keyboard input for Linux  
**When:** Phase 9 - Linux native implementation  
**Why:** Proper keyboard handling on X11/Wayland

### 6. filedescriptor = "0.8.2"
**Purpose:** Cross-platform file descriptor handling  
**When:** Phase 9 - Linux/macOS native backends  
**Why:** Event loop integration on Unix platforms

### 7. foreign-types = "0.5"
**Purpose:** FFI type wrappers  
**When:** Phase 8 - macOS native implementation  
**Why:** Simplifies Objective-C/Swift FFI

### 8. windows-registry = "0.5"
**Purpose:** Windows Registry access  
**When:** Phase 7+ - Windows enhancements  
**Why:** Read system settings (theme, animations, DPI)

**Usage:**
```rust
use windows_registry::CURRENT_USER;

// Check if dark mode is enabled
let key = CURRENT_USER.open("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")?;
let apps_use_light_theme: u32 = key.get_value("AppsUseLightTheme")?;
let is_dark_mode = apps_use_light_theme == 0;
```

### 9. open = "5.2.0"
**Purpose:** Open URLs/files with system default app  
**When:** Phase 7+ - Platform utilities  
**Why:** Common platform feature

**Usage:**
```rust
// Open URL in browser
open::that("https://example.com")?;

// Open file with default app
open::that("document.pdf")?;
```

## Low Priority (Future)

### 10. ashpd = { version = "0.11", features = ["async-std"] }
**Purpose:** Desktop portals for Linux sandboxed apps  
**When:** Phase 9+ - Linux desktop integration  
**Why:** File dialogs, notifications in Flatpak/Snap

### 11. oo7 = { version = "0.5.0", features = ["async-std", "native_crypto"] }
**Purpose:** Secret storage for Linux  
**When:** Phase 9+ - Linux keychain integration  
**Why:** Store credentials securely

### 12. windows-numerics = "0.2"
**Purpose:** SIMD math for Windows  
**When:** Rendering optimization phase  
**Why:** Performance (but platform layer doesn't need this)

## Dependencies to AVOID

These are rendering-specific and should stay in `flui_engine` or `flui_rendering`:

- ❌ **blade-graphics** - GPU rendering (not platform concern)
- ❌ **cosmic-text** - Text shaping (rendering layer)
- ❌ **font-kit** - Font loading (rendering layer)
- ❌ **lyon** - Path tessellation (rendering layer)
- ❌ **resvg** - SVG rendering (rendering layer)
- ❌ **etagere** - Texture packing (rendering layer)
- ❌ **taffy** - Layout engine (should be in flui_layout)

## Current flui-platform Dependencies

```toml
[dependencies]
anyhow = "1.0"
parking_lot = "0.12"
windows = { version = "0.58", features = [...] }
tracing = "0.1"
tokio = { version = "1.43", features = ["rt-multi-thread", "sync", "time"] }
num_cpus = "1.13"

[target.'cfg(target_os = "macos")'.dependencies]
# Will add in Phase 8

[target.'cfg(target_os = "linux")'.dependencies]
# Will add in Phase 9
```

## Recommended Action Plan

### Phase 7.6 (Immediate - Optional Polish)

Add these 3 non-breaking improvements:

```toml
[dependencies]
flume = "0.11"              # Better MPSC for executor
raw-window-handle = "0.6"   # Standard window handle trait
waker-fn = "1.2.0"          # Cleaner async code
```

**Effort:** 1-2 hours  
**Impact:** Better performance, standard compliance, cleaner code

### Phase 7.7 (Windows Polish)

```toml
[target.'cfg(windows)'.dependencies]
windows-registry = "0.5"    # System settings
open = "5.2.0"              # Open URLs/files
```

**Effort:** 2-3 hours  
**Impact:** Dark mode detection, better system integration

### Phase 8 (macOS Native)

```toml
[target.'cfg(target_os = "macos")'.dependencies]
cocoa = "0.26.0"
cocoa-foundation = "0.2.0"
core-foundation = "0.10.0"
foreign-types = "0.5"
open = "5.2.0"
```

### Phase 9 (Linux Native)

```toml
[target.'cfg(any(target_os = "linux", target_os = "freebsd"))'.dependencies]
calloop = "0.14.3"
xkbcommon = { version = "0.8.0", features = ["wayland", "x11"] }
filedescriptor = "0.8.2"
open = "5.2.0"
ashpd = { version = "0.11", optional = true }
oo7 = { version = "0.5.0", optional = true }
```

## Notes

1. **Version alignment:** GPUI uses specific versions, we should match where possible
2. **Feature flags:** Add features only when needed (Wayland, X11, etc.)
3. **Optional deps:** Use `optional = true` for non-essential features
4. **Git dependencies:** Avoid git deps (GPUI uses custom forks, we shouldn't)
5. **Edition 2024:** GPUI uses Rust 2024 edition - we should consider upgrading

## Comparison with GPUI

| Dependency | GPUI | FLUI | Recommendation |
|------------|------|------|----------------|
| anyhow | ✅ 1.0.86 | ✅ 1.0 | Match version |
| parking_lot | ✅ 0.12.1 | ✅ 0.12 | OK |
| tokio | ❌ | ✅ 1.43 | Keep (GPUI uses smol) |
| num_cpus | ✅ 1.13 | ✅ 1.13 | ✅ |
| flume | ✅ 0.11 | ❌ | **Add** |
| raw-window-handle | ✅ 0.6 | ❌ | **Add** |
| waker-fn | ✅ 1.2.0 | ❌ | **Add** |
| windows-registry | ✅ 0.5 | ❌ | Add later |

**Key Difference:** GPUI uses `smol` for async runtime, we use `tokio`. This is fine - tokio is more popular and better documented.

## Conclusion

Рекомендую добавить в **Phase 7.6**:
1. `flume = "0.11"` - Замена для `tokio::sync::mpsc`
2. `raw-window-handle = "0.6"` - Стандарт для window handles
3. `waker-fn = "1.2.0"` - Упрощение async code

Остальные зависимости добавлять по мере реализации нативных платформ (Phase 8-9).
