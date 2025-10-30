# Overflow Handling in Flui

## Problem Statement

When content doesn't fit in a container (e.g., Row/Column), what should happen?

### Current Behavior
Currently, Flui simply clamps the size to constraints, which means:
- Content that doesn't fit is **invisibly cut off**
- No visual indication of the problem
- No warning to the developer
- User has no control over the behavior

### Example Problem

```rust
// This card is 350px wide
Card::builder()
    .child(
        Container::builder()
            .width(350.0)  // Card width
            .child(
                Row::builder()
                    .children(vec![
                        // These might total > 350px!
                        Text::builder().data("Long text...").build().into(),
                        Button::builder().text("Button").build().into(),
                        // ... more widgets
                    ])
                    .build()
            )
            .build()
    )
    .build()
```

When the screen shrinks, the Row's children don't fit, but there's no indication!

## Solution: Follow Flutter's Approach

Flutter has a proven solution with three levels:

### 1. **Debug Mode - Visual Warning**

Show a clear visual indicator when overflow occurs (only in debug builds):

```rust
impl MultiRender for RenderFlex {
    fn paint(&self, tree: &ElementTree, child_ids: &[ElementId], offset: Offset) -> BoxedLayer {
        let mut layer = /* normal painting */;

        // In debug mode, show overflow indicator
        #[cfg(debug_assertions)]
        if let Some(overflow) = self.calculate_overflow() {
            layer = paint_overflow_indicator(layer, overflow);
            eprintln!("⚠️  RenderFlex overflow: {:.1}px on {:?} axis",
                      overflow, self.direction);
        }

        layer
    }
}

fn paint_overflow_indicator(layer: BoxedLayer, overflow: f32) -> BoxedLayer {
    // Paint yellow-black diagonal stripes (Flutter style)
    // This makes overflow VERY obvious to developers
}
```

### 2. **Clip Behavior Control**

Give developers control over how overflow is handled:

```rust
pub enum Clip {
    /// No clipping - content can overflow (default for performance)
    None,

    /// Hard edge clipping (fast, no anti-aliasing)
    HardEdge,

    /// Anti-aliased clipping (smoother edges, slower)
    AntiAlias,

    /// Highest quality clipping with save layer (slowest)
    AntiAliasWithSaveLayer,
}

pub struct RenderFlex {
    pub direction: Axis,
    pub clip_behavior: Clip,  // NEW: Control clipping
    // ... other fields
}

impl Row {
    pub fn builder() -> RowBuilder {
        RowBuilder {
            clip_behavior: Clip::None,  // Default
            // ...
        }
    }
}
```

**Usage:**

```rust
Row::builder()
    .clip_behavior(Clip::HardEdge)  // Clip overflow
    .children(vec![...])
    .build()

// Or allow overflow (for custom scroll handling)
Row::builder()
    .clip_behavior(Clip::None)
    .children(vec![...])
    .build()
```

### 3. **Provide Solutions**

Developers should have tools to handle overflow properly:

#### A) **Flexible/Expanded** (Already implemented!)

```rust
Row::builder()
    .children(vec![
        Flexible::builder()
            .flex(1)
            .child(Text::builder().data("Adapts to space").build())
            .build()
            .into(),
        Text::builder().data("Fixed size").build().into(),
    ])
    .build()
```

#### B) **Wrap** (Already implemented!)

```rust
Wrap::builder()
    .spacing(8.0)
    .run_spacing(8.0)
    .children(vec![
        // Automatically wraps to next line
        widget1.into(),
        widget2.into(),
        widget3.into(),
    ])
    .build()
```

#### C) **ScrollView** (Future implementation)

```rust
SingleChildScrollView::builder()
    .scroll_direction(Axis::Horizontal)
    .child(
        Row::builder()
            .children(vec![...])  // Can overflow, will scroll
            .build()
    )
    .build()
```

## Implementation Plan

### Phase 1: Debug Overflow Indicator ✅ **Recommended for immediate implementation**

```rust
// In RenderFlex
pub struct RenderFlex {
    // ... existing fields

    #[cfg(debug_assertions)]
    overflow_amount: f32,  // Track overflow for debug rendering
}

impl MultiRender for RenderFlex {
    fn layout(...) -> Size {
        // ... existing layout code

        #[cfg(debug_assertions)]
        {
            // Calculate if we have overflow
            let max_size = match self.direction {
                Axis::Horizontal => constraints.max_width,
                Axis::Vertical => constraints.max_height,
            };

            self.overflow_amount = (total_main_size - max_size).max(0.0);

            if self.overflow_amount > 0.0 {
                eprintln!(
                    "⚠️  RenderFlex overflow: {:.1}px does not fit in {:.1}px ({:?} axis)",
                    total_main_size,
                    max_size,
                    self.direction
                );
            }
        }

        // ... return size
    }

    fn paint(...) -> BoxedLayer {
        // ... normal painting

        #[cfg(debug_assertions)]
        if self.overflow_amount > 0.0 {
            // Paint overflow indicator
            return self.paint_with_overflow_indicator(normal_layer);
        }

        normal_layer
    }
}

#[cfg(debug_assertions)]
impl RenderFlex {
    fn paint_with_overflow_indicator(&self, layer: BoxedLayer) -> BoxedLayer {
        // Create a ContainerLayer with:
        // 1. Original content
        // 2. Yellow-black striped indicator on overflow side

        // Flutter uses a repeated diagonal stripe pattern
        // Yellow (#FFFF00) and black stripes at 45° angle

        // For now, we can use a simple red border or background
        // to indicate overflow
    }
}
```

### Phase 2: Clip Behavior

```rust
// Add to RenderFlex
pub struct RenderFlex {
    pub clip_behavior: Clip,
    // ...
}

impl MultiRender for RenderFlex {
    fn paint(...) -> BoxedLayer {
        let mut layer = /* paint children */;

        // Apply clipping if needed
        match self.clip_behavior {
            Clip::None => layer,  // No clipping
            Clip::HardEdge => apply_clip_rect(layer, self.size, false),
            Clip::AntiAlias => apply_clip_rect(layer, self.size, true),
            Clip::AntiAliasWithSaveLayer => apply_clip_with_save_layer(layer, self.size),
        }
    }
}
```

### Phase 3: ScrollView Support (Future)

When implementing ScrollView widgets, they will:
1. Allow unlimited size to children
2. Provide viewport clipping
3. Handle scroll gestures

## Recommendations

### For Current Implementation

1. **Add debug overflow indicator immediately**
   - This will help developers catch layout issues early
   - Zero runtime cost in release builds
   - Matches Flutter behavior

2. **Add Clip enum and clip_behavior field**
   - Simple addition to RenderFlex
   - Gives developers control
   - Minimal performance impact (only when clipping is enabled)

3. **Document the solutions**
   - Update examples to show Flexible/Expanded usage
   - Add Wrap examples for responsive layouts
   - Document when to use each approach

### For Developers Using Flui

**When you see overflow, you have options:**

1. **Use Flexible/Expanded** - Most common solution
   ```rust
   Row::builder()
       .children(vec![
           Expanded::builder()
               .child(Text::builder().data("Grows").build())
               .build()
               .into(),
       ])
       .build()
   ```

2. **Use Wrap** - For multi-line layouts
   ```rust
   Wrap::builder()
       .children(vec![/* widgets wrap to new line */])
       .build()
   ```

3. **Reduce content** - Sometimes simplest!
   ```rust
   Text::builder()
       .data("Shorter text")  // Or use text_overflow: ellipsis
       .build()
   ```

4. **Enable clipping** - Hide overflow (use sparingly)
   ```rust
   Row::builder()
       .clip_behavior(Clip::HardEdge)
       .children(vec![...])
       .build()
   ```

## Comparison with Flutter

| Feature | Flutter | Flui (Proposed) | Status |
|---------|---------|-----------------|--------|
| Debug overflow indicator | ✅ Yellow/black stripes | ✅ Will implement | TODO |
| Console warning | ✅ Yes | ✅ Yes | TODO |
| Clip behavior control | ✅ `clipBehavior` | ✅ `clip_behavior` | TODO |
| Flexible/Expanded | ✅ Yes | ✅ Yes | ✅ Done |
| Wrap widget | ✅ Yes | ✅ Yes | ✅ Done |
| ScrollView | ✅ Yes | 🚧 Future | Planned |

## Example: Fixing Your Profile Card

Your profile card overflow can be fixed with Flexible:

```rust
// Before: Buttons might overflow on small screens
Row::builder()
    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
    .children(vec![
        Button::builder().text("Follow").build().into(),
        Button::builder().text("Message").build().into(),
    ])
    .build()

// After: Buttons adapt to available space
Row::builder()
    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
    .children(vec![
        Flexible::builder()
            .flex(1)
            .child(Button::builder().text("Follow").build())
            .build()
            .into(),
        SizedBox::builder().width(8.0).build().into(),
        Flexible::builder()
            .flex(1)
            .child(Button::builder().text("Message").build())
            .build()
            .into(),
    ])
    .build()
```

## Summary

**The framework (Flui core) should:**
1. ✅ Warn developers in debug mode (visual + console)
2. ✅ Provide clip_behavior control
3. ✅ Offer tools (Flexible, Wrap, ScrollView)

**The developer should:**
1. ✅ Choose the right layout approach
2. ✅ Use Flexible/Expanded for adaptive sizing
3. ✅ Use Wrap for multi-line layouts
4. ✅ Use ScrollView for scrollable content
5. ⚠️ Use clipping only as last resort

This matches Flutter's philosophy: **Make problems visible in development, give developers tools to solve them properly.**
