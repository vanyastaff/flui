# FLUI Painting Architecture

**Version:** 0.1.0
**Date:** 2025-01-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document describes the architecture of **flui_painting** crate, which provides the **high-level painting API layer** between RenderObjects and the GPU engine. It acts as Flutter's Canvas API equivalent - recording drawing commands that will be executed by the engine.

**Key Responsibilities:**
1. **Canvas API** - Flutter-compatible drawing interface (drawRect, drawPath, drawText)
2. **DisplayList** - Record drawing commands for later execution
3. **Paint/Path** - High-level styling and geometry primitives
4. **Abstraction Layer** - Isolate rendering code from GPU implementation details

**Architecture Pattern:** **Command Pattern** - Record commands now, execute later by engine

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Canvas API](#canvas-api)
3. [DisplayList](#displaylist)
4. [Paint and Path](#paint-and-path)
5. [Integration Flow](#integration-flow)
6. [Implementation Plan](#implementation-plan)

---

## Architecture Overview

### Position in the Stack

```text
┌─────────────────────────────────────────────────────────────┐
│                   flui_rendering                            │
│              (RenderObject implementations)                  │
│                                                              │
│  impl RenderBox {                                           │
│    fn paint(&self) -> BoxedLayer {                         │
│      let mut canvas = Canvas::new();                       │
│      canvas.draw_rect(...);  // ← Uses flui_painting      │
│      canvas.finish()         // → Returns Layer           │
│    }                                                        │
│  }                                                          │
└──────────────────────┬──────────────────────────────────────┘
                       │ uses
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                   flui_painting                             │
│            (High-level painting API)                         │
│                                                              │
│  Canvas {                                                   │
│    drawRect(rect, paint)                                    │
│    drawPath(path, paint)                                    │
│    drawText(text, offset, style)                           │
│  }                                                          │
│    ↓ records to                                            │
│  DisplayList {                                              │
│    commands: Vec<DrawCommand>                              │
│  }                                                          │
└──────────────────────┬──────────────────────────────────────┘
                       │ executed by
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                    flui_engine                              │
│          (Low-level GPU rendering)                           │
│                                                              │
│  PictureLayer {                                             │
│    display_list: DisplayList                               │
│  }                                                          │
│    ↓                                                        │
│  WgpuPainter {                                              │
│    rect(rect, paint)    // GPU impl with wgpu             │
│    path(path, paint)    // Tessellate with Lyon           │
│    text(text, style)    // Render with Glyphon            │
│  }                                                          │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Principles

1. **Abstraction** - RenderObjects don't know about GPU/wgpu
2. **Recording** - Commands recorded now, executed later
3. **Flutter Compatible** - Same API as Flutter's Canvas
4. **Zero GPU Logic** - flui_painting has NO wgpu/lyon/glyphon code

---

## Canvas API

### Canvas (Main Drawing Interface)

```rust
// crates/flui_painting/src/canvas.rs

use crate::{DisplayList, DrawCommand, Paint, Path};
use flui_types::{Color, Matrix4, Offset, Point, RRect, Rect, Size, TextStyle};

/// High-level drawing canvas (Flutter-compatible API)
///
/// Canvas records drawing commands into a DisplayList without
/// performing any actual rendering. Rendering happens later
/// in flui_engine via WgpuPainter.
pub struct Canvas {
    /// Commands being recorded
    display_list: DisplayList,

    /// Current transform matrix
    transform: Matrix4,

    /// Current clip bounds
    clip_stack: Vec<ClipOp>,

    /// Save/restore stack
    save_stack: Vec<CanvasState>,
}

impl Canvas {
    /// Creates a new canvas
    pub fn new() -> Self {
        Self {
            display_list: DisplayList::new(),
            transform: Matrix4::identity(),
            clip_stack: Vec::new(),
            save_stack: Vec::new(),
        }
    }

    // ===== Transform Operations =====

    /// Translates the coordinate system
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.transform = self.transform.translate(dx, dy, 0.0);
    }

    /// Scales the coordinate system
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.transform = self.transform.scale(sx, sy, 1.0);
    }

    /// Rotates the coordinate system (radians)
    pub fn rotate(&mut self, radians: f32) {
        self.transform = self.transform.rotate_z(radians);
    }

    // ===== Save/Restore =====

    /// Saves the current state (transform, clip)
    pub fn save(&mut self) {
        self.save_stack.push(CanvasState {
            transform: self.transform,
            clip_depth: self.clip_stack.len(),
        });
    }

    /// Restores the most recently saved state
    pub fn restore(&mut self) {
        if let Some(state) = self.save_stack.pop() {
            self.transform = state.transform;
            self.clip_stack.truncate(state.clip_depth);
        }
    }

    // ===== Clipping =====

    /// Clips to a rectangle
    pub fn clip_rect(&mut self, rect: Rect) {
        self.clip_stack.push(ClipOp::Rect(rect));
        self.display_list.push(DrawCommand::ClipRect {
            rect,
            transform: self.transform,
        });
    }

    /// Clips to a rounded rectangle
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_stack.push(ClipOp::RRect(rrect));
        self.display_list.push(DrawCommand::ClipRRect {
            rrect,
            transform: self.transform,
        });
    }

    /// Clips to a path
    pub fn clip_path(&mut self, path: &Path) {
        self.clip_stack.push(ClipOp::Path(path.clone()));
        self.display_list.push(DrawCommand::ClipPath {
            path: path.clone(),
            transform: self.transform,
        });
    }

    // ===== Drawing Primitives =====

    /// Draws a line
    pub fn draw_line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawLine {
            p1,
            p2,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a rectangle
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawRect {
            rect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a rounded rectangle
    pub fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawRRect {
            rrect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a circle
    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawCircle {
            center,
            radius,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an oval
    pub fn draw_oval(&mut self, rect: Rect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawOval {
            rect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a path
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawPath {
            path: path.clone(),
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws text
    pub fn draw_text(&mut self, text: &str, offset: Offset, style: &TextStyle, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawText {
            text: text.to_string(),
            offset,
            style: style.clone(),
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an image
    pub fn draw_image(&mut self, image: ImageHandle, dst: Rect, paint: Option<&Paint>) {
        self.display_list.push(DrawCommand::DrawImage {
            image,
            dst,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws a shadow
    pub fn draw_shadow(&mut self, path: &Path, color: Color, elevation: f32) {
        self.display_list.push(DrawCommand::DrawShadow {
            path: path.clone(),
            color,
            elevation,
            transform: self.transform,
        });
    }

    // ===== Finalization =====

    /// Finishes recording and returns the display list
    pub fn finish(self) -> DisplayList {
        self.display_list
    }
}

#[derive(Debug, Clone)]
struct CanvasState {
    transform: Matrix4,
    clip_depth: usize,
}

#[derive(Debug, Clone)]
enum ClipOp {
    Rect(Rect),
    RRect(RRect),
    Path(Path),
}

use flui_types::painting::ImageHandle;
```

**Key Point:** Canvas только **записывает** команды, НЕ выполняет рендеринг!

---

## DisplayList

### DisplayList (Command Recording)

```rust
// crates/flui_painting/src/display_list.rs

use crate::{Paint, Path};
use flui_types::{Color, Matrix4, Offset, Point, RRect, Rect, TextStyle};
use flui_types::painting::ImageHandle;

/// A recorded sequence of drawing commands
///
/// DisplayList is immutable after recording and can be
/// replayed multiple times by the engine.
#[derive(Debug, Clone)]
pub struct DisplayList {
    commands: Vec<DrawCommand>,
    bounds: Rect,
}

impl DisplayList {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
        }
    }

    pub(crate) fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }

    /// Returns an iterator over commands
    pub fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands.iter()
    }

    /// Returns the bounds
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Returns the number of commands
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// A single drawing command
#[derive(Debug, Clone)]
pub enum DrawCommand {
    // Clipping
    ClipRect { rect: Rect, transform: Matrix4 },
    ClipRRect { rrect: RRect, transform: Matrix4 },
    ClipPath { path: Path, transform: Matrix4 },

    // Primitives
    DrawLine { p1: Point, p2: Point, paint: Paint, transform: Matrix4 },
    DrawRect { rect: Rect, paint: Paint, transform: Matrix4 },
    DrawRRect { rrect: RRect, paint: Paint, transform: Matrix4 },
    DrawCircle { center: Point, radius: f32, paint: Paint, transform: Matrix4 },
    DrawOval { rect: Rect, paint: Paint, transform: Matrix4 },
    DrawPath { path: Path, paint: Paint, transform: Matrix4 },

    // Text
    DrawText {
        text: String,
        offset: Offset,
        style: TextStyle,
        paint: Paint,
        transform: Matrix4,
    },

    // Image
    DrawImage {
        image: ImageHandle,
        dst: Rect,
        paint: Option<Paint>,
        transform: Matrix4,
    },

    // Effects
    DrawShadow {
        path: Path,
        color: Color,
        elevation: f32,
        transform: Matrix4,
    },
}
```

---

## Paint and Path

### Paint (Drawing Style)

```rust
// crates/flui_painting/src/paint.rs

use flui_types::{Color, Color32};

/// Describes how to draw on a canvas
#[derive(Debug, Clone)]
pub struct Paint {
    /// Paint style (fill or stroke)
    pub style: PaintStyle,

    /// Color
    pub color: Color,

    /// Shader (gradient)
    pub shader: Option<Shader>,

    /// Stroke width
    pub stroke_width: f32,

    /// Stroke cap
    pub stroke_cap: StrokeCap,

    /// Stroke join
    pub stroke_join: StrokeJoin,

    /// Blend mode
    pub blend_mode: BlendMode,

    /// Anti-aliasing
    pub anti_alias: bool,
}

impl Paint {
    pub fn fill(color: Color) -> Self {
        Self {
            style: PaintStyle::Fill,
            color,
            shader: None,
            stroke_width: 0.0,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            blend_mode: BlendMode::SrcOver,
            anti_alias: true,
        }
    }

    pub fn stroke(color: Color, width: f32) -> Self {
        Self {
            style: PaintStyle::Stroke,
            color,
            shader: None,
            stroke_width: width,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            blend_mode: BlendMode::SrcOver,
            anti_alias: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintStyle {
    Fill,
    Stroke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    SrcOver,
    Plus,
    Multiply,
    // ... more modes
}

#[derive(Debug, Clone)]
pub enum Shader {
    LinearGradient {
        start: Point,
        end: Point,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
    },
    RadialGradient {
        center: Point,
        radius: f32,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
    },
}

use flui_types::Point;
```

### Path (Vector Geometry)

```rust
// crates/flui_painting/src/path.rs

use flui_types::{Offset, Point, Rect};

/// A complex 2D path
///
/// Path is a high-level geometry description.
/// Actual tessellation happens in flui_engine via Lyon.
#[derive(Debug, Clone)]
pub struct Path {
    /// Path commands (high-level)
    commands: Vec<PathCommand>,

    /// Cached bounds
    bounds: Option<Rect>,
}

impl Path {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: None,
        }
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(PathCommand::MoveTo(Point::new(x, y)));
        self.bounds = None;
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(PathCommand::LineTo(Point::new(x, y)));
        self.bounds = None;
    }

    pub fn quad_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.commands.push(PathCommand::QuadTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
        ));
        self.bounds = None;
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) {
        self.commands.push(PathCommand::CubicTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
            Point::new(x3, y3),
        ));
        self.bounds = None;
    }

    pub fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }

    /// Convenience: add rectangle
    pub fn add_rect(&mut self, rect: Rect) {
        self.move_to(rect.left(), rect.top());
        self.line_to(rect.right(), rect.top());
        self.line_to(rect.right(), rect.bottom());
        self.line_to(rect.left(), rect.bottom());
        self.close();
    }

    /// Returns the path commands (for engine to tessellate)
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    pub fn bounds(&self) -> Rect {
        // Calculate bounds from commands
        self.bounds.unwrap_or(Rect::ZERO)
    }
}

#[derive(Debug, Clone)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}
```

---

## Integration Flow

### RenderObject → Canvas → DisplayList → Layer

```rust
// In flui_rendering/src/objects/render_box.rs

use flui_painting::{Canvas, Paint};
use flui_engine::layer::PictureLayer;

impl RenderBox {
    fn paint(&self) -> BoxedLayer {
        // Create canvas
        let mut canvas = Canvas::new();

        // Draw using high-level API
        let paint = Paint::fill(self.color);
        canvas.draw_rect(self.bounds, &paint);

        // Finish recording
        let display_list = canvas.finish();

        // Create layer with display list
        let layer = PictureLayer::with_display_list(display_list);
        Box::new(layer)
    }
}
```

### Layer → WgpuPainter (in flui_engine)

```rust
// In flui_engine/src/layer/picture.rs

use flui_painting::{DisplayList, DrawCommand};
use crate::painter::WgpuPainter;

impl PictureLayer {
    fn composite(&self, painter: &mut WgpuPainter) {
        // Execute display list using GPU painter
        for cmd in self.display_list.commands() {
            match cmd {
                DrawCommand::DrawRect { rect, paint, transform } => {
                    painter.set_transform(*transform);
                    painter.rect(*rect, paint); // ← GPU rendering
                }
                DrawCommand::DrawPath { path, paint, transform } => {
                    painter.set_transform(*transform);
                    painter.path(path, paint); // ← Tessellate + GPU
                }
                DrawCommand::DrawText { text, offset, style, paint, transform } => {
                    painter.set_transform(*transform);
                    painter.text(text, *offset, style, paint); // ← Glyphon
                }
                // ... other commands
            }
        }
    }
}
```

---

## Implementation Plan

### Phase 1: Core Canvas API (Week 1, ~800 LOC)
- Canvas struct with transform/clip stack
- Basic drawing methods (rect, circle, line)
- Save/restore implementation

### Phase 2: DisplayList (Week 2, ~400 LOC)
- DrawCommand enum
- DisplayList recording
- Command iteration API

### Phase 3: Paint and Path (Week 3, ~600 LOC)
- Paint struct with styles
- Path struct with commands
- Shader types (gradient)

### Phase 4: Integration (Week 4, ~400 LOC)
- Canvas → PictureLayer integration
- DisplayList → WgpuPainter execution
- Testing with RenderObjects

**Total Estimated LOC: ~2,200**

---

## Summary

**flui_painting** provides the **high-level Canvas API** as a bridge between rendering and engine:

- ✅ Flutter-compatible Canvas API (drawRect, drawPath, drawText)
- ✅ DisplayList command recording (Command Pattern)
- ✅ Paint/Path high-level primitives
- ✅ NO GPU code - pure abstraction layer
- ✅ Executed by flui_engine/WgpuPainter

**Clear Separation:**
- **flui_rendering** creates Canvases
- **flui_painting** records DisplayLists
- **flui_engine** executes with GPU

This architecture ensures RenderObjects remain GPU-agnostic and testable!
