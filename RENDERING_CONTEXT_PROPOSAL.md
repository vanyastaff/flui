# Context-Based Layout API Proposal

## Current Problem: Callback-Based Layout (❌ Anti-pattern)

### Current Implementation (flui)

```rust
// ❌ UNSAFE callback pattern with raw pointers
unsafe {
    let self_ptr = self as *mut Self;

    let mut layout_child = |child_id: ElementId, child_constraints: BoxConstraints| {
        (*self_ptr).perform_layout(child_id, child_constraints)
    };

    render_object.perform_layout(id, constraints, &mut layout_child)?;
}
```

**Problems:**
1. ❌ Requires `unsafe` raw pointer
2. ❌ Callback closure captures environment
3. ❌ Complex lifetime management
4. ❌ Not idiomatic Rust
5. ❌ Hard to reason about

---

## Proposed Solution: Context-Based API (✅ Like Flutter)

### Flutter's Approach

```dart
class RenderBox {
  @override
  void performLayout() {
    // Access children through 'this' context
    final child = this.child;
    if (child != null) {
      // Layout child with direct method call
      child.layout(constraints, parentUsesSize: true);
      size = child.size;
    }
  }
}
```

### Rust Context-Based Design

```rust
/// Layout context provides safe access to tree operations
pub struct LayoutContext<'tree, P: Protocol> {
    /// Current element being laid out
    element_id: ElementId,

    /// Reference to element tree (immutable)
    tree: &'tree ElementTree,

    /// Constraints for this layout pass
    constraints: P::Constraints,

    /// Cache for layout results
    cache: &'tree mut LayoutCache,

    /// Protocol marker
    _phantom: PhantomData<P>,
}

impl<'tree, P: Protocol> LayoutContext<'tree, P> {
    /// Layout a child element
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: P::Constraints,
    ) -> Result<P::Geometry, LayoutError> {
        // Safe tree access through immutable reference
        let child = self.tree.get(child_id)?;

        // Recursive layout (no unsafe!)
        self.tree.layout_element(child_id, constraints, self.cache)
    }

    /// Get child's geometry after layout
    pub fn child_geometry(&self, child_id: ElementId) -> Option<&P::Geometry> {
        self.cache.get_geometry(child_id)
    }

    /// Access parent data
    pub fn parent_data<T: ParentData>(&self, child_id: ElementId) -> Option<&T> {
        self.tree.get(child_id)?
            .parent_data()
            .and_then(|pd| pd.downcast_ref::<T>())
    }
}
```

### RenderObject with Context

```rust
pub trait RenderObject: Send + Sync + 'static {
    /// Protocol this RenderObject uses
    type Protocol: RenderProtocol;

    /// Layout with context (NO callbacks!)
    fn layout(
        &mut self,
        ctx: &mut LayoutContext<'_, Self::Protocol>,
    ) -> Result<Self::Protocol::Geometry, LayoutError>;

    /// Paint with context
    fn paint(
        &self,
        ctx: &mut PaintContext<'_, Self::Protocol>,
    ) -> Result<(), PaintError>;

    /// Hit test with context
    fn hit_test(
        &self,
        ctx: &HitTestContext<'_, Self::Protocol>,
        position: Offset,
    ) -> bool;
}
```

### Example: RenderPadding Implementation

```rust
pub struct RenderPadding {
    padding: EdgeInsets,
    child_size: Size,
}

impl RenderObject for RenderPadding {
    type Protocol = BoxProtocol;

    fn layout(
        &mut self,
        ctx: &mut LayoutContext<'_, BoxProtocol>,
    ) -> Result<Size, LayoutError> {
        // Get child from context (safe!)
        let child_id = ctx.children().single()?;

        // Deflate constraints by padding
        let child_constraints = ctx.constraints()
            .deflate(self.padding);

        // Layout child through context (no callback!)
        let child_size = ctx.layout_child(child_id, child_constraints)?;

        // Store for painting
        self.child_size = child_size;

        // Return our size
        Ok(Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        ))
    }

    fn paint(
        &self,
        ctx: &mut PaintContext<'_, BoxProtocol>,
    ) -> Result<(), PaintError> {
        // Get child
        let child_id = ctx.children().single()?;

        // Paint child with offset
        let child_offset = Offset::new(
            self.padding.left,
            self.padding.top,
        );

        ctx.paint_child(child_id, child_offset)?;
        Ok(())
    }

    fn hit_test(
        &self,
        ctx: &HitTestContext<'_, BoxProtocol>,
        position: Offset,
    ) -> bool {
        // Check if inside padding
        let bounds = Rect::from_ltwh(
            self.padding.left,
            self.padding.top,
            self.child_size.width,
            self.child_size.height,
        );

        if !bounds.contains(position) {
            return false;
        }

        // Hit test child
        let child_id = ctx.children().single().unwrap();
        let child_position = position - Offset::new(
            self.padding.left,
            self.padding.top,
        );

        ctx.hit_test_child(child_id, child_position)
    }
}
```

---

## Context Design Patterns

### 1. Immutable Tree Reference Pattern

**Key insight:** Tree is immutable during layout, only state changes.

```rust
pub struct ElementTree {
    elements: Slab<Element>,
    // ... tree structure
}

impl ElementTree {
    /// Immutable access to element
    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(id.index())
    }

    /// Layout with mutable cache, immutable tree
    pub fn layout_element(
        &self,
        id: ElementId,
        constraints: impl Into<Constraints>,
        cache: &mut LayoutCache,
    ) -> Result<Geometry, LayoutError> {
        let element = self.get(id)?;
        let render_object = element.render_object();

        // Create context
        let mut ctx = LayoutContext {
            element_id: id,
            tree: self,  // Immutable borrow
            constraints: constraints.into(),
            cache,
            _phantom: PhantomData,
        };

        // Layout (no unsafe!)
        render_object.layout(&mut ctx)
    }
}
```

### 2. Cache-Based State Pattern

**Separate state storage from tree structure:**

```rust
/// Cache for layout results (mutable)
pub struct LayoutCache {
    geometries: HashMap<ElementId, Geometry>,
    offsets: HashMap<ElementId, Offset>,
}

impl LayoutCache {
    pub fn store_geometry(&mut self, id: ElementId, geometry: Geometry) {
        self.geometries.insert(id, geometry);
    }

    pub fn get_geometry(&self, id: ElementId) -> Option<&Geometry> {
        self.geometries.get(&id)
    }
}

/// Context borrows both tree and cache
pub struct LayoutContext<'tree, P: Protocol> {
    tree: &'tree ElementTree,        // Immutable
    cache: &'tree mut LayoutCache,  // Mutable
    // ...
}
```

### 3. Type-Safe Child Access Pattern

**Leverage Rust's type system for child access:**

```rust
/// Type-safe child accessor based on arity
pub trait ChildrenAccess<A: Arity> {
    fn children(&self) -> ChildrenView<'_, A>;
}

/// View over children with arity-specific methods
pub struct ChildrenView<'a, A: Arity> {
    element_id: ElementId,
    tree: &'a ElementTree,
    _arity: PhantomData<A>,
}

// Specialization for Single child
impl<'a> ChildrenView<'a, Single> {
    pub fn single(&self) -> Result<ElementId, LayoutError> {
        let element = self.tree.get(self.element_id)?;
        element.children().first().copied()
            .ok_or(LayoutError::NoChild)
    }

    pub fn get(&self) -> Result<ElementId, LayoutError> {
        self.single()
    }
}

// Specialization for Variable children
impl<'a> ChildrenView<'a, Variable> {
    pub fn iter(&self) -> impl Iterator<Item = ElementId> + 'a {
        let element = self.tree.get(self.element_id).unwrap();
        element.children().iter().copied()
    }

    pub fn count(&self) -> usize {
        let element = self.tree.get(self.element_id).unwrap();
        element.children().len()
    }

    pub fn get(&self, index: usize) -> Option<ElementId> {
        let element = self.tree.get(self.element_id).unwrap();
        element.children().get(index).copied()
    }
}

// Usage in RenderObject
impl RenderObject for RenderFlex {
    fn layout(&mut self, ctx: &mut LayoutContext<'_, BoxProtocol>) -> Result<Size, LayoutError> {
        // Type-safe iteration
        for child_id in ctx.children().iter() {
            let child_size = ctx.layout_child(child_id, child_constraints)?;
            // ...
        }
        Ok(total_size)
    }
}
```

---

## Benefits of Context-Based API

### Safety
✅ **No unsafe code** - all tree access through safe references
✅ **Borrow checker enforced** - immutable tree, mutable cache
✅ **No raw pointers** - proper Rust lifetimes

### Ergonomics
✅ **Clear API** - `ctx.layout_child(id, constraints)` vs callback
✅ **Better errors** - Result types, no panics
✅ **IDE support** - proper autocomplete, type inference

### Performance
✅ **Zero cost** - compiler optimizes away context wrapper
✅ **Cache locality** - cache is separate from tree structure
✅ **No allocations** - context is stack-allocated

### Maintainability
✅ **Easy to test** - mock context for unit tests
✅ **Clear contracts** - lifetimes document borrowing
✅ **Extensible** - add methods to context without changing RenderObject

---

## Implementation Strategy

### Phase 1: Context Types

```rust
// crates/flui_rendering/src/context/mod.rs
pub mod layout;
pub mod paint;
pub mod hit_test;

pub use layout::LayoutContext;
pub use paint::PaintContext;
pub use hit_test::HitTestContext;
```

### Phase 2: Trait Updates

```rust
// Update RenderObject trait
pub trait RenderObject: Send + Sync + 'static {
    type Protocol: RenderProtocol;

    // Old (deprecated)
    #[deprecated(since = "0.2.0", note = "Use layout with context")]
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: Constraints,
        layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
    ) -> Geometry { ... }

    // New (preferred)
    fn layout(
        &mut self,
        ctx: &mut LayoutContext<'_, Self::Protocol>,
    ) -> Result<Self::Protocol::Geometry, LayoutError> {
        // Default impl calls old method for backwards compat
        Ok(self.perform_layout(...))
    }
}
```

### Phase 3: Gradual Migration

**Week 1-2:**
- Implement context types
- Update core RenderObjects (Padding, Flex, Stack)
- Add tests

**Week 3-4:**
- Migrate all built-in RenderObjects
- Update documentation
- Performance benchmarks

**Week 5-6:**
- Deprecate callback-based API
- Community migration period
- Bug fixes

---

## Testing Strategy

### Unit Tests with Mock Context

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct MockLayoutContext {
        constraints: BoxConstraints,
        children: Vec<ElementId>,
        child_results: HashMap<ElementId, Size>,
    }

    impl MockLayoutContext {
        fn new(constraints: BoxConstraints) -> Self {
            Self {
                constraints,
                children: vec![],
                child_results: HashMap::new(),
            }
        }

        fn add_child(&mut self, id: ElementId, size: Size) {
            self.children.push(id);
            self.child_results.insert(id, size);
        }
    }

    #[test]
    fn test_padding_layout() {
        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        let mut ctx = MockLayoutContext::new(
            BoxConstraints::tight(Size::new(100.0, 100.0))
        );

        // Mock child
        let child_id = ElementId::new(1);
        ctx.add_child(child_id, Size::new(80.0, 80.0));

        // Layout
        let size = padding.layout(&mut ctx).unwrap();

        // Verify
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
```

---

## Migration Example

### Before (callback-based)

```rust
impl RenderObject for MyRender {
    fn perform_layout(
        &mut self,
        _id: ElementId,
        constraints: Constraints,
        layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
    ) -> Geometry {
        // Callback style
        let child_size = layout_child(child_id, child_constraints);
        // ...
    }
}
```

### After (context-based)

```rust
impl RenderObject for MyRender {
    fn layout(
        &mut self,
        ctx: &mut LayoutContext<'_, BoxProtocol>,
    ) -> Result<Size, LayoutError> {
        // Context style
        let child_id = ctx.children().single()?;
        let child_size = ctx.layout_child(child_id, child_constraints)?;
        // ...
        Ok(my_size)
    }
}
```

---

## Conclusion

Context-based API:
- ✅ **Eliminates unsafe code** in layout
- ✅ **Matches Flutter patterns** (familiar API)
- ✅ **Improves ergonomics** (clearer, safer)
- ✅ **Enables better testing** (mockable contexts)
- ✅ **More idiomatic Rust** (no callbacks, proper lifetimes)

This is the right architectural direction for flui_rendering!

Ready to implement? 🚀
