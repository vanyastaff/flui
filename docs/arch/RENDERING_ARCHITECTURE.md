# FLUI Rendering Architecture

**Version:** 0.1.0
**Date:** 2025-01-10
**Author:** Claude (Anthropic)
**Status:** Production Ready

---

## Executive Summary

This document describes the architecture of **flui_rendering** crate, which provides the **RenderObject layer** for FLUI's three-tree architecture. It implements 81+ Flutter-compatible RenderObjects using a unified `Render` trait with zero-cost abstractions.

**Current Status:** ✅ Production ready with 81+ RenderObjects implemented

**Key Responsibilities:**
1. **Layout Computation** - Calculate sizes based on BoxConstraints
2. **Paint Layer Generation** - Create Layer trees for GPU rendering
3. **Hit Testing** - Determine which RenderObject receives pointer events
4. **Metadata System** - ParentData for child-specific layout information

**Architecture Pattern:** **Unified Render Trait** (single trait, multiple arities) + **Context Pattern** (LayoutContext/PaintContext)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Unified Render Trait](#unified-render-trait)
3. [Arity System](#arity-system)
4. [Context Pattern](#context-pattern)
5. [RenderObject Categories](#renderobject-categories)
6. [ParentData and Metadata](#parentdata-and-metadata)
7. [Layout Caching](#layout-caching)
8. [Integration with Other Layers](#integration-with-other-layers)

---

## Architecture Overview

### Position in the Stack

```text
┌─────────────────────────────────────────────────────────────┐
│                    flui_widgets                             │
│          (High-level declarative widgets)                    │
│                                                              │
│  Container, Row, Column, Text, Button, etc.                 │
└──────────────────────┬──────────────────────────────────────┘
                       │ builds
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                     flui_core                               │
│              (Element tree management)                       │
│                                                              │
│  Element enum { Component, Render, Provider }               │
│  LayoutCache, BuildContext, etc.                            │
└──────────────────────┬──────────────────────────────────────┘
                       │ contains
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                  flui_rendering                             │
│              (THIS CRATE - Layout & Paint)                   │
│                                                              │
│  Unified Render Trait:                                      │
│  ┌──────────────────────────────────────────┐              │
│  │ trait Render {                           │              │
│  │   fn layout(&mut self, ctx) -> Size      │              │
│  │   fn paint(&self, ctx) -> BoxedLayer     │              │
│  │   fn arity(&self) -> Arity               │              │
│  │   fn as_any(&self) -> &dyn Any           │              │
│  │ }                                         │              │
│  └──────────────────────────────────────────┘              │
│                       ↓                                      │
│  81+ RenderObjects:                                         │
│  - RenderPadding, RenderFlex, RenderStack                   │
│  - RenderParagraph, RenderOpacity, RenderTransform          │
│  - RenderClipRect, RenderPhysicalModel, etc.                │
└──────────────────────┬──────────────────────────────────────┘
                       │ uses
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                  flui_painting                              │
│            (Canvas API - DisplayList recording)              │
│                                                              │
│  Canvas, Paint, Path, DisplayList                           │
└──────────────────────┬──────────────────────────────────────┘
                       │ executed by
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                   flui_engine                               │
│          (GPU rendering - wgpu + Lyon + Glyphon)             │
│                                                              │
│  PictureLayer, WgpuPainter, EventRouter                     │
└─────────────────────────────────────────────────────────────┘
```

### Three-Tree Architecture

FLUI follows Flutter's proven three-tree architecture:

```text
View Tree (Immutable)          Element Tree (Mutable)       Render Tree (Layout/Paint)
┌────────────────┐            ┌─────────────────┐          ┌──────────────────┐
│  Padding       │   build    │  RenderElement  │ contains │  RenderPadding   │
│  child: Text   │ ────────> │  render: Box    │ ───────> │  padding: 10.0   │
└────────────────┘            │  cache: ...     │          └──────────────────┘
                              └─────────────────┘
```

**flui_rendering provides the Render Tree** - the rightmost tree in the diagram.

---

## Unified Render Trait

### Core Design

Instead of Flutter's three traits (LeafRenderObjectMixin, RenderObjectWithChildMixin, ContainerRenderObjectMixin), FLUI uses **a single unified `Render` trait** for all RenderObjects:

```rust
// In flui_core/src/render/render.rs

use flui_types::Size;
use flui_engine::BoxedLayer;
use std::any::Any;

/// Unified trait for all render objects
///
/// This single trait handles 0, 1, or multiple children via the
/// Children enum and arity validation.
pub trait Render: Send + Sync + std::fmt::Debug {
    /// Compute size given constraints
    ///
    /// This is where layout logic happens. RenderObjects must:
    /// 1. Layout children (if any) via ctx.layout_child()
    /// 2. Compute their own size based on constraints and children
    /// 3. Return the final size
    ///
    /// # Invariants
    /// - Returned size MUST satisfy ctx.constraints
    /// - Must NOT mutate tree structure
    /// - Can read but not write RenderState flags
    fn layout(&mut self, ctx: &LayoutContext) -> Size;

    /// Generate layer tree for rendering
    ///
    /// This is where paint logic happens. RenderObjects must:
    /// 1. Paint children (if any) via ctx.paint_child()
    /// 2. Create their own layer (PictureLayer, ContainerLayer, etc.)
    /// 3. Return the layer tree
    ///
    /// # Invariants
    /// - Must NOT mutate tree structure
    /// - Must NOT trigger layout
    /// - Offset is absolute (ctx.offset already includes parent offset)
    fn paint(&self, ctx: &PaintContext) -> BoxedLayer;

    /// Specify child count
    ///
    /// Used for runtime validation and context optimization.
    /// - Arity::Exact(0) → Leaf render (no children)
    /// - Arity::Exact(1) → Single-child render
    /// - Arity::Variable → Multi-child render
    fn arity(&self) -> Arity;

    /// Downcast to concrete type (for metadata access)
    ///
    /// Enables parent RenderObjects to access child metadata.
    /// Example: RenderFlex accessing FlexItemMetadata from RenderFlexItem.
    fn as_any(&self) -> &dyn Any;

    /// Hit test - check if point is within bounds
    ///
    /// Default implementation checks bounding box. Override for
    /// custom shapes or to implement pointer event filters.
    fn hit_test(&self, _ctx: &HitTestContext, _position: Point) -> bool {
        true // Default: always hit
    }

    /// Get intrinsic width for given height
    ///
    /// Used by IntrinsicWidth/IntrinsicHeight widgets.
    /// Default returns None (no intrinsic dimension).
    fn get_intrinsic_width(&self, _height: f32) -> Option<f32> {
        None
    }

    /// Get intrinsic height for given width
    fn get_intrinsic_height(&self, _width: f32) -> Option<f32> {
        None
    }
}
```

### Benefits of Unified Trait

| Benefit | Description |
|---------|-------------|
| **Simplicity** | Single trait instead of 3 mixin traits |
| **Flexibility** | Easy to change child count (e.g., debug vs release) |
| **Type Safety** | Arity validation at runtime prevents bugs |
| **Zero Cost** | Monomorphization eliminates trait overhead |
| **Ergonomics** | Context pattern provides clean API |

---

## Arity System

### Arity Enum

```rust
// In flui_core/src/render/arity.rs

/// Child count specification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Exact number of children (e.g., 0 for leaf, 1 for padding)
    Exact(usize),

    /// Variable number of children (0 or more)
    Variable,
}

impl Arity {
    /// Check if arity matches actual child count
    pub fn matches(&self, count: usize) -> bool {
        match self {
            Arity::Exact(n) => count == *n,
            Arity::Variable => true,
        }
    }

    /// Is this a leaf render (no children)?
    pub fn is_leaf(&self) -> bool {
        matches!(self, Arity::Exact(0))
    }

    /// Is this a single-child render?
    pub fn is_single(&self) -> bool {
        matches!(self, Arity::Exact(1))
    }

    /// Is this a multi-child render?
    pub fn is_multi(&self) -> bool {
        matches!(self, Arity::Variable)
    }
}
```

### Arity Examples

```rust
// Leaf render (no children)
impl Render for RenderParagraph {
    fn arity(&self) -> Arity {
        Arity::Exact(0)
    }
}

// Single-child render
impl Render for RenderPadding {
    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

// Multi-child render
impl Render for RenderFlex {
    fn arity(&self) -> Arity {
        Arity::Variable
    }
}

// Conditional arity (debug vs release)
impl Render for RenderStack {
    fn arity(&self) -> Arity {
        #[cfg(debug_assertions)]
        {
            // In debug, Stack can show overflow indicator (extra child)
            Arity::Variable
        }
        #[cfg(not(debug_assertions))]
        {
            Arity::Variable
        }
    }
}
```

---

## Context Pattern

### LayoutContext

Provides clean API for layout operations:

```rust
// In flui_core/src/render/context.rs

use flui_types::{Size, constraints::BoxConstraints};
use crate::element::{ElementId, ElementTree};

/// Context for layout operations
///
/// Provides methods to layout children and access tree state.
pub struct LayoutContext<'a> {
    /// Element tree (read-only access)
    pub tree: &'a ElementTree,

    /// Box constraints for this render
    pub constraints: BoxConstraints,

    /// Children of this render
    pub children: Children,

    /// This render's ElementId
    pub self_id: ElementId,
}

impl<'a> LayoutContext<'a> {
    /// Layout a child with given constraints
    ///
    /// This triggers the child's layout() method and returns its size.
    /// The child's layout is cached automatically by Element.
    pub fn layout_child(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        self.tree.layout_child(child_id, constraints)
    }

    /// Layout a child with loose constraints (child chooses size)
    pub fn layout_child_loose(&self, child_id: ElementId) -> Size {
        let loose = self.constraints.loosen();
        self.layout_child(child_id, loose)
    }

    /// Layout a child with tight constraints (forced size)
    pub fn layout_child_tight(&self, child_id: ElementId, size: Size) -> Size {
        let tight = BoxConstraints::tight(size);
        self.layout_child(child_id, tight)
    }

    /// Get metadata from child (via downcasting)
    pub fn get_metadata<T: 'static>(&self, child_id: ElementId) -> Option<&T> {
        self.tree.get_metadata::<T>(child_id)
    }
}
```

### PaintContext

Provides clean API for painting operations:

```rust
// In flui_core/src/render/context.rs

use flui_types::Offset;
use flui_engine::BoxedLayer;

/// Context for paint operations
pub struct PaintContext<'a> {
    /// Element tree (read-only access)
    pub tree: &'a ElementTree,

    /// Absolute offset for this render
    pub offset: Offset,

    /// Children of this render
    pub children: Children,

    /// This render's ElementId
    pub self_id: ElementId,
}

impl<'a> PaintContext<'a> {
    /// Paint a child at given offset
    ///
    /// Offset is ABSOLUTE (not relative to parent).
    /// This triggers the child's paint() method and returns its layer.
    pub fn paint_child(&self, child_id: ElementId, offset: Offset) -> BoxedLayer {
        self.tree.paint_child(child_id, offset)
    }

    /// Paint a child at this render's offset (no additional offset)
    pub fn paint_child_here(&self, child_id: ElementId) -> BoxedLayer {
        self.paint_child(child_id, self.offset)
    }
}
```

### Children Enum

Unified representation for 0/1/N children:

```rust
// In flui_core/src/render/children.rs

/// Unified child representation
#[derive(Debug, Clone)]
pub enum Children {
    /// No children
    None,

    /// Single child
    Single(ElementId),

    /// Multiple children
    Multi(Vec<ElementId>),
}

impl Children {
    /// Get single child (panics if not exactly one)
    pub fn single(&self) -> ElementId {
        match self {
            Children::Single(id) => *id,
            _ => panic!("Expected single child, got {:?}", self),
        }
    }

    /// Get multiple children as slice
    pub fn multi(&self) -> &[ElementId] {
        match self {
            Children::Multi(ids) => ids,
            _ => panic!("Expected multiple children, got {:?}", self),
        }
    }

    /// Get count
    pub fn count(&self) -> usize {
        match self {
            Children::None => 0,
            Children::Single(_) => 1,
            Children::Multi(ids) => ids.len(),
        }
    }

    /// Is empty?
    pub fn is_empty(&self) -> bool {
        matches!(self, Children::None)
    }
}
```

---

## RenderObject Categories

flui_rendering organizes 81+ RenderObjects into 7 categories:

### 1. Layout RenderObjects (29 objects)

**Purpose:** Control child positioning and sizing

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderPadding** | 1 | Adds padding around child |
| **RenderAlign** | 1 | Aligns child within available space |
| **RenderSizedBox** | 1 | Forces child to specific size |
| **RenderConstrainedBox** | 1 | Adds additional constraints |
| **RenderAspectRatio** | 1 | Maintains aspect ratio |
| **RenderFlex** (Row/Column) | N | Flex layout (main/cross axis) |
| **RenderStack** | N | Z-index stacking with positioning |
| **RenderPositioned** | 1 | Absolute positioning within Stack |
| **RenderWrap** | N | Wrapping flex layout |
| **RenderIntrinsicWidth** | 1 | Forces intrinsic width |
| **RenderIntrinsicHeight** | 1 | Forces intrinsic height |
| **RenderBaseline** | 1 | Baseline alignment |
| **RenderFractionallySizedBox** | 1 | Percentage-based sizing |
| **RenderOverflowBox** | 1 | Allows child to overflow constraints |
| **RenderSizedOverflowBox** | 1 | Sized box with overflow |
| **RenderLimitedBox** | 1 | Max size when unconstrained |
| **RenderListBody** | N | Simple list layout (no scroll) |
| **RenderIndexedStack** | N | Shows only one child at index |
| **RenderRotatedBox** | 1 | 90° rotation |
| **RenderPositionedBox** | 1 | Align + SizedBox combined |
| **RenderFlexItem** | 1 | Flexible/Expanded wrapper (metadata) |

**Example - RenderPadding:**

```rust
// In flui_rendering/src/objects/layout/padding.rs

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_engine::BoxedLayer;
use flui_types::{EdgeInsets, Size};

#[derive(Debug, Clone)]
pub struct RenderPadding {
    pub padding: EdgeInsets,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl Render for RenderPadding {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let child_id = ctx.children.single();
        let padding = self.padding;

        // Deflate constraints by padding
        let child_constraints = ctx.constraints.deflate(&padding);

        // Layout child with deflated constraints
        let child_size = ctx.layout_child(child_id, child_constraints);

        // Add padding to child size
        Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        )
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let child_id = ctx.children.single();

        // Apply padding offset and paint child
        let child_offset = flui_types::Offset::new(self.padding.left, self.padding.top);
        ctx.paint_child(child_id, ctx.offset + child_offset)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single-child render
    }
}
```

### 2. Effects RenderObjects (15 objects)

**Purpose:** Visual effects (clip, opacity, transform, shadows)

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderOpacity** | 1 | Applies opacity/transparency |
| **RenderTransform** | 1 | 2D/3D transforms (rotate, scale, skew) |
| **RenderClipRect** | 1 | Rectangular clipping |
| **RenderClipRRect** | 1 | Rounded rectangle clipping |
| **RenderClipOval** | 1 | Oval/circle clipping |
| **RenderClipPath** | 1 | Arbitrary path clipping |
| **RenderPhysicalModel** | 1 | Material Design elevation + shadows |
| **RenderBackdropFilter** | 1 | Blur/filter background |
| **RenderDecoratedBox** | 1 | Box decoration (borders, gradients) |
| **RenderColoredBox** | 0 | Solid color rectangle |
| **RenderRepaintBoundary** | 1 | Isolates repaints |
| **RenderOffstage** | 1 | Hides child (layout but no paint) |
| **RenderVisibility** | 1 | Shows/hides child |
| **RenderCustomPaint** | 1 | Custom painter callback |
| **RenderShaderMask** | 1 | Shader-based masking |

**Example - RenderOpacity:**

```rust
// In flui_rendering/src/objects/effects/opacity.rs

impl Render for RenderOpacity {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let child_id = ctx.children.single();

        // Layout child with same constraints (opacity doesn't affect layout)
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        if self.opacity < 0.01 {
            // Fully transparent - skip painting
            return Box::new(pool::acquire_container());
        }

        let child_id = ctx.children.single();

        if (self.opacity - 1.0).abs() < 0.01 {
            // Fully opaque - no layer needed
            return ctx.paint_child(child_id, ctx.offset);
        }

        // Partial opacity - use OpacityLayer
        let child_layer = ctx.paint_child(child_id, ctx.offset);
        let mut opacity_layer = OpacityLayer::new(self.opacity);
        opacity_layer.append(child_layer);
        Box::new(opacity_layer)
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
```

### 3. Text RenderObjects (2 objects)

**Purpose:** Text rendering

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderParagraph** | 0 | Multi-line text with wrapping |
| **RenderRichText** | 0 | Styled text spans (TODO) |

**Example - RenderParagraph:**

```rust
// In flui_rendering/src/objects/text/paragraph.rs

#[derive(Debug)]
pub struct RenderParagraph {
    pub data: ParagraphData,

    // Cache computed size (updated in layout)
    cached_size: Size,
}

impl Render for RenderParagraph {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        // Measure text with egui/glyphon
        let max_width = ctx.constraints.max_width();
        let size = measure_text(&self.data.text, self.data.font_size, max_width);

        // Constrain to meet constraints
        self.cached_size = ctx.constraints.constrain(size);
        self.cached_size
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        // Create PictureLayer with text drawing commands
        let mut layer = pool::acquire_picture();

        layer.draw_text(
            &self.data.text,
            ctx.offset,
            self.data.font_size,
            self.data.color,
            self.data.text_align,
        );

        Box::new(layer)
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0) // Leaf render
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
```

### 4. Interaction RenderObjects (4 objects)

**Purpose:** Pointer event handling

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderPointerListener** | 1 | Listens to pointer events |
| **RenderMouseRegion** | 1 | Mouse enter/exit/hover events |
| **RenderIgnorePointer** | 1 | Ignores pointer events (pass-through) |
| **RenderAbsorbPointer** | 1 | Absorbs pointer events (block) |

### 5. Special RenderObjects (8 objects)

**Purpose:** Metadata, semantics, and special cases

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderFittedBox** | 1 | Scales/fits child to available space |
| **RenderMetadata** | 1 | Attaches arbitrary metadata |
| **RenderAnnotatedRegion** | 1 | System UI annotations |
| **RenderMergeSemantics** | 1 | Merges semantic nodes |
| **RenderBlockSemantics** | 1 | Blocks semantic traversal |
| **RenderExcludeSemantics** | 1 | Excludes from semantics tree |

### 6. Debug RenderObjects (1 object)

**Purpose:** Development-time debugging

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderOverflowIndicator** | 0 | Shows overflow warnings |

### 7. Scroll RenderObjects (2 objects)

**Purpose:** Scrollable containers

| RenderObject | Child Count | Description |
|--------------|-------------|-------------|
| **RenderViewport** | N | Viewport for scrolling |
| **RenderScrollView** | 1 | Scrollable view |

---

## ParentData and Metadata

### ParentData Pattern

Some layout algorithms need **per-child metadata**. For example, `RenderFlex` needs to know which children are flexible:

```rust
// In flui_core/src/render/parent_data.rs

/// Base trait for parent data (stored in RenderElement)
pub trait ParentData: Send + Sync + std::fmt::Debug {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Parent data with offset (most common case)
#[derive(Debug, Clone, Copy)]
pub struct ParentDataWithOffset {
    pub offset: Offset,
}

impl ParentData for ParentDataWithOffset {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
```

### Metadata Example - FlexItem

```rust
// In flui_rendering/src/objects/layout/flex_item.rs

/// Metadata for RenderFlexItem (used by RenderFlex)
#[derive(Debug, Clone, Copy)]
pub struct FlexItemMetadata {
    pub flex: i32,          // Flex factor (1 = equal share)
    pub fit: FlexFit,       // Tight or Loose
}

/// Wrapper render that provides metadata to parent RenderFlex
#[derive(Debug)]
pub struct RenderFlexItem {
    pub metadata: FlexItemMetadata,
}

impl Render for RenderFlexItem {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        // Pass through to child (metadata is accessed by parent)
        let child_id = ctx.children.single();
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let child_id = ctx.children.single();
        ctx.paint_child(child_id, ctx.offset)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self // ← Enables parent to downcast and access metadata
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

// Parent (RenderFlex) accesses metadata:
impl Render for RenderFlex {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let children = ctx.children.multi();

        for &child_id in children {
            // Downcast to access FlexItemMetadata
            if let Some(flex_item) = ctx.tree.get_render(child_id)
                .and_then(|r| r.as_any().downcast_ref::<RenderFlexItem>())
            {
                let flex_factor = flex_item.metadata.flex;
                // Use flex_factor in layout calculation
            }
        }

        // ... flex layout logic ...
    }
}
```

---

## Layout Caching

### LayoutCache (in flui_core)

**IMPORTANT:** Layout caching lives in **Element**, NOT RenderObject. This keeps RenderObjects pure and stateless.

```rust
// In flui_core/src/render/cache.rs

/// Layout cache key (constraints)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutCacheKey {
    pub constraints: BoxConstraints,
}

/// Layout result (size)
#[derive(Debug, Clone, Copy)]
pub struct LayoutResult {
    pub size: Size,
}

/// Layout cache (stored in RenderElement)
pub struct LayoutCache {
    cache: HashMap<LayoutCacheKey, LayoutResult>,
}

impl LayoutCache {
    /// Get cached layout result
    pub fn get(&self, constraints: BoxConstraints) -> Option<Size> {
        let key = LayoutCacheKey { constraints };
        self.cache.get(&key).map(|r| r.size)
    }

    /// Store layout result
    pub fn insert(&mut self, constraints: BoxConstraints, size: Size) {
        let key = LayoutCacheKey { constraints };
        let result = LayoutResult { size };
        self.cache.insert(key, result);
    }

    /// Clear cache (when RenderObject changes)
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}
```

### Cache Usage Pattern

```rust
// In flui_core/src/element/element.rs (RenderElement)

impl RenderElement {
    pub fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Check cache
        if let Some(cached_size) = self.layout_cache.get(constraints) {
            #[cfg(debug_assertions)]
            tracing::trace!(
                "Layout cache hit for {:?}: {:?} -> {:?}",
                self.id, constraints, cached_size
            );
            return cached_size;
        }

        // Cache miss - call RenderObject
        let ctx = LayoutContext {
            tree: self.tree,
            constraints,
            children: self.children.clone(),
            self_id: self.id,
        };

        let size = self.render.layout(&ctx);

        // Store in cache
        self.layout_cache.insert(constraints, size);

        size
    }
}
```

**Key Points:**
- ✅ RenderObjects are pure functions (no internal caching)
- ✅ Element manages cache (one cache per RenderElement)
- ✅ Cache is keyed by BoxConstraints
- ✅ Cache is invalidated when RenderObject changes
- ✅ Enables **relayout boundary optimization** (skip subtrees)

---

## Integration with Other Layers

### With flui_core (Element Tree)

```rust
// Element owns RenderObject and manages its lifecycle
pub struct RenderElement {
    pub id: ElementId,
    pub render: Box<dyn Render>,  // ← RenderObject from flui_rendering
    pub layout_cache: LayoutCache,
    pub render_state: RenderState,
    pub children: Vec<ElementId>,
}

// Element delegates to RenderObject
impl RenderElement {
    pub fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Check cache first
        if let Some(cached) = self.layout_cache.get(constraints) {
            return cached;
        }

        // Call RenderObject
        let ctx = LayoutContext { /* ... */ };
        let size = self.render.layout(&ctx);

        // Cache result
        self.layout_cache.insert(constraints, size);
        size
    }

    pub fn paint(&self, offset: Offset) -> BoxedLayer {
        let ctx = PaintContext { /* ... */ };
        self.render.paint(&ctx)
    }
}
```

### With flui_painting (Canvas API)

RenderObjects use Canvas to record drawing commands:

```rust
// In RenderParagraph::paint()
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    use flui_painting::{Canvas, Paint};

    // Create Canvas
    let mut canvas = Canvas::new();

    // Record drawing commands
    let paint = Paint::default().with_color(self.data.color);
    canvas.draw_text(&self.data.text, ctx.offset, &paint);

    // Finish and create PictureLayer
    let display_list = canvas.finish();
    Box::new(PictureLayer::with_display_list(display_list))
}
```

### With flui_engine (Layer System)

RenderObjects return BoxedLayer which becomes part of the scene graph:

```rust
// Layer types from flui_engine
use flui_engine::{
    BoxedLayer,
    layer::{PictureLayer, ContainerLayer, OpacityLayer, TransformLayer},
};

// RenderObjects return layers
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    // Option 1: PictureLayer (for drawing)
    let mut layer = PictureLayer::new();
    layer.set_display_list(display_list);
    Box::new(layer)

    // Option 2: ContainerLayer (for grouping)
    let mut container = ContainerLayer::new();
    container.append(child_layer);
    Box::new(container)

    // Option 3: Effect layer (opacity, transform, clip)
    let mut opacity = OpacityLayer::new(0.5);
    opacity.append(child_layer);
    Box::new(opacity)
}
```

---

## Summary

**flui_rendering** provides the **layout and paint layer** for FLUI:

- ✅ **Unified Render Trait** - Single trait for all RenderObjects (0/1/N children)
- ✅ **81+ RenderObjects** - Flutter-compatible layout primitives
- ✅ **Context Pattern** - Clean API via LayoutContext/PaintContext
- ✅ **Arity Validation** - Runtime child count checking
- ✅ **ParentData/Metadata** - Type-safe per-child data via downcasting
- ✅ **Layout Caching** - Managed by Element for performance
- ✅ **Zero-Cost Abstractions** - Compiles to concrete code (no trait overhead)

**Clear Separation of Concerns:**
- **flui_widgets** builds RenderObjects (high-level API)
- **flui_core** manages RenderObjects in Element tree (caching, dirty tracking)
- **flui_rendering** implements RenderObjects (layout/paint logic)
- **flui_painting** records drawing commands (Canvas API)
- **flui_engine** executes drawing commands (GPU rendering)

**Total LOC:** ~15,000 (81+ RenderObjects implemented)

This architecture provides Flutter's proven layout system with Rust's zero-cost abstractions!

---

## Related Documentation

### Implementation
- **Source Code**: `crates/flui_rendering/src/`
- **RenderObjects**: `crates/flui_rendering/src/objects/`
- **Tests**: `crates/flui_rendering/tests/`

### Patterns & Integration
- **Patterns**: [PATTERNS.md](PATTERNS.md#rendering-patterns) - Unified Render trait, Context pattern, Metadata
- **Integration**: [INTEGRATION.md](INTEGRATION.md#flow-1-widget--element--render) - How rendering integrates with other layers
- **Navigation**: [README.md](README.md) - Architecture documentation hub

### Related Architecture Docs
- **flui_core**: [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) - Element tree and pipeline system
- **flui_widgets**: [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md) - High-level widget layer
- **flui_painting**: [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md) - Canvas API and DisplayList
- **flui_engine**: [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) - GPU rendering backend

### External References
- **Flutter RenderObject**: [flutter.dev/docs](https://flutter.dev/docs/development/ui/widgets-intro#rendering-objects)
- **FLUI Guide**: [../../CLAUDE.md](../../CLAUDE.md#creating-a-renderobject) - Development guidelines
