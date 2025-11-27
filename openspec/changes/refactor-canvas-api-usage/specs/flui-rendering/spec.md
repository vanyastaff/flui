## MODIFIED Requirements

### Requirement: Canvas API Usage Patterns

RenderObjects in flui_rendering SHALL use the modern Canvas API patterns from flui_painting for improved readability, safety, and performance.

---

## 1. State Management Patterns

### 1.1 Chaining API (Preferred for paint methods)

Use when painting children between save/restore:

```rust
// ✅ GOOD - Chaining with restored()
ctx.canvas()
    .saved()
    .translated(offset.dx, offset.dy)
    .rotated(angle)
    .clipped_rect(clip_rect);
ctx.paint_child(child_id, Offset::ZERO);
ctx.canvas().restored();

// ❌ BAD - Old style
ctx.canvas().save();
ctx.canvas().translate(offset.dx, offset.dy);
ctx.canvas().rotate(angle);
ctx.canvas().clip_rect(clip_rect);
ctx.paint_child(child_id, Offset::ZERO);
ctx.canvas().restore();
```

### 1.2 Scoped `with_*` Methods (Preferred for self-contained drawing)

Use when all drawing happens inside the closure (no child painting):

```rust
// ✅ GOOD - Self-contained drawing
ctx.canvas().with_translate(100.0, 50.0, |c| {
    c.draw_rect(rect, &paint);
    c.draw_circle(center, radius, &paint);
});

// ✅ GOOD - Nested transforms
ctx.canvas().with_save(|c| {
    c.translate(offset.dx, offset.dy);
    c.rotate(angle);
    c.draw_path(&path, &paint);
});
```

### 1.3 When to Use Each

| Pattern | Use When |
|---------|----------|
| `saved()...restored()` | Need to paint children between save/restore |
| `with_save(\|c\| {...})` | All drawing is self-contained, no child painting |
| `with_translate()` | Simple translation with self-contained drawing |
| `with_clip_rect()` | Clipping with self-contained drawing |
| `with_opacity()` | Opacity layer with self-contained drawing |

---

## 2. Transform Patterns

### 2.1 Single Transform

```rust
// Translation only
ctx.canvas().saved().translated(x, y);

// Rotation only
ctx.canvas().saved().rotated(angle);

// Scale only
ctx.canvas().saved().scaled(factor);
ctx.canvas().saved().scaled_xy(sx, sy);
```

### 2.2 Combined Transforms

```rust
// Translate + Rotate (common for rotated boxes)
ctx.canvas()
    .saved()
    .translated(offset.dx, offset.dy)
    .rotated(self.quarter_turns.radians());

// Translate + Scale (common for fitted boxes)
ctx.canvas()
    .saved()
    .translated(offset.dx, offset.dy)
    .scaled_xy(scale_x, scale_y);

// Translate + Rotate + Scale
ctx.canvas()
    .saved()
    .translated(pivot_x, pivot_y)
    .rotated(angle)
    .scaled(scale)
    .translated(-pivot_x, -pivot_y);

// Using Transform API
ctx.canvas()
    .saved()
    .transformed(&self.transform);
```

### 2.3 Pivot Point Rotation

```rust
// ✅ GOOD - Using rotated_around
ctx.canvas()
    .saved()
    .rotated_around(angle, center_x, center_y);

// ✅ GOOD - Manual pivot (when more control needed)
ctx.canvas()
    .saved()
    .translated(pivot_x, pivot_y)
    .rotated(angle)
    .translated(-pivot_x, -pivot_y);
```

---

## 3. Clipping Patterns

### 3.1 Rectangle Clipping

```rust
// Simple rect clip
ctx.canvas().saved().clipped_rect(clip_rect);

// With offset
ctx.canvas()
    .saved()
    .translated(offset.dx, offset.dy)
    .clipped_rect(local_rect);
```

### 3.2 Rounded Rectangle Clipping

```rust
ctx.canvas().saved().clipped_rrect(rrect);
```

### 3.3 Path Clipping

```rust
ctx.canvas().saved().clipped_path(&path);
```

### 3.4 Conditional Clipping

```rust
let needs_clip = self.clip_behavior != ClipBehavior::None;
if needs_clip {
    ctx.canvas().saved().clipped_rect(clip_rect);
}

// ... paint children ...

if needs_clip {
    ctx.canvas().restored();
}
```

---

## 4. Drawing Convenience Methods

### 4.1 Rounded Rectangles

```rust
// ✅ GOOD - Convenience method for uniform radius
ctx.canvas().draw_rounded_rect(rect, radius, &paint);

// ✅ GOOD - Per-corner radii
ctx.canvas().draw_rounded_rect_corners(rect, tl, tr, br, bl, &paint);

// ❌ BAD - Manual RRect construction
let rrect = RRect::from_rect_circular(rect, radius);
ctx.canvas().draw_rrect(rrect, &paint);
```

### 4.2 Pill Shapes (Fully Rounded)

```rust
// ✅ GOOD - For scrollbar handles, badges, buttons
ctx.canvas().draw_pill(rect, &paint);

// ❌ BAD - Manual calculation
let radius = rect.height().min(rect.width()) / 2.0;
let rrect = RRect::from_rect_circular(rect, radius);
ctx.canvas().draw_rrect(rrect, &paint);
```

### 4.3 Ring Shapes (Donut/Progress)

```rust
// ✅ GOOD - For circular progress indicators
ctx.canvas().draw_ring(center, outer_radius, inner_radius, &paint);

// ❌ BAD - Manual DRRect
let outer = RRect::from_rect_circular(...);
let inner = RRect::from_rect_circular(...);
ctx.canvas().draw_drrect(outer, inner, &paint);
```

### 4.4 Chaining Draw Methods

```rust
// ✅ GOOD - Multiple shapes in chain
ctx.canvas()
    .rect(background_rect, &bg_paint)
    .circle(center, radius, &circle_paint)
    .path(&icon_path, &icon_paint);
```

---

## 5. Batch Drawing

### 5.1 When to Use

Use batch methods when drawing multiple similar shapes with the same paint:

```rust
// ✅ GOOD - Grid cells, chart bars, particles
ctx.canvas().draw_rects(&rects, &paint);

// ✅ GOOD - Scatter plot points
ctx.canvas().draw_circles(&circles, &paint);

// ✅ GOOD - Graph lines
ctx.canvas().draw_lines(&lines, &paint);
```

### 5.2 Grid Patterns

```rust
// ✅ GOOD - Chess board, calendar grid
ctx.canvas().draw_grid(cols, rows, cell_width, cell_height, |c, col, row| {
    let color = if (col + row) % 2 == 0 { Color::WHITE } else { Color::BLACK };
    c.draw_rect(Rect::from_xywh(0.0, 0.0, cell_width - gap, cell_height - gap), &Paint::fill(color));
});
```

### 5.3 Repeat Patterns

```rust
// Horizontal repeat (toolbar icons, tab bar)
ctx.canvas().repeat_x(count, spacing, |c, i| {
    c.draw_circle(Point::new(radius, radius), radius, &paint);
});

// Vertical repeat (list items)
ctx.canvas().repeat_y(count, spacing, |c, i| {
    c.draw_rect(item_rect, &paint);
});

// Radial repeat (clock marks, radial menu)
ctx.canvas().repeat_radial(12, radius, |c, i, angle| {
    c.draw_circle(Point::ZERO, mark_radius, &paint);
});
```

---

## 6. Conditional Drawing

### 6.1 Simple Conditions

```rust
// ✅ GOOD - Conditional chain
ctx.canvas()
    .when(self.show_border, |c| {
        c.rect(border_rect, &border_paint)
    })
    .rect(content_rect, &content_paint);

// ✅ GOOD - If/else in chain
ctx.canvas()
    .when_else(self.is_selected,
        |c| c.rect(rect, &selected_paint),
        |c| c.rect(rect, &normal_paint)
    );
```

### 6.2 Optional Values

```rust
// ✅ GOOD - Draw if Some
ctx.canvas().draw_if_some(self.icon, |c, icon| {
    c.draw_image(icon, icon_rect, None);
});
```

### 6.3 Block Conditional

```rust
// ✅ GOOD - Multiple conditional operations
ctx.canvas().draw_if(self.is_visible, |c| {
    c.draw_rect(background, &bg_paint);
    c.draw_text(text, offset, &style, &text_paint);
    c.draw_rect(border, &border_paint);
});
```

---

## 7. Layer Effects

### 7.1 Opacity

```rust
// ✅ GOOD - For opacity 0.0 < value < 1.0
ctx.canvas().save_layer_opacity(None, self.opacity);
ctx.paint_child(child_id, offset);
ctx.canvas().restored();

// ✅ GOOD - Self-contained opacity
ctx.canvas().with_opacity(0.5, Some(bounds), |c| {
    c.draw_rect(rect, &paint);
});
```

### 7.2 Blend Modes

```rust
ctx.canvas().save_layer_blend(Some(bounds), BlendMode::Multiply);
// ... draw operations ...
ctx.canvas().restored();
```

---

## 8. Debug Helpers

### 8.1 Development Only

```rust
#[cfg(debug_assertions)]
{
    // Show bounds
    ctx.canvas().debug_rect(bounds, Color::RED);
    
    // Show anchor point
    ctx.canvas().debug_point(anchor, 5.0, Color::GREEN);
    
    // Show coordinate axes
    ctx.canvas().debug_axes(50.0);
    
    // Show alignment grid
    ctx.canvas().debug_grid(viewport, 50.0, Color::from_rgba(128, 128, 128, 64));
}
```

---

## 9. DisplayList Caching

### 9.1 Static Content

```rust
// ✅ GOOD - Cache complex static content
let icon = Canvas::record(|c| {
    c.draw_circle(Point::new(16.0, 16.0), 14.0, &outline);
    c.draw_path(&checkmark_path, &fill);
});

// Replay multiple times
ctx.canvas().draw_picture(&icon);
ctx.canvas().with_translate(50.0, 0.0, |c| {
    c.draw_picture(&icon);
});
```

---

## 10. Performance Guidelines

| Do | Don't |
|----|-------|
| Use `draw_rects()` for 10+ rects | Call `draw_rect()` in a loop |
| Use `draw_pill()` for rounded ends | Manually calculate pill radius |
| Use `saved()...restored()` chaining | Separate `save()` and `restore()` calls |
| Use `draw_rounded_rect()` | Construct RRect then draw_rrect |
| Cache DisplayList for static content | Re-record every frame |
| Use `would_be_clipped()` for culling | Draw invisible content |
| Use `with_*` for self-contained drawing | Manual save/restore for simple cases |

---

## Scenarios

#### Scenario: Clip with transform uses chaining API
- **GIVEN** a RenderObject that needs to clip and transform
- **WHEN** implementing the paint method
- **THEN** use chaining: `canvas.saved().translated(x, y).clipped_rect(rect)`

#### Scenario: Rounded rectangle uses convenience method
- **GIVEN** a RenderObject that draws rounded rectangles
- **WHEN** all corners have the same radius
- **THEN** use `canvas.draw_rounded_rect(rect, radius, &paint)`

#### Scenario: Scrollbar uses pill shape
- **GIVEN** a scrollable RenderObject with scrollbar handles
- **WHEN** painting the scrollbar handle
- **THEN** use `canvas.draw_pill(rect, &paint)`

#### Scenario: Image transform uses chaining
- **GIVEN** a RenderImage that applies transforms
- **WHEN** applying multiple transforms in sequence
- **THEN** use chaining: `canvas.saved().translated(x, y).scaled_xy(sx, sy)`

#### Scenario: Grid layout uses batch drawing
- **GIVEN** a RenderGrid painting multiple cells
- **WHEN** cells have same styling
- **THEN** use `canvas.draw_rects(&rects, &paint)` or `canvas.draw_grid()`

#### Scenario: Opacity layer for partial transparency
- **GIVEN** a RenderOpacity with 0.0 < opacity < 1.0
- **WHEN** painting child with opacity
- **THEN** use `canvas.save_layer_opacity(bounds, opacity)` then `restored()`

#### Scenario: Debug visualization in development
- **GIVEN** debug mode is enabled
- **WHEN** showing layout bounds
- **THEN** use `canvas.debug_rect(bounds, Color::RED)`
