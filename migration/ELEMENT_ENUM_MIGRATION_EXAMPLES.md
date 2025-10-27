# Element Enum Migration - Code Examples & Patterns

> **Companion document to ELEMENT_ENUM_MIGRATION_ROADMAP.md**  
> Contains detailed code examples and migration patterns

---

## 📖 Table of Contents

1. [Before & After Comparison](#before--after-comparison)
2. [Common Patterns](#common-patterns)
3. [Type-Safe Accessors](#type-safe-accessors)
4. [Error Handling](#error-handling)
5. [Performance Patterns](#performance-patterns)
6. [Testing Strategies](#testing-strategies)

---

## 🔄 Before & After Comparison

### Pattern 1: Element Tree Access

#### ❌ Before (Box<dyn>)
```rust
// Old: Runtime downcasting required
fn process_element(tree: &ElementTree, id: ElementId) {
    let element = tree.get(id).unwrap();
    
    // ❌ Runtime downcast - can fail!
    if let Some(component) = element.downcast_ref::<ComponentElement>() {
        component.rebuild();
    } else if let Some(stateful) = element.downcast_ref::<StatefulElement>() {
        stateful.rebuild();
    } else if let Some(render) = element.downcast_ref::<RenderObjectElement>() {
        render.mark_needs_layout();
    }
    // ❌ Easy to forget a type - runtime panic!
}
```

#### ✅ After (enum Element)
```rust
// New: Compile-time exhaustive matching
fn process_element(tree: &ElementTree, id: ElementId) {
    let element = tree.get(id).unwrap();
    
    // ✅ Exhaustive match - compiler checks all variants!
    match element {
        Element::Component(c) => c.rebuild(),
        Element::Stateful(s) => s.rebuild(),
        Element::Inherited(i) => i.rebuild(),
        Element::Render(r) => r.mark_needs_layout(),
        Element::ParentData(p) => p.rebuild(),
    }
    // ✅ If we add new variant, compiler forces us to handle it!
}
```

---

### Pattern 2: Element Insertion

#### ❌ Before (Box<dyn>)
```rust
// Old: Box allocation for every element
fn create_ui(tree: &mut ElementTree) -> ElementId {
    // ❌ Heap allocation 1
    let header = Box::new(ComponentElement::new(HeaderWidget));
    let header_id = tree.insert(header);
    
    // ❌ Heap allocation 2
    let body = Box::new(StatefulElement::new(BodyWidget));
    let body_id = tree.insert(body);
    
    // ❌ Heap allocation 3
    let footer = Box::new(RenderObjectElement::new(FooterWidget));
    let footer_id = tree.insert(footer);
    
    header_id
}
```

#### ✅ After (enum Element)
```rust
// New: Stack allocation, moved into enum
fn create_ui(tree: &mut ElementTree) -> ElementId {
    // ✅ Stack allocation, single move into enum
    let header = Element::Component(ComponentElement::new(HeaderWidget));
    let header_id = tree.insert(header);
    
    // ✅ No Box, no heap allocation!
    let body = Element::Stateful(StatefulElement::new(BodyWidget));
    let body_id = tree.insert(body);
    
    // ✅ Direct enum construction
    let footer = Element::Render(RenderObjectElement::new(FooterWidget));
    let footer_id = tree.insert(footer);
    
    header_id
}
```

---

### Pattern 3: Type Checking

#### ❌ Before (Box<dyn>)
```rust
// Old: Runtime type checking with TypeId
fn is_render_element(element: &dyn DynElement) -> bool {
    use std::any::{Any, TypeId};
    
    // ❌ Runtime check, no compiler help
    element.type_id() == TypeId::of::<RenderObjectElement</* ??? */>()
    // ❌ Can't even specify full type due to generics!
}

fn get_render_element(element: &dyn DynElement) -> Option<&RenderObjectElement> {
    // ❌ Unsafe downcast, can panic
    element.downcast_ref::<RenderObjectElement>()
    // ❌ Doesn't know about generic parameters
}
```

#### ✅ After (enum Element)
```rust
// New: Compile-time type checking with pattern matching
fn is_render_element(element: &Element) -> bool {
    // ✅ Exhaustive, compile-time check
    matches!(element, Element::Render(_))
}

fn get_render_element(element: &Element) -> Option<&RenderObjectElement> {
    // ✅ Type-safe, no runtime overhead
    match element {
        Element::Render(r) => Some(r),
        _ => None,
    }
}

// Or even better, use helper methods:
fn process_if_render(element: &Element) {
    // ✅ Built-in type-safe accessor
    if let Some(render) = element.as_render() {
        render.layout(constraints);
    }
}
```

---

## 🎯 Common Patterns

### Pattern: Tree Traversal

```rust
/// Visit all elements in tree (depth-first)
fn visit_tree<F>(tree: &ElementTree, root_id: ElementId, visitor: &mut F)
where
    F: FnMut(ElementId, &Element),
{
    if let Some(element) = tree.get(root_id) {
        // Visit current element
        visitor(root_id, element);
        
        // ✅ Type-safe children access via enum
        for child_id in element.children() {
            visit_tree(tree, child_id, visitor);
        }
    }
}

// Usage:
visit_tree(&tree, root_id, &mut |id, element| {
    match element {
        Element::Component(c) => println!("Component: {:?}", c),
        Element::Stateful(s) => println!("Stateful: {:?}", s),
        Element::Render(r) => println!("Render: {:?}", r),
        Element::Inherited(i) => println!("Inherited: {:?}", i),
        Element::ParentData(p) => println!("ParentData: {:?}", p),
    }
});
```

### Pattern: Dirty Element Collection

```rust
/// Collect all dirty elements that need rebuild
fn collect_dirty_elements(tree: &ElementTree) -> Vec<(ElementId, ElementType)> {
    let mut dirty = Vec::new();
    
    for (id, node) in tree.iter() {
        let element = &node.element;
        
        if element.is_dirty() {
            // ✅ Pattern match to categorize
            let element_type = match element {
                Element::Component(_) => ElementType::Component,
                Element::Stateful(_) => ElementType::Stateful,
                Element::Inherited(_) => ElementType::Inherited,
                Element::Render(_) => ElementType::Render,
                Element::ParentData(_) => ElementType::ParentData,
            };
            
            dirty.push((id, element_type));
        }
    }
    
    dirty
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementType {
    Component,
    Stateful,
    Inherited,
    Render,
    ParentData,
}
```

### Pattern: Rebuild Pipeline

```rust
/// Rebuild all dirty elements in correct order
fn rebuild_dirty_elements(tree: &mut ElementTree) -> Vec<ChildChange> {
    let dirty_ids = tree.collect_dirty();
    let mut changes = Vec::new();
    
    for id in dirty_ids {
        if let Some(element) = tree.get_mut(id) {
            // ✅ Match to call appropriate rebuild
            let element_changes = match element {
                Element::Component(c) => c.rebuild(id),
                Element::Stateful(s) => s.rebuild(id),
                Element::Inherited(i) => i.rebuild(id),
                Element::Render(r) => r.rebuild(id),
                Element::ParentData(p) => p.rebuild(id),
            };
            
            changes.extend(element_changes);
        }
    }
    
    changes
}

type ChildChange = (ElementId, BoxedWidget, usize);
```

---

## 🔐 Type-Safe Accessors

### Helper Methods on Element Enum

```rust
impl Element {
    /// Safe component access with detailed error
    pub fn expect_component(&self, msg: &str) -> &ComponentElement {
        self.as_component().expect(msg)
    }
    
    /// Safe mutable component access
    pub fn expect_component_mut(&mut self, msg: &str) -> &mut ComponentElement {
        self.as_component_mut().expect(msg)
    }
    
    /// Try to call method on specific variant
    pub fn try_with_component<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&ComponentElement) -> R,
    {
        self.as_component().map(f)
    }
    
    /// Try to call method on specific variant (mutable)
    pub fn try_with_component_mut<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut ComponentElement) -> R,
    {
        self.as_component_mut().map(f)
    }
}

// Usage examples:
let element = tree.get(id).unwrap();

// Safe unwrap with message
let component = element.expect_component("Expected component element");

// Safe optional operation
if let Some(result) = element.try_with_component(|c| c.widget()) {
    println!("Widget: {:?}", result);
}

// Safe mutable operation
tree.get_mut(id).unwrap().try_with_stateful_mut(|s| {
    s.mark_dirty();
});
```

---

## ⚠️ Error Handling

### Pattern: Robust Element Access

```rust
/// Safe element access with detailed error context
pub fn get_element_safe(
    tree: &ElementTree,
    id: ElementId,
) -> Result<&Element, ElementError> {
    tree.get(id).ok_or_else(|| ElementError::NotFound { id })
}

/// Safe element mutation with error context
pub fn get_element_mut_safe(
    tree: &mut ElementTree,
    id: ElementId,
) -> Result<&mut Element, ElementError> {
    tree.get_mut(id).ok_or_else(|| ElementError::NotFound { id })
}

/// Expect specific element variant
pub fn expect_variant<T>(
    element: &Element,
    id: ElementId,
) -> Result<&T, ElementError>
where
    T: 'static,
{
    match element {
        Element::Component(c) if std::any::TypeId::of::<T>() == std::any::TypeId::of::<ComponentElement>() => {
            // Safe because we checked TypeId
            Ok(unsafe { &*(c as *const _ as *const T) })
        }
        Element::Stateful(s) if std::any::TypeId::of::<T>() == std::any::TypeId::of::<StatefulElement>() => {
            Ok(unsafe { &*(s as *const _ as *const T) })
        }
        Element::Render(r) if std::any::TypeId::of::<T>() == std::any::TypeId::of::<RenderObjectElement>() => {
            Ok(unsafe { &*(r as *const _ as *const T) })
        }
        _ => Err(ElementError::WrongVariant {
            id,
            expected: std::any::type_name::<T>(),
            actual: element_variant_name(element),
        }),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ElementError {
    #[error("Element {id:?} not found in tree")]
    NotFound { id: ElementId },
    
    #[error("Element {id:?} has wrong variant: expected {expected}, got {actual}")]
    WrongVariant {
        id: ElementId,
        expected: &'static str,
        actual: &'static str,
    },
}

fn element_variant_name(element: &Element) -> &'static str {
    match element {
        Element::Component(_) => "Component",
        Element::Stateful(_) => "Stateful",
        Element::Inherited(_) => "Inherited",
        Element::Render(_) => "Render",
        Element::ParentData(_) => "ParentData",
    }
}
```

---

## ⚡ Performance Patterns

### Pattern: Batch Operations

```rust
/// Process multiple elements efficiently
pub fn batch_mark_dirty(tree: &mut ElementTree, ids: &[ElementId]) {
    for &id in ids {
        // ✅ Direct access, no vtable overhead
        if let Some(element) = tree.get_mut(id) {
            element.mark_dirty();  // Dispatched via match (fast!)
        }
    }
}

/// Collect specific element type efficiently
pub fn collect_render_elements(tree: &ElementTree) -> Vec<(ElementId, &RenderObjectElement)> {
    let mut renders = Vec::new();
    
    for (id, node) in tree.iter() {
        // ✅ Single match, compiler optimizes
        if let Element::Render(r) = &node.element {
            renders.push((id, r));
        }
    }
    
    renders
}
```

### Pattern: Cache-Friendly Iteration

```rust
/// Iterate elements in cache-friendly order
pub fn iterate_elements_linear<F>(tree: &ElementTree, mut f: F)
where
    F: FnMut(ElementId, &Element),
{
    // ✅ Slab iteration is cache-friendly
    // Elements stored contiguously in memory
    for (id, node) in tree.iter() {
        f(id, &node.element);
    }
}

/// Process by depth (breadth-first)
pub fn process_by_depth(tree: &ElementTree, root_id: ElementId) {
    let mut queue = VecDeque::new();
    queue.push_back(root_id);
    
    while let Some(id) = queue.pop_front() {
        if let Some(element) = tree.get(id) {
            // Process element
            process_element(element);
            
            // ✅ Efficient children access via enum
            for child_id in element.children() {
                queue.push_back(child_id);
            }
        }
    }
}
```

---

## 🧪 Testing Strategies

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_element_variant_access() {
        let element = Element::Component(ComponentElement::new(TestWidget));
        
        // Test correct variant
        assert!(element.as_component().is_some());
        assert!(element.as_stateful().is_none());
        assert!(element.as_render().is_none());
    }
    
    #[test]
    fn test_element_tree_insertion() {
        let mut tree = ElementTree::new();
        
        let id = tree.insert(Element::Component(
            ComponentElement::new(TestWidget)
        ));
        
        assert!(tree.contains(id));
        assert!(tree.get(id).is_some());
    }
    
    #[test]
    fn test_element_exhaustive_match() {
        let elements = vec![
            Element::Component(ComponentElement::new(TestWidget)),
            Element::Stateful(StatefulElement::new(TestStatefulWidget)),
            Element::Render(RenderObjectElement::new(TestRenderWidget)),
        ];
        
        for element in &elements {
            // This must compile - proves exhaustiveness
            match element {
                Element::Component(_) => {},
                Element::Stateful(_) => {},
                Element::Inherited(_) => {},
                Element::Render(_) => {},
                Element::ParentData(_) => {},
            }
        }
    }
    
    #[test]
    fn test_element_dispatch_performance() {
        let element = Element::Component(ComponentElement::new(TestWidget));
        
        // Warm up
        for _ in 0..1000 {
            let _ = element.is_dirty();
        }
        
        // Measure
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            let _ = element.is_dirty();  // Match dispatch
        }
        let elapsed = start.elapsed();
        
        // Should be very fast (match is ~1-2 cycles)
        assert!(elapsed.as_micros() < 1000, "Dispatch too slow: {:?}", elapsed);
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    
    #[test]
    fn test_full_rebuild_cycle() {
        let mut tree = ElementTree::new();
        
        // Build tree
        let root_id = tree.insert(Element::Component(
            ComponentElement::new(RootWidget)
        ));
        
        let child_id = tree.insert(Element::Stateful(
            StatefulElement::new(ChildWidget)
        ));
        
        // Mark dirty
        tree.get_mut(child_id).unwrap().mark_dirty();
        
        // Rebuild
        let changes = rebuild_dirty_elements(&mut tree);
        
        assert!(!changes.is_empty());
    }
    
    #[test]
    fn test_tree_traversal() {
        let mut tree = ElementTree::new();
        
        // Build complex tree
        let root = tree.insert(Element::Render(
            RenderObjectElement::new(ContainerWidget)
        ));
        
        let child1 = tree.insert(Element::Component(
            ComponentElement::new(TextWidget::new("Hello"))
        ));
        
        let child2 = tree.insert(Element::Component(
            ComponentElement::new(TextWidget::new("World"))
        ));
        
        // Count elements
        let mut count = 0;
        visit_tree(&tree, root, &mut |_, _| count += 1);
        
        assert_eq!(count, 3);
    }
}
```

---

## 📊 Performance Comparison Examples

### Benchmark: Element Access

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_element_access_old(c: &mut Criterion) {
    let mut tree = ElementTreeOld::new();
    let ids: Vec<_> = (0..1000)
        .map(|_| tree.insert(Box::new(ComponentElement::new(TestWidget))))
        .collect();
    
    c.bench_function("old_element_access", |b| {
        b.iter(|| {
            for &id in &ids {
                let element = black_box(tree.get(id).unwrap());
                black_box(element.is_dirty());  // Vtable dispatch
            }
        });
    });
}

fn bench_element_access_new(c: &mut Criterion) {
    let mut tree = ElementTree::new();
    let ids: Vec<_> = (0..1000)
        .map(|_| tree.insert(Element::Component(ComponentElement::new(TestWidget))))
        .collect();
    
    c.bench_function("new_element_access", |b| {
        b.iter(|| {
            for &id in &ids {
                let element = black_box(tree.get(id).unwrap());
                black_box(element.is_dirty());  // Match dispatch
            }
        });
    });
}

criterion_group!(benches, bench_element_access_old, bench_element_access_new);
criterion_main!(benches);

// Expected results:
// old_element_access: 150 μs
// new_element_access: 40 μs
// Speedup: 3.75x ✓✓✓
```

---

## 🎓 Key Lessons

### What Makes Enum Better

1. **Compile-Time Safety**
   - Exhaustive pattern matching
   - No forgotten cases
   - Compiler enforces correctness

2. **Performance**
   - Direct dispatch (no vtable)
   - Better cache locality
   - Easier for compiler to optimize

3. **Maintainability**
   - Clear, explicit types
   - No hidden downcasts
   - Self-documenting code

### When to Use Each Pattern

| Pattern | Use Case |
|---------|----------|
| `match element { ... }` | When you need to handle all variants |
| `element.as_component()` | When you expect specific variant |
| `element.try_with_component()` | When you want to conditionally operate |
| `expect_variant()` | When variant must be correct (with good error) |

---

**Ready to migrate?** Start with the roadmap and use these patterns! 🚀
