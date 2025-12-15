# Delegation Pattern

**Ambassador-based trait delegation for automatic method forwarding**

---

## Overview

FLUI uses the `ambassador` crate to delegate trait implementations to container fields. This eliminates boilerplate by automatically forwarding trait methods to the appropriate container, allowing render objects to focus on their specific behavior.

---

## Ambassador Basics

### Installation

```toml
[dependencies]
ambassador = "0.4"
```

### Macro Usage

```rust
use ambassador::{delegatable_trait, Delegate};

// 1. Mark trait as delegatable
#[delegatable_trait]
pub trait RenderProxyBox: SingleChildRenderBox {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Default implementation
    }
}

// 2. Use Delegate derive macro
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,  // This field provides RenderProxyBox methods
    opacity: f32,
}

// 3. Implement marker trait
impl RenderProxyBox for RenderOpacity {}

// ✅ All RenderProxyBox methods now automatically delegate to self.proxy
```

---

## How Delegation Works

### Macro Expansion

When you write:

```rust
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}
```

Ambassador generates:

```rust
impl RenderProxyBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.proxy.perform_layout(constraints)
    }
    
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.proxy.hit_test_children(result, position)
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        self.proxy.paint(context, offset)
    }
    
    // ... all other trait methods
}
```

### Selective Overriding

You can override specific methods while keeping delegation for others:

```rust
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

// Override specific methods
impl RenderBox for RenderOpacity {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.opacity == 0.0 {
            return;  // Custom behavior
        }
        
        if let Some(child) = self.proxy.child() {
            if self.opacity < 1.0 {
                context.push_opacity(self.opacity, |ctx| {
                    ctx.paint_child(child, offset)
                });
            } else {
                context.paint_child(child, offset)
            }
        }
    }
    
    fn always_needs_compositing(&self) -> bool {
        self.opacity > 0.0 && self.opacity < 1.0
    }
    
    // All other RenderBox methods still delegate to proxy
}
```

---

## Delegation Hierarchy

### Multiple Delegation Levels

Delegation works through multiple trait levels:

```
RenderOpacity
    ├── delegates RenderProxyBox to "proxy"
    │       ↓
    │   ProxyBox<BoxProtocol>
    │       ├── implements RenderProxyBox
    │       └── blanket impl → SingleChildRenderBox
    │                              ↓
    │                          blanket impl → RenderBox
    │                              ↓
    │                          blanket impl → RenderObject
    └── Result: RenderOpacity has ALL methods from ALL traits
```

### Delegation Chain Example

```rust
// Level 1: Delegate to container
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

// Level 2: ProxyBox implements RenderProxyBox
impl RenderProxyBox for ProxyBox {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Implementation
    }
}

// Level 3: Blanket impl provides SingleChildRenderBox
impl<T: RenderProxyBox> SingleChildRenderBox for T {
    fn child(&self) -> Option<&dyn RenderBox> {
        RenderProxyBox::child(self)
    }
}

// Level 4: Blanket impl provides RenderBox
impl<T: SingleChildRenderBox> RenderBox for T {
    // Delegates all RenderBox methods
}

// Level 5: Blanket impl provides RenderObject
impl<T: RenderBox> RenderObject for T {
    // Delegates all RenderObject methods
}

// Result: RenderOpacity.mark_needs_paint() works!
```

---

## Common Delegation Patterns

### Pattern 1: ProxyBox Delegation

Most common - size equals child size.

```rust
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

// Override paint only
impl RenderBox for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Custom paint implementation
    }
}

// Everything else delegates to proxy:
// ✅ perform_layout() → proxy
// ✅ hit_test_children() → proxy
// ✅ child() → proxy
// ✅ mark_needs_paint() → proxy → blanket → RenderObject
```

### Pattern 2: ShiftedBox Delegation

Custom child positioning.

```rust
#[derive(Debug, Delegate)]
#[delegate(SingleChildRenderBox, target = "shifted")]
pub struct RenderPadding {
    shifted: ShiftedBox,
    padding: EdgeInsets,
}

impl RenderShiftedBox for RenderPadding {
    fn child_offset(&self) -> Offset {
        *self.shifted.offset()
    }
}

impl RenderBox for RenderPadding {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Custom layout implementation
    }
    
    fn size(&self) -> Size {
        *self.shifted.geometry()
    }
}

// Delegates to shifted:
// ✅ child() → shifted
// ✅ hit_test_children() → shifted (uses child_offset)
// ✅ paint() → shifted (uses child_offset)
```

### Pattern 3: No Delegation (Multi-Child)

Multi-child objects implement traits manually.

```rust
#[derive(Debug)]
pub struct RenderFlex {
    children: BoxChildren<FlexParentData>,
    direction: Axis,
    _size: Size,
}

// No #[derive(Delegate)] - implement everything manually

impl MultiChildRenderBox for RenderFlex {
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
        self.children.iter()
    }
    
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox> {
        self.children.iter_mut()
    }
}

impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Full implementation
    }
    
    fn size(&self) -> Size {
        self._size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Full implementation
    }
}

// Still get RenderObject through blanket impl:
// ✅ mark_needs_paint() → blanket impl → RenderObject
```

---

## Delegatable Traits

All FLUI traits are marked with `#[delegatable_trait]`:

```rust
// Base trait
#[delegatable_trait]
pub trait RenderObject { }

// Box protocol traits
#[delegatable_trait]
pub trait RenderBox: RenderObject { }

#[delegatable_trait]
pub trait SingleChildRenderBox: RenderBox { }

#[delegatable_trait]
pub trait RenderProxyBox: SingleChildRenderBox { }

#[delegatable_trait]
pub trait RenderShiftedBox: SingleChildRenderBox { }

#[delegatable_trait]
pub trait RenderAligningShiftedBox: RenderShiftedBox { }

#[delegatable_trait]
pub trait MultiChildRenderBox: RenderBox { }

#[delegatable_trait]
pub trait HitTestProxy: RenderProxyBox { }

#[delegatable_trait]
pub trait ClipProxy<T>: RenderProxyBox { }

#[delegatable_trait]
pub trait PhysicalModelProxy: ClipProxy<RRect> { }

// Sliver protocol traits
#[delegatable_trait]
pub trait RenderSliver: RenderObject { }

#[delegatable_trait]
pub trait RenderProxySliver: RenderSliver { }

#[delegatable_trait]
pub trait RenderSliverSingleBoxAdapter: RenderSliver { }

#[delegatable_trait]
pub trait RenderSliverMultiBoxAdaptor: RenderSliver { }

#[delegatable_trait]
pub trait RenderSliverPersistentHeader: RenderSliver { }

// Total: 14 delegatable traits
```

---

## Benefits

### 1. Minimal Boilerplate

**Without delegation:**
```rust
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.proxy.child()
    }
    
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.proxy.child_mut()
    }
    
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.proxy.perform_layout(constraints)
    }
    
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.proxy.hit_test_children(result, position)
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.proxy.paint(ctx, offset)
    }
    
    // ... 10+ more methods
}

impl RenderBox for RenderOpacity {
    fn size(&self) -> Size {
        self.proxy.size()
    }
    
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.proxy.compute_min_intrinsic_width(height)
    }
    
    // ... 15+ more methods
}

impl RenderObject for RenderOpacity {
    fn mark_needs_layout(&mut self) {
        self.proxy.mark_needs_layout()
    }
    
    // ... 20+ more methods
}

// Total: ~50-70 lines of repetitive forwarding
```

**With delegation:**
```rust
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

impl RenderBox for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Only override what's different
    }
}

// Total: ~15 lines
```

**Reduction: 70% less code**

### 2. Type Safety

Delegation is checked at compile time:

```rust
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "wrong_field")]  // ❌ Compile error
pub struct RenderOpacity {
    proxy: ProxyBox,  // Field must exist
    opacity: f32,
}
```

### 3. Easy Refactoring

Change trait signatures in one place:

```rust
#[delegatable_trait]
pub trait RenderProxyBox {
    fn new_method(&self) -> i32 {
        42
    }
}

// ✅ All delegating structs automatically get new_method()
// No need to update 35+ render objects manually
```

---

## Delegation Anti-Patterns

### ❌ Don't: Delegate to wrong container type

```rust
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "children")]  // ❌ Wrong!
pub struct RenderOpacity {
    children: BoxChildren<BoxParentData>,  // Multi-child container
    opacity: f32,
}
```

**Problem:** `BoxChildren` doesn't implement `RenderProxyBox`

### ❌ Don't: Mix delegation styles

```rust
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

// ❌ Don't manually implement delegated trait
impl RenderProxyBox for RenderOpacity {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.proxy.child()  // Redundant - already delegated!
    }
}
```

### ✅ Do: Override only specific methods

```rust
#[derive(Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}  // Marker impl only

// Override at lower trait level
impl RenderBox for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Custom implementation
    }
    
    // All other RenderBox methods still delegate
}
```

---

## Debugging Delegation

### Check what gets delegated

Use `cargo expand` to see generated code:

```bash
cargo install cargo-expand
cargo expand objects::box::effects::opacity
```

Output shows all generated delegation methods:

```rust
impl RenderProxyBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        RenderProxyBox::perform_layout(&mut self.proxy, constraints)
    }
    
    // ... all methods expanded
}
```

### Common compilation errors

**Error: trait bound not satisfied**
```
error[E0277]: the trait bound `BoxChildren: RenderProxyBox` is not satisfied
```

**Solution:** Check container type matches delegated trait

**Error: multiple applicable items in scope**
```
error[E0034]: multiple applicable items in scope for `child`
```

**Solution:** Remove manual trait implementation, let delegation handle it

---

## File Organization

```
flui-rendering/src/
├── traits/
│   ├── render_object.rs      #[delegatable_trait]
│   ├── box/
│   │   ├── render_box.rs     #[delegatable_trait]
│   │   ├── proxy_box.rs      #[delegatable_trait]
│   │   └── ...
│   └── sliver/
│       ├── render_sliver.rs  #[delegatable_trait]
│       └── ...
│
└── objects/
    └── box/
        └── effects/
            └── opacity.rs     #[derive(Delegate)]
```

---

## Summary

| Aspect | Details |
|--------|---------|
| **Macro** | `#[derive(Delegate)]` + `#[delegate(Trait, target = "field")]` |
| **Requirement** | Trait must be marked `#[delegatable_trait]` |
| **Benefits** | 70% less boilerplate, type-safe, easy refactoring |
| **Pattern** | Delegate to container field, override specific methods |
| **Delegatable Traits** | 14 (all FLUI traits) |
| **Typical Usage** | Single-child objects (Proxy, Shifted, Aligning) |

---

## Next Steps

- [[Trait Hierarchy]] - All delegatable traits
- [[Containers]] - What containers to delegate to
- [[Implementation Guide]] - Using delegation in practice

---

**See Also:**
- [[Object Catalog]] - Examples of delegation usage
- Ambassador crate: https://github.com/hobofan/ambassador
