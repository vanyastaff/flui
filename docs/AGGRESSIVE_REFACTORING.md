# 🔥 Aggressive Rust-Idiomatic Refactoring Plan

## Philosophy

**OUT**: Flutter naming conventions, camelCase, legacy patterns
**IN**: Rust idioms, snake_case, modern patterns, zero-cost abstractions

This is a BREAKING refactoring - we prioritize correctness and Rust conventions over backwards compatibility.

---

## 🎯 Core Principles

### 1. **Rust Naming Conventions**
- ✅ `snake_case` for ALL methods and functions
- ✅ `UpperCamelCase` for types and traits
- ✅ `SCREAMING_SNAKE_CASE` for constants
- ❌ No camelCase (Flutter style)

### 2. **Rust Error Handling**
- ✅ `Result<T, E>` for fallible operations
- ✅ Custom error types with `thiserror`
- ✅ `Option<T>` only when None is valid state
- ❌ No panic in library code
- ❌ No `unwrap()` in public APIs

### 3. **Ownership & Borrowing**
- ✅ Clear ownership semantics
- ✅ Prefer `&self` over cloning
- ✅ Use `Cow<'_, T>` when appropriate
- ✅ Zero-copy where possible
- ❌ Minimize `Arc<RwLock<T>>` - use channels instead

### 4. **Modern Rust Features**
- ✅ Use `impl Trait` for return types
- ✅ Use associated types in traits
- ✅ Leverage type system for compile-time guarantees
- ✅ Use `#[must_use]` on important return types
- ✅ Use const generics where applicable

---

## 📋 Breaking Changes by Category

### A. Method Naming (snake_case)

#### BuildContext

| Old (Flutter style) | New (Rust idiomatic) | Notes |
|-------------------|---------------------|--------|
| `element_id()` | `element_id()` | ✅ Already correct |
| `mark_needs_build()` | `mark_dirty()` | Shorter, clearer |
| `visit_ancestor_elements()` | `walk_ancestors()` | More Rust-like |
| `visit_child_elements()` | `walk_children()` | More Rust-like |
| `depend_on_inherited_widget()` | `subscribe_to<W>()` | Clearer intent |
| `get_inherited_widget()` | `find_inherited<W>()` | No "get" prefix |
| `find_ancestor_widget_of_type()` | `find_ancestor<W>()` | Generic, shorter |
| `find_ancestor_element_of_type()` | `find_ancestor_element<E>()` | Consistent |
| `find_ancestor_render_object_of_type()` | `find_ancestor_render<R>()` | Shorter |
| `find_ancestor_state_of_type()` | `find_ancestor_state<S>()` | Consistent |
| `find_root_ancestor_state_of_type()` | `find_root_state<S>()` | Shorter |
| `get_element_for_inherited_widget_of_exact_type()` | `find_inherited_element<W>()` | Much shorter! |
| `find_render_object()` | `render_object()` | Property-like |

#### Element

| Old | New | Notes |
|-----|-----|-------|
| `mount()` | `mount()` | ✅ Already good |
| `unmount()` | `unmount()` | ✅ Already good |
| `mark_dirty()` | `mark_dirty()` | ✅ Already good |
| `is_dirty()` | `is_dirty()` | ✅ Already good |
| `visit_children()` | `walk_children()` | Iterator pattern |
| `visit_children_mut()` | `walk_children_mut()` | Iterator pattern |
| `render_object()` | `render_object()` | ✅ Already good |
| `render_object_mut()` | `render_object_mut()` | ✅ Already good |
| `widget_type_id()` | `widget_type_id()` | ✅ Already good |
| `child_ids()` | `children()` | Shorter, return iterator |

#### Widget

| Old | New | Notes |
|-----|-----|-------|
| `create_element()` | `into_element(self)` | Consuming method |
| `can_update()` | `can_update_with()` | Clearer |

#### ElementTree

| Old | New | Notes |
|-----|-----|-------|
| `mount_root()` | `set_root()` | Clearer |
| `mount_child()` | `insert_child()` | Standard collection name |
| `unmount_element()` | `remove()` | Standard collection name |
| `update_element()` | `update()` | Shorter |
| `mark_element_dirty()` | `mark_dirty()` | Shorter |
| `rebuild_dirty_elements()` | `rebuild()` | Shorter |
| `visit_all_elements()` | `iter()` | Iterator pattern |
| `visit_all_elements_mut()` | `iter_mut()` | Iterator pattern |
| `get_element()` | `get()` | Standard collection name |
| `get_element_mut()` | `get_mut()` | Standard collection name |
| `has_dirty_elements()` | `has_dirty()` | Shorter |

---

### B. Type Renaming & Simplification

#### Remove Flutter Terminology

| Old | New | Rationale |
|-----|-----|-----------|
| `BuildContext` | `Context` | Shorter, "build" is obvious |
| `StatelessWidget` | Keep | This is actually clear |
| `StatefulWidget` | Keep | This is actually clear |
| `InheritedWidget` | `Provider<T>` | More Rust-like, clearer |
| `InheritedElement` | `ProviderElement<T>` | Consistent |
| `ComponentElement` | `CompositeElement` | More accurate term |
| `PipelineOwner` | `RenderPipeline` | Clearer |

#### Simplify Generic Names

```rust
// OLD (verbose)
find_ancestor_widget_of_type::<MyWidget>()

// NEW (clean)
find_ancestor::<MyWidget>()
```

---

### C. Error Handling Revolution

#### Create Custom Error Types

```rust
// crates/flui_core/src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Element {0} not found in tree")]
    ElementNotFound(ElementId),

    #[error("Invalid parent-child relationship")]
    InvalidHierarchy,

    #[error("Element {0} is not mounted")]
    NotMounted(ElementId),

    #[error("Cannot update element: type mismatch")]
    TypeMismatch,

    #[error("Rebuild failed: {0}")]
    RebuildFailed(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
```

#### Update API Signatures

```rust
// OLD (panics or returns None silently)
pub fn mount_child(&mut self, parent: ElementId, widget: Box<dyn Widget>, slot: usize) -> Option<ElementId>

// NEW (explicit error handling)
pub fn insert_child(&mut self, parent: ElementId, widget: Box<dyn Widget>, slot: usize) -> Result<ElementId>
```

---

### D. Module Structure Revolution

```
crates/flui_core/src/
├── lib.rs                    # Public API, re-exports
├── error.rs                  # Error types
│
├── foundation/               # Core primitives
│   ├── mod.rs
│   ├── id.rs                # ElementId, WidgetId
│   ├── slot.rs              # Slot type
│   └── lifecycle.rs         # Lifecycle states
│
├── widget/                   # Widget system
│   ├── mod.rs               # Widget trait
│   ├── stateless.rs         # StatelessWidget
│   ├── stateful.rs          # StatefulWidget + State
│   └── provider.rs          # Provider<T> (was InheritedWidget)
│
├── element/                  # Element system
│   ├── mod.rs               # Element trait, ElementTree
│   ├── composite.rs         # CompositeElement (was ComponentElement)
│   ├── stateful.rs          # StatefulElement
│   ├── provider.rs          # ProviderElement<T>
│   └── render/              # RenderObject elements
│       ├── mod.rs
│       ├── leaf.rs          # LeafRenderElement
│       ├── single.rs        # SingleChildRenderElement
│       └── multi.rs         # MultiChildRenderElement
│
├── render/                   # Rendering system
│   ├── mod.rs               # RenderObject trait
│   ├── widget.rs            # RenderWidget traits
│   └── parent_data.rs       # ParentData system
│
├── context/                  # Build context
│   └── mod.rs               # Context (was BuildContext)
│
├── tree/                     # Tree management
│   ├── mod.rs
│   ├── element_tree.rs      # ElementTree
│   └── pipeline.rs          # RenderPipeline (was PipelineOwner)
│
└── constraints/              # Layout constraints
    └── mod.rs               # BoxConstraints
```

---

### E. Trait Design Improvements

#### 1. Widget Trait - Use Associated Types

```rust
// OLD
pub trait Widget: DynClone + Downcast + Debug + Send + Sync {
    fn create_element(&self) -> Box<dyn Element>;
}

// NEW
pub trait Widget: Debug + Send + Sync + 'static {
    type Element: Element;

    /// Consume self and create element
    fn into_element(self) -> Self::Element;

    /// Check if can update with new widget
    fn can_update_with(&self, other: &Self) -> bool {
        true // Default: always can update same type
    }
}
```

#### 2. Element Trait - Iterators not Visitors

```rust
// OLD (visitor pattern - not Rust-like)
fn visit_children(&self, visitor: &mut dyn FnMut(&dyn Element));

// NEW (iterator pattern - Rust idiomatic)
fn children(&self) -> impl Iterator<Item = &dyn Element>;
fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn Element>;
```

#### 3. Context - Builder Pattern

```rust
// OLD
context.find_ancestor_widget_of_type::<MyWidget>()

// NEW - support chaining
context
    .ancestors()
    .find_map(|elem| elem.as_widget::<MyWidget>())
```

---

### F. Performance Optimizations

#### 1. Replace Arc<RwLock<Tree>> with Channels

```rust
// OLD (shared mutable state - locks everywhere)
tree: Arc<RwLock<ElementTree>>

// NEW (message passing - more Rust-like)
enum TreeCommand {
    Insert { parent: ElementId, widget: Box<dyn Widget> },
    Remove { id: ElementId },
    MarkDirty { id: ElementId },
}

struct ElementTree {
    sender: mpsc::Sender<TreeCommand>,
    // Tree state owned by single thread
}
```

#### 2. Use SmallVec for Children

```rust
use smallvec::SmallVec;

// Most widgets have 0-3 children - avoid heap allocation
type ChildList = SmallVec<[ElementId; 3]>;
```

#### 3. Intern Strings

```rust
// Widget type names, keys - use string interning
use string_cache::DefaultAtom as Atom;

struct ElementId {
    id: u64,
    type_name: Atom, // Interned, cheap to clone
}
```

---

### G. Type Safety Improvements

#### 1. Typed Element References

```rust
// OLD (runtime downcasting)
let elem = tree.get_element(id).unwrap();
let stateful = elem.downcast_ref::<StatefulElement>().unwrap();

// NEW (typed at compile time)
struct ElementRef<'a, E: Element> {
    inner: &'a E,
    id: ElementId,
}

let stateful: ElementRef<StatefulElement> = tree.get(id)?;
```

#### 2. PhantomData for Type Safety

```rust
// Ensure Context<W> only used with correct widget type
pub struct Context<W: Widget> {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
    _phantom: PhantomData<W>,
}
```

#### 3. Must-Use Annotations

```rust
#[must_use = "call rebuild() to apply dirty marks"]
pub fn mark_dirty(&mut self, id: ElementId) -> DirtyGuard;
```

---

## 🚀 Implementation Strategy

### Phase 1: Foundation (Week 1)

**Goal**: Set up new module structure, error types

- [ ] Create new module structure
- [ ] Create `error.rs` with custom error types
- [ ] Create `foundation/` module with core types
- [ ] Update `lib.rs` exports

**Deliverable**: Compiling code with new structure

### Phase 2: Widget & Element API (Week 1-2)

**Goal**: Rust-idiomatic traits

- [ ] Rewrite `Widget` trait with associated types
- [ ] Rewrite `Element` trait with iterators
- [ ] Remove visitor pattern, use iterators
- [ ] Add proper error handling to all methods
- [ ] Rename all methods to snake_case

**Deliverable**: New trait APIs with tests

### Phase 3: Context Redesign (Week 2)

**Goal**: Ergonomic, Rust-like Context API

- [ ] Rename `BuildContext` → `Context`
- [ ] Rename all methods (see table above)
- [ ] Add iterator-based tree traversal
- [ ] Remove verbose method names

**Deliverable**: Clean Context API

### Phase 4: Tree Management (Week 2-3)

**Goal**: Replace locks with channels

- [ ] Design message-passing API
- [ ] Implement `ElementTree` with channels
- [ ] Remove `Arc<RwLock<Tree>>` from Context
- [ ] Performance testing

**Deliverable**: Lock-free tree operations

### Phase 5: Update Dependents (Week 3)

**Goal**: Update all crates to new API

- [ ] Update `flui_widgets` (17 widgets)
- [ ] Update `flui_rendering` (14 RenderObjects)
- [ ] Update `flui_app`
- [ ] Update all examples
- [ ] Update tests

**Deliverable**: All crates working with new API

### Phase 6: Documentation (Week 4)

**Goal**: Comprehensive docs

- [ ] Rustdoc for all public APIs
- [ ] Migration guide from old API
- [ ] Examples in docs
- [ ] Book chapter on new architecture

**Deliverable**: Complete documentation

---

## 📊 Success Metrics

- ✅ **Zero** `unwrap()` in public APIs
- ✅ **100%** snake_case naming
- ✅ **All** fallible operations return `Result<T, E>`
- ✅ **Zero** panics in normal code paths
- ✅ **Performance**: ≥ current performance (no regressions)
- ✅ **Tests**: All tests passing
- ✅ **Coverage**: ≥ 90% code coverage
- ✅ **Docs**: 100% public API documented

---

## 🔄 Migration Path

### For Users

Create a migration tool:

```rust
// flui_migrate CLI tool
flui_migrate --from 0.1.0 --to 0.2.0 src/

// Auto-renames:
// - context.mark_needs_build() → context.mark_dirty()
// - context.find_ancestor_widget_of_type::<W>() → context.find_ancestor::<W>()
// etc.
```

### Compatibility Layer (Optional)

Provide deprecated aliases for 1 version:

```rust
#[deprecated(since = "0.2.0", note = "use `mark_dirty` instead")]
pub fn mark_needs_build(&mut self) {
    self.mark_dirty()
}
```

---

## 💥 Breaking Changes Summary

### API Breaks

1. **All method names** changed to snake_case
2. **Many method names** shortened/simplified
3. **Return types** changed from `Option<T>` to `Result<T, E>`
4. **Trait signatures** changed (associated types, iterators)
5. **Module paths** changed completely
6. **Some types renamed** (BuildContext → Context, etc.)

### Behavioral Changes

1. **Errors instead of panics** - code that panicked now returns errors
2. **Explicit error handling** - must use `?` or `.unwrap()`
3. **Consuming methods** - `into_element()` takes ownership

### Performance Impacts

1. **Improved**: Lock-free tree operations (message passing)
2. **Improved**: SmallVec for small child lists
3. **Improved**: String interning for type names
4. **Neutral**: Most other changes zero-cost

---

## ⚠️ Risks & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Break all user code | HIGH | Migration tool, deprecation warnings |
| Performance regression | MEDIUM | Benchmarks before/after, optimize |
| Deadlocks with channels | LOW | Careful design, testing |
| Incomplete migration | MEDIUM | Staged rollout, feature flags |

---

## 🎯 Decision: Go/No-Go?

### Pros
- ✅ **Modern Rust code** - idiomatic, safe, fast
- ✅ **Better ergonomics** - shorter names, clearer APIs
- ✅ **Type safety** - compile-time guarantees
- ✅ **Performance** - lock-free, zero-cost abstractions
- ✅ **Maintainability** - cleaner code, better structure

### Cons
- ❌ **Breaking changes** - all user code breaks
- ❌ **Migration effort** - 2-3 weeks of work
- ❌ **Risk** - might introduce bugs

### Recommendation

**🚀 GO FOR IT!**

We're at 15% overall progress - perfect time for aggressive refactoring.
Better to break now than after 1.0 release.

---

**Status**: 📋 **PROPOSED - AWAITING APPROVAL**
**Estimated Time**: 3-4 weeks
**Risk Level**: HIGH (breaking changes)
**Reward**: VERY HIGH (modern, idiomatic codebase)

**Next Step**: Get approval and start Phase 1! 💪
