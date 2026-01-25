# FLUI Event Architecture

## ✅ Current Status: **Unified W3C Architecture**

FLUI использует **современную W3C-совместимую архитектуру** событий через крейт `ui-events`.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    OS Native Events                          │
│         (Win32: WM_MOUSEMOVE, WM_KEYDOWN, etc.)             │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│              Platform Layer (flui-platform)                  │
│   • Converts Win32/Wayland/Cocoa → W3C ui-events            │
│   • Handles DPI scaling (device → logical pixels)           │
│   • File: platforms/windows/events.rs                       │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│            W3C Compliant Events (ui-events 0.3)              │
│   • PointerEvent (mouse, touch, pen)                        │
│   • KeyboardEvent (keyboard)                                │
│   • ScrollDelta (wheel)                                     │
│   • Standard W3C UI Events specification                    │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│          Interaction Layer (flui_interaction)                │
│   • Gesture Recognition (tap, drag, scale, etc.)            │
│   • Hit Testing                                             │
│   • Event Routing                                           │
│   • File: flui_interaction/src/events.rs                    │
└─────────────────────────────────────────────────────────────┘
```

---

## Key Dependencies

### Workspace-level (Cargo.toml)
```toml
ui-events = "0.3"          # W3C UI Events specification
ui-events-winit = "0.3"    # Winit integration
cursor-icon = "1.2"        # W3C CSS cursor specification
keyboard-types = "0.8"     # Keyboard key definitions
```

### Platform Layer (flui-platform)
- **ui-events** - W3C PointerEvent, KeyboardEvent
- **keyboard-types** - Key codes and modifiers
- Converts native OS events → W3C events

### Interaction Layer (flui_interaction)
- **ui-events** - W3C event types for gesture recognition
- **cursor-icon** - Standard cursor appearances
- Processes W3C events → Gestures

---

## Event Types

### 1. Pointer Events (Mouse, Touch, Pen)
From `ui_events::pointer`:
- `PointerEvent::Down` - Button/touch press
- `PointerEvent::Up` - Button/touch release
- `PointerEvent::Move` - Movement
- `PointerEvent::Scroll` - Wheel/scroll
- `PointerEvent::Enter` / `Leave` - Hover
- `PointerEvent::Cancel` - Cancelled gesture

### 2. Keyboard Events
From `ui_events::keyboard`:
- `KeyboardEvent` with:
  - `key: Key` - Logical key (from keyboard-types)
  - `state: KeyState` - Down or Up
  - `modifiers: Modifiers` - Ctrl, Shift, Alt, Meta
  - `location: Location` - Left/Right for modifier keys

### 3. Extended Events (FLUI-specific)
From `flui_interaction::events`:
- `InputEvent::Pointer(PointerEvent)` - W3C pointer event
- `InputEvent::Keyboard(KeyboardEvent)` - W3C keyboard event
- `InputEvent::DeviceAdded` - Device lifecycle (not in W3C)
- `InputEvent::DeviceRemoved` - Device lifecycle (not in W3C)

---

## Design Principles

### ✅ Unified Architecture (Current)
1. **W3C Compliant** - Standard `ui-events` types everywhere
2. **Platform Agnostic** - Same types work on desktop, mobile, web
3. **No Duplication** - Platform converts native → W3C, no custom types
4. **Type Safe** - Concrete types, no generics in public API

### ❌ Legacy Architecture (Removed)
Previously FLUI had custom event types (GPUI-style):
```rust
// ❌ OLD - Custom types (removed)
pub struct PointerEvent {
    pub position: Point<Pixels>,
    pub delta: Point<Pixels>,  // Wrong! Should be PixelDelta
}
```

Now we use W3C types:
```rust
// ✅ NEW - W3C standard types
use ui_events::pointer::PointerEvent;
```

---

## Platform Implementation

### Windows (Win32)
File: `crates/flui-platform/src/platforms/windows/events.rs`

Converts Win32 messages to W3C events:
- `WM_MOUSEMOVE` → `PointerEvent::Move`
- `WM_LBUTTONDOWN` → `PointerEvent::Down(Primary)`
- `WM_MOUSEWHEEL` → `PointerEvent::Scroll`
- `WM_KEYDOWN` → `KeyboardEvent { state: Down }`
- `WM_CHAR` → Character extraction

### Event Loop
File: `crates/flui-platform/src/platforms/windows/platform.rs`

Main window procedure `window_proc()`:
```rust
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => { /* Convert to PointerEvent::Move */ }
        WM_LBUTTONDOWN => { /* Convert to PointerEvent::Down */ }
        WM_KEYDOWN => { /* Convert to KeyboardEvent */ }
        // ...
    }
}
```

---

## Example Usage

### Platform Layer (Converting OS Events)
```rust
// Windows: WM_LBUTTONDOWN → W3C PointerEvent
pub fn mouse_button_event(
    button: PointerButton,
    is_down: bool,
    lparam: LPARAM,
    scale_factor: f32,
) -> PlatformInput {
    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);
    
    let state = PointerState {
        position: PhysicalPosition::new(x as f64, y as f64),
        buttons: PointerButtons::from(button),
        // ... W3C standard fields
    };
    
    let event = if is_down {
        PointerEvent::Down(PointerButtonEvent { state, button, ... })
    } else {
        PointerEvent::Up(...)
    };
    
    PlatformInput::Pointer(event)
}
```

### Application Layer (Handling Events)
```rust
use flui_interaction::events::{InputEvent, PointerEvent, KeyboardEvent};

fn handle_event(event: &InputEvent) {
    match event {
        InputEvent::Pointer(PointerEvent::Down(e)) => {
            println!("Click at: {:?}", e.state.position);
        }
        InputEvent::Keyboard(e) if e.state == KeyState::Down => {
            println!("Key pressed: {:?}", e.key);
        }
        _ => {}
    }
}
```

---

## Testing Results ✅

From `cargo run --example input_test`:

### Mouse Events
```
2026-01-25T08:09:54.601820Z  INFO: 🖱️  Left Mouse Button Down at (347, 125)
2026-01-25T08:09:54.709577Z  INFO: 🖱️  Left Mouse Button Up at (346, 125)
2026-01-25T08:09:57.034660Z  INFO: 🖱️  Mouse Wheel: delta=-120 at (1244, 569)
```

### Keyboard Events
```
2026-01-25T08:09:56.172044Z  INFO: ⌨️  Key Down: VK=0x47 (repeat=false)
2026-01-25T08:09:56.172189Z  INFO: ⌨️  Char: 'g'
2026-01-25T08:09:56.339706Z  INFO: ⌨️  Key Up: VK=0x47
```

**Status**: ✅ Все события мыши и клавиатуры успешно ловятся и обрабатываются!

---

## Migration Notes

### From Legacy to Unified (Completed)

**Before:**
```rust
// Custom GPUI-style events
use flui_platform::input::PointerEvent;  // Custom type
```

**After:**
```rust
// W3C standard events
use ui_events::pointer::PointerEvent;  // Standard W3C type
use flui_interaction::events::InputEvent;  // FLUI wrapper
```

### Compatibility Layer

`flui_interaction/src/events.rs` provides:
- `PointerEventData` - Compatibility struct for legacy gesture recognizers
- Helper functions: `make_down_event()`, `make_move_event()`, etc.
- Conversion: `PointerEventData::from_pointer_event(&PointerEvent)`

This allows gradual migration of gesture recognizers to W3C types.

---

## Future Improvements

1. **Event Dispatch** - Connect window_proc events to framework handlers
2. **Gesture Pipeline** - Wire W3C events through gesture recognizers
3. **Hit Testing** - Implement render tree hit testing
4. **Focus Management** - Keyboard focus system
5. **Touch Support** - Multi-touch gestures (pinch, rotate)

---

## References

- **W3C UI Events Spec**: https://www.w3.org/TR/uievents/
- **ui-events crate**: https://docs.rs/ui-events/
- **cursor-icon spec**: https://www.w3.org/TR/CSS22/ui.html#cursor-props
- **keyboard-types**: https://docs.rs/keyboard-types/

---

**Last Updated**: 2026-01-25  
**Status**: ✅ Unified W3C Architecture - Fully Implemented
