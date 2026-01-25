# FLUI Type System Guide

**Date:** 2026-01-24  
**Purpose:** Definitive guide for choosing the correct types in FLUI

---

## Quick Reference Table

| Use Case | Type | Example | Rationale |
|----------|------|---------|-----------|
| **Absolute position** | `Offset<Pixels>` | Mouse cursor, widget position | Coordinates in logical pixel space |
| **Movement delta** | `Offset<PixelDelta>` | Mouse movement, drag delta | Relative change, not absolute position |
| **Velocity** | `Velocity` | Fling velocity, scroll speed | Has `Offset<Pixels>` internally (pixels/second) |
| **Size/dimensions** | `Size<Pixels>` | Widget width/height | Always positive, logical pixels |
| **Bounds/rect** | `Rect<Pixels>` | Hit test area, clip region | Position + Size in logical pixels |
| **Device pixels** | `DevicePixels` | GPU buffers, native window size | Physical pixels on screen |
| **Scaled pixels** | `ScaledPixels` | DPI-scaled output | Intermediate conversion step |

---

## Type Definitions

### Core Units

```rust
// From flui_types/src/geometry/units.rs

/// Logical pixels (layout and measurement)
pub struct Pixels(pub f32);

/// Physical pixels on device
pub struct DevicePixels(pub i32);

/// DPI-scaled pixels
pub struct ScaledPixels(pub f32);

/// Relative pixel change (NOT absolute position)
pub struct PixelDelta(pub f32);
```

### Geometric Types

```rust
// From flui_types/src/geometry/

/// 2D offset (Flutter-style with dx/dy)
pub struct Offset<T: Unit> {
    pub dx: T,
    pub dy: T,
}

/// 2D point (x/y naming)
pub struct Point<T: Unit> {
    pub x: T,
    pub y: T,
}

/// 2D size
pub struct Size<T: Unit> {
    pub width: T,
    pub height: T,
}

/// Rectangle (origin + size)
pub struct Rect<T: Unit> {
    pub origin: Point<T>,
    pub size: Size<T>,
}
```

### Gesture Types

```rust
// From flui_types/src/gestures/velocity.rs

/// Velocity in pixels per second
pub struct Velocity {
    /// IMPORTANT: This is Offset<Pixels>, NOT Offset<f32>!
    pub pixels_per_second: Offset<Pixels>,
}

/// Velocity estimate with confidence
pub struct VelocityEstimate {
    pub duration: Duration,
    pub offset: Offset<Pixels>,
    pub pixels_per_second: Offset<Pixels>,  // NOT Offset<f32>!
    pub confidence: f32,
}
```

---

## Rules for Event Structures

### ✅ CORRECT: Monomorphic Events (GPUI Style)

```rust
// flui_interaction events should use CONCRETE types

pub struct PointerEventData {  // NO generic parameter
    pub pointer_id: PointerId,
    pub pointer_type: PointerType,
    
    // Position: where the pointer IS (absolute)
    pub position: Offset<Pixels>,
    
    // Movement: how much it MOVED (relative delta)
    pub movement: Offset<PixelDelta>,
    
    // Velocity: speed in pixels/second
    pub velocity: Velocity,  // Already has Offset<Pixels> inside
    
    pub buttons: PointerButtons,
    pub pressure: f32,
}

pub struct ScrollEventData {  // NO generic parameter
    pub delta: Offset<PixelDelta>,  // Scroll amount
    pub phase: ScrollPhase,
}

pub struct GestureBinding {  // NO generic parameter
    // All internal state uses concrete types
    hit_test_cache: HashMap<PointerId, HitTestResult>,
    velocity_tracker: VelocityTracker,  // Uses Offset<Pixels> internally
}
```

### ❌ WRONG: Generic Events (Current Problem)

```rust
// This causes 592 compilation errors!

pub struct PointerEventData<U: Unit> {  // ❌ Generic!
    pub position: Offset<U>,  // ❌ Compiler can't infer U
    pub movement: Offset<U>,  // ❌ Should be PixelDelta, not U
}

pub struct GestureBinding<U: Unit> {  // ❌ Forced to be generic!
    cache: HashMap<PointerId, HitTestResult<U>>,  // ❌ ERROR!
}
```

---

## Semantic Meaning of Types

### Pixels vs PixelDelta

**Key difference:** Pixels are **absolute**, PixelDelta is **relative**

```rust
// ✅ Position (absolute coordinate in space)
let cursor_position = Offset::<Pixels>::new(px(100.0), px(200.0));

// ✅ Movement (relative change, dimensionless in a sense)
let movement_delta = Offset::<PixelDelta>::new(delta_px(5.0), delta_px(-3.0));

// ✅ New position after movement
let new_position = Offset::<Pixels>::new(
    cursor_position.dx + movement_delta.dx.to_pixels(),
    cursor_position.dy + movement_delta.dy.to_pixels(),
);

// ❌ WRONG: Using Offset<f32> loses type safety
let bad_delta = Offset::<f32>::new(5.0, -3.0);  // ❌ No unit information!
```

### Velocity Representation

**Velocity is NOT dimensionless!** It has units: **pixels per second**

```rust
// ✅ CORRECT: Velocity struct with proper units
pub struct Velocity {
    pub pixels_per_second: Offset<Pixels>,  // px/s has dimension!
}

let velocity = Velocity::new(Offset::new(px(100.0), px(50.0)));
// Meaning: moving 100 pixels/sec right, 50 pixels/sec down

// ❌ WRONG: Using f32 loses dimensional information
let bad_velocity = Offset::<f32>::new(100.0, 50.0);  // 100 what per what?
```

---

## Conversion Patterns

### Platform Boundary (OS → FLUI)

Convert immediately at the platform layer:

```rust
// In DesktopEmbedder or Platform implementation
impl DesktopEmbedder {
    fn handle_winit_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let scale_factor = self.window.scale_factor();
        
        // Convert to logical pixels IMMEDIATELY
        let logical_pos = Offset {
            dx: Pixels((position.x / scale_factor) as f32),
            dy: Pixels((position.y / scale_factor) as f32),
        };
        
        // Create event with concrete type
        let event = PointerEventData {
            position: logical_pos,  // Offset<Pixels>
            movement: self.calculate_movement(),  // Offset<PixelDelta>
            velocity: self.velocity_tracker.estimate(),  // Velocity
            ..
        };
        
        self.app_binding.handle_pointer_event(event);
    }
    
    fn calculate_movement(&self) -> Offset<PixelDelta> {
        // Movement is the difference from last position
        let delta_dx = current.dx - last.dx;  // Pixels - Pixels
        let delta_dy = current.dy - last.dy;
        
        Offset {
            dx: PixelDelta(delta_dx.get()),  // Convert to PixelDelta
            dy: PixelDelta(delta_dy.get()),
        }
    }
}
```

### Rendering Boundary (FLUI → GPU)

Convert to device pixels when passing to renderer:

```rust
impl RenderingPipeline {
    fn render(&mut self, scene: &Scene) {
        for layer in &scene.layers {
            // Convert logical to device pixels for GPU
            let device_bounds = layer.bounds.map(|px| {
                px.to_device_pixels(self.scale_factor)
            });
            
            self.gpu.draw_rect(device_bounds);
        }
    }
}
```

---

## Migration Guide: Fixing the 592 Errors

### Step 1: Remove Generic Parameters from Events

**Before:**
```rust
pub struct PointerEventData<U: Unit = Pixels> {
    pub position: Offset<U>,
    pub movement: Offset<U>,
}
```

**After:**
```rust
pub struct PointerEventData {  // NO generic!
    pub position: Offset<Pixels>,
    pub movement: Offset<PixelDelta>,
}
```

### Step 2: Remove Generic Parameters from Bindings

**Before:**
```rust
pub struct GestureBinding<U: Unit = Pixels> {
    hit_test_cache: HashMap<PointerId, HitTestResult<U>>,
    velocity_tracker: VelocityTracker<U>,
}
```

**After:**
```rust
pub struct GestureBinding {  // NO generic!
    hit_test_cache: HashMap<PointerId, HitTestResult>,
    velocity_tracker: VelocityTracker,  // Uses Offset<Pixels> internally
}
```

### Step 3: Update Event Handlers

**Before:**
```rust
impl GestureBinding<U> {
    pub fn handle_pointer_event<U: Unit>(&mut self, event: PointerEventData<U>) {
        // ERROR: Can't store generic U in non-generic cache
        self.hit_test_cache.insert(event.pointer_id, result);
    }
}
```

**After:**
```rust
impl GestureBinding {
    pub fn handle_pointer_event(&mut self, event: PointerEventData) {
        // Works! event.position is Offset<Pixels>
        self.hit_test_cache.insert(event.pointer_id, result);
    }
}
```

### Step 4: Update VelocityTracker

**Before:**
```rust
pub struct VelocityTracker<U: Unit> {
    samples: Vec<(Duration, Offset<U>)>,
}

impl<U: Unit> VelocityTracker<U> {
    pub fn add_sample(&mut self, position: Offset<U>) { /* ... */ }
    pub fn estimate(&self) -> Velocity<U> { /* ... */ }  // ERROR: Velocity is not generic!
}
```

**After:**
```rust
pub struct VelocityTracker {  // NO generic!
    samples: Vec<(Duration, Offset<Pixels>)>,
}

impl VelocityTracker {
    pub fn add_sample(&mut self, position: Offset<Pixels>) { /* ... */ }
    pub fn estimate(&self) -> Velocity { /* Works! */ }
}
```

---

## Common Pitfalls

### ❌ Pitfall 1: Using f32 for Units

```rust
// ❌ WRONG: Loses type information
let delta = Offset::<f32>::new(5.0, -3.0);
let velocity = Offset::<f32>::new(100.0, 50.0);

// ✅ CORRECT: Preserves semantic meaning
let delta = Offset::<PixelDelta>::new(delta_px(5.0), delta_px(-3.0));
let velocity = Velocity::new(Offset::new(px(100.0), px(50.0)));
```

### ❌ Pitfall 2: Making Events Generic

```rust
// ❌ WRONG: Creates 592 compilation errors
pub struct MyEvent<U: Unit> {
    pub position: Offset<U>,
}

// ✅ CORRECT: Use concrete type
pub struct MyEvent {
    pub position: Offset<Pixels>,
}
```

### ❌ Pitfall 3: Mixing Absolute and Relative

```rust
// ❌ WRONG: position and delta have different semantic meaning
pub struct PointerEvent {
    pub position: Offset<Pixels>,
    pub movement: Offset<Pixels>,  // Should be PixelDelta!
}

// ✅ CORRECT: Clear semantic distinction
pub struct PointerEvent {
    pub position: Offset<Pixels>,    // Absolute
    pub movement: Offset<PixelDelta>, // Relative
}
```

---

## Summary

### Type Selection Flowchart

```
Is it a coordinate in space?
├─ YES → Offset<Pixels> or Point<Pixels>
└─ NO  → Is it a relative change?
         ├─ YES → Offset<PixelDelta>
         └─ NO  → Is it speed/velocity?
                  ├─ YES → Velocity (contains Offset<Pixels>)
                  └─ NO  → Is it for GPU?
                           ├─ YES → DevicePixels / ScaledPixels
                           └─ NO  → Size<Pixels> or other
```

### Golden Rules

1. **Never make events generic** - Use concrete types (`Pixels`, `PixelDelta`)
2. **Never use `f32` for units** - Always use newtype wrappers
3. **Convert at boundaries** - Platform → `Pixels`, Renderer → `DevicePixels`
4. **Positions are absolute** - Use `Offset<Pixels>`
5. **Deltas are relative** - Use `Offset<PixelDelta>`
6. **Velocities have units** - Use `Velocity` struct, not `Offset<f32>`

---

**Next Step:** Apply these rules to fix the 592 errors in `flui_interaction`!
