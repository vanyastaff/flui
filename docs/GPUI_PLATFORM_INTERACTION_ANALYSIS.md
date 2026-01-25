# GPUI Platform-Interaction Connection Analysis

**Date:** 2026-01-24  
**Purpose:** Understand how GPUI connects platform events to interaction handling, inform FLUI's architecture decisions

---

## Table of Contents
1. [Executive Summary](#executive-summary)
2. [GPUI Architecture Overview](#gpui-architecture-overview)
3. [Event Flow in GPUI](#event-flow-in-gpui)
4. [Type System for Coordinates](#type-system-for-coordinates)
5. [Comparison: GPUI vs FLUI](#comparison-gpui-vs-flui)
6. [Key Takeaways for FLUI](#key-takeaways-for-flui)
7. [Recommendation: Generic Type Strategy](#recommendation-generic-type-strategy)

---

## Executive Summary

**Key Finding:** GPUI uses **monomorphic types** (`Point<Pixels>` everywhere) instead of generic types for event positions.

**Architecture:** GPUI has a **two-tier architecture** (Platform → Window), not three-tier like FLUI:
```
OS → Platform → Window (includes App context)
```

**Event Handling:** Window directly receives `PlatformInput` events and dispatches them via:
1. Hit testing against the last rendered frame
2. Capture phase (back-to-front) for special handlers
3. Bubble phase (front-to-back) for normal handlers

**Critical Insight:** GPUI avoids the generic type complexity that FLUI is facing by:
- Using `Point<Pixels>` for ALL positions (mouse, touch, bounds)
- Using `Point<ScaledPixels>` only for scaled output
- NO generic types in event structures

---

## GPUI Architecture Overview

### Core Components

```rust
// 1. Platform trait (src/platform.rs)
pub trait Platform: 'static {
    fn run(&self, on_finish_launching: Box<dyn FnOnce()>);
    fn quit(&self);
    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Box<dyn PlatformWindow>;
    // ... clipboard, displays, etc.
}

// 2. Window struct (src/window.rs)
pub struct Window {
    platform_window: Box<dyn PlatformWindow>,
    mouse_position: Point<Pixels>,  // <-- ALWAYS Point<Pixels>
    rendered_frame: Frame,
    // ... other state
}

// 3. PlatformInput events (src/interactive.rs)
pub enum PlatformInput {
    KeyDown(KeyDownEvent),
    MouseDown(MouseDownEvent),
    MouseMove(MouseMoveEvent),
    MouseUp(MouseUpEvent),
    // ...
}

// 4. Mouse events with Point<Pixels>
pub struct MouseDownEvent {
    pub button: MouseButton,
    pub position: Point<Pixels>,  // <-- NOT generic!
    pub modifiers: Modifiers,
    pub click_count: usize,
    pub first_mouse: bool,
}
```

### No Separation of Platform and Interaction

**CRITICAL:** In GPUI, there is **NO** separate "interaction" layer. Everything is in `Window`:

```rust
impl Window {
    // Window directly handles platform events
    pub fn dispatch_event(
        &mut self, 
        event: PlatformInput,  // Comes directly from Platform
        cx: &mut App
    ) -> DispatchEventResult {
        // Update internal mouse position state
        self.mouse_position = event.position();
        
        // Dispatch to mouse or keyboard handlers
        if let Some(mouse_event) = event.mouse_event() {
            self.dispatch_mouse_event(mouse_event, cx);
        } else if let Some(key_event) = event.keyboard_event() {
            self.dispatch_key_event(key_event, cx);
        }
    }
}
```

---

## Event Flow in GPUI

### Complete Flow Diagram

```
┌─────────────────────┐
│   Operating System  │
│  (Win32, Wayland,   │
│   X11, macOS)       │
└──────────┬──────────┘
           │ Native events (WM_MOUSEMOVE, etc.)
           ▼
┌─────────────────────────────────────────────────────┐
│              Platform Implementation                 │
│  (WindowsPlatform, LinuxPlatform, MacPlatform)      │
│                                                      │
│  • Convert native events to PlatformInput           │
│  • Mouse position: ALWAYS Point<Pixels>             │
│  • No unit conversions at this layer                │
└──────────┬──────────────────────────────────────────┘
           │ PlatformInput enum
           ▼
┌─────────────────────────────────────────────────────┐
│                    Window                            │
│                                                      │
│  pub fn dispatch_event(                             │
│      &mut self,                                      │
│      event: PlatformInput,  // <-- Direct!          │
│      cx: &mut App                                    │
│  ) -> DispatchEventResult                           │
│                                                      │
│  Flow:                                               │
│  1. Update internal state (mouse_position)          │
│  2. Hit test against rendered_frame                 │
│  3. Dispatch to listeners                           │
└──────────┬──────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────┐
│              Event Dispatch Tree                     │
│                                                      │
│  Capture Phase (back-to-front):                     │
│    for listener in mouse_listeners:                 │
│        listener(event, DispatchPhase::Capture, ...)  │
│                                                      │
│  Bubble Phase (front-to-back):                      │
│    for listener in mouse_listeners.rev():           │
│        listener(event, DispatchPhase::Bubble, ...)   │
└─────────────────────────────────────────────────────┘
```

### Event Dispatching Code

```rust
// src/window.rs:3673
pub fn dispatch_event(&mut self, event: PlatformInput, cx: &mut App) -> DispatchEventResult {
    // Track mouse position with our own state
    let event = match event {
        PlatformInput::MouseMove(mouse_move) => {
            self.mouse_position = mouse_move.position;  // Point<Pixels>
            self.modifiers = mouse_move.modifiers;
            PlatformInput::MouseMove(mouse_move)
        }
        PlatformInput::MouseDown(mouse_down) => {
            self.mouse_position = mouse_down.position;  // Point<Pixels>
            self.modifiers = mouse_down.modifiers;
            PlatformInput::MouseDown(mouse_down)
        }
        // ... other events
    };

    if let Some(any_mouse_event) = event.mouse_event() {
        self.dispatch_mouse_event(any_mouse_event, cx);
    } else if let Some(any_key_event) = event.keyboard_event() {
        self.dispatch_key_event(any_key_event, cx);
    }
    
    DispatchEventResult {
        propagate: cx.propagate_event,
        default_prevented: self.default_prevented,
    }
}

fn dispatch_mouse_event(&mut self, event: &dyn Any, cx: &mut App) {
    let hit_test = self.rendered_frame.hit_test(self.mouse_position());
    
    // Capture phase
    for listener in &mut mouse_listeners {
        listener(event, DispatchPhase::Capture, self, cx);
        if !cx.propagate_event { break; }
    }
    
    // Bubble phase
    if cx.propagate_event {
        for listener in mouse_listeners.iter_mut().rev() {
            listener(event, DispatchPhase::Bubble, self, cx);
            if !cx.propagate_event { break; }
        }
    }
}
```

---

## Type System for Coordinates

### GPUI's Approach: Monomorphic Types

```rust
// src/geometry.rs
pub struct Point<T: Clone + Debug + Default + PartialEq> {
    pub x: T,
    pub y: T,
}

// But in practice, GPUI uses ONLY these concrete types:
// - Point<Pixels>       - Logical pixels (99% of usage)
// - Point<ScaledPixels> - Physical pixels (only for GPU output)
// - Point<DevicePixels> - Device-specific (rare)

// Mouse events: ALWAYS Point<Pixels>
pub struct MouseDownEvent {
    pub position: Point<Pixels>,  // NOT Point<U> or Point<T>
    // ...
}

pub struct MouseMoveEvent {
    pub position: Point<Pixels>,  // NOT Point<U> or Point<T>
    // ...
}

// Window state: ALWAYS Point<Pixels>
pub struct Window {
    mouse_position: Point<Pixels>,  // NOT generic
    viewport_size: Size<Pixels>,    // NOT generic
    // ...
}
```

### Why This Works

**Key insight:** GPUI defines `Point<T>` generically but **never uses it generically in public APIs**.

```rust
// ✅ GPUI approach (monomorphic)
fn handle_mouse_down(&mut self, event: MouseDownEvent) {
    let pos: Point<Pixels> = event.position;  // Known type
    self.mouse_position = pos;                // Known type
}

// ❌ What FLUI is trying to do (generic)
fn handle_mouse_down<U: Unit>(&mut self, event: MouseDownEvent<U>) {
    let pos: Point<U> = event.position;  // Generic - causes 592 errors
    self.mouse_position = pos;           // Type mismatch errors
}
```

### Platform-Specific Conversions

Each platform converts native coordinates to `Point<Pixels>` **before** creating events:

```rust
// src/platform/windows/events.rs (lines 100-200)
fn handle_mouse_down_msg(&self, handle: HWND, button: MouseButton, lparam: LPARAM) {
    // Windows gives us device pixels
    let x_device = lparam.loword() as i32;
    let y_device = lparam.hiword() as i32;
    
    // Convert to logical pixels IMMEDIATELY
    let position = logical_point(
        x_device as f32,
        y_device as f32,
        self.state.scale_factor.get()
    );
    
    // Create event with Point<Pixels> (NOT Point<DevicePixels>)
    let event = MouseDownEvent {
        button,
        position,  // Point<Pixels>
        modifiers: self.modifiers,
        click_count: self.calculate_click_count(),
        first_mouse: false,
    };
    
    // Dispatch to window
    self.window.dispatch_event(PlatformInput::MouseDown(event), &mut cx);
}
```

---

## Comparison: GPUI vs FLUI

### Architecture Differences

| Aspect | GPUI | FLUI (Current) |
|--------|------|----------------|
| **Layers** | 2-tier (Platform → Window) | 3-tier (Platform → App → Interaction) |
| **Event Types** | `PlatformInput` enum | `ui-events` crate (W3C) |
| **Position Type** | `Point<Pixels>` (monomorphic) | `Offset<U>` (generic) |
| **Conversion** | At platform boundary | Deferred to interaction layer |
| **Interaction Crate** | No separate crate | Separate `flui_interaction` |
| **Dependencies** | Window depends on Platform | Interaction independent of Platform |

### Event Type Comparison

```rust
// GPUI approach
pub enum PlatformInput {
    MouseDown(MouseDownEvent),
    MouseMove(MouseMoveEvent),
    // ... ~10 variants
}

pub struct MouseDownEvent {
    pub button: MouseButton,
    pub position: Point<Pixels>,  // Concrete type
    pub modifiers: Modifiers,
    pub click_count: usize,
}

// FLUI approach (current)
use ui_events::prelude::*;  // W3C spec

pub struct PointerEventData<U = Pixels> {  // Generic!
    pub pointer_id: PointerId,
    pub pointer_type: PointerType,
    pub position: Offset<U>,     // Generic position
    pub buttons: PointerButtons,
    // ...
}
```

### Why FLUI Has 592 Generic Errors

```rust
// flui_interaction trying to be generic
pub struct GestureBinding {
    // What type is U here?
    hit_test_cache: HashMap<PointerId, HitTestResult<U>>,  // ERROR!
    
    pub fn handle_pointer_event<U: Unit>(
        &mut self, 
        event: PointerEventData<U>  // U is generic
    ) {
        // Store in cache... but what type?
        self.hit_test_cache.insert(event.pointer_id, result);  // ERROR!
    }
}

// GPUI's solution: DON'T be generic
pub struct Window {
    mouse_position: Point<Pixels>,  // Concrete type
    mouse_hit_test: HitTestResult,  // No generics
    
    pub fn handle_mouse_event(&mut self, event: MouseDownEvent) {
        // event.position is ALWAYS Point<Pixels>
        self.mouse_position = event.position;  // Works!
    }
}
```

---

## Key Takeaways for FLUI

### 1. **Simpler is Better**

GPUI's two-tier architecture (Platform → Window) is simpler than FLUI's three-tier (Platform → App → Interaction).

**Recommendation:** Consider merging `flui_interaction` into `flui_app` or simplifying the boundaries.

### 2. **Avoid Generic Event Types**

GPUI defines `Point<T>` generically but uses it monomorphically in all public APIs.

**Current FLUI mistake:**
```rust
// ❌ Making events generic creates 592 errors
pub struct PointerEventData<U: Unit> {
    pub position: Offset<U>,
}
```

**GPUI's wisdom:**
```rust
// ✅ Use concrete types in events
pub struct MouseDownEvent {
    pub position: Point<Pixels>,  // NOT Point<U>
}
```

### 3. **Convert at Platform Boundary**

GPUI converts device coordinates to logical pixels **immediately** at the platform layer.

```rust
// Platform layer (Windows example)
fn handle_mouse_msg(device_x: i32, device_y: i32) -> PlatformInput {
    let logical_pos = Point {
        x: Pixels(device_x as f32 / scale_factor),
        y: Pixels(device_y as f32 / scale_factor),
    };
    
    PlatformInput::MouseDown(MouseDownEvent {
        position: logical_pos,  // Already converted!
        // ...
    })
}
```

### 4. **Window Owns State**

In GPUI, `Window` directly owns mouse state and hit testing:

```rust
pub struct Window {
    mouse_position: Point<Pixels>,
    mouse_hit_test: HitTestResult,
    rendered_frame: Frame,
    // ...
}
```

This is simpler than FLUI's distributed state across multiple bindings.

### 5. **Direct Dispatch Pattern**

GPUI's event dispatch is **synchronous and direct**:

```
Platform → Window.dispatch_event() → dispatch_mouse_event() → Listeners
```

No intermediate layers, no async complexity.

### 6. **W3C Events May Be Overkill**

FLUI uses `ui-events` crate for W3C compliance, but GPUI shows you don't need full W3C spec for a native UI framework.

**GPUI's events:**
- MouseDown, MouseMove, MouseUp
- KeyDown, KeyUp, ModifiersChanged
- ScrollWheel, FileDrop
- ~10 total variants

**W3C ui-events:**
- PointerEvent, TouchEvent, MouseEvent (overlapping)
- Full DOM event model (capture, target, bubble)
- Event composition, coalescing, etc.
- 40+ event types

---

## Recommendation: Generic Type Strategy

Based on GPUI analysis, here's the recommended approach for FLUI:

### Option C (Modified): Mixed with Monomorphic Events

```rust
// 1. Keep generic Point/Offset in flui_types
pub struct Offset<T = Pixels, U = Pixels> {
    pub x: T,
    pub y: U,
}

// 2. Use CONCRETE types in flui_interaction events (GPUI style)
pub struct PointerEventData {  // NO generic parameter!
    pub pointer_id: PointerId,
    pub pointer_type: PointerType,
    pub position: Offset<Pixels>,      // Concrete type for positions
    pub movement: Offset<PixelDelta>,  // Concrete type for deltas (NOT f32!)
    pub buttons: PointerButtons,
    pub pressure: f32,
}

// 3. Platform converts immediately (GPUI style)
impl DesktopEmbedder {
    fn handle_winit_event(&mut self, event: WinitEvent) {
        match event {
            WinitEvent::CursorMoved { position, .. } => {
                let logical_pos = Offset {
                    x: Pixels(position.x as f32 / scale_factor),
                    y: Pixels(position.y as f32 / scale_factor),
                };
                
                let event = PointerEventData {
                    position: logical_pos,  // Concrete type
                    // ...
                };
                
                self.app_binding.handle_pointer_event(event);
            }
        }
    }
}

// 4. GestureBinding uses concrete types (no generics!)
pub struct GestureBinding {
    hit_test_cache: HashMap<PointerId, HitTestResult>,  // No generic!
    
    pub fn handle_pointer_event(&mut self, event: PointerEventData) {
        // event.position is ALWAYS Offset<Pixels>
        self.hit_test_cache.insert(event.pointer_id, result);  // Works!
    }
}
```

### Why This Solves the 592 Errors

**Before (Generic approach):**
```rust
// ❌ Generic parameter cascades through entire system
pub struct PointerEventData<U: Unit> {
    pub position: Offset<U>,
}

pub struct GestureBinding<U: Unit> {  // Forced to be generic!
    hit_test_cache: HashMap<PointerId, HitTestResult<U>>,
}

// Compiler can't infer U in 592 places!
```

**After (Monomorphic approach):**
```rust
// ✅ Concrete types, no generics in public API
pub struct PointerEventData {
    pub position: Offset<Pixels>,
}

pub struct GestureBinding {  // No generic parameter!
    hit_test_cache: HashMap<PointerId, HitTestResult>,
}

// Everything just works, no inference needed!
```

---

## Conclusion

**What GPUI does right:**
1. ✅ Monomorphic event types (`Point<Pixels>` everywhere)
2. ✅ Convert coordinates at platform boundary
3. ✅ Simple two-tier architecture
4. ✅ Direct, synchronous event dispatch
5. ✅ Window owns interaction state

**What FLUI should adopt:**
1. 🎯 Use `Offset<Pixels>` (concrete) in all event structures
2. 🎯 Convert coordinates in Platform → App boundary
3. 🎯 Keep `flui_interaction` independent but NOT generic
4. 🎯 Consider merging GestureBinding into AppBinding

**What FLUI should keep:**
1. ✅ W3C-compliant event model (more standard)
2. ✅ Three-tier architecture (more modular)
3. ✅ Separation of Platform and Interaction (cleaner)
4. ✅ Generic `Offset<T, U>` in flui_types (for flexibility)

**Action Item:**
Apply **Option C (Modified)** to `flui_interaction`:
- Change `PointerEventData<U>` → `PointerEventData` (no generic)
- Use `Offset<Pixels>` for positions (absolute coordinates)
- Use `Offset<PixelDelta>` for movement deltas (relative changes)
- Use `Velocity` struct (already has `Offset<Pixels>` inside) for velocities
- Fix 592 errors in one sweep

---

**References:**
- `.gpui/src/window.rs` - Event dispatching logic
- `.gpui/src/interactive.rs` - Event type definitions
- `.gpui/src/geometry.rs` - Point/Size type system
- `.gpui/src/platform/windows/events.rs` - Platform conversion example
