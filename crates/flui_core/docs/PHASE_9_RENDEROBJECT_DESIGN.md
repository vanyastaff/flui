# Phase 9: RenderObject Enhancement - Design Document

**Date:** 2025-10-20
**Status:** Planning

---

## Overview

Phase 9 enhances the RenderObject trait with Flutter's complete lifecycle system:
1. **Layout tracking** - Mark dirty, propagate layout requests
2. **Paint tracking** - Mark dirty, propagate paint requests
3. **Compositing** - Layer tree management
4. **ParentData lifecycle** - Setup, adopt/drop children
5. **Attach/Detach** - Connect/disconnect from pipeline
6. **Depth tracking** - Tree depth management

These features enable efficient incremental layout and paint.

---

## Flutter's RenderObject Lifecycle

```dart
abstract class RenderObject {
  // Dirty flags
  bool _needsLayout = true;
  bool _needsPaint = true;
  bool _needsCompositingBitsUpdate = false;

  // Tree structure
  int _depth = 0;
  PipelineOwner? _owner;
  ParentData? parentData;

  // Mark dirty methods
  void markNeedsLayout() {
    if (_needsLayout) return;
    _needsLayout = true;
    if (_relayoutBoundary != this) {
      parent?.markNeedsLayout();  // Propagate up
    } else {
      _owner?.requestLayout(this);  // Add to dirty list
    }
  }

  void markNeedsPaint() {
    if (_needsPaint) return;
    _needsPaint = true;
    if (isRepaintBoundary) {
      _owner?.requestPaint(this);
    } else {
      parent?.markNeedsPaint();  // Propagate up
    }
  }

  // Lifecycle
  void attach(PipelineOwner owner) {
    _owner = owner;
    visitChildren((child) => child.attach(owner));
  }

  void detach() {
    _owner = null;
    visitChildren((child) => child.detach());
  }

  // Parent data
  void setupParentData(RenderObject child) {
    if (child.parentData is! ExpectedParentData) {
      child.parentData = createParentData();
    }
  }

  void adoptChild(RenderObject child) {
    setupParentData(child);
    markNeedsLayout();
    markNeedsCompositingBitsUpdate();
  }

  void dropChild(RenderObject child) {
    child.parentData?.detach();
    child.parentData = null;
    markNeedsLayout();
  }

  // Depth management
  void redepthChild(RenderObject child) {
    if (child._depth <= _depth) {
      child._depth = _depth + 1;
      child.redepthChildren();
    }
  }
}
```

### Key Concepts

1. **Dirty flags:** `needs_layout`, `needs_paint`, `needs_compositing_bits_update`
2. **Propagation:** Dirty marks propagate up tree until boundary
3. **Boundaries:** Relayout boundaries and repaint boundaries isolate changes
4. **Attach/Detach:** Connect/disconnect from PipelineOwner
5. **Depth:** Tree depth for correct traversal order

---

## Current State in Flui

```rust
pub trait RenderObject: AnyRenderObject + Sized {
    type ParentData: ParentData;
    type Child: Sized;

    fn parent_data(&self) -> Option<&Self::ParentData>;
    fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData>;
}
```

**Missing:**
- ❌ Dirty flags (`needs_layout`, `needs_paint`)
- ❌ `mark_needs_layout()` / `mark_needs_paint()`
- ❌ `attach()` / `detach()` lifecycle
- ❌ `setup_parent_data()` / `adopt_child()` / `drop_child()`
- ❌ Depth tracking and `redepth_child()`
- ❌ PipelineOwner integration

---

## Design for Rust Implementation

### 1. Add Dirty Tracking to RenderObject

```rust
pub trait RenderObject: AnyRenderObject + Sized {
    type ParentData: ParentData;
    type Child: Sized;

    // Existing methods
    fn parent_data(&self) -> Option<&Self::ParentData>;
    fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData>;

    // NEW: Dirty flags
    fn needs_layout(&self) -> bool;
    fn needs_paint(&self) -> bool;
    fn needs_compositing_bits_update(&self) -> bool;

    // NEW: Mark dirty methods
    fn mark_needs_layout(&mut self);
    fn mark_needs_paint(&mut self);
    fn mark_needs_compositing_bits_update(&mut self);

    // NEW: Boundaries (optional, defaults to false)
    fn sizedByParent(&self) -> bool { false }
    fn is_repaint_boundary(&self) -> bool { false }
}
```

### 2. Add Lifecycle Methods

```rust
pub trait RenderObject: AnyRenderObject + Sized {
    // ... existing methods ...

    // NEW: Attach/Detach
    fn attach(&mut self, owner: &mut PipelineOwner) {
        // Default: do nothing
        // Subclasses override to attach children
    }

    fn detach(&mut self) {
        // Default: do nothing
        // Subclasses override to detach children
    }

    // NEW: Depth tracking
    fn depth(&self) -> usize;
    fn set_depth(&mut self, depth: usize);
    fn redepth_child(&mut self, child: &mut dyn AnyRenderObject);
}
```

### 3. Add ParentData Lifecycle

```rust
pub trait RenderObject: AnyRenderObject + Sized {
    // ... existing methods ...

    // NEW: ParentData setup
    fn setup_parent_data(&mut self, child: &mut dyn AnyRenderObject) {
        // Default: ensure child has correct parent data type
    }

    // NEW: Child adoption
    fn adopt_child(&mut self, child: &mut dyn AnyRenderObject) {
        self.setup_parent_data(child);
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
    }

    fn drop_child(&mut self, child: &mut dyn AnyRenderObject) {
        // Clear child's parent data
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
    }
}
```

### 4. Update PipelineOwner

```rust
pub struct PipelineOwner {
    // Existing fields...

    // NEW: Dirty tracking
    nodes_needing_layout: Vec<ElementId>,
    nodes_needing_paint: Vec<ElementId>,
    nodes_needing_compositing_bits_update: Vec<ElementId>,
}

impl PipelineOwner {
    // NEW: Request layout
    pub fn request_layout(&mut self, node: ElementId) {
        if !self.nodes_needing_layout.contains(&node) {
            self.nodes_needing_layout.push(node);
        }
    }

    // NEW: Request paint
    pub fn request_paint(&mut self, node: ElementId) {
        if !self.nodes_needing_paint.contains(&node) {
            self.nodes_needing_paint.push(node);
        }
    }

    // NEW: Flush layout
    pub fn flush_layout(&mut self) {
        // Sort by depth (parents before children)
        self.nodes_needing_layout.sort_by_key(|&id| {
            // Get depth from element
            0 // Placeholder
        });

        for node_id in std::mem::take(&mut self.nodes_needing_layout) {
            // Perform layout
        }
    }

    // NEW: Flush paint
    pub fn flush_paint(&mut self) {
        for node_id in std::mem::take(&mut self.nodes_needing_paint) {
            // Perform paint
        }
    }
}
```

---

## Implementation Plan

### Step 1: Add Dirty Flags to RenderObject Trait

1. Add `needs_layout()`, `needs_paint()`, `needs_compositing_bits_update()` getters
2. Add `mark_needs_layout()`, `mark_needs_paint()`, `mark_needs_compositing_bits_update()` setters
3. Add boundary methods: `sizedByParent()`, `is_repaint_boundary()`

### Step 2: Add Lifecycle Methods

1. Add `attach(&mut self, owner)` and `detach(&mut self)`
2. Add `depth()`, `set_depth()`, `redepth_child()`
3. Add `visit_children()` for tree traversal

### Step 3: Add ParentData Lifecycle

1. Add `setup_parent_data(child)`
2. Add `adopt_child(child)` - sets up parent data, marks dirty
3. Add `drop_child(child)` - clears parent data, marks dirty

### Step 4: Enhance PipelineOwner

1. Add `nodes_needing_layout`, `nodes_needing_paint` vectors
2. Add `request_layout()`, `request_paint()` methods
3. Add `flush_layout()`, `flush_paint()` methods
4. Sort by depth before processing

### Step 5: Update Existing RenderObject Implementations

Update implementations in:
- `render/widget.rs` (test implementations)
- Any concrete RenderObject types

### Step 6: Testing

1. Unit tests for dirty flag propagation
2. Unit tests for attach/detach
3. Unit tests for adopt_child/drop_child
4. Integration tests with PipelineOwner
5. Performance tests (1000+ nodes)

---

## Example: Enhanced RenderObject

```rust
pub struct RenderBox {
    // Layout
    size: Size,
    constraints: BoxConstraints,
    needs_layout_flag: bool,
    needs_paint_flag: bool,
    needs_compositing_bits_update_flag: bool,

    // Tree structure
    parent_data: Option<BoxParentData>,
    depth: usize,
    owner: Option<Arc<RwLock<PipelineOwner>>>,

    // Children (for multi-child)
    children: Vec<Box<dyn AnyRenderObject>>,
}

impl RenderObject for RenderBox {
    type ParentData = BoxParentData;
    type Child = Box<dyn AnyRenderObject>;

    fn parent_data(&self) -> Option<&Self::ParentData> {
        self.parent_data.as_ref()
    }

    fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData> {
        self.parent_data.as_mut()
    }

    // Dirty flags
    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn needs_compositing_bits_update(&self) -> bool {
        self.needs_compositing_bits_update_flag
    }

    // Mark dirty
    fn mark_needs_layout(&mut self) {
        if self.needs_layout_flag {
            return; // Already dirty
        }

        self.needs_layout_flag = true;

        // If we have an owner, request layout
        if let Some(owner) = &self.owner {
            owner.write().request_layout(self.id());
        }
    }

    fn mark_needs_paint(&mut self) {
        if self.needs_paint_flag {
            return;
        }

        self.needs_paint_flag = true;

        if let Some(owner) = &self.owner {
            if self.is_repaint_boundary() {
                owner.write().request_paint(self.id());
            } else {
                // Propagate to parent
                // (would need parent reference)
            }
        }
    }

    // Lifecycle
    fn attach(&mut self, owner: &mut PipelineOwner) {
        self.owner = Some(owner.clone());

        // Attach all children
        for child in &mut self.children {
            child.attach(owner);
        }
    }

    fn detach(&mut self) {
        self.owner = None;

        // Detach all children
        for child in &mut self.children {
            child.detach();
        }
    }

    // Depth
    fn depth(&self) -> usize {
        self.depth
    }

    fn set_depth(&mut self, depth: usize) {
        if self.depth != depth {
            self.depth = depth;

            // Update children
            for child in &mut self.children {
                self.redepth_child(child.as_mut());
            }
        }
    }

    fn redepth_child(&mut self, child: &mut dyn AnyRenderObject) {
        if child.depth() <= self.depth {
            child.set_depth(self.depth + 1);
        }
    }

    // ParentData
    fn adopt_child(&mut self, child: &mut dyn AnyRenderObject) {
        self.setup_parent_data(child);
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
    }

    fn drop_child(&mut self, child: &mut dyn AnyRenderObject) {
        // Clear parent data
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
    }
}
```

---

## Challenges and Solutions

### Challenge 1: Propagating dirty marks up the tree

**Problem:** `mark_needs_layout()` needs to propagate to parent, but we don't have parent reference.

**Solutions:**
1. **Store parent ID** and pass tree/owner for lookup
2. **Use PipelineOwner** to track relationships
3. **Propagate via ElementTree** (elements know parents)

**Chosen:** Use PipelineOwner + ElementTree integration.

### Challenge 2: Borrow checker with owner

**Problem:** RenderObject needs mutable access to PipelineOwner while owner holds references to RenderObjects.

**Solution:** Use `Arc<RwLock<PipelineOwner>>` and acquire locks only when needed.

### Challenge 3: Depth updates across tree

**Problem:** Changing depth requires traversing entire subtree.

**Solution:** Lazy depth updates - only update when accessed or when performing operations that require correct depth.

---

## Benefits

1. **Efficient incremental layout:** Only layout dirty nodes
2. **Efficient incremental paint:** Only paint dirty regions
3. **Boundary optimization:** Isolate changes to subtrees
4. **Depth-ordered processing:** Parents before children
5. **Lifecycle management:** Proper attach/detach of subtrees

---

## Performance

### Layout Phase
- **Before:** O(n) - layout entire tree
- **After:** O(k) where k = dirty nodes
- **Typical:** 1-10 dirty nodes out of 1000+ total

### Paint Phase
- **Before:** O(n) - paint entire tree
- **After:** O(k) where k = dirty nodes + boundaries
- **Typical:** 1-5 paint regions out of 1000+ nodes

### Expected Improvement
- **90-99% reduction** in layout/paint work for typical updates
- **Enables 60fps** even with complex UIs

---

## Next Steps

1. Design and implement dirty flag tracking
2. Implement mark_needs_layout() with propagation
3. Implement mark_needs_paint() with boundaries
4. Add attach/detach lifecycle
5. Add ParentData lifecycle (adopt_child, drop_child)
6. Enhance PipelineOwner with dirty lists
7. Add flush_layout() and flush_paint()
8. Write comprehensive tests
9. Performance benchmarks

**Estimated time:** 4-5 hours
