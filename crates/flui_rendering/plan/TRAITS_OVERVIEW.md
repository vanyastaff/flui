# FLUI Rendering Traits Overview

Based on analysis of Flutter rendering architecture and `impl/` documentation.

## Trait Hierarchy Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              CORE TRAITS                                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐           │
│  │   RenderState   │     │    Protocol     │     │   Constraints   │           │
│  │  (typestate)    │     │  (layout type)  │     │  (layout input) │           │
│  ├─────────────────┤     ├─────────────────┤     ├─────────────────┤           │
│  │ Detached        │     │ BoxProtocol     │     │ BoxConstraints  │           │
│  │ Attached        │     │ SliverProtocol  │     │ SliverConstraints│          │
│  │ LaidOut         │     └─────────────────┘     └─────────────────┘           │
│  │ Painted         │                                                            │
│  └─────────────────┘                                                            │
│                                                                                 │
│  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐           │
│  │    Geometry     │     │   ParentData    │     │ HitTestResult   │           │
│  │  (layout out)   │     │ (child data)    │     │  (hit testing)  │           │
│  ├─────────────────┤     ├─────────────────┤     ├─────────────────┤           │
│  │ Size            │     │ BoxParentData   │     │ BoxHitTestResult│           │
│  │ SliverGeometry  │     │ SliverParentData│     │ SliverHitTestRes│           │
│  └─────────────────┘     └─────────────────┘     └─────────────────┘           │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                           RENDER NODE TRAITS                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│                         ┌───────────────────┐                                   │
│                         │   RenderObject    │  (base behavior)                  │
│                         └─────────┬─────────┘                                   │
│                                   │                                             │
│              ┌────────────────────┼────────────────────┐                        │
│              │                    │                    │                        │
│              ▼                    ▼                    ▼                        │
│  ┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐             │
│  │  LayoutProtocol   │ │  PaintProtocol    │ │  HitTestProtocol  │             │
│  │  <P: Protocol>    │ │                   │ │  <P: Protocol>    │             │
│  ├───────────────────┤ ├───────────────────┤ ├───────────────────┤             │
│  │ perform_layout()  │ │ paint()           │ │ hit_test()        │             │
│  │ sized_by_parent() │ │ paint_bounds()    │ │ hit_test_self()   │             │
│  │ compute_dry_layout│ │ is_repaint_bound  │ │ hit_test_children │             │
│  └───────────────────┘ └───────────────────┘ └───────────────────┘             │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                           CHILD MANAGEMENT                                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  Uses Arity from flui-tree:                                                     │
│                                                                                 │
│  ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐               │
│  │      Leaf       │   │     Single      │   │    Optional     │               │
│  │   (0 children)  │   │   (1 child)     │   │  (0-1 child)    │               │
│  └─────────────────┘   └─────────────────┘   └─────────────────┘               │
│                                                                                 │
│  ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐               │
│  │    Variable     │   │    Exact<N>     │   │  Range<MIN,MAX> │               │
│  │  (N children)   │   │ (exactly N)     │   │  (MIN..MAX)     │               │
│  └─────────────────┘   └─────────────────┘   └─────────────────┘               │
│                                                                                 │
│  Additional patterns:                                                           │
│                                                                                 │
│  ┌─────────────────┐   ┌─────────────────┐                                     │
│  │   ProxyChild    │   │  ShiftedChild   │                                     │
│  │ (delegate to 1) │   │ (offset child)  │                                     │
│  └─────────────────┘   └─────────────────┘                                     │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## 1. Core Traits

### RenderState (Typestate Markers)

```rust
mod sealed {
    pub trait Sealed {}
}

/// Marker trait for render lifecycle states.
pub trait RenderState: sealed::Sealed + Copy + Default + 'static {
    const IS_ATTACHED: bool;
    const IS_LAID_OUT: bool;
    const IS_PAINTED: bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Detached;

#[derive(Debug, Clone, Copy, Default)]
pub struct Attached;

#[derive(Debug, Clone, Copy, Default)]
pub struct LaidOut;

#[derive(Debug, Clone, Copy, Default)]
pub struct Painted;
```

### Protocol

```rust
/// Layout protocol (Box vs Sliver).
pub trait Protocol: sealed::Sealed + 'static {
    /// Constraints from parent.
    type Constraints: Constraints;
    
    /// Geometry computed during layout.
    type Geometry: Geometry;
    
    /// Data stored on child by parent.
    type ParentData: ParentData;
    
    /// Hit test result type.
    type HitTestResult: HitTestResultTrait;
    
    /// Hit test position type.
    type HitTestPosition: Copy;
    
    fn name() -> &'static str;
}

pub struct BoxProtocol;
pub struct SliverProtocol;
```

### Constraints

```rust
/// Layout input from parent to child.
pub trait Constraints: Clone + PartialEq + Debug + Send + Sync + 'static {
    /// Whether exactly one size satisfies these constraints.
    fn is_tight(&self) -> bool;
    
    /// Whether constraints are in canonical form.
    fn is_normalized(&self) -> bool;
}
```

### Geometry

```rust
/// Layout output from child to parent.
pub trait Geometry: Clone + Debug + Send + Sync + 'static {
    /// Check if geometry is valid.
    fn is_valid(&self) -> bool { true }
}

// Size implements Geometry for BoxProtocol
// SliverGeometry implements Geometry for SliverProtocol
```

### ParentData

```rust
/// Data stored on child by parent for positioning.
pub trait ParentData: Default + Debug + Send + Sync + 'static {
    /// Called when detaching from parent.
    fn detach(&mut self) {}
}
```

### HitTestResultTrait

```rust
/// Base trait for hit test results.
pub trait HitTestResultTrait: Default {
    /// Add target to hit path.
    fn add(&mut self, target: RenderNodeId);
    
    /// Push transform for subsequent entries.
    fn push_transform(&mut self, transform: Mat4);
    
    /// Pop most recent transform.
    fn pop_transform(&mut self);
    
    /// Whether any targets were hit.
    fn is_empty(&self) -> bool;
    
    /// Get hit path.
    fn path(&self) -> &[HitTestEntry];
}
```

## 2. Render Object Traits

### RenderObject (Base)

```rust
/// Base trait for all render objects.
/// Handles tree structure, dirty flags, attach/detach.
pub trait RenderObject: Send + Sync + 'static {
    /// Unique debug name.
    fn debug_name(&self) -> &'static str;
    
    /// Visit all children.
    fn visit_children(&self, visitor: &mut dyn FnMut(RenderNodeId));
    
    /// Visit children mutably.
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(RenderNodeId));
    
    /// Number of children.
    fn child_count(&self) -> usize;
}
```

### LayoutProtocol

```rust
/// Layout behavior for a specific protocol.
pub trait LayoutProtocol<P: Protocol>: RenderObject {
    /// Whether size is determined solely by constraints.
    /// If true, layout is split into performResize + performLayout.
    fn sized_by_parent(&self) -> bool { false }
    
    /// Perform layout and compute geometry.
    /// Called with constraints stored in node.
    fn perform_layout(
        &mut self,
        constraints: &P::Constraints,
        children: &mut dyn ChildLayouter<P>,
    ) -> P::Geometry;
    
    /// Compute layout without side effects (for intrinsics).
    fn compute_dry_layout(
        &self,
        constraints: &P::Constraints,
    ) -> P::Geometry {
        // Default: not supported
        panic!("compute_dry_layout not implemented")
    }
}
```

### PaintProtocol

```rust
/// Paint behavior.
pub trait PaintProtocol: RenderObject {
    /// Paint this render object.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    /// Estimated bounds for debugging.
    fn paint_bounds(&self) -> Rect;
    
    /// Whether this creates its own compositing layer.
    fn is_repaint_boundary(&self) -> bool { false }
    
    /// Whether this always needs compositing.
    fn always_needs_compositing(&self) -> bool { false }
}
```

### HitTestProtocol

```rust
/// Hit testing behavior for a specific protocol.
pub trait HitTestProtocol<P: Protocol>: RenderObject {
    /// Hit test at position.
    fn hit_test(
        &self,
        result: &mut P::HitTestResult,
        position: P::HitTestPosition,
    ) -> bool {
        if self.hit_test_self(position) {
            result.add(self.id());
        }
        self.hit_test_children(result, position)
    }
    
    /// Test if position hits this object (not children).
    fn hit_test_self(&self, position: P::HitTestPosition) -> bool;
    
    /// Test children, adding to result.
    fn hit_test_children(
        &self,
        result: &mut P::HitTestResult,
        position: P::HitTestPosition,
    ) -> bool;
}
```

## 3. Box Protocol Traits

### BoxLayout

```rust
/// Box-specific layout methods.
pub trait BoxLayout: LayoutProtocol<BoxProtocol> {
    /// Compute minimum intrinsic width.
    fn compute_min_intrinsic_width(&self, height: f64) -> f64 { 0.0 }
    
    /// Compute maximum intrinsic width.
    fn compute_max_intrinsic_width(&self, height: f64) -> f64 { 0.0 }
    
    /// Compute minimum intrinsic height.
    fn compute_min_intrinsic_height(&self, width: f64) -> f64 { 0.0 }
    
    /// Compute maximum intrinsic height.
    fn compute_max_intrinsic_height(&self, width: f64) -> f64 { 0.0 }
    
    /// Compute baseline distance from top.
    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> Option<f64> { None }
}
```

### BoxHitTest

```rust
/// Box-specific hit testing.
pub trait BoxHitTest: HitTestProtocol<BoxProtocol> {
    /// Default: hit if within size bounds.
    fn hit_test_self(&self, position: Offset) -> bool {
        let size = self.size();
        position.x >= 0.0 && position.x < size.width &&
        position.y >= 0.0 && position.y < size.height
    }
}
```

## 4. Sliver Protocol Traits

### SliverLayout

```rust
/// Sliver-specific layout methods.
pub trait SliverLayout: LayoutProtocol<SliverProtocol> {
    /// Calculate paint offset for a child at given layout offset.
    fn child_main_axis_position(&self, child: RenderNodeId) -> f64;
    
    /// Calculate cross axis offset for child.
    fn child_cross_axis_position(&self, child: RenderNodeId) -> f64 { 0.0 }
    
    /// Get scroll offset correction to apply.
    fn scroll_offset_correction(&self) -> Option<f64> { None }
}
```

### SliverHitTest

```rust
/// Sliver-specific hit testing.
pub trait SliverHitTest: HitTestProtocol<SliverProtocol> {
    /// Test hit in sliver coordinate system.
    fn hit_test_self(&self, main_axis: f64, cross_axis: f64) -> bool;
}
```

## 5. Child Pattern Traits

### ProxyChild (Delegates to Single Child)

```rust
/// A render object that delegates most behavior to its single child.
/// Used for visual effects: opacity, transform, clip, etc.
pub trait ProxyChild<P: Protocol>: LayoutProtocol<P> + PaintProtocol {
    fn child(&self) -> Option<RenderNodeId>;
    
    // Default implementations delegate to child
    
    fn perform_layout(
        &mut self,
        constraints: &P::Constraints,
        children: &mut dyn ChildLayouter<P>,
    ) -> P::Geometry {
        if let Some(child_id) = self.child() {
            children.layout_child(child_id, constraints.clone())
        } else {
            P::Geometry::default()
        }
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child_id) = self.child() {
            context.paint_child(child_id, offset);
        }
    }
}
```

### ShiftedChild (Single Child at Offset)

```rust
/// A render object that positions its child at a non-zero offset.
/// Used for layout: padding, alignment, positioning.
pub trait ShiftedChild<P: Protocol>: LayoutProtocol<P> + PaintProtocol
where
    P::ParentData: HasOffset,
{
    fn child(&self) -> Option<RenderNodeId>;
    fn child_offset(&self) -> Offset;
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child_id) = self.child() {
            context.paint_child(child_id, offset + self.child_offset());
        }
    }
}

/// ParentData that has an offset field.
pub trait HasOffset {
    fn offset(&self) -> Offset;
    fn set_offset(&mut self, offset: Offset);
}
```

### ContainerChild (Multiple Children)

```rust
/// A render object with multiple children.
/// Uses linked list via ParentData sibling pointers.
pub trait ContainerChild<P: Protocol>: LayoutProtocol<P> + PaintProtocol
where
    P::ParentData: ContainerParentDataTrait,
{
    fn first_child(&self) -> Option<RenderNodeId>;
    fn last_child(&self) -> Option<RenderNodeId>;
    fn child_count(&self) -> usize;
    
    fn child_before(&self, child: RenderNodeId) -> Option<RenderNodeId>;
    fn child_after(&self, child: RenderNodeId) -> Option<RenderNodeId>;
    
    fn insert(&mut self, child: RenderNodeId, after: Option<RenderNodeId>);
    fn remove(&mut self, child: RenderNodeId);
}

/// ParentData with sibling links.
pub trait ContainerParentDataTrait: ParentData + HasOffset {
    fn previous_sibling(&self) -> Option<RenderNodeId>;
    fn next_sibling(&self) -> Option<RenderNodeId>;
    fn set_previous_sibling(&mut self, id: Option<RenderNodeId>);
    fn set_next_sibling(&mut self, id: Option<RenderNodeId>);
}
```

## 6. Pipeline Traits

### PipelineOwner

```rust
/// Coordinates the rendering pipeline phases.
pub trait PipelineOwnerTrait {
    /// Request visual update (schedules frame).
    fn request_visual_update(&mut self);
    
    /// Flush layout for all dirty nodes.
    fn flush_layout(&mut self);
    
    /// Flush compositing bits update.
    fn flush_compositing_bits(&mut self);
    
    /// Flush paint for all dirty nodes.
    fn flush_paint(&mut self);
    
    /// Flush semantics update.
    fn flush_semantics(&mut self);
}
```

### ChildLayouter (Layout Helper)

```rust
/// Helper for laying out children during perform_layout.
pub trait ChildLayouter<P: Protocol> {
    /// Layout child with constraints.
    fn layout_child(
        &mut self,
        child: RenderNodeId,
        constraints: P::Constraints,
    ) -> P::Geometry;
    
    /// Get child's computed geometry (after layout).
    fn child_geometry(&self, child: RenderNodeId) -> &P::Geometry;
    
    /// Get child's parent data.
    fn child_parent_data(&self, child: RenderNodeId) -> &P::ParentData;
    
    /// Get mutable child parent data.
    fn child_parent_data_mut(&mut self, child: RenderNodeId) -> &mut P::ParentData;
}
```

## 7. Context Traits

### PaintingContext

```rust
/// Context for paint phase operations.
/// Owns canvas, manages compositing layers.
pub trait PaintingContextTrait {
    /// Get current canvas.
    fn canvas(&mut self) -> &mut dyn Canvas;
    
    /// Paint a child at offset.
    fn paint_child(&mut self, child: RenderNodeId, offset: Offset);
    
    /// Push clip rect (may create layer).
    fn push_clip_rect<F>(&mut self, rect: Rect, painter: F)
    where F: FnOnce(&mut Self);
    
    /// Push opacity (creates layer).
    fn push_opacity<F>(&mut self, alpha: f32, offset: Offset, painter: F)
    where F: FnOnce(&mut Self);
    
    /// Push transform (may create layer).
    fn push_transform<F>(&mut self, transform: Mat4, painter: F)
    where F: FnOnce(&mut Self);
    
    /// Add composited layer directly.
    fn add_layer(&mut self, layer: Layer);
    
    /// Hint: painting is complex, consider caching.
    fn set_is_complex_hint(&mut self);
    
    /// Hint: will change next frame, don't cache.
    fn set_will_change_hint(&mut self);
}
```

## 8. Transition Traits

### Attachable

```rust
/// Nodes that can be attached to a render tree.
pub trait Attachable {
    type Attached;
    
    fn attach(self, owner: &mut dyn PipelineOwnerTrait) -> Self::Attached;
}
```

### Layoutable

```rust
/// Attached nodes that can perform layout.
pub trait Layoutable<P: Protocol> {
    type LaidOut;
    
    fn layout(self, constraints: P::Constraints) -> Self::LaidOut;
}
```

### Paintable

```rust
/// Laid out nodes that can paint.
pub trait Paintable {
    type Painted;
    
    fn paint(self, context: &mut dyn PaintingContextTrait) -> Self::Painted;
}
```

### Invalidatable

```rust
/// Nodes that can be invalidated (marked dirty).
pub trait Invalidatable {
    type NeedsLayout;
    type NeedsPaint;
    
    fn mark_needs_layout(self) -> Self::NeedsLayout;
    fn mark_needs_paint(self) -> Self::NeedsPaint;
}
```

## Summary Table

| Category | Trait | Purpose |
|----------|-------|---------|
| **State** | `RenderState` | Lifecycle typestate markers |
| **Protocol** | `Protocol` | Box vs Sliver layout system |
| **Core** | `Constraints` | Layout input (parent → child) |
| **Core** | `Geometry` | Layout output (child → parent) |
| **Core** | `ParentData` | Child positioning data |
| **Core** | `HitTestResultTrait` | Hit test path recording |
| **Node** | `RenderObject` | Base behavior, tree structure |
| **Node** | `LayoutProtocol<P>` | Protocol-specific layout |
| **Node** | `PaintProtocol` | Painting behavior |
| **Node** | `HitTestProtocol<P>` | Protocol-specific hit testing |
| **Box** | `BoxLayout` | Intrinsics, baseline |
| **Box** | `BoxHitTest` | Box coordinate hit testing |
| **Sliver** | `SliverLayout` | Scroll positioning |
| **Sliver** | `SliverHitTest` | Axis coordinate hit testing |
| **Child** | `ProxyChild` | Delegate to single child |
| **Child** | `ShiftedChild` | Child at offset |
| **Child** | `ContainerChild` | Multiple children (linked list) |
| **Pipeline** | `PipelineOwnerTrait` | Phase coordination |
| **Pipeline** | `ChildLayouter<P>` | Child layout helper |
| **Context** | `PaintingContextTrait` | Canvas + layer management |
| **Transition** | `Attachable` | Detached → Attached |
| **Transition** | `Layoutable<P>` | Attached → LaidOut |
| **Transition** | `Paintable` | LaidOut → Painted |
| **Transition** | `Invalidatable` | Mark dirty (state regression) |

## File Organization

```
crates/flui_rendering/src/
├── lib.rs
├── state/
│   ├── mod.rs           # RenderState trait + markers
│   └── transitions.rs   # Attachable, Layoutable, Paintable, Invalidatable
├── protocol/
│   ├── mod.rs           # Protocol trait
│   ├── box_protocol.rs  # BoxProtocol, BoxConstraints, Size, BoxParentData
│   └── sliver_protocol.rs
├── traits/
│   ├── mod.rs           # Re-exports
│   ├── constraints.rs   # Constraints trait
│   ├── geometry.rs      # Geometry trait
│   ├── parent_data.rs   # ParentData, HasOffset, ContainerParentDataTrait
│   ├── hit_test.rs      # HitTestResultTrait, HitTestEntry
│   ├── render_object.rs # RenderObject base trait
│   ├── layout.rs        # LayoutProtocol, BoxLayout, SliverLayout
│   ├── paint.rs         # PaintProtocol, PaintingContextTrait
│   └── hit_test_protocol.rs # HitTestProtocol, BoxHitTest, SliverHitTest
├── patterns/
│   ├── mod.rs
│   ├── proxy.rs         # ProxyChild
│   ├── shifted.rs       # ShiftedChild
│   └── container.rs     # ContainerChild
├── pipeline/
│   ├── mod.rs
│   ├── owner.rs         # PipelineOwnerTrait, PipelineOwner
│   └── child_layouter.rs # ChildLayouter
└── node/
    ├── mod.rs
    ├── render_node.rs   # RenderNode<P, A, S>
    └── aliases.rs       # BoxLeaf, BoxSingle, etc.
```
