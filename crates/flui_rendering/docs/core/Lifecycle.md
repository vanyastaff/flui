# Lifecycle

**Render object lifecycle states and transitions**

---

## Overview

`RenderLifecycle` is a single enum that replaces multiple boolean flags from Flutter (`_needsLayout`, `_needsPaint`, etc.). It provides compile-time guarantees about valid state transitions and reduces memory usage.

---

## Enum Definition

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RenderLifecycle {
    /// Not attached to pipeline
    ///
    /// Flutter: `owner == null`
    Detached = 0,
    
    /// Attached to pipeline but not yet laid out
    ///
    /// Flutter: `owner != null` (initial state after attach)
    Attached = 1,
    
    /// Needs layout
    ///
    /// Flutter: `_needsLayout == true`
    NeedsLayout = 2,
    
    /// Layout complete, ready for paint
    ///
    /// Flutter: `_needsLayout == false`
    LaidOut = 3,
    
    /// Needs paint
    ///
    /// Flutter: `_needsPaint == true`
    NeedsPaint = 4,
    
    /// Paint complete
    ///
    /// Flutter: `_needsPaint == false`
    Painted = 5,
    
    /// Resource has been disposed (terminal state)
    ///
    /// Flutter: `dispose()` called, `_debugDisposed = true`
    Disposed = 6,
}
```

**Size:** 1 byte (`#[repr(u8)]`)

**Memory savings vs Flutter:**
- Flutter: 3-4 bools = 3-4 bytes + padding = 4-8 bytes
- FLUI: 1 byte enum

---

## State Transitions

### Valid Transitions

```
                    ┌──────────────┐
                    │   Detached   │ (initial)
                    └──────┬───────┘
                           │ attach()
                           ▼
                    ┌──────────────┐
                    │   Attached   │
                    └──────┬───────┘
                           │ mark_needs_layout()
                           ▼
        ┌──────────┬───────────────┐
        │          │  NeedsLayout  │
        │          └───────┬───────┘
        │                  │ perform_layout()
        │                  ▼
        │          ┌───────────────┐
        │          │    LaidOut    │◀─────────┐
        │          └───────┬───────┘          │
        │                  │ mark_needs_paint()│
        │                  ▼                   │
        │          ┌───────────────┐          │
        │          │  NeedsPaint   │          │
        │          └───────┬───────┘          │
        │                  │ paint()          │
        │                  ▼                  │
        │          ┌───────────────┐          │
        │          │    Painted    │          │
        │          └───────┬───────┘          │
        │                  │                  │
        │                  └──────────────────┘
        │                     mark_needs_layout()
        │
        └─────────────────────────────────────▶ Disposed
                           (terminal)
```

### Transition Rules

```rust
impl RenderLifecycle {
    /// Check if transition is valid
    pub fn can_transition_to(self, next: Self) -> bool {
        use RenderLifecycle::*;
        matches!(
            (self, next),
            // From Detached
            (Detached, Attached)
            | (Detached, Disposed)
            
            // From Attached
            | (Attached, NeedsLayout)
            | (Attached, Disposed)
            
            // From NeedsLayout
            | (NeedsLayout, LaidOut)
            | (NeedsLayout, Disposed)
            
            // From LaidOut
            | (LaidOut, NeedsPaint)
            | (LaidOut, NeedsLayout)  // Relayout
            | (LaidOut, Disposed)
            
            // From NeedsPaint
            | (NeedsPaint, Painted)
            | (NeedsPaint, NeedsLayout)  // Relayout during paint
            | (NeedsPaint, Disposed)
            
            // From Painted
            | (Painted, NeedsLayout)  // Relayout
            | (Painted, NeedsPaint)   // Repaint
            | (Painted, Disposed)
        )
    }
}
```

---

## Helper Methods

```rust
impl RenderLifecycle {
    /// Can perform layout in this state?
    pub fn can_layout(self) -> bool {
        matches!(self, Self::Attached | Self::NeedsLayout)
    }
    
    /// Can perform paint in this state?
    pub fn can_paint(self) -> bool {
        matches!(self, Self::LaidOut | Self::NeedsPaint)
    }
    
    /// Is node attached to pipeline?
    pub fn is_attached(self) -> bool {
        matches!(
            self,
            Self::Attached
                | Self::NeedsLayout
                | Self::LaidOut
                | Self::NeedsPaint
                | Self::Painted
        )
    }
    
    /// Is node usable?
    pub fn is_usable(self) -> bool {
        !matches!(self, Self::Detached | Self::Disposed)
    }
    
    /// Needs layout?
    pub fn needs_layout(self) -> bool {
        matches!(self, Self::Attached | Self::NeedsLayout)
    }
    
    /// Needs paint?
    pub fn needs_paint(self) -> bool {
        matches!(self, Self::NeedsPaint)
    }
}
```

---

## Usage in RenderNode

```rust
pub struct RenderNode {
    render_object: Box<dyn RenderObject>,
    
    // Single byte instead of multiple bools
    lifecycle: RenderLifecycle,
    
    // Other fields...
    parent: Option<RenderId>,
    depth: usize,
    // ...
}

impl RenderNode {
    pub fn new(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object,
            lifecycle: RenderLifecycle::Detached,  // Initial state
            // ...
        }
    }
}
```

---

## Integration with Pipeline

### mark_needs_layout()

```rust
impl RenderTree {
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        let node = &mut self.nodes[id];
        
        // Check if already marked
        if node.lifecycle == RenderLifecycle::NeedsLayout {
            return;
        }
        
        // Validate transition
        debug_assert!(
            node.lifecycle.can_transition_to(RenderLifecycle::NeedsLayout),
            "Invalid transition: {:?} -> NeedsLayout",
            node.lifecycle
        );
        
        // Update lifecycle
        node.lifecycle = RenderLifecycle::NeedsLayout;
        
        // Add to dirty list or propagate
        if node.relayout_boundary == Some(true) {
            self.nodes_needing_layout.push(id);
        } else if let Some(parent_id) = node.parent {
            self.mark_needs_layout(parent_id);
        }
    }
}
```

### flush_layout()

```rust
impl RenderTree {
    pub fn flush_layout(&mut self) {
        while !self.nodes_needing_layout.is_empty() {
            let mut dirty = std::mem::take(&mut self.nodes_needing_layout);
            dirty.sort_by_key(|&id| self.nodes[id].depth);
            
            for id in dirty {
                let node = &self.nodes[id];
                
                // Only layout if still needs it
                if node.lifecycle != RenderLifecycle::NeedsLayout {
                    continue;
                }
                
                self.layout_node(id);
            }
        }
    }
    
    fn layout_node(&mut self, id: RenderId) {
        let node = &mut self.nodes[id];
        
        debug_assert_eq!(
            node.lifecycle,
            RenderLifecycle::NeedsLayout,
            "Cannot layout node in state: {:?}",
            node.lifecycle
        );
        
        // Perform layout
        let constraints = node.constraints.expect("No constraints");
        node.render_object.perform_layout(constraints);
        
        // Update lifecycle
        node.lifecycle = RenderLifecycle::LaidOut;
        
        // Mark needs paint
        self.mark_needs_paint(id);
    }
}
```

### mark_needs_paint()

```rust
impl RenderTree {
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        let node = &mut self.nodes[id];
        
        // Check if already marked
        if node.lifecycle == RenderLifecycle::NeedsPaint {
            return;
        }
        
        // Must be laid out first
        debug_assert!(
            matches!(node.lifecycle, RenderLifecycle::LaidOut | RenderLifecycle::Painted),
            "Cannot paint before layout: {:?}",
            node.lifecycle
        );
        
        // Update lifecycle
        node.lifecycle = RenderLifecycle::NeedsPaint;
        
        // Add to dirty list
        self.nodes_needing_paint.push(id);
    }
}
```

---

## Attach/Detach

### attach()

```rust
impl RenderTree {
    pub fn attach(&mut self, id: RenderId) {
        let node = &mut self.nodes[id];
        
        debug_assert_eq!(
            node.lifecycle,
            RenderLifecycle::Detached,
            "Can only attach detached nodes"
        );
        
        // Update lifecycle
        node.lifecycle = RenderLifecycle::Attached;
        
        // Mark needs layout
        self.mark_needs_layout(id);
        
        // Attach children
        let children = node.children.clone();
        for child_id in children {
            self.attach(child_id);
        }
    }
}
```

### detach()

```rust
impl RenderTree {
    pub fn detach(&mut self, id: RenderId) {
        let node = &self.nodes[id];
        
        debug_assert!(
            node.lifecycle.is_attached(),
            "Node is not attached: {:?}",
            node.lifecycle
        );
        
        // Detach children first
        let children = node.children.clone();
        for child_id in children {
            self.detach(child_id);
        }
        
        // Update lifecycle
        self.nodes[id].lifecycle = RenderLifecycle::Detached;
        
        // Remove from dirty lists
        self.nodes_needing_layout.retain(|&nid| nid != id);
        self.nodes_needing_paint.retain(|&nid| nid != id);
    }
}
```

---

## Dispose Pattern

### Terminal State

```rust
impl RenderTree {
    pub fn dispose_node(&mut self, id: RenderId) {
        let node = &mut self.nodes[id];
        
        // Prevent double-dispose
        assert!(
            node.lifecycle != RenderLifecycle::Disposed,
            "Node already disposed"
        );
        
        // Detach if attached
        if node.lifecycle.is_attached() {
            self.detach(id);
        }
        
        // Mark as disposed (terminal state)
        node.lifecycle = RenderLifecycle::Disposed;
        
        // Cleanup resources
        node.render_object.dispose();
    }
}

impl Drop for RenderNode {
    fn drop(&mut self) {
        // Ensure disposed before drop
        if self.lifecycle != RenderLifecycle::Disposed {
            self.render_object.dispose();
        }
    }
}
```

---

## Debug Assertions

### Development Checks

```rust
impl RenderTree {
    pub fn perform_layout(&mut self, id: RenderId, constraints: RenderConstraints) {
        let node = &self.nodes[id];
        
        // Validate state
        debug_assert!(
            node.lifecycle.can_layout(),
            "Cannot layout in state: {:?}",
            node.lifecycle
        );
        
        debug_assert!(
            node.lifecycle != RenderLifecycle::Disposed,
            "Cannot layout disposed node"
        );
        
        // Perform layout...
    }
    
    pub fn paint(&self, id: RenderId, ctx: &mut PaintingContext, offset: Offset) {
        let node = &self.nodes[id];
        
        // Validate state
        debug_assert!(
            node.lifecycle.can_paint(),
            "Cannot paint in state: {:?}. Must layout first.",
            node.lifecycle
        );
        
        debug_assert!(
            node.lifecycle != RenderLifecycle::Disposed,
            "Cannot paint disposed node"
        );
        
        // Paint...
    }
}
```

---

## Flutter Comparison

| Flutter | FLUI | Notes |
|---------|------|-------|
| `owner == null` | `Detached` | Not in tree |
| `owner != null` | `Attached` | In tree |
| `_needsLayout == true` | `NeedsLayout` | Dirty layout |
| `_needsLayout == false` | `LaidOut` | Clean layout |
| `_needsPaint == true` | `NeedsPaint` | Dirty paint |
| `_needsPaint == false` | `Painted` | Clean paint |
| `_debugDisposed == true` | `Disposed` | Terminal state |

### Flutter Flags (Multiple Bools)

```dart
class RenderObject {
  bool _needsLayout = true;     // 1 byte
  bool _needsPaint = true;      // 1 byte
  bool _needsCompositingBitsUpdate = false;  // separate
  bool _needsSemanticsUpdate = true;         // separate
  // Total: 2-4 bytes for lifecycle alone
}
```

### FLUI Lifecycle (Single Enum)

```rust
pub struct RenderNode {
    lifecycle: RenderLifecycle,  // 1 byte (includes layout + paint state)
    dirty_flags: DirtyFlags,     // 1 byte (compositing + semantics)
    // Total: 2 bytes for all states
}

bitflags! {
    pub struct DirtyFlags: u8 {
        const NEEDS_COMPOSITING_BITS = 1 << 0;
        const NEEDS_SEMANTICS = 1 << 1;
    }
}
```

---

## Benefits

### 1. Type Safety

```rust
// ✅ Compile-time checks
fn paint(node: &RenderNode) {
    assert!(node.lifecycle.can_paint());  // Runtime check
}

// ❌ Flutter: can call paint() anytime (runtime crash)
```

### 2. Memory Efficiency

**Per node savings:**
- Flutter: 2-4 bytes (multiple bools)
- FLUI: 1 byte (enum)
- **Savings: 1-3 bytes per node**

**For 10,000 nodes:**
- Flutter: 20-40 KB
- FLUI: 10 KB
- **Total savings: 10-30 KB**

### 3. Clear State Machine

```rust
// ✅ Explicit state transitions
node.lifecycle = RenderLifecycle::NeedsLayout;

// vs Flutter's implicit state (hard to track)
node._needsLayout = true;
node._needsPaint = false;
// What's the actual state?
```

### 4. Better Debug Messages

```rust
panic!("Cannot paint in state: {:?}", node.lifecycle);
// Output: "Cannot paint in state: NeedsLayout"

// vs Flutter:
panic!("Cannot paint: _needsLayout={} _needsPaint={}", 
       node._needsLayout, node._needsPaint);
// Output: "Cannot paint: _needsLayout=true _needsPaint=false"
// Less clear!
```

---

## Pattern Matching

```rust
// Rust-idiomatic state handling
match node.lifecycle {
    RenderLifecycle::Detached => {
        // Can't do anything
    }
    RenderLifecycle::Attached | RenderLifecycle::NeedsLayout => {
        // Can layout
        self.layout_node(id);
    }
    RenderLifecycle::LaidOut | RenderLifecycle::NeedsPaint => {
        // Can paint
        self.paint_node(id);
    }
    RenderLifecycle::Painted => {
        // Ready to composite
    }
    RenderLifecycle::Disposed => {
        panic!("Node disposed!");
    }
}
```

---

## Summary

| Aspect | Details |
|--------|---------|
| **Size** | 1 byte (u8) |
| **States** | 7 (Detached → Disposed) |
| **Memory Savings** | 1-3 bytes per node vs Flutter |
| **Type Safety** | Compile-time state validation |
| **Clarity** | Single source of truth |
| **Debug** | Clear state in error messages |

---

## Next Steps

- [[Render Tree]] - How lifecycle integrates with tree
- [[Pipeline]] - How lifecycle drives frame production
- [[Implementation Guide]] - Using lifecycle in objects

---

**See Also:**
- [[Protocol]] - Type system foundation
- [[Trait Hierarchy]] - Where lifecycle is checked
