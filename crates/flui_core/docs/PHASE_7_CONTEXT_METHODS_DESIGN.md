# Phase 7: Enhanced Context Methods - Design Document

**Date:** 2025-10-20
**Status:** 🚧 In Progress
**Priority:** MEDIUM
**Complexity:** LOW-MEDIUM

---

## Overview

This phase adds comprehensive tree navigation and query methods to `Context`, matching Flutter's BuildContext API. These methods enable widgets to find ancestors, query render objects, check mounting status, and traverse the element tree.

### Current State

✅ **Already Implemented:**
- Basic ancestor iteration (`context.ancestors()`)
- Child iteration (`context.children()`)
- Descendant iteration (`context.descendants()`)
- InheritedWidget access (Phase 6)
- Basic tree navigation

❌ **Missing:**
- Type-safe ancestor finding
- State object access
- RenderObject queries
- Size/layout queries
- Mounting status checks
- Child element visitation

### Goals

1. **Type-Safe Navigation**: Find specific widget types in the tree
2. **State Access**: Access StatefulWidget state from context
3. **RenderObject Queries**: Get render objects for layout info
4. **Rust-Idiomatic API**: Short, ergonomic method names
5. **Zero Breaking Changes**: Maintain backward compatibility

---

## Architecture

### 1. Tree Navigation Methods

```rust
impl Context {
    // ========== Widget Finding ==========

    /// Find nearest ancestor widget of exact type T
    pub fn find_ancestor_widget<T: Widget + 'static>(&self) -> Option<T>;

    /// Find nearest ancestor StatefulWidget's state
    pub fn find_ancestor_state<S: State + 'static>(&self) -> Option<&S>;

    /// Find root (topmost) ancestor StatefulWidget's state
    pub fn find_root_ancestor_state<S: State + 'static>(&self) -> Option<&S>;

    /// Find nearest ancestor RenderObject of type T
    pub fn find_ancestor_render_object<T: RenderObject + 'static>(&self) -> Option<&T>;

    // ========== Child Visitation ==========

    /// Visit each child element
    pub fn visit_child_elements<F>(&self, visitor: F)
    where
        F: FnMut(&dyn AnyElement);

    /// Visit children - short form
    pub fn visit_children<F>(&self, visitor: F)
    where
        F: FnMut(&dyn AnyElement);
}
```

### 2. Layout & Rendering Queries

```rust
impl Context {
    // ========== RenderObject Access ==========

    /// Get this element's RenderObject
    pub fn find_render_object(&self) -> Option<&dyn AnyRenderObject>;

    /// Get this element's RenderObject - short form
    pub fn render_object(&self) -> Option<&dyn AnyRenderObject>;

    /// Get RenderObject as specific type
    pub fn render_object_as<T: RenderObject + 'static>(&self) -> Option<&T>;

    // ========== Size Queries ==========

    /// Get widget size (after layout)
    pub fn size(&self) -> Option<Size>;

    // ========== Owner Access ==========

    /// Get BuildOwner reference
    pub fn owner(&self) -> Option<Arc<RwLock<BuildOwner>>>;

    // ========== Mounting Status ==========

    /// Check if element is still mounted in tree
    pub fn mounted(&self) -> bool;  // Already exists!

    /// Check if element is valid
    pub fn is_valid(&self) -> bool;  // Already exists!
}
```

### 3. Rust-Idiomatic Short Names

```rust
impl Context {
    // Short aliases for common operations

    /// Find ancestor widget - short form
    pub fn ancestor<T: Widget + 'static>(&self) -> Option<T> {
        self.find_ancestor_widget::<T>()
    }

    /// Find ancestor state - short form
    pub fn ancestor_state<S: State + 'static>(&self) -> Option<&S> {
        self.find_ancestor_state::<S>()
    }

    /// Find RenderObject - short form
    pub fn render<T: RenderObject + 'static>(&self) -> Option<&T> {
        self.render_object_as::<T>()
    }
}
```

---

## Implementation Plan

### Step 1: Widget Finding ✅
- [ ] Implement `find_ancestor_widget<T>()`
- [ ] Add generic widget type matching
- [ ] Handle widget cloning for return value
- [ ] Add unit tests

### Step 2: State Finding ✅
- [ ] Implement `find_ancestor_state<S>()`
- [ ] Implement `find_root_ancestor_state<S>()`
- [ ] Add StatefulElement state access
- [ ] Handle lifetime issues with references
- [ ] Add unit tests

### Step 3: RenderObject Queries ✅
- [ ] Implement `find_render_object()`
- [ ] Implement `render_object_as<T>()`
- [ ] Add type-safe downcasting
- [ ] Add unit tests

### Step 4: Child Visitation ✅
- [ ] Implement `visit_child_elements()`
- [ ] Add closure-based visitor pattern
- [ ] Handle early termination
- [ ] Add unit tests

### Step 5: Size Queries ✅
- [ ] Implement `size()` method
- [ ] Access RenderObject size
- [ ] Handle not-yet-laid-out case
- [ ] Add unit tests

### Step 6: Short Aliases ✅
- [ ] Add `ancestor<T>()` alias
- [ ] Add `ancestor_state<S>()` alias
- [ ] Add `render<T>()` alias
- [ ] Add `visit_children()` alias

### Step 7: Testing ✅
- [ ] 15+ comprehensive tests
- [ ] Edge cases (no ancestor, wrong type)
- [ ] Lifecycle state tests
- [ ] Integration tests

### Step 8: Documentation ✅
- [ ] Update Context documentation
- [ ] Add usage examples
- [ ] Create completion document

---

## API Examples

### Example 1: Find Ancestor Widget

```rust
impl StatelessWidget for MyButton {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Find nearest Scaffold ancestor
        if let Some(scaffold) = context.find_ancestor_widget::<Scaffold>() {
            println!("Found scaffold: {:?}", scaffold);
        }

        // Short form
        let scaffold = context.ancestor::<Scaffold>();

        Box::new(Button::new("Click me"))
    }
}
```

### Example 2: Find Ancestor State

```rust
impl StatelessWidget for ChildWidget {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Access parent's state
        if let Some(parent_state) = context.find_ancestor_state::<MyParentState>() {
            parent_state.increment_counter();
        }

        // Short form
        let state = context.ancestor_state::<MyParentState>();

        Box::new(Text::new("Child"))
    }
}
```

### Example 3: RenderObject Queries

```rust
impl StatelessWidget for Inspector {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Get this widget's RenderObject
        if let Some(render_obj) = context.find_render_object() {
            println!("RenderObject: {:?}", render_obj);
        }

        // Get specific RenderObject type
        if let Some(render_box) = context.render_object_as::<RenderBox>() {
            let size = render_box.size();
            println!("Size: {:?}", size);
        }

        // Short form
        let render_box = context.render::<RenderBox>();

        Box::new(Container)
    }
}
```

### Example 4: Size Queries

```rust
impl StatelessWidget for SizeInspector {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Get widget size after layout
        if let Some(size) = context.size() {
            println!("Widget size: {}x{}", size.width, size.height);
        } else {
            println!("Not laid out yet");
        }

        Box::new(Container)
    }
}
```

### Example 5: Visit Children

```rust
impl StatelessWidget for ParentInspector {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Visit all child elements
        context.visit_child_elements(|child| {
            println!("Child: {:?}", child.id());
        });

        // Short form
        context.visit_children(|child| {
            println!("Child: {:?}", child);
        });

        Box::new(Container)
    }
}
```

---

## Challenges & Solutions

### Challenge 1: Lifetime Issues with State References

**Problem:** Returning `&S` from `find_ancestor_state()` has lifetime issues with RwLock guards.

**Solution:** Return references tied to Context lifetime, or use Arc for shared ownership.

```rust
// Option 1: Reference with Context lifetime
pub fn find_ancestor_state<'a, S: State>(&'a self) -> Option<&'a S>

// Option 2: Use unsafe with careful lifetime management
// Option 3: Clone state if possible
```

### Challenge 2: Widget Cloning for Return

**Problem:** `find_ancestor_widget<T>()` needs to clone widget to return owned value.

**Solution:** Require `Clone` bound on Widget types.

```rust
pub fn find_ancestor_widget<T>(&self) -> Option<T>
where
    T: Widget + Clone + 'static
{
    // Clone widget from element
}
```

### Challenge 3: Type-Safe Downcasting

**Problem:** Need to downcast `dyn AnyElement` to specific element types.

**Solution:** Use `downcast_ref()` from downcast-rs crate (already available).

```rust
if let Some(stateful_elem) = element.downcast_ref::<StatefulElement<W>>() {
    // Access state
}
```

---

## Comparison with Flutter

| Flutter Method | Flui Method | Flui Short Form |
|----------------|-------------|-----------------|
| `findAncestorWidgetOfExactType<T>()` | `find_ancestor_widget<T>()` | `ancestor<T>()` |
| `findAncestorStateOfType<T>()` | `find_ancestor_state<T>()` | `ancestor_state<T>()` |
| `findRootAncestorStateOfType<T>()` | `find_root_ancestor_state<T>()` | - |
| `findAncestorRenderObjectOfType<T>()` | `find_ancestor_render_object<T>()` | - |
| `visitChildElements(visitor)` | `visit_child_elements(visitor)` | `visit_children(visitor)` |
| `findRenderObject()` | `find_render_object()` | `render_object()` |
| `size` | `size()` | - |
| `owner` | `owner()` | - |
| `mounted` | `mounted()` | - (already exists) |

**Result:** 100% Flutter-compatible API with Rust-idiomatic short forms!

---

## Performance Considerations

### Tree Traversal Complexity

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `find_ancestor_widget<T>()` | O(depth) | Linear walk up tree |
| `find_ancestor_state<S>()` | O(depth) | Linear walk up tree |
| `visit_child_elements()` | O(children) | Iterate direct children only |
| `render_object()` | O(1) | Direct access |
| `size()` | O(1) | Direct access |

### Optimization Strategies

1. **Early Termination**: Stop searching when found
2. **Type Caching**: Cache TypeId comparisons
3. **Inline Small Functions**: Use `#[inline]` for getters

---

## Testing Strategy

### Unit Tests (10+ tests)
```rust
#[test]
fn test_find_ancestor_widget() { }

#[test]
fn test_find_ancestor_widget_not_found() { }

#[test]
fn test_find_ancestor_state() { }

#[test]
fn test_visit_child_elements() { }

#[test]
fn test_render_object() { }

#[test]
fn test_size_query() { }
```

### Integration Tests (5+ tests)
```rust
#[test]
fn test_nested_widget_finding() { }

#[test]
fn test_state_access_from_child() { }

#[test]
fn test_render_object_queries() { }
```

---

## Breaking Changes

**None!** All new methods are additions to existing Context API.

---

## Files to Create/Modify

### Modified Files
1. **`src/context/mod.rs`** (~+150 lines)
   - Add tree navigation methods
   - Add layout query methods
   - Add short aliases

2. **`src/context/navigation.rs`** (new, ~200 lines)
   - Widget finding implementation
   - State finding implementation
   - RenderObject queries

### Test Files
1. **`tests/context_navigation_tests.rs`** (new, ~400 lines)
   - Comprehensive navigation tests

---

## Success Criteria

✅ **Phase 7 is complete when:**

1. [ ] All tree navigation methods implemented
2. [ ] State finding works correctly
3. [ ] RenderObject queries functional
4. [ ] Size queries return correct values
5. [ ] Child visitation works
6. [ ] 15+ tests passing
7. [ ] Zero breaking changes
8. [ ] Complete documentation

---

## Next Steps After Phase 7

1. **Phase 10**: Error Handling & Debugging
2. **Phase 11**: Notification System
3. **Phase 13**: Performance Optimizations

---

**Last Updated:** 2025-10-20
**Status:** 🚧 Design Complete, Ready for Implementation
**Estimated Time:** 3-4 hours
