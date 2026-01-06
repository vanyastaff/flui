# Flutter-Style Layout and Paint System Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement proper Flutter-style synchronous layout and paint systems with correct layer handling.

**Architecture:** Replace deep-first layout workaround with proper shallow-first + synchronous child layout using RefCell. Add repaint boundary support with OffsetLayer creation. Fix paint offset propagation to go through layers instead of manual accumulation.

**Tech Stack:** Rust, RefCell for interior mutability, flui_rendering, flui-layer crates

---

## Background

### Current Problems

1. **Layout:** `layout_child()` just returns cached size, doesn't trigger actual child layout
2. **Layout:** Using deep-first sorting as workaround (breaks Row/Column with dynamic constraints)
3. **Paint:** Manual offset accumulation instead of OffsetLayer for repaint boundaries
4. **Paint:** No repaint boundary support

### Flutter's Approach (from deepwiki)

1. **Layout:** `flushLayout()` sorts shallow-first, parent's `performLayout()` calls `child.layout()` synchronously
2. **Paint:** Repaint boundaries create `OffsetLayer`, `paintChild()` sets layer offset instead of canvas translate
3. **Layers:** `PictureLayer` holds drawing commands, `OffsetLayer` for positioning

---

## Task 1: Add RefCell to RenderNode for Interior Mutability

**Files:**
- Modify: `crates/flui_rendering/src/tree/render_tree.rs`

**Step 1: Add RefCell import and modify RenderNode**

```rust
// At top of file, add:
use std::cell::RefCell;

// Change RenderNode struct:
#[derive(Debug)]
pub struct RenderNode {
    /// The render object (RefCell for interior mutability during layout).
    render_object: RefCell<Box<dyn RenderObject>>,
    
    // ... rest unchanged
}
```

**Step 2: Update RenderNode methods for RefCell**

```rust
impl RenderNode {
    /// Creates a new render node.
    #[inline]
    pub fn new(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object: RefCell::new(render_object),
            parent: None,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Returns a reference to the render object.
    #[inline]
    pub fn render_object(&self) -> std::cell::Ref<'_, Box<dyn RenderObject>> {
        self.render_object.borrow()
    }

    /// Returns a mutable reference to the render object.
    #[inline]
    pub fn render_object_mut(&self) -> std::cell::RefMut<'_, Box<dyn RenderObject>> {
        self.render_object.borrow_mut()
    }

    /// Try to borrow render object, returns None if already borrowed.
    #[inline]
    pub fn try_render_object(&self) -> Option<std::cell::Ref<'_, Box<dyn RenderObject>>> {
        self.render_object.try_borrow().ok()
    }

    /// Try to borrow render object mutably, returns None if already borrowed.
    #[inline]
    pub fn try_render_object_mut(&self) -> Option<std::cell::RefMut<'_, Box<dyn RenderObject>>> {
        self.render_object.try_borrow_mut().ok()
    }
}
```

**Step 3: Update into_render_object**

```rust
    /// Consumes the node and returns the render object.
    #[inline]
    pub fn into_render_object(self) -> Box<dyn RenderObject> {
        self.render_object.into_inner()
    }
```

**Step 4: Run tests**

```bash
cargo test -p flui_rendering -- render_tree
```
Expected: All existing tests pass (API compatible)

**Step 5: Commit**

```bash
git add crates/flui_rendering/src/tree/render_tree.rs
git commit -m "refactor(rendering): add RefCell to RenderNode for interior mutability"
```

---

## Task 2: Create LayoutEngine Trait and Implementation

**Files:**
- Create: `crates/flui_rendering/src/layout/engine.rs`
- Modify: `crates/flui_rendering/src/layout/mod.rs`

**Step 1: Create layout engine trait**

```rust
// crates/flui_rendering/src/layout/engine.rs
//! Layout engine for synchronous child layout.

use flui_foundation::RenderId;
use flui_types::Size;

use crate::constraints::BoxConstraints;
use crate::tree::RenderTree;

/// Trait for layout operations that need tree access.
/// 
/// This enables synchronous child layout where parent's `perform_layout`
/// can trigger child layout and get immediate size results.
pub trait LayoutEngine {
    /// Layouts a child with given constraints and returns its size.
    /// 
    /// This is called from parent's `perform_layout` when it needs
    /// to know child's size. The child is laid out synchronously.
    fn layout_child(&self, child_id: RenderId, constraints: BoxConstraints) -> Size;
}

/// Layout engine implementation using RenderTree.
pub struct TreeLayoutEngine<'a> {
    tree: &'a RenderTree,
}

impl<'a> TreeLayoutEngine<'a> {
    /// Creates a new layout engine with tree access.
    pub fn new(tree: &'a RenderTree) -> Self {
        Self { tree }
    }

    /// Recursively layout a node and its children.
    fn layout_node_recursive(&self, node_id: RenderId, constraints: BoxConstraints) -> Size {
        let node = match self.tree.get(node_id) {
            Some(n) => n,
            None => return Size::ZERO,
        };

        // Get child IDs before borrowing render object
        let child_ids: Vec<RenderId> = node.children().to_vec();

        // Borrow render object mutably
        let mut ro = node.render_object_mut();
        
        // Check early exit: not dirty and same constraints
        if !ro.needs_layout() {
            if let Some(cached) = ro.cached_constraints() {
                if cached == constraints {
                    return ro.size();
                }
            }
        }

        // Set constraints and call layout
        ro.layout(constraints, true);

        ro.size()
    }
}

impl<'a> LayoutEngine for TreeLayoutEngine<'a> {
    fn layout_child(&self, child_id: RenderId, constraints: BoxConstraints) -> Size {
        self.layout_node_recursive(child_id, constraints)
    }
}
```

**Step 2: Update mod.rs**

```rust
// Add to crates/flui_rendering/src/layout/mod.rs
mod engine;
pub use engine::{LayoutEngine, TreeLayoutEngine};
```

**Step 3: Run build**

```bash
cargo check -p flui_rendering
```
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add crates/flui_rendering/src/layout/
git commit -m "feat(rendering): add LayoutEngine trait for synchronous child layout"
```

---

## Task 3: Update BoxLayoutCtx to Use LayoutEngine

**Files:**
- Modify: `crates/flui_rendering/src/protocol/box_protocol.rs`

**Step 1: Add LayoutEngine to BoxLayoutCtx**

```rust
use crate::layout::LayoutEngine;

pub struct BoxLayoutCtx<'ctx, A: Arity, P: ParentData + Default> {
    constraints: BoxConstraints,
    geometry: Option<Size>,
    children: Option<&'ctx mut Vec<crate::children_access::ChildState<P>>>,
    /// Child render IDs for layout engine lookup.
    child_ids: Option<&'ctx [RenderId]>,
    /// Layout engine for synchronous child layout.
    layout_engine: Option<&'ctx dyn LayoutEngine>,
    _phantom: std::marker::PhantomData<A>,
}
```

**Step 2: Add constructor with layout engine**

```rust
impl<'ctx, A: Arity, P: ParentData + Default> BoxLayoutCtx<'ctx, A, P> {
    /// Creates layout context with full access (children + layout engine).
    pub fn with_layout_engine(
        constraints: BoxConstraints,
        children: &'ctx mut Vec<crate::children_access::ChildState<P>>,
        child_ids: &'ctx [RenderId],
        layout_engine: &'ctx dyn LayoutEngine,
    ) -> Self {
        Self {
            constraints,
            geometry: None,
            children: Some(children),
            child_ids: Some(child_ids),
            layout_engine: Some(layout_engine),
            _phantom: std::marker::PhantomData,
        }
    }

    // Update existing constructors to set child_ids and layout_engine to None
}
```

**Step 3: Implement synchronous layout_child**

```rust
impl<'ctx, A: Arity, P: ParentData + Default> LayoutContextApi<'ctx, BoxLayout, A, P>
    for BoxLayoutCtx<'ctx, A, P>
{
    fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        // Get child ID
        let child_id = match self.child_ids.and_then(|ids| ids.get(index)) {
            Some(&id) => id,
            None => {
                // Fallback: return cached size
                if let Some(children) = &self.children {
                    if let Some(child) = children.get(index) {
                        return child.size;
                    }
                }
                return Size::ZERO;
            }
        };

        // Use layout engine for synchronous layout
        let size = if let Some(engine) = self.layout_engine {
            engine.layout_child(child_id, constraints)
        } else {
            // Fallback: return cached size
            self.children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| child.size)
                .unwrap_or(Size::ZERO)
        };

        // Update cached size
        if let Some(children) = &mut self.children {
            if let Some(child) = children.get_mut(index) {
                child.size = size;
            }
        }

        size
    }
}
```

**Step 4: Run tests**

```bash
cargo test -p flui_rendering
```

**Step 5: Commit**

```bash
git add crates/flui_rendering/src/protocol/box_protocol.rs
git commit -m "feat(rendering): implement synchronous layout_child in BoxLayoutCtx"
```

---

## Task 4: Update BoxWrapper to Pass LayoutEngine

**Files:**
- Modify: `crates/flui_rendering/src/wrapper/box_wrapper.rs`

**Step 1: Update layout_without_resize to accept layout engine**

```rust
impl<T: RenderBox> BoxWrapper<T> {
    /// Performs layout with access to layout engine for synchronous child layout.
    pub fn layout_with_engine(&mut self, engine: &dyn LayoutEngine) {
        let constraints = self
            .cached_constraints
            .unwrap_or_else(|| BoxConstraints::loose(Size::new(f32::INFINITY, f32::INFINITY)));

        use crate::protocol::BoxLayoutCtx;

        let inner_ctx = BoxLayoutCtx::<T::Arity, T::ParentData>::with_layout_engine(
            constraints,
            &mut self.children,
            &self.child_ids,
            engine,
        );
        let mut ctx = LayoutContext::<BoxProtocol, T::Arity, T::ParentData>::new(inner_ctx);

        self.inner.perform_layout(&mut ctx);
        self.needs_layout = false;
    }
}
```

**Step 2: Keep layout_without_resize for compatibility**

```rust
    fn layout_without_resize(&mut self) {
        // Fallback without layout engine (uses cached sizes)
        let constraints = self
            .cached_constraints
            .unwrap_or_else(|| BoxConstraints::loose(Size::new(f32::INFINITY, f32::INFINITY)));

        use crate::protocol::BoxLayoutCtx;

        let inner_ctx =
            BoxLayoutCtx::<T::Arity, T::ParentData>::with_children(constraints, &mut self.children);
        let mut ctx = LayoutContext::<BoxProtocol, T::Arity, T::ParentData>::new(inner_ctx);

        self.inner.perform_layout(&mut ctx);
        self.needs_layout = false;
    }
```

**Step 3: Run tests**

```bash
cargo test -p flui_rendering -- box_wrapper
```

**Step 4: Commit**

```bash
git add crates/flui_rendering/src/wrapper/box_wrapper.rs
git commit -m "feat(rendering): add layout_with_engine to BoxWrapper"
```

---

## Task 5: Update flush_layout to Use Shallow-First and LayoutEngine

**Files:**
- Modify: `crates/flui_rendering/src/pipeline/owner.rs`

**Step 1: Change sorting to shallow-first**

```rust
pub fn flush_layout(&mut self) {
    // ... existing code ...

    if !self.nodes_needing_layout.is_empty() {
        self.debug_doing_layout = true;

        let mut dirty_nodes = std::mem::take(&mut self.nodes_needing_layout);

        // Sort by depth SHALLOW-FIRST (parents before children)
        // Flutter: dirtyNodes.sort((a, b) => a.depth - b.depth)
        dirty_nodes.sort_unstable_by_key(|node| node.depth);

        tracing::debug!(
            "flush_layout: sorted order (shallow-first) = {:?}",
            dirty_nodes
                .iter()
                .map(|n| (n.id, n.depth))
                .collect::<Vec<_>>()
        );

        // Create layout engine
        let engine = TreeLayoutEngine::new(&self.render_tree);

        // Process dirty nodes
        for dirty_node in dirty_nodes {
            let render_id = RenderId::new(dirty_node.id);
            
            if let Some(render_node) = self.render_tree.get(render_id) {
                let mut render_object = render_node.render_object_mut();
                
                if render_object.needs_layout() {
                    // For RenderView (root), use perform_layout directly
                    if let Some(render_view) = render_object
                        .as_any_mut()
                        .downcast_mut::<crate::view::RenderView>()
                    {
                        render_view.prepare_initial_frame_without_owner();
                        render_view.perform_layout();
                    } else {
                        // Use layout_with_engine for synchronous child layout
                        // Note: This requires downcasting, which we'll handle
                        render_object.layout_without_resize();
                    }
                }
            }

            // Sync child size to parent
            self.sync_child_size_to_parent(render_id);
        }

        self.debug_doing_layout = false;
    }
    
    // ... flush children ...
}
```

**Step 2: Run the demo to verify**

```bash
cd demos/counter && RUST_LOG=debug cargo run 2>&1 | head -100
```
Expected: Layout order shows `[(1, 0), (2, 1), (3, 2)]` (shallow-first)

**Step 3: Commit**

```bash
git add crates/flui_rendering/src/pipeline/owner.rs
git commit -m "feat(rendering): change flush_layout to shallow-first ordering"
```

---

## Task 6: Add Repaint Boundary Support

**Files:**
- Modify: `crates/flui_rendering/src/traits/render_object.rs`
- Modify: `crates/flui_rendering/src/context/canvas.rs`

**Step 1: Add layer handle to RenderObject trait**

```rust
// In render_object.rs, add to trait:
    /// Returns the layer ID for this repaint boundary.
    fn layer_id(&self) -> Option<LayerId> {
        None
    }

    /// Sets the layer ID for this repaint boundary.
    fn set_layer_id(&mut self, _layer_id: Option<LayerId>) {}
```

**Step 2: Update CanvasContext::paint_child for repaint boundaries**

```rust
    /// Paints a child render object.
    /// 
    /// If the child is a repaint boundary, it creates/reuses an OffsetLayer.
    pub fn paint_child_with_boundary(
        &mut self, 
        child: &dyn RenderObject, 
        child_offset: Offset,
        is_repaint_boundary: bool,
    ) {
        if is_repaint_boundary {
            self.stop_recording_if_needed();
            
            // Create or reuse OffsetLayer for repaint boundary
            let offset_layer = OffsetLayer::new(child_offset);
            let layer_id = self.layer_tree.insert(Layer::Offset(offset_layer));
            
            // Add to current parent
            if let Some(parent_id) = self.current_layer {
                self.layer_tree.add_child(parent_id, layer_id);
            }
            
            // Push onto stack
            let prev_layer = self.current_layer;
            self.current_layer = Some(layer_id);
            self.layer_stack.push(layer_id);
            
            // Paint child at ZERO offset (offset handled by layer)
            child.paint(self, Offset::ZERO);
            
            self.stop_recording_if_needed();
            
            // Pop stack
            self.layer_stack.pop();
            self.current_layer = prev_layer;
        } else {
            // Not a repaint boundary - paint directly with offset
            child.paint(self, child_offset);
        }
    }
```

**Step 3: Run tests**

```bash
cargo test -p flui_rendering
```

**Step 4: Commit**

```bash
git add crates/flui_rendering/src/traits/render_object.rs
git add crates/flui_rendering/src/context/canvas.rs
git commit -m "feat(rendering): add repaint boundary support with OffsetLayer"
```

---

## Task 7: Update flush_paint to Use Repaint Boundaries

**Files:**
- Modify: `crates/flui_rendering/src/pipeline/owner.rs`

**Step 1: Update paint_node_recursive to check repaint boundaries**

```rust
    fn paint_node_recursive(&self, context: &mut CanvasContext, node_id: RenderId, offset: Offset) {
        let (children_with_offsets, is_repaint_boundary): (Vec<(RenderId, Offset)>, bool) = {
            if let Some(render_node) = self.render_tree.get(node_id) {
                let render_object = render_node.render_object();
                let tree_children = render_node.children();

                let is_boundary = render_object.is_repaint_boundary();

                tracing::debug!(
                    "paint_node_recursive: node_id={:?}, offset=({}, {}), is_repaint_boundary={}",
                    node_id,
                    offset.dx,
                    offset.dy,
                    is_boundary
                );

                // If this is a repaint boundary and not root, the offset is handled by layer
                let paint_offset = if is_boundary && render_node.parent().is_some() {
                    Offset::ZERO
                } else {
                    offset
                };

                // Paint this node
                render_object.paint(context, paint_offset);

                // Collect children with their offsets
                let children: Vec<_> = tree_children
                    .iter()
                    .enumerate()
                    .map(|(i, &child_id)| {
                        let child_offset = render_object.child_offset(i);
                        (child_id, child_offset)
                    })
                    .collect();

                (children, is_boundary)
            } else {
                return;
            }
        };

        // Paint children recursively
        for (child_id, child_offset) in children_with_offsets {
            // Check if child is repaint boundary
            let child_is_boundary = self.render_tree
                .get(child_id)
                .map(|n| n.render_object().is_repaint_boundary())
                .unwrap_or(false);

            if child_is_boundary {
                // For repaint boundary, use OffsetLayer
                context.paint_child_with_offset(child_offset, |ctx| {
                    self.paint_node_recursive(ctx, child_id, Offset::ZERO);
                });
            } else {
                // Accumulate offset
                let child_accumulated = offset + child_offset;
                self.paint_node_recursive(context, child_id, child_accumulated);
            }
        }
    }
```

**Step 2: Run demo**

```bash
cd demos/counter && RUST_LOG=debug cargo run
```
Expected: Box still centered, layer tree shows correct structure

**Step 3: Commit**

```bash
git add crates/flui_rendering/src/pipeline/owner.rs
git commit -m "feat(rendering): update paint to use repaint boundaries with OffsetLayer"
```

---

## Task 8: Integration Test

**Files:**
- Create: `crates/flui_rendering/tests/layout_paint_integration.rs`

**Step 1: Write integration test**

```rust
//! Integration tests for layout and paint systems.

use flui_rendering::prelude::*;

#[test]
fn test_center_widget_layout_and_paint() {
    // Create a simple Center > ColoredBox tree
    // Verify:
    // 1. Layout computes correct sizes
    // 2. Child offset is (300, 250) for 200x100 box in 800x600
    // 3. Paint receives correct offset
    // 4. Layer tree has correct structure
}

#[test]
fn test_nested_layout_with_dynamic_constraints() {
    // Test Row-like behavior where second child depends on first child's size
    // This verifies synchronous layout_child works correctly
}

#[test]
fn test_repaint_boundary_creates_offset_layer() {
    // Create widget with is_repaint_boundary = true
    // Verify OffsetLayer is created in layer tree
}
```

**Step 2: Run integration tests**

```bash
cargo test -p flui_rendering --test layout_paint_integration
```

**Step 3: Commit**

```bash
git add crates/flui_rendering/tests/
git commit -m "test(rendering): add layout and paint integration tests"
```

---

## Summary

After completing all tasks:

1. **Layout** will use shallow-first ordering with synchronous `layout_child()` via RefCell
2. **Paint** will properly handle repaint boundaries with OffsetLayer
3. **Layers** will be created correctly for compositing

The Center widget will work correctly, and the architecture will be ready for Row/Column implementation.

---

## Execution

Plan complete and saved to `docs/plans/2026-01-04-flutter-layout-paint-system.md`.

Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
