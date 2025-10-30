# Overflow Indicator Implementation Guide

## Overview

This guide shows how to implement Flutter-style overflow indicators in Flui's RenderFlex.

## Visual Design

Flutter uses a distinctive **yellow and black diagonal stripe pattern** that looks like warning tape:

```
┌─────────────────────────┐
│                         │
│   Normal Content        │
│                         │
│                    ╱╱╱╱╱│ ← Yellow/Black stripes
│                    ╱╱╱╱╱│   indicating overflow
└─────────────────────────┘
```

## Implementation Strategy

### Phase 1: Simple Red Border (Quick Win)

Start with a simple red border to indicate overflow - easy to implement and clearly visible.

```rust
// In crates/flui_rendering/src/objects/layout/flex.rs

#[derive(Debug)]
pub struct RenderFlex {
    pub direction: Axis,
    pub main_axis_alignment: MainAxisAlignment,
    pub main_axis_size: MainAxisSize,
    pub cross_axis_alignment: CrossAxisAlignment,
    pub text_baseline: TextBaseline,

    // Cache for paint
    child_offsets: Vec<Offset>,

    // NEW: Track overflow for debug rendering
    #[cfg(debug_assertions)]
    overflow_pixels: f32,

    #[cfg(debug_assertions)]
    container_size: Size,
}

impl MultiRender for RenderFlex {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        // ... existing layout code ...

        // Calculate final size
        let size = match direction {
            Axis::Horizontal => {
                let width = if main_axis_size.is_max() {
                    constraints.max_width
                } else {
                    total_main_size.min(constraints.max_width)
                };
                Size::new(
                    width,
                    max_cross_size.clamp(constraints.min_height, constraints.max_height),
                )
            }
            Axis::Vertical => {
                let height = if main_axis_size.is_max() {
                    constraints.max_height
                } else {
                    total_main_size.min(constraints.max_height)
                };
                Size::new(
                    max_cross_size.clamp(constraints.min_width, constraints.max_width),
                    height,
                )
            }
        };

        // NEW: Track overflow in debug mode
        #[cfg(debug_assertions)]
        {
            let max_size = match self.direction {
                Axis::Horizontal => constraints.max_width,
                Axis::Vertical => constraints.max_height,
            };

            self.overflow_pixels = (total_main_size - max_size).max(0.0);
            self.container_size = size;

            if self.overflow_pixels > 0.0 {
                eprintln!(
                    "⚠️  RenderFlex overflow detected!\n\
                     Direction: {:?}\n\
                     Content size: {:.1}px\n\
                     Container size: {:.1}px\n\
                     Overflow: {:.1}px\n\
                     Location: (add stack trace if possible)",
                    self.direction,
                    total_main_size,
                    max_size,
                    self.overflow_pixels
                );
            }
        }

        size
    }

    fn paint(
        &self,
        tree: &ElementTree,
        child_ids: &[ElementId],
        offset: Offset,
    ) -> BoxedLayer {
        // Paint children normally
        let mut container = pool::container_layer();

        for (i, &child_id) in child_ids.iter().enumerate() {
            let child_offset = self.child_offsets.get(i)
                .copied()
                .unwrap_or(Offset::ZERO);
            let layer = tree.paint_child(child_id, offset + child_offset);
            container.add_child(layer);
        }

        // NEW: In debug mode, add overflow indicator
        #[cfg(debug_assertions)]
        if self.overflow_pixels > 0.0 {
            return self.paint_with_overflow_indicator(container, offset);
        }

        Box::new(container)
    }
}

#[cfg(debug_assertions)]
impl RenderFlex {
    /// Paint overflow indicator (red border version)
    fn paint_with_overflow_indicator(
        &self,
        content_layer: pool::PooledContainerLayer,
        offset: Offset,
    ) -> BoxedLayer {
        use flui_engine::layer::pool;
        use flui_engine::layer::picture::DrawCommand;
        use flui_engine::painter::Paint;
        use flui_types::Rect;
        use flui_types::painting::PaintingStyle;

        // Create a new container with content + indicator
        let mut container = pool::container_layer();

        // Add original content
        container.add_child(Box::new(content_layer));

        // Create indicator layer
        let mut indicator = pool::picture_layer();

        // Red border paint
        let border_paint = Paint {
            color: Color::rgb(255, 0, 0),  // Red
            style: PaintingStyle::Stroke,
            stroke_width: 4.0,
            anti_alias: false,  // Sharp edges for visibility
            ..Default::default()
        };

        // Draw red border around the container
        let rect = Rect::from_origin_size(
            offset.to_point(),
            self.container_size
        );

        indicator.add_command(DrawCommand::Rect {
            rect,
            paint: border_paint,
        });

        // Add indicator on top
        container.add_child(Box::new(indicator));

        Box::new(container)
    }
}
```

### Phase 2: Diagonal Stripe Pattern (Flutter Style)

Implement the classic yellow/black diagonal stripes:

```rust
#[cfg(debug_assertions)]
impl RenderFlex {
    /// Paint overflow indicator (Flutter-style stripes)
    fn paint_with_overflow_indicator(
        &self,
        content_layer: pool::PooledContainerLayer,
        offset: Offset,
    ) -> BoxedLayer {
        use flui_engine::layer::pool;
        use flui_engine::layer::picture::DrawCommand;
        use flui_engine::painter::Paint;
        use flui_types::{Rect, Point};
        use flui_types::painting::PaintingStyle;

        let mut container = pool::container_layer();
        container.add_child(Box::new(content_layer));

        // Create overflow indicator region
        let indicator_thickness = 10.0; // Width of stripe band

        let overflow_rect = match self.direction {
            Axis::Horizontal => {
                // Overflow on right side
                Rect::from_ltrb(
                    offset.x + self.container_size.width - indicator_thickness,
                    offset.y,
                    offset.x + self.container_size.width,
                    offset.y + self.container_size.height,
                )
            }
            Axis::Vertical => {
                // Overflow on bottom side
                Rect::from_ltrb(
                    offset.x,
                    offset.y + self.container_size.height - indicator_thickness,
                    offset.x + self.container_size.width,
                    offset.y + self.container_size.height,
                )
            }
        };

        let mut indicator = pool::picture_layer();

        // Paint diagonal stripes
        self.paint_diagonal_stripes(&mut indicator, overflow_rect);

        container.add_child(Box::new(indicator));
        Box::new(container)
    }

    /// Paint diagonal yellow/black stripes (warning tape pattern)
    fn paint_diagonal_stripes(
        &self,
        picture: &mut pool::PooledPictureLayer,
        rect: Rect,
    ) {
        use flui_engine::layer::picture::DrawCommand;
        use flui_engine::painter::Paint;
        use flui_types::Point;
        use flui_types::painting::PaintingStyle;
        use std::sync::Arc;

        const STRIPE_WIDTH: f32 = 8.0;
        const YELLOW: Color = Color::rgb(255, 255, 0);
        const BLACK: Color = Color::rgb(0, 0, 0);

        let yellow_paint = Paint::fill(YELLOW);
        let black_paint = Paint::fill(BLACK);

        // First, fill entire area with yellow
        picture.add_command(DrawCommand::Rect {
            rect,
            paint: yellow_paint.clone(),
        });

        // Then draw black diagonal stripes
        let diagonal_distance = rect.width + rect.height;
        let num_stripes = (diagonal_distance / STRIPE_WIDTH).ceil() as i32;

        for i in 0..num_stripes {
            if i % 2 == 0 {
                continue; // Skip even stripes (leave yellow)
            }

            // Calculate stripe position
            let offset = i as f32 * STRIPE_WIDTH;

            // Create diagonal stripe as polygon
            let points = if self.direction == Axis::Horizontal {
                // Vertical stripes for horizontal overflow
                vec![
                    Point::new(rect.left + offset, rect.top),
                    Point::new(rect.left + offset + STRIPE_WIDTH, rect.top),
                    Point::new(rect.left + offset + STRIPE_WIDTH, rect.bottom),
                    Point::new(rect.left + offset, rect.bottom),
                ]
            } else {
                // Horizontal stripes for vertical overflow
                vec![
                    Point::new(rect.left, rect.top + offset),
                    Point::new(rect.right, rect.top + offset),
                    Point::new(rect.right, rect.top + offset + STRIPE_WIDTH),
                    Point::new(rect.left, rect.top + offset + STRIPE_WIDTH),
                ]
            };

            picture.add_command(DrawCommand::Polygon {
                points: Arc::new(points),
                paint: black_paint.clone(),
            });
        }
    }
}
```

### Phase 3: Better Diagonal Stripes (45° angle)

For true diagonal stripes at 45° like Flutter:

```rust
#[cfg(debug_assertions)]
impl RenderFlex {
    /// Paint true 45° diagonal stripes
    fn paint_diagonal_stripes(
        &self,
        picture: &mut pool::PooledPictureLayer,
        rect: Rect,
    ) {
        use flui_engine::layer::picture::DrawCommand;
        use flui_engine::painter::Paint;
        use flui_types::Point;
        use std::sync::Arc;

        const STRIPE_WIDTH: f32 = 10.0;
        const YELLOW: Color = Color::rgb(255, 200, 0);
        const BLACK: Color = Color::rgb(0, 0, 0);

        // Fill background with yellow
        picture.add_command(DrawCommand::Rect {
            rect,
            paint: Paint::fill(YELLOW),
        });

        // Draw 45° diagonal black stripes
        let diagonal_distance = (rect.width.powi(2) + rect.height.powi(2)).sqrt();
        let num_stripes = (diagonal_distance / (STRIPE_WIDTH * 2.0)).ceil() as i32;

        for i in 0..num_stripes {
            // Calculate stripe position along diagonal
            let stripe_offset = i as f32 * STRIPE_WIDTH * 2.0;

            // Create 45° diagonal stripe
            // This is a parallelogram that crosses the rect at 45°
            let start_x = rect.left - rect.height + stripe_offset;
            let end_x = start_x + STRIPE_WIDTH;

            let points = vec![
                Point::new(start_x, rect.bottom),
                Point::new(start_x + rect.height, rect.top),
                Point::new(end_x + rect.height, rect.top),
                Point::new(end_x, rect.bottom),
            ];

            picture.add_command(DrawCommand::Polygon {
                points: Arc::new(points),
                paint: Paint::fill(BLACK),
            });
        }
    }
}
```

## Alternative: Use Path for Stripes

For more precise control:

```rust
fn paint_diagonal_stripes_with_path(
    &self,
    picture: &mut pool::PooledPictureLayer,
    rect: Rect,
) {
    use flui_types::painting::path::{Path, PathBuilder};

    const STRIPE_WIDTH: f32 = 10.0;

    // Background
    picture.add_command(DrawCommand::Rect {
        rect,
        paint: Paint::fill(Color::rgb(255, 200, 0)),
    });

    // Create path for all black stripes
    let mut path_builder = PathBuilder::new();

    let diagonal = (rect.width.powi(2) + rect.height.powi(2)).sqrt();
    let num_stripes = (diagonal / (STRIPE_WIDTH * 2.0)).ceil() as i32;

    for i in 0..num_stripes {
        let offset = i as f32 * STRIPE_WIDTH * 2.0;
        let x = rect.left - rect.height + offset;

        // Add stripe to path
        path_builder.move_to(x, rect.bottom);
        path_builder.line_to(x + rect.height, rect.top);
        path_builder.line_to(x + rect.height + STRIPE_WIDTH, rect.top);
        path_builder.line_to(x + STRIPE_WIDTH, rect.bottom);
        path_builder.close();
    }

    let path = path_builder.build();

    picture.add_command(DrawCommand::Path {
        path: Arc::new(path),
        paint: Paint::fill(Color::BLACK),
    });
}
```

## Testing

Create a test example to verify the indicator:

```rust
// examples/overflow_test.rs

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

#[derive(Debug, Clone)]
struct OverflowTestApp;

flui_core::impl_into_widget!(OverflowTestApp, stateless);

impl StatelessWidget for OverflowTestApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(20.0))
            .child(
                Column::builder()
                    .children(vec![
                        Text::builder()
                            .data("Overflow Test - Resize window to see indicators")
                            .size(20.0)
                            .build()
                            .into(),

                        SizedBox::builder().height(20.0).build().into(),

                        // This Row will overflow when window is small
                        Container::builder()
                            .width(300.0)  // Fixed width
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(240, 240, 240)),
                                ..Default::default()
                            })
                            .child(
                                Row::builder()
                                    .children(vec![
                                        // Total width > 300px = overflow!
                                        Text::builder().data("Item 1 ").build().into(),
                                        Text::builder().data("Item 2 ").build().into(),
                                        Text::builder().data("Item 3 ").build().into(),
                                        Text::builder().data("Item 4 ").build().into(),
                                        Text::builder().data("Item 5 ").build().into(),
                                        Button::builder().text("Button").build().into(),
                                    ])
                                    .build()
                            )
                            .build()
                            .into(),
                    ])
                    .build()
            )
            .build()
    }
}

fn main() -> Result<(), eframe::Error> {
    println!("⚠️  DEBUG MODE: Watch for overflow warnings in console");
    println!("    Yellow/black stripes will appear where overflow occurs");
    run_app(OverflowTestApp.into_widget())
}
```

## Summary

### Quick Implementation (Recommended Start)

1. **Add overflow tracking fields** to RenderFlex (debug only)
2. **Calculate overflow** in layout() method
3. **Print warning** to console
4. **Draw red border** in paint() method

This takes ~30 minutes and provides immediate value!

### Full Implementation

1. **Phase 1**: Red border (quick win)
2. **Phase 2**: Simple stripes (vertical/horizontal)
3. **Phase 3**: 45° diagonal stripes (Flutter-accurate)

### Benefits

- ✅ Makes overflow **immediately visible**
- ✅ Zero cost in release builds (cfg debug_assertions)
- ✅ Matches Flutter developer experience
- ✅ Helps developers fix layout issues early

### Next Steps

1. Implement Phase 1 (red border)
2. Test with overflow_test.rs
3. Refine to Phase 2/3 as needed
4. Add to other render objects (RenderStack, etc.)
