# Transform Visual Rendering - Implementation Notes

## Overview

This document explains how visual transform rendering (rotation, scale) could be implemented for Container widget, based on egui's shape rendering capabilities.

## Current State

**API**: ✅ Complete
- `with_transform(Transform)` method implemented
- `with_transform_alignment(Alignment)` for pivot point
- Transform struct with rotation, scale, translation

**Rendering**: ⚠️ Placeholder
- Transform values are stored but not applied visually
- Widgets render normally without transformation

## egui Transform Capabilities

### What egui CAN Do

egui supports rotation for **individual shapes**, not widget hierarchies:

```rust
// Example: RotatedLabel using TextShape
impl Widget for RotatedLabel {
    fn ui(self, ui: &mut Ui) -> Response {
        let galley = ui.fonts(|f| f.layout_no_wrap(
            self.text.to_string(),
            FontId::default(),
            ui.visuals().text_color()
        ));

        let pos = ui.cursor().min;
        let rot = Rot2::from_angle(self.angle);

        ui.painter().add(TextShape {
            pos,
            galley,
            underline: Stroke::NONE,
            angle: self.angle, // ← KEY: TextShape supports rotation!
            fallback_color: ui.visuals().text_color(),
            opacity_factor: 1.0,
        });

        ui.allocate_rect(rect, Sense::hover())
    }
}
```

**Key Points**:
- `TextShape` has `angle` property for rotation
- `Mesh` can be rotated via `Mesh::with_vertices()` + manual rotation
- Individual shapes (rectangles, circles, etc.) can be transformed

### What egui CANNOT Do

egui cannot transform **widget hierarchies**:
- Widgets are immediate-mode - they position themselves during layout
- No scene graph or transform stack
- Child widgets don't inherit parent transforms
- Interactive widgets (buttons, inputs) lose interactivity when rendered as shapes

## Implementation Approaches

### Approach 1: Simple Content Rendering (Recommended for MVP)

**Use Case**: Container with simple, non-interactive content (text, icons, shapes)

**Implementation**:
1. Detect if container has transform
2. Instead of creating child UI, render content as shapes
3. Apply rotation/scale to shapes using egui painter

**Pros**:
- Works for text, labels, simple decorations
- Clean visual output
- Relatively simple to implement

**Cons**:
- Loses widget interactivity
- Only works for renderable content (text, shapes)
- Complex widgets (buttons, inputs) can't be rendered this way

**Code Sketch**:
```rust
impl Container {
    pub fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // ... existing layout code ...

        if let Some(transform) = self.transform {
            // Render as transformed shapes instead of widgets
            self.render_transformed_content(ui, transform, inner_rect)
        } else {
            // Normal widget rendering
            self.render_child_widget(ui, inner_rect)
        }
    }

    fn render_transformed_content(
        &self,
        ui: &mut egui::Ui,
        transform: Transform,
        rect: egui::Rect,
    ) -> egui::Response {
        // Calculate pivot point
        let pivot = self.transform_alignment
            .unwrap_or(Alignment::CENTER)
            .calculate_offset(rect.size(), rect.size())
            + rect.min.to_vec2();

        // Apply rotation
        let rot = Rot2::from_angle(transform.rotation);

        // Render child content as shapes
        // This is where we'd need to convert widget tree to shapes
        // For MVP: only support text content

        ui.allocate_rect(rect, Sense::hover())
    }
}
```

### Approach 2: Render-to-Texture (Full Solution)

**Use Case**: Container with complex, interactive widget hierarchies

**Implementation**:
1. Render child widgets to an offscreen texture
2. Transform the texture (rotate, scale)
3. Blit transformed texture to screen

**Pros**:
- Works for any widget hierarchy
- Preserves visual appearance of complex UIs
- Can cache texture for performance

**Cons**:
- Loses widget interactivity (clicks, hover, etc.)
- Requires texture allocation and rendering overhead
- More complex implementation
- egui doesn't have built-in render-to-texture support

**Feasibility**: Requires custom egui extension or integration with graphics backend

### Approach 3: Transform Propagation (Ideal but Not Possible in egui)

**Use Case**: True hierarchical transforms like Flutter/web

**Implementation**:
- Push transform to stack before child rendering
- Children inherit and accumulate transforms
- Pop transform after rendering

**Why Not Possible**:
- egui is immediate-mode without transform stack
- Widgets position themselves absolutely during layout
- No parent-child transform inheritance
- Would require fundamental egui architecture changes

## Recommended Path Forward

### Phase 1: Documentation (DONE ✅)
- Document current API state
- Explain egui limitations
- Provide examples of what's possible

### Phase 2: Simple Transform Rendering (Optional)
Implement visual rotation for simple cases:

1. **Detect renderable content**:
   - Check if child is simple text/label
   - Detect static decorative content

2. **Render as shapes**:
   - Convert text to TextShape with rotation
   - Apply transform to decoration shapes

3. **Fallback**:
   - If content is complex, render without transform (current behavior)
   - Log warning about interactivity loss

**Estimated Effort**: 2-3 hours

### Phase 3: Advanced Rendering (Future)
- Implement render-to-texture for complex content
- Requires graphics backend integration
- Consider egui-gizmo or similar libraries

**Estimated Effort**: 1-2 days

## User-Facing Documentation

For now, document the current state:

```rust
/// Set the transformation to apply to the container.
///
/// # Note: Visual Rendering Limitations
///
/// While the transform API is fully implemented, visual rendering is currently
/// limited by egui's immediate-mode architecture:
///
/// - **API**: Fully working - transform values are stored and accessible
/// - **Rendering**: Placeholder - transforms are not visually applied
///
/// egui supports rotation for individual shapes (TextShape, Mesh), but not for
/// widget hierarchies. Future versions may implement shape-based rendering for
/// simple content, but interactive widgets would lose interactivity.
///
/// For rotation of individual text elements, consider using egui::TextShape directly:
/// ```ignore
/// ui.painter().add(TextShape {
///     angle: rotation_radians,
///     ..TextShape::new(pos, galley)
/// });
/// ```
pub fn with_transform(mut self, transform: Transform) -> Self {
    self.transform = Some(transform);
    self
}
```

## Conclusion

**Current Recommendation**: Keep API as-is, document limitations clearly

The user's RotatedLabel example shows that rotation IS possible in egui, but only for:
- Individual shapes (TextShape, Mesh, etc.)
- Non-interactive content
- Content that can be rendered as primitives

For Container widget with arbitrary child widgets:
- ✅ API is complete and ready
- ⚠️ Visual rendering would lose interactivity
- 📝 Document trade-offs clearly
- 🔮 Consider implementing for simple use cases in future

The 100% API parity goal is achieved. Visual rendering is an enhancement that comes with significant trade-offs.

---

**Status**: Documentation complete, implementation optional based on use case priority
**Date**: 2025-10-16
