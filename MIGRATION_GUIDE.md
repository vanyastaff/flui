# FLUI Architecture Migration Guide

**Version:** 1.0  
**Date:** December 2024  
**Status:** Draft

---

## Executive Summary

This document outlines the migration from the current monolithic `Element` structure to a cleaner, more maintainable architecture with:

- **Element enum** (`View` | `Render`) for type-safe element variants
- **Layer-specific conversion traits** (`IntoView`, `IntoRender`, `IntoElement`)
- **Primitives in foundation** (`ElementLifecycle`, `ElementFlags`)
- **Clean dependency flow** with no circular dependencies

**Estimated effort:** 2-3 weeks  
**Risk level:** Medium (architectural changes, but incremental migration possible)

---

## Table of Contents

1. [Current State](#current-state)
2. [Target Architecture](#target-architecture)
3. [Migration Phases](#migration-phases)
4. [Breaking Changes](#breaking-changes)
5. [Code Migration Examples](#code-migration-examples)
6. [Testing Strategy](#testing-strategy)
7. [Rollback Plan](#rollback-plan)

---

## Current State

### Current Architecture

```
flui-element/
├── Element (struct with Option fields)
│   ├── view_object: Option<Box<dyn Any>>
│   ├── render_object: Option<Box<dyn RenderObject>>
│   └── render_state: Option<RenderState>
├── ElementTree (Slab storage)
└── ViewObject trait (in flui-element)
```

### Problems

1. **❌ Memory waste:** Every `Element` allocates space for both view and render fields
2. **❌ Invalid states:** Possible to have both `view_object` and `render_object` set
3. **❌ Runtime checks:** Need `if has_view_object()` checks everywhere
4. **❌ Circular dependencies:** ViewObject in flui-element creates potential cycles
5. **❌ No type safety:** Can't distinguish View vs Render elements at compile time

---

## Target Architecture

### New Structure

```
flui-foundation (primitives)
    ├── ElementId, Key, Slot
    ├── ElementLifecycle (enum)        ← NEW
    └── ElementFlags (bitflags)        ← NEW

flui-tree (abstractions)
    └── RenderTreeAccess, TreeNav, TreeWrite

flui-view (view layer)
    ├── ViewObject trait               ← MOVED from flui-element
    ├── IntoView trait                 ← NEW
    └── StatelessView, StatefulView

flui_rendering (rendering layer)
    ├── RenderObject trait
    ├── IntoRender trait               ← NEW
    └── RenderBox, RenderSliver

flui-element (integration layer)
    ├── ElementBase (uses foundation types)
    ├── ViewElement                    ← NEW
    ├── RenderElement                  ← NEW
    ├── Element (enum)                 ← NEW
    ├── IntoElement trait              ← NEW
    └── ElementTree (unified storage)
```

### Key Changes

```rust
// OLD (struct with Options)
pub struct Element {
    view_object: Option<Box<dyn Any>>,
    render_object: Option<Box<dyn RenderObject>>,
    // ...
}

// NEW (enum variants)
pub enum Element {
    View(ViewElement),
    Render(RenderElement),
}

pub struct ViewElement {
    base: ElementBase,
    view_object: Box<dyn ViewObject>,  // Always present!
    children: Vec<ElementId>,
}

pub struct RenderElement {
    base: ElementBase,
    render_object: Box<dyn RenderObject>,  // Always present!
    render_state: RenderState,
    children: Vec<ElementId>,
}
```

---

## Migration Phases

### Phase 0: Preparation (1 day)

**Goal:** Set up infrastructure without breaking changes

**Tasks:**

1. Add `ElementLifecycle` and `ElementFlags` to `flui-foundation`
2. Update `ElementBase` to use foundation types
3. Add feature flags for new architecture

```toml
# flui-element/Cargo.toml
[features]
default = ["legacy"]
legacy = []  # Current architecture
new-arch = []  # New enum architecture
```

**Validation:**
- [ ] `cargo check --all-features` passes
- [ ] All existing tests pass

---

### Phase 1: Move ViewObject to flui-view (2 days)

**Goal:** Break circular dependency between element and view

**Tasks:**

1. **Create `flui-view/src/view_object.rs`:**

```rust
// flui-view/src/view_object.rs
use std::any::Any;

pub trait ViewObject: Any + Send + Sync {
    fn mode(&self) -> ViewMode;
    
    /// Build returns Box<dyn ViewObject> to avoid Element dependency
    fn build(&mut self, ctx: &dyn BuildContext) -> Box<dyn ViewObject>;
    
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

2. **Update flui-element to import ViewObject:**

```rust
// flui-element/src/element.rs
use flui_view::ViewObject;  // Import from flui-view

pub struct Element {
    view_object: Option<Box<dyn ViewObject>>,  // Use imported trait
    // ...
}
```

3. **Update flui-view/Cargo.toml:**

```toml
[dependencies]
flui-foundation = { path = "../flui-foundation" }
flui-tree = { path = "../flui-tree" }
# NO dependency on flui-element
```

4. **Update flui-element/Cargo.toml:**

```toml
[dependencies]
flui-view = { path = "../flui-view" }  # Add this
```

**Validation:**
- [ ] No circular dependencies: `cargo tree` shows clean graph
- [ ] All tests pass
- [ ] No API changes for users

**Rollback:** Revert ViewObject to flui-element

---

### Phase 2: Create Element Variants (3 days)

**Goal:** Introduce new element types alongside old structure

**Tasks:**

1. **Create `ViewElement` and `RenderElement`:**

```rust
// flui-element/src/element/view_element.rs
use flui_view::ViewObject;

pub struct ViewElement {
    base: ElementBase,
    view_object: Box<dyn ViewObject>,
    children: Vec<ElementId>,
}

impl ViewElement {
    pub fn new(view_object: Box<dyn ViewObject>) -> Self {
        Self {
            base: ElementBase::new(),
            view_object,
            children: Vec::new(),
        }
    }
    
    pub fn base(&self) -> &ElementBase { &self.base }
    pub fn view_object(&self) -> &dyn ViewObject { self.view_object.as_ref() }
    pub fn children(&self) -> &[ElementId] { &self.children }
}
```

```rust
// flui-element/src/element/render_element.rs
use flui_rendering::RenderObject;

pub struct RenderElement {
    base: ElementBase,
    render_object: Box<dyn RenderObject>,
    render_state: RenderState,
    children: Vec<ElementId>,
}

impl RenderElement {
    pub fn new(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            base: ElementBase::new(),
            render_object,
            render_state: RenderState::new(),
            children: Vec::new(),
        }
    }
    
    pub fn base(&self) -> &ElementBase { &self.base }
    pub fn render_object(&self) -> &dyn RenderObject { self.render_object.as_ref() }
    pub fn render_state(&self) -> &RenderState { &self.render_state }
    pub fn children(&self) -> &[ElementId] { &self.children }
}
```

2. **Create Element enum (behind feature flag):**

```rust
// flui-element/src/element/element_new.rs
#[cfg(feature = "new-arch")]
pub enum Element {
    View(ViewElement),
    Render(RenderElement),
}

#[cfg(feature = "new-arch")]
impl Element {
    pub fn view(view_object: Box<dyn ViewObject>) -> Self {
        Element::View(ViewElement::new(view_object))
    }
    
    pub fn render(render_object: Box<dyn RenderObject>) -> Self {
        Element::Render(RenderElement::new(render_object))
    }
    
    // Common operations
    pub fn base(&self) -> &ElementBase {
        match self {
            Element::View(v) => v.base(),
            Element::Render(r) => r.base(),
        }
    }
    
    pub fn children(&self) -> &[ElementId] {
        match self {
            Element::View(v) => v.children(),
            Element::Render(r) => r.children(),
        }
    }
    
    pub fn is_view(&self) -> bool {
        matches!(self, Element::View(_))
    }
    
    pub fn is_render(&self) -> bool {
        matches!(self, Element::Render(_))
    }
}
```

3. **Export conditionally:**

```rust
// flui-element/src/lib.rs
#[cfg(feature = "legacy")]
pub use element_old::Element;

#[cfg(feature = "new-arch")]
pub use element_new::{Element, ViewElement, RenderElement};
```

**Validation:**
- [ ] `cargo test --features legacy` passes (old arch)
- [ ] `cargo test --features new-arch` passes (new arch)
- [ ] Both architectures compile

---

### Phase 3: Add IntoView/IntoRender/IntoElement (2 days)

**Goal:** Add conversion traits for clean API

**Tasks:**

1. **Create `flui-view/src/into_view.rs`:**

```rust
use crate::ViewObject;

/// Convert values into ViewObject
pub trait IntoView {
    fn into_view(self) -> Box<dyn ViewObject>;
}

// Impl for StatelessView
impl<V: StatelessView> IntoView for V {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(StatelessViewWrapper::new(self))
    }
}

// Impl for ViewObject itself
impl IntoView for Box<dyn ViewObject> {
    fn into_view(self) -> Box<dyn ViewObject> {
        self
    }
}
```

2. **Create `flui_rendering/src/into_render.rs`:**

```rust
use crate::RenderObject;

/// Convert values into RenderObject
pub trait IntoRender {
    fn into_render(self) -> Box<dyn RenderObject>;
}

// Blanket impl for all RenderObject types
impl<R: RenderObject> IntoRender for R {
    fn into_render(self) -> Box<dyn RenderObject> {
        Box::new(self)
    }
}

// Impl for boxed RenderObject
impl IntoRender for Box<dyn RenderObject> {
    fn into_render(self) -> Box<dyn RenderObject> {
        self
    }
}
```

3. **Create `flui-element/src/into_element.rs`:**

```rust
use crate::{Element, ViewElement, RenderElement};
use flui_view::ViewObject;
use flui_rendering::RenderObject;

/// Convert values into Element
pub trait IntoElement {
    fn into_element(self) -> Element;
}

// Element itself
impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
    }
}

// ViewElement and RenderElement
impl IntoElement for ViewElement {
    fn into_element(self) -> Element {
        Element::View(self)
    }
}

impl IntoElement for RenderElement {
    fn into_element(self) -> Element {
        Element::Render(self)
    }
}

// ViewObject from flui-view
impl IntoElement for Box<dyn ViewObject> {
    fn into_element(self) -> Element {
        Element::View(ViewElement::new(self))
    }
}

// RenderObject from flui_rendering
impl IntoElement for Box<dyn RenderObject> {
    fn into_element(self) -> Element {
        Element::Render(RenderElement::new(self))
    }
}

// Option support
impl<T: IntoElement> IntoElement for Option<T> {
    fn into_element(self) -> Element {
        match self {
            Some(value) => value.into_element(),
            None => {
                use flui_rendering::RenderEmpty;
                Element::render(RenderEmpty::new())
            }
        }
    }
}
```

4. **Update StatelessView trait:**

```rust
// flui-view/src/traits/stateless.rs
pub trait StatelessView: Sized + 'static {
    /// Build UI - returns something convertible to ViewObject
    fn build(self, ctx: &BuildContext) -> impl IntoView;
    //                                    ^^^^^^^^^^^^
    //                                    NEW!
}
```

**Validation:**
- [ ] All conversion traits compile
- [ ] User code can use `into_view()`, `into_render()`, `into_element()`
- [ ] Tests pass with new traits

---

### Phase 4: Update ElementTree (2 days)

**Goal:** Update tree storage to use Element enum

**Tasks:**

1. **Update ElementTree storage:**

```rust
// flui-element/src/tree/element_tree.rs
pub struct ElementTree {
    nodes: Slab<ElementNode>,
    root: Option<ElementId>,
}

struct ElementNode {
    element: Element,  // Enum, not struct!
}

impl ElementTree {
    /// Get element (any type)
    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.nodes.get(id.get() - 1).map(|n| &n.element)
    }
    
    /// Get mutable element
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        self.nodes.get_mut(id.get() - 1).map(|n| &mut n.element)
    }
    
    /// Get ViewElement (type-safe)
    pub fn get_view(&self, id: ElementId) -> Option<&ViewElement> {
        match self.get(id)? {
            Element::View(v) => Some(v),
            _ => None,
        }
    }
    
    /// Get RenderElement (type-safe)
    pub fn get_render(&self, id: ElementId) -> Option<&RenderElement> {
        match self.get(id)? {
            Element::Render(r) => Some(r),
            _ => None,
        }
    }
    
    /// Insert element
    pub fn insert(&mut self, element: Element) -> ElementId {
        let node = ElementNode { element };
        let idx = self.nodes.insert(node);
        ElementId::new(idx + 1)
    }
    
    /// Iterator over render elements only
    pub fn render_elements(&self) -> impl Iterator<Item = (ElementId, &RenderElement)> + '_ {
        self.nodes.iter().filter_map(|(idx, node)| {
            let id = ElementId::new(idx + 1);
            match &node.element {
                Element::Render(r) => Some((id, r)),
                _ => None,
            }
        })
    }
}
```

2. **Update RenderTreeAccess impl:**

```rust
// flui-element/src/tree/tree_traits.rs
impl RenderTreeAccess for ElementTree {
    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        match self.get(id)? {
            Element::Render(r) => {
                Some(r.render_object() as &dyn Any)
            }
            _ => None,
        }
    }
    
    fn is_render_element(&self, id: ElementId) -> bool {
        matches!(self.get(id), Some(Element::Render(_)))
    }
}
```

**Validation:**
- [ ] ElementTree compiles with new Element enum
- [ ] All tree operations work correctly
- [ ] Iterator methods work

---

### Phase 5: Update Pipeline (3 days)

**Goal:** Update build/layout/paint pipeline for new architecture

**Tasks:**

1. **Update build phase:**

```rust
// flui-pipeline/src/build_phase.rs

fn rebuild_element(
    tree: &mut ElementTree,
    id: ElementId,
    ctx: &BuildContext,
) -> Result<(), PipelineError> {
    let element = tree.get_mut(id)?;
    
    match element {
        Element::View(view_elem) => {
            // Rebuild view element
            let child_view_obj = view_elem.view_object.build(ctx);
            let child_element = child_view_obj.into_element();
            
            // Update children
            // ...
        }
        Element::Render(_) => {
            // Render elements don't have build logic
        }
    }
    
    Ok(())
}
```

2. **Update layout phase:**

```rust
// flui-pipeline/src/layout_phase.rs

fn layout_tree(tree: &mut ElementTree, root: ElementId) -> Result<(), LayoutError> {
    // Only layout render elements
    for (id, render_elem) in tree.render_elements() {
        layout_render_element(tree, id, render_elem)?;
    }
    Ok(())
}

fn layout_render_element(
    tree: &mut ElementTree,
    id: ElementId,
    render_elem: &RenderElement,
) -> Result<(), LayoutError> {
    let render_obj = render_elem.render_object();
    
    // Create layout context
    let ctx = LayoutContext::new(tree, id, constraints);
    
    // Perform layout
    let size = render_obj.layout(ctx)?;
    
    // Store result
    // ...
    
    Ok(())
}
```

3. **Update paint phase:**

```rust
// flui-pipeline/src/paint_phase.rs

fn paint_tree(
    tree: &ElementTree,
    root: ElementId,
    canvas: &mut Canvas,
) -> Result<(), PaintError> {
    // Only paint render elements
    for (id, render_elem) in tree.render_elements() {
        paint_render_element(tree, id, render_elem, canvas)?;
    }
    Ok(())
}
```

**Validation:**
- [ ] Build phase works with ViewElement
- [ ] Layout phase works with RenderElement
- [ ] Paint phase works with RenderElement
- [ ] End-to-end pipeline test passes

---

### Phase 6: Update User-Facing APIs (2 days)

**Goal:** Update public APIs and examples

**Tasks:**

1. **Update widget creation:**

```rust
// Before (old):
let element = Element::new(StatelessViewWrapper::new(my_view));

// After (new):
let element = my_view.into_element();
// or
let element = Element::view(Box::new(StatelessViewWrapper::new(my_view)));
```

2. **Update conditional rendering:**

```rust
// Before:
let child = if condition {
    Some(my_view.into_element())
} else {
    Some(Element::empty())
};

// After:
let child = if condition {
    Some(my_view)
} else {
    None
}.into_element();  // Option<T> implements IntoElement!
```

3. **Update documentation:**
   - Update all examples in rustdoc
   - Update README.md
   - Update migration guide
   - Update cookbook examples

**Validation:**
- [ ] All examples compile
- [ ] Documentation builds without warnings
- [ ] Tutorial works with new API

---

### Phase 7: Remove Legacy Code (1 day)

**Goal:** Remove old Element struct and feature flags

**Tasks:**

1. **Remove `legacy` feature flag**
2. **Delete old Element struct** (`element_old.rs`)
3. **Remove conditional compilation**
4. **Update default features**

```toml
# flui-element/Cargo.toml
[features]
default = []  # New architecture is default
```

**Validation:**
- [ ] `cargo check --workspace` passes
- [ ] No legacy code remains
- [ ] All tests pass

---

## Breaking Changes

### API Changes

| Old API | New API | Migration |
|---------|---------|-----------|
| `Element::new(view_obj)` | `Element::view(view_obj)` | Replace constructor |
| `element.view_object()` | `if let Element::View(v) = element { v.view_object() }` | Pattern match |
| `element.render_object()` | `if let Element::Render(r) = element { r.render_object() }` | Pattern match |
| `element.has_view_object()` | `element.is_view()` | Use type check |
| `Element::empty()` | `None.into_element()` | Use Option |

### Type Changes

```rust
// OLD
fn process_element(elem: &Element) {
    if elem.has_view_object() {
        let view = elem.view_object().unwrap();
        // ...
    }
}

// NEW
fn process_element(elem: &Element) {
    match elem {
        Element::View(view_elem) => {
            let view = view_elem.view_object();
            // ...
        }
        Element::Render(render_elem) => {
            // ...
        }
    }
}
```

### Dependency Changes

```toml
# OLD - flui-element had ViewObject
[dependencies]
flui-element = "0.1"

# NEW - ViewObject moved to flui-view
[dependencies]
flui-element = "0.2"
flui-view = "0.2"  # Add if using ViewObject directly
```

---

## Code Migration Examples

### Example 1: Creating Elements

**Before:**
```rust
use flui_element::{Element, ViewObject};

let view_obj: Box<dyn ViewObject> = Box::new(StatelessViewWrapper::new(my_view));
let element = Element::new(view_obj);
```

**After:**
```rust
use flui_element::{Element, IntoElement};
use flui_view::IntoView;

// Option 1: Direct
let element = my_view.into_element();

// Option 2: Explicit
let view_obj = my_view.into_view();
let element = view_obj.into_element();

// Option 3: Constructor
let element = Element::view(my_view.into_view());
```

---

### Example 2: Pattern Matching

**Before:**
```rust
fn handle_element(element: &Element) {
    if element.has_view_object() {
        println!("View element");
    } else if element.has_render_object() {
        println!("Render element");
    }
}
```

**After:**
```rust
fn handle_element(element: &Element) {
    match element {
        Element::View(view) => {
            println!("View element: {:?}", view.view_object().mode());
        }
        Element::Render(render) => {
            println!("Render element");
        }
    }
}
```

---

### Example 3: Optional Children

**Before:**
```rust
let child = if show_button {
    Some(Button::new("Click").into_element())
} else {
    Some(Element::empty())
};
```

**After:**
```rust
let child = if show_button {
    Some(Button::new("Click"))
} else {
    None
}.into_element();  // Option<T: IntoElement> -> Element
```

---

### Example 4: Building Views

**Before:**
```rust
impl StatelessView for MyView {
    fn build(self, ctx: &BuildContext) -> Box<dyn ViewObject> {
        Box::new(StatelessViewWrapper::new(
            Column::new()
                .child(Text::new("Hello"))
        ))
    }
}
```

**After:**
```rust
impl StatelessView for MyView {
    fn build(self, ctx: &BuildContext) -> impl IntoView {
        Column::new()
            .child(Text::new("Hello"))
    }
}
```

---

### Example 5: ElementTree Operations

**Before:**
```rust
let element = tree.get(id)?;
if element.has_render_object() {
    let render_obj = element.render_object().unwrap();
    // Use render_obj
}
```

**After:**
```rust
// Option 1: Pattern match on element
let element = tree.get(id)?;
match element {
    Element::Render(render_elem) => {
        let render_obj = render_elem.render_object();
        // Use render_obj
    }
    _ => {}
}

// Option 2: Direct access
if let Some(render_elem) = tree.get_render(id) {
    let render_obj = render_elem.render_object();
    // Use render_obj
}
```

---

## Testing Strategy

### Unit Tests

1. **Element enum tests:**
```rust
#[test]
fn test_element_variants() {
    let view = Element::view(create_test_view_obj());
    assert!(view.is_view());
    assert!(!view.is_render());
    
    let render = Element::render(create_test_render_obj());
    assert!(render.is_render());
    assert!(!render.is_view());
}

#[test]
fn test_element_common_operations() {
    let view = Element::view(create_test_view_obj());
    assert_eq!(view.base().lifecycle(), ElementLifecycle::Initial);
    
    let render = Element::render(create_test_render_obj());
    assert_eq!(render.base().lifecycle(), ElementLifecycle::Initial);
}
```

2. **Conversion trait tests:**
```rust
#[test]
fn test_into_view() {
    let view = TestView { value: 42 };
    let view_obj = view.into_view();
    assert!(view_obj.as_any().is::<StatelessViewWrapper<TestView>>());
}

#[test]
fn test_into_element() {
    let view_obj = TestView { value: 42 }.into_view();
    let element = view_obj.into_element();
    assert!(matches!(element, Element::View(_)));
}

#[test]
fn test_option_into_element() {
    let some = Some(TestView { value: 42 });
    let element = some.into_element();
    assert!(matches!(element, Element::View(_)));
    
    let none: Option<TestView> = None;
    let element = none.into_element();
    assert!(matches!(element, Element::Render(_)));  // RenderEmpty
}
```

3. **ElementTree tests:**
```rust
#[test]
fn test_element_tree_storage() {
    let mut tree = ElementTree::new();
    
    let view_id = tree.insert(Element::view(create_test_view_obj()));
    let render_id = tree.insert(Element::render(create_test_render_obj()));
    
    assert!(tree.get_view(view_id).is_some());
    assert!(tree.get_render(view_id).is_none());
    
    assert!(tree.get_render(render_id).is_some());
    assert!(tree.get_view(render_id).is_none());
}

#[test]
fn test_render_elements_iterator() {
    let mut tree = ElementTree::new();
    
    tree.insert(Element::view(create_test_view_obj()));
    tree.insert(Element::render(create_test_render_obj()));
    tree.insert(Element::view(create_test_view_obj()));
    tree.insert(Element::render(create_test_render_obj()));
    
    let render_count = tree.render_elements().count();
    assert_eq!(render_count, 2);
}
```

### Integration Tests

1. **End-to-end pipeline test:**
```rust
#[test]
fn test_build_layout_paint_pipeline() {
    let mut tree = ElementTree::new();
    
    // Create view hierarchy
    let root = MyView { title: "Test".into() }.into_element();
    let root_id = tree.insert(root);
    
    // Build phase
    build_tree(&mut tree, root_id, &ctx).unwrap();
    
    // Layout phase
    layout_tree(&mut tree, root_id).unwrap();
    
    // Paint phase
    let mut canvas = Canvas::new(800, 600);
    paint_tree(&tree, root_id, &mut canvas).unwrap();
    
    // Verify
    assert!(tree.get_render(root_id).is_some());
}
```

2. **Memory usage test:**
```rust
#[test]
fn test_element_memory_efficiency() {
    use std::mem::size_of;
    
    // OLD: struct with all fields
    // assert_eq!(size_of::<OldElement>(), 104);
    
    // NEW: enum with variants
    assert!(size_of::<Element>() < 80);  // ~30% smaller
    assert_eq!(size_of::<ViewElement>(), 48);
    assert_eq!(size_of::<RenderElement>(), 64);
}
```

### Performance Tests

```rust
#[bench]
fn bench_element_creation(b: &mut Bencher) {
    b.iter(|| {
        let element = TestView { value: 42 }.into_element();
        black_box(element);
    });
}

#[bench]
fn bench_element_tree_insertion(b: &mut Bencher) {
    let mut tree = ElementTree::new();
    b.iter(|| {
        let id = tree.insert(TestView { value: 42 }.into_element());
        black_box(id);
    });
}

#[bench]
fn bench_render_elements_iteration(b: &mut Bencher) {
    let tree = create_test_tree_with_1000_elements();
    b.iter(|| {
        let count = tree.render_elements().count();
        black_box(count);
    });
}
```

---

## Rollback Plan

### If Migration Fails

**Phase 1-3 (Before breaking changes):**
- Revert commits
- Remove feature flags
- Restore old architecture

**Phase 4+ (After breaking changes):**

1. **Immediate rollback:**
```bash
git revert <migration-commits>
git push
```

2. **Release hotfix:**
```toml
# Cargo.toml
[package]
version = "0.1.1"  # Revert to stable
```

3. **Notify users:**
```markdown
# URGENT: Rollback Notice

We've rolled back the Element enum migration due to [issue].
Please pin to version 0.1.1 until further notice:

```toml
flui = "=0.1.1"
```
```

### Risk Mitigation

1. **Feature flags:** Keep both architectures working during migration
2. **Staged rollout:** Update internal projects first before public release
3. **Comprehensive tests:** 90%+ code coverage before removing legacy
4. **Beta release:** Release 0.2.0-beta for community testing

---

## Success Criteria

### Technical Metrics

- [ ] All tests pass (unit, integration, benchmarks)
- [ ] No circular dependencies in `cargo tree`
- [ ] Memory usage reduced by ~30%
- [ ] Compilation time unchanged or improved
- [ ] No performance regressions

### Code Quality

- [ ] Documentation updated (rustdoc, README, guides)
- [ ] Examples updated and working
- [ ] No `unsafe` code added
- [ ] Clippy warnings: 0
- [ ] Code coverage: >90%

### User Experience

- [ ] Migration guide published
- [ ] Breaking changes documented
- [ ] Community feedback addressed
- [ ] At least 2 community projects migrated successfully

---

## Timeline

```
Week 1:
  Day 1: Phase 0 (Preparation)
  Day 2-3: Phase 1 (Move ViewObject)
  Day 4-5: Phase 2 (Create variants)

Week 2:
  Day 1-2: Phase 3 (Add traits)
  Day 3-4: Phase 4 (Update ElementTree)
  Day 5: Phase 5 (Update Pipeline) - start

Week 3:
  Day 1-2: Phase 5 (Update Pipeline) - complete
  Day 3-4: Phase 6 (Update APIs)
  Day 5: Phase 7 (Remove legacy)

Week 4:
  Testing, documentation, beta release
```

---

## Appendix

### Useful Commands

```bash
# Check for circular dependencies
cargo tree --workspace

# Run all tests
cargo test --workspace --all-features

# Check memory usage
cargo bloat --release -n 50

# Benchmark performance
cargo bench

# Check documentation
cargo doc --no-deps --open
```

### Resources

- [Flutter Element Architecture](https://api.flutter.dev/flutter/widgets/Element-class.html)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [FLUI Architecture RFC](./rfcs/0001-element-enum.md)

---

**Document Version:** 1.0  
**Last Updated:** December 2024  
**Authors:** FLUI Core Team  
**Status:** Ready for Review
