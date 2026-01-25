# Window Events: GPUI vs FLUI Comparison

## Executive Summary

**FLUI Status**: ⚠️ **Partially Implemented** - Базовые события окна определены, но callbacks не полностью подключены.

**GPUI Status**: ✅ **Fully Implemented** - Полный набор callbacks для всех событий окна.

---

## Event Types Comparison

### 1. Window Lifecycle Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Window Created** | ✅ Implicit | ✅ `WindowEvent::Created` | ✅ Defined |
| **Window Closed** | ✅ `on_close()` | ✅ `WindowEvent::Closed` | ⚠️ Defined, not wired |
| **Should Close** | ✅ `on_should_close()` | ✅ `WindowEvent::CloseRequested` | ⚠️ Defined, not wired |

### 2. Window State Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Resize** | ✅ `on_resize(Size, f32)` | ✅ `WindowEvent::Resized` | ⚠️ TODO in window_proc |
| **Move** | ✅ `on_moved()` | ✅ `WindowEvent::Moved` | ⚠️ TODO in window_proc |
| **DPI Changed** | ✅ `on_resize()` with scale | ✅ `WindowEvent::ScaleFactorChanged` | ⚠️ TODO in window_proc |
| **Redraw Requested** | ✅ `on_request_frame()` | ✅ `WindowEvent::RedrawRequested` | ❌ Not implemented |
| **Fullscreen Toggle** | ✅ `toggle_fullscreen()` | ❌ Not defined | ❌ Missing |
| **Minimize** | ✅ `minimize()` | ❌ Not defined | ❌ Missing |
| **Maximize/Zoom** | ✅ `zoom()` | ❌ Not defined | ❌ Missing |

### 3. Focus & Activation Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Active Status** | ✅ `on_active_status_change(bool)` | ✅ `WindowEvent::FocusChanged` | ⚠️ Defined, not wired |
| **Hover Status** | ✅ `on_hover_status_change(bool)` | ❌ Not defined | ❌ Missing |

### 4. Appearance Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Appearance Changed** | ✅ `on_appearance_changed()` | ❌ Not defined | ❌ Missing |
| **Dark/Light Mode** | ✅ Via appearance | ❌ Not defined | ❌ Missing |

### 5. Input Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Mouse Events** | ✅ `on_input(PlatformInput)` | ✅ Logging only | ⚠️ Not dispatched |
| **Keyboard Events** | ✅ `on_input(PlatformInput)` | ✅ Logging only | ⚠️ Not dispatched |
| **IME Events** | ✅ `update_ime_position()` | ❌ Not defined | ❌ Missing |

### 6. Drag & Drop Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **File Drop** | ✅ `IDropTarget` impl | ❌ Not defined | ❌ Missing |
| **Drag Enter** | ✅ `DragEnter()` | ❌ Not defined | ❌ Missing |
| **Drag Over** | ✅ `DragOver()` | ❌ Not defined | ❌ Missing |
| **Drag Leave** | ✅ `DragLeave()` | ❌ Not defined | ❌ Missing |
| **Drop** | ✅ `Drop()` | ❌ Not defined | ❌ Missing |

### 7. Display/Monitor Events

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Display Changed** | ✅ Tracked in state | ❌ Not defined | ❌ Missing |
| **Monitor Config** | ✅ `WindowsDisplay` | ✅ `PlatformDisplay` | ✅ Basic support |

### 8. Window Controls

| Event | GPUI | FLUI | Status |
|-------|------|------|--------|
| **Hit Test Window Control** | ✅ `on_hit_test_window_control()` | ❌ Not defined | ❌ Missing |
| **Custom Title Bar** | ✅ `hide_title_bar` | ❌ Not defined | ❌ Missing |

---

## Architecture Comparison

### GPUI Architecture

```rust
// GPUI: Callback-based architecture
pub struct WindowsWindowState {
    pub callbacks: Callbacks,
    // ... other state
}

pub struct Callbacks {
    request_frame: RefCell<Option<Box<dyn FnMut(RequestFrameOptions)>>>,
    input: RefCell<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>>,
    active_status_change: RefCell<Option<Box<dyn FnMut(bool)>>>,
    resize: RefCell<Option<Box<dyn FnMut(Size<Pixels>, f32)>>>,
    moved: RefCell<Option<Box<dyn FnMut()>>>,
    should_close: RefCell<Option<Box<dyn FnMut() -> bool>>>,
    close: RefCell<Option<Box<dyn FnOnce()>>>,
    appearance_changed: RefCell<Option<Box<dyn FnMut()>>>,
    // ... more callbacks
}

impl PlatformWindow for WindowsWindow {
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.state.callbacks.resize.set(Some(callback));
    }
    
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.state.callbacks.input.set(Some(callback));
    }
    // ... register all callbacks
}
```

**Window Procedure (GPUI):**
```rust
// Somewhere in window_proc or message handler
unsafe extern "system" fn window_proc(...) -> LRESULT {
    match msg {
        WM_SIZE => {
            // Calculate new size
            if let Some(mut callback) = callbacks.resize.take() {
                callback(new_size, scale_factor);
                callbacks.resize.set(Some(callback));
            }
        }
        WM_MOVE => {
            if let Some(mut callback) = callbacks.moved.take() {
                callback();
                callbacks.moved.set(Some(callback));
            }
        }
        // ... handle all events
    }
}
```

### FLUI Architecture

```rust
// FLUI: Event enum + handler registry (simpler but incomplete)
pub struct PlatformHandlers {
    pub quit: Option<Box<dyn FnMut() + Send>>,
    pub reopen: Option<Box<dyn FnMut() + Send>>,
    pub window_event: Option<Box<dyn FnMut(WindowEvent) + Send>>,
    pub open_urls: Option<Box<dyn FnMut(Vec<String>) + Send>>,
    pub keyboard_layout_changed: Option<Box<dyn FnMut() + Send>>,
}

pub enum WindowEvent {
    Created(WindowId),
    CloseRequested { window_id: WindowId },
    Closed(WindowId),
    FocusChanged { window_id: WindowId, focused: bool },
    Resized { window_id: WindowId, size: Size<DevicePixels> },
    ScaleFactorChanged { window_id: WindowId, scale_factor: f64 },
    RedrawRequested { window_id: WindowId },
    Moved { id: WindowId, position: Point<Pixels> },
}
```

**Window Procedure (FLUI - Current):**
```rust
// crates/flui-platform/src/platforms/windows/platform.rs
unsafe extern "system" fn window_proc(...) -> LRESULT {
    match msg {
        WM_SIZE => {
            tracing::trace!("WM_SIZE: {}x{}", width, height);
            // TODO: Handle resize
            LRESULT(0)
        }
        WM_MOVE => {
            tracing::trace!("WM_MOVE: ({}, {})", x, y);
            // TODO: Handle move
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            tracing::debug!("🖱️  Mouse Move: ({}, {})", x, y);
            // TODO: Create event and dispatch
            LRESULT(0)
        }
        // ... just logging, not dispatching
    }
}
```

---

## Key Differences

### 1. Callback Granularity

**GPUI**: Fine-grained callbacks per event type
```rust
window.on_resize(|size, scale| { /* ... */ });
window.on_moved(|| { /* ... */ });
window.on_input(|input| { /* ... */ });
```

**FLUI**: Single callback for all window events
```rust
platform.set_window_event_handler(|event| {
    match event {
        WindowEvent::Resized { size, .. } => { /* ... */ },
        WindowEvent::Moved { .. } => { /* ... */ },
        // ... handle all in one place
    }
});
```

**Trade-offs:**
- GPUI: More flexible, but more verbose
- FLUI: Simpler, but less flexible (harder to register separate handlers)

### 2. Event Dispatch

**GPUI**: Direct callback invocation
```rust
if let Some(mut callback) = self.callbacks.resize.take() {
    callback(size, scale_factor);
    self.callbacks.resize.set(Some(callback));
}
```

**FLUI**: Handler registry pattern (planned, not fully implemented)
```rust
handlers.invoke_window_event(WindowEvent::Resized { 
    window_id, 
    size 
});
```

### 3. Input Handling

**GPUI**: Single `on_input()` callback handles all input
```rust
window.on_input(|input: PlatformInput| {
    match input {
        PlatformInput::KeyDown(event) => { /* ... */ },
        PlatformInput::MouseDown(event) => { /* ... */ },
        PlatformInput::FileDrop(event) => { /* ... */ },
        // ... all input types
    }
    DispatchEventResult::Handled
});
```

**FLUI**: Currently just logging, needs dispatch implementation
```rust
// Current: Just logs
WM_KEYDOWN => {
    tracing::info!("⌨️  Key Down: VK={:#04x}", vk);
    LRESULT(0)
}

// Planned: Should create InputEvent and dispatch
WM_KEYDOWN => {
    let event = key_down_event(wparam, lparam);
    dispatch_input_event(event);
    LRESULT(0)
}
```

---

## Missing Features in FLUI

### High Priority (Core Functionality)

1. ❌ **Event Dispatch System**
   - Connect window_proc events → PlatformHandlers
   - Implement `dispatch_window_event()`, `dispatch_input_event()`
   - Wire callbacks from framework

2. ❌ **Window State Callbacks**
   - Resize handling (WM_SIZE → WindowEvent::Resized)
   - Move handling (WM_MOVE → WindowEvent::Moved)
   - DPI change (WM_DPICHANGED → ScaleFactorChanged)

3. ❌ **Input Dispatch**
   - Mouse events → flui_interaction
   - Keyboard events → flui_interaction
   - Create proper event pipeline

### Medium Priority (Enhanced Functionality)

4. ❌ **Fullscreen/Minimize/Maximize**
   - `toggle_fullscreen()` method
   - `minimize()` method
   - `zoom()` / `maximize()` method
   - Window state tracking

5. ❌ **Appearance/Theme**
   - Dark/light mode detection
   - Appearance change notifications
   - System theme integration

6. ❌ **IME Support**
   - `update_ime_position()`
   - Composition events
   - Text input handling

### Low Priority (Nice to Have)

7. ❌ **Drag & Drop**
   - File drop support
   - `IDropTarget` implementation
   - Drag enter/over/leave/drop events

8. ❌ **Custom Title Bar**
   - Hit test for window controls
   - Custom title bar rendering
   - Snap layouts integration

9. ❌ **Hover Status**
   - Mouse enter/leave tracking
   - Hover state notifications

---

## Implementation Roadmap

### Phase 1: Core Event Dispatch ✅ (Partially Done)

- [x] Define `WindowEvent` enum
- [x] Define `PlatformHandlers` registry
- [x] Basic window_proc logging
- [ ] **TODO**: Wire events to handlers
- [ ] **TODO**: Implement dispatch methods

### Phase 2: Window State Events

- [ ] Implement WM_SIZE handling
- [ ] Implement WM_MOVE handling
- [ ] Implement WM_DPICHANGED handling
- [ ] Add window bounds tracking
- [ ] Add scale factor updates

### Phase 3: Input Pipeline

- [ ] Connect mouse events to handlers
- [ ] Connect keyboard events to handlers
- [ ] Implement event → InputEvent conversion
- [ ] Wire to flui_interaction

### Phase 4: Extended Features

- [ ] Fullscreen/minimize/maximize
- [ ] Appearance notifications
- [ ] IME support
- [ ] Drag & drop

---

## Code Examples

### How GPUI Handles Resize

```rust
// .gpui/src/platform/windows/window.rs (simplified)
impl WindowsWindowInner {
    fn handle_msg(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_SIZE => {
                let width = loword(lparam.0 as u32);
                let height = hiword(lparam.0 as u32);
                let size = Size { 
                    width: Pixels(width as f32), 
                    height: Pixels(height as f32) 
                };
                let scale = self.state.scale_factor.get();
                
                // Update internal state
                self.state.logical_size.set(size);
                
                // Invoke callback
                if let Some(mut callback) = self.state.callbacks.resize.take() {
                    callback(size, scale);
                    self.state.callbacks.resize.set(Some(callback));
                }
                
                LRESULT(0)
            }
            // ... other messages
        }
    }
}
```

### How FLUI Should Handle Resize (Proposed)

```rust
// crates/flui-platform/src/platforms/windows/platform.rs
unsafe extern "system" fn window_proc(...) -> LRESULT {
    match msg {
        WM_SIZE => {
            let width = get_x_lparam(lparam);
            let height = get_y_lparam(lparam);
            
            // Get window from HWND
            if let Some(window) = window_from_hwnd(hwnd) {
                let size = Size::new(
                    DevicePixels(width as i32), 
                    DevicePixels(height as i32)
                );
                
                // Update window state
                window.update_size(size);
                
                // Dispatch event
                let event = WindowEvent::Resized {
                    window_id: window.id(),
                    size,
                };
                dispatch_window_event(event);
            }
            
            LRESULT(0)
        }
        // ... other messages
    }
}
```

---

## Testing Status

### Current (Input Events)
✅ Mouse clicks logged  
✅ Mouse wheel logged  
✅ Keyboard events logged  
✅ Character input logged  

### Missing (Window Events)
❌ Resize not dispatched  
❌ Move not dispatched  
❌ Focus change not dispatched  
❌ DPI change not handled  

---

## Recommendations

### Immediate Actions

1. **Implement Event Dispatch**
   ```rust
   // Add to WindowsPlatform
   fn dispatch_window_event(&self, event: WindowEvent) {
       if let Some(mut handler) = self.handlers.lock().window_event.take() {
           handler(event);
           self.handlers.lock().window_event = Some(handler);
       }
   }
   ```

2. **Wire WM_SIZE, WM_MOVE, WM_DPICHANGED**
   - Remove TODO comments
   - Create WindowEvent
   - Call dispatch_window_event()

3. **Connect Input to Framework**
   - Create input dispatch path
   - Wire to flui_interaction
   - Test gesture recognition

### Long-term Strategy

- **Follow GPUI Pattern**: Use fine-grained callbacks for flexibility
- **Keep W3C Events**: Maintain W3C compliance for input
- **Hybrid Approach**: WindowEvent enum + per-event callbacks
- **Progressive Enhancement**: Add features as needed

---

**Last Updated**: 2026-01-25  
**Status**: 📊 Comparison Complete - Implementation ~30% Complete
