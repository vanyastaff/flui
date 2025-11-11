# Spec Delta: Painter API Viewport Bounds

**Change ID:** `fix-drawcommand-transforms`
**Capability:** `painter-api`
**Status:** Implemented

## ADDED Requirements

### Requirement: Painter Viewport Bounds Access

The Painter trait SHALL provide a method to query the viewport bounds (rendering surface dimensions).

**Rationale**: Enable DrawColor and other full-canvas operations to fill the entire viewport regardless of drawn content bounds. Required for correct Canvas.drawColor() semantics matching Flutter's behavior.

#### Scenario: Viewport Bounds Query

**Given** a Painter implementation
**When** viewport_bounds() is called
**Then** it SHALL return a Rect representing the full viewport from (0, 0) to (width, height)
**And** bounds SHALL reflect current viewport size

```rust
pub trait Painter {
    // ... existing methods ...

    /// Get the viewport bounds (entire rendering surface)
    ///
    /// Returns a rectangle from (0, 0) to (viewport_width, viewport_height).
    /// Used by DrawColor and other operations that need to fill the entire canvas.
    fn viewport_bounds(&self) -> Rect;
}
```

#### Scenario: WgpuPainter Viewport Bounds

**Given** a WgpuPainter with size (800, 600)
**When** viewport_bounds() is called
**Then** it SHALL return Rect::from_ltrb(0.0, 0.0, 800.0, 600.0)

```rust
impl Painter for WgpuPainter {
    fn viewport_bounds(&self) -> Rect {
        Rect::from_ltrb(0.0, 0.0, self.size.0 as f32, self.size.1 as f32)
    }
}
```

#### Scenario: Viewport Resize Updates

**Given** a WgpuPainter with initial size (800, 600)
**When** resize(1024, 768) is called
**And** viewport_bounds() is queried
**Then** it SHALL return updated Rect::from_ltrb(0.0, 0.0, 1024.0, 768.0)

```rust
let mut painter = WgpuPainter::new(device, queue, format, (800, 600));
assert_eq!(painter.viewport_bounds(), Rect::from_ltrb(0.0, 0.0, 800.0, 600.0));

painter.resize(1024, 768);
assert_eq!(painter.viewport_bounds(), Rect::from_ltrb(0.0, 0.0, 1024.0, 768.0));
```

#### Scenario: Viewport Bounds vs Display List Bounds

**Given** a Painter with viewport (800, 600)
**And** DisplayList with content bounds (50, 50, 150, 150)
**When** DrawColor command queries viewport_bounds()
**Then** it SHALL return (0, 0, 800, 600), NOT (50, 50, 150, 150)
**And** full viewport SHALL be filled with color

```rust
// Correct behavior
let viewport = painter.viewport_bounds();         // Rect(0, 0, 800, 600) ✅
let content = display_list.bounds();              // Rect(50, 50, 150, 150) ❌ Wrong for DrawColor

// DrawColor uses viewport:
painter.rect(viewport, &Paint::fill(color));      // ✅ Fills entire screen
```

## MODIFIED Requirements

_No existing requirements modified_

## REMOVED Requirements

_No requirements removed_

## Dependencies

- **flui_types**: Provides Rect geometry type
- **Related Changes**: Used by `engine-execution` spec for DrawColor implementation

## Implementation Notes

### Trait Method Placement

The `viewport_bounds()` method is added after clipping methods in the Painter trait:

```rust
pub trait Painter {
    // Core drawing methods
    fn rect(&mut self, rect: Rect, paint: &Paint);
    fn rrect(&mut self, rrect: RRect, paint: &Paint);
    // ...

    // Transform stack
    fn save(&mut self);
    fn restore(&mut self);
    fn translate(&mut self, offset: Offset);
    fn rotate(&mut self, angle: f32);
    fn scale(&mut self, sx: f32, sy: f32);

    // Clipping
    fn clip_rect(&mut self, rect: Rect);
    fn clip_rrect(&mut self, rrect: RRect);

    // Viewport information
    fn viewport_bounds(&self) -> Rect;  // ← NEW

    // Advanced methods with default implementations
    // ...
}
```

### WgpuPainter Implementation

The implementation accesses the existing `size: (u32, u32)` field:

```rust
pub struct WgpuPainter {
    // ...
    /// Viewport size (width, height)
    size: (u32, u32),
    // ...
}

impl Painter for WgpuPainter {
    fn viewport_bounds(&self) -> Rect {
        Rect::from_ltrb(
            0.0,
            0.0,
            self.size.0 as f32,
            self.size.1 as f32
        )
    }
}
```

### Migration for Custom Painter Implementations

Custom Painter implementations MUST add the viewport_bounds() method:

**Before (won't compile):**
```rust
struct MyCustomPainter {
    width: u32,
    height: u32,
}

impl Painter for MyCustomPainter {
    // ... existing methods ...
    // ❌ Missing viewport_bounds() - compilation error
}
```

**After (compiles):**
```rust
struct MyCustomPainter {
    width: u32,
    height: u32,
}

impl Painter for MyCustomPainter {
    // ... existing methods ...

    fn viewport_bounds(&self) -> Rect {
        Rect::from_ltrb(0.0, 0.0, self.width as f32, self.height as f32)
    }
}
```

## Validation

### Trait Compilation

```bash
cargo check -p flui_engine  # ✅ Trait compiles
```

### Implementation Validation

- WgpuPainter implementation added ✅
- Returns correct viewport dimensions ✅
- Updates on resize() ✅

## Use Cases

### Primary Use Case: DrawColor

DrawColor command fills entire viewport:

```rust
DrawCommand::DrawColor { color, transform, .. } => {
    Self::with_transform(painter, transform, |painter| {
        let viewport_bounds = painter.viewport_bounds();
        let paint = Paint::fill(*color);
        painter.rect(viewport_bounds, &paint);
    });
}
```

### Future Use Cases

1. **Full-Screen Effects**: Blur, overlay, color filters
2. **Background Gradients**: Full viewport linear/radial gradients
3. **Cursor Bounds**: Constrain mouse/touch coordinates to viewport
4. **Hit Testing**: Check if point is within visible area
5. **Viewport-Relative Positioning**: Position elements relative to screen edges

## Breaking Changes

**API Change:** Added new required method to Painter trait

**Impact:**
- WgpuPainter: Updated ✅
- Custom implementations: Must add method

**Migration Effort:** Low - single method implementation

**Migration Guide:**

```rust
// Add to custom Painter implementation:
fn viewport_bounds(&self) -> Rect {
    Rect::from_ltrb(0.0, 0.0, self.width as f32, self.height as f32)
}
```

## Notes

- Viewport bounds always start at (0, 0) origin
- Size is in physical pixels (not logical/DPI-scaled)
- Immutable method (`&self`, not `&mut self`) - viewport size is read-only during rendering
- Coordinate system matches Canvas: origin top-left, Y-axis down
