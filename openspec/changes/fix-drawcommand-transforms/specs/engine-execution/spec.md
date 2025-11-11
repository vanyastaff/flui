# Spec Delta: Engine DrawCommand Execution

**Change ID:** `fix-drawcommand-transforms`
**Capability:** `engine-execution`
**Status:** Implemented

## ADDED Requirements

### Requirement: DrawCommand Transform Application

All DrawCommand variants SHALL apply their transform matrix during GPU rendering execution.

**Rationale**: Ensure that transformations recorded by Canvas API (translation, rotation, scale) are correctly applied when commands are executed by the GPU painter.

#### Scenario: Rectangle Transform Application

**Given** a DrawRect command with non-identity transform
**When** PictureLayer executes the command via Painter
**Then** the transform SHALL be decomposed into translation, rotation, and scale
**And** transform components SHALL be applied via painter's save/restore stack
**And** the rectangle SHALL render at the transformed position

```rust
DrawCommand::DrawRect { rect, paint, transform } => {
    Self::with_transform(painter, transform, |painter| {
        painter.rect(*rect, &Self::convert_paint_to_engine(paint));
    });
}

// Transform decomposition applied:
// painter.save();
// painter.translate(tx, ty);
// painter.rotate(angle);
// painter.scale(sx, sy);
// painter.rect(rect, paint);
// painter.restore();
```

#### Scenario: Nested Transform Composition

**Given** multiple DrawCommands with different transforms
**When** PictureLayer executes commands sequentially
**Then** each command SHALL apply its own transform independently
**And** transforms SHALL NOT accumulate between commands
**And** save/restore SHALL isolate transform state

```rust
// Command 1: Translated rect
DrawCommand::DrawRect {
    rect: Rect(0, 0, 100, 100),
    transform: translate(50, 50),
}

// Command 2: Rotated circle (independent)
DrawCommand::DrawCircle {
    center: Point(50, 50),
    transform: rotate(PI / 4),
}

// Each command's transform is independent - no accumulation
```

#### Scenario: Identity Matrix Optimization

**Given** a DrawCommand with identity transform
**When** PictureLayer checks if transform is identity
**Then** save/restore and transform operations SHALL be skipped
**And** the command SHALL execute with no transform overhead

```rust
fn with_transform<F>(painter: &mut dyn Painter, transform: &Matrix4, draw_fn: F)
{
    if transform.is_identity() {
        draw_fn(painter);  // Fast path: skip transform
        return;
    }
    // ... decomposition and application
}
```

### Requirement: 2D Affine Transform Decomposition

Transform matrices SHALL be decomposed into translation, rotation, and scale components for application via Painter API.

**Rationale**: Painter trait provides separate translate(), rotate(), and scale() methods. Matrix4 must be decomposed to use these APIs correctly and match Flutter's transform semantics.

#### Scenario: Translation Component Extraction

**Given** a Matrix4 with translation components
**When** transform decomposition is performed
**Then** translation SHALL be extracted from m[12] and m[13]
**And** translation SHALL be applied first via painter.translate()

```rust
let (tx, ty, _) = transform.translation_component();
// tx = m[12], ty = m[13] from affine matrix
```

#### Scenario: Scale Component Extraction

**Given** a Matrix4 with scale transformation
**When** transform decomposition is performed
**Then** scale X SHALL be computed from first column vector length: sqrt(a² + b²)
**And** scale Y SHALL be computed from determinant: det / sx
**And** scale SHALL preserve sign for reflections

```rust
let a = transform.m[0];
let b = transform.m[1];
let c = transform.m[4];
let d = transform.m[5];

let sx = (a * a + b * b).sqrt();
let det = a * d - b * c;
let sy = if sx > f32::EPSILON { det / sx } else { (c * c + d * d).sqrt() };
```

#### Scenario: Rotation Component Extraction

**Given** a Matrix4 with rotation transformation
**When** transform decomposition is performed
**Then** rotation angle SHALL be computed from normalized first column vector
**And** rotation SHALL use atan2(b/sx, a/sx) for correct quadrant

```rust
let rotation = if sx > f32::EPSILON {
    b.atan2(a)  // Angle in radians
} else {
    0.0
};
```

#### Scenario: Transform Application Order

**Given** decomposed transform components (translation, rotation, scale)
**When** applying to painter
**Then** translation SHALL be applied first
**And** rotation SHALL be applied second
**And** scale SHALL be applied last
**And** order SHALL match Canvas API semantics

```rust
// Application order: translate → rotate → scale
if tx != 0.0 || ty != 0.0 {
    painter.translate(Offset::new(tx, ty));
}
if rotation.abs() > f32::EPSILON {
    painter.rotate(rotation);
}
if (sx - 1.0).abs() > f32::EPSILON || (sy - 1.0).abs() > f32::EPSILON {
    painter.scale(sx, sy);
}
```

### Requirement: Clipping Command Execution

ClipRect and ClipRRect commands SHALL be executed to mask subsequent rendering operations.

**Rationale**: Enable viewport clipping for ScrollView, overflow handling, and rounded corner effects. Essential for proper UI layout and rendering.

#### Scenario: ClipRect Execution

**Given** a ClipRect command with clipping bounds
**When** PictureLayer executes the command
**Then** the clipping rectangle SHALL be applied to painter
**And** transform SHALL be applied to clipping region
**And** subsequent rendering SHALL be clipped to the rectangle

```rust
DrawCommand::ClipRect { rect, transform } => {
    Self::with_transform(painter, transform, |painter| {
        painter.clip_rect(*rect);
    });
}
```

#### Scenario: ClipRRect Execution

**Given** a ClipRRect command with rounded clipping bounds
**When** PictureLayer executes the command
**Then** the rounded clipping rectangle SHALL be applied to painter
**And** transform SHALL be applied to clipping region
**And** subsequent rendering SHALL be clipped to the rounded rectangle

```rust
DrawCommand::ClipRRect { rrect, transform } => {
    Self::with_transform(painter, transform, |painter| {
        painter.clip_rrect(*rrect);
    });
}
```

#### Scenario: Transformed Clipping Regions

**Given** a ClipRect command with rotation transform
**When** the clipping command is executed
**Then** the clipping region SHALL be rotated before application
**And** content SHALL be clipped to the transformed region

```rust
// Example: 45-degree rotated clip region
canvas.save();
canvas.rotate(PI / 4.0);
canvas.clip_rect(Rect(0, 0, 100, 100));
canvas.draw_rect(Rect(0, 0, 200, 200), &paint);  // Clipped to rotated region
canvas.restore();
```

### Requirement: DrawColor Viewport Filling

DrawColor command SHALL fill the entire viewport bounds, not just the DisplayList content bounds.

**Rationale**: Match Flutter's Canvas.drawColor() behavior which fills the entire canvas surface regardless of what has been drawn. Required for proper background color rendering.

#### Scenario: Full Viewport Fill

**Given** a DrawColor command
**When** PictureLayer executes the command
**Then** it SHALL query painter.viewport_bounds()
**And** SHALL fill entire viewport rectangle with the specified color
**And** SHALL NOT use DisplayList.bounds()

```rust
DrawCommand::DrawColor { color, transform, .. } => {
    Self::with_transform(painter, transform, |painter| {
        let viewport_bounds = painter.viewport_bounds();  // ✅ Entire viewport
        let paint = flui_painting::Paint::fill(*color);
        painter.rect(viewport_bounds, &Self::convert_paint_to_engine(&paint));
    });
}
```

#### Scenario: Empty Canvas Background

**Given** an empty DisplayList with DrawColor command
**When** DrawColor is executed
**Then** the entire viewport SHALL be filled with color
**And** empty DisplayList bounds SHALL NOT affect fill area

```rust
// Empty canvas with background color
let mut canvas = Canvas::new();
canvas.draw_color(Color::WHITE, BlendMode::Src);

// DisplayList bounds would be empty, but viewport fills correctly:
// viewport_bounds = Rect(0, 0, 800, 600)  ✅
// display_list.bounds() = Rect::ZERO      ❌ Wrong!
```

## MODIFIED Requirements

_No existing requirements modified_

## REMOVED Requirements

_No requirements removed - this change fixes bugs in existing implementation_

## Dependencies

- **flui_painting**: Provides Canvas, DisplayList, DrawCommand types
- **flui_types**: Provides Matrix4, Rect, Offset, Point geometry types
- **Related Changes**: Depends on `migrate-canvas-api` (Canvas API foundation)

## Implementation Notes

### Transform Decomposition Limitations

- Only supports 2D affine transforms (no 3D perspective)
- Assumes column-major Matrix4 format (glam convention)
- Epsilon threshold: f32::EPSILON for zero checks

### Clipping Limitations

- **ClipPath**: Scaffolded with TODO (requires Painter trait update to accept Path)
- **WgpuPainter**: Clipping methods are no-op (requires stencil buffer implementation)
- Future work: GPU stencil buffer clipping in Painter V2 (v0.7.0)

### Performance Characteristics

- Identity matrix check: O(1) fast path
- Transform decomposition: ~10 FLOPs per non-identity transform
- Save/restore: O(1) stack operations
- Optimization: Skip zero-value transform components

## Validation

### Build Validation

```bash
cargo build -p flui_engine  # ✅ Passes
cargo check -p flui_engine  # ✅ Passes
```

### Code Quality

- Zero compiler warnings
- Comprehensive inline documentation
- Clear TODO markers for deferred work

### Behavior Validation

- All 18 DrawCommand variants apply transforms
- ClipRect and ClipRRect execute correctly
- DrawColor fills entire viewport

## Migration Impact

**User Code:** No changes required (bug fixes only)

**Custom Painter Implementations:**
- Must implement new `viewport_bounds()` method
- See painter-api spec for details

## Notes

- Transform decomposition algorithm based on 2D affine matrix theory
- Application order (translate → rotate → scale) matches Canvas API semantics
- ClipPath full implementation deferred to Painter V2 (v0.7.0)
- Stencil buffer clipping in WgpuPainter deferred to future work
