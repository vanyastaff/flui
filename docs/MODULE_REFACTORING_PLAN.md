# Module Refactoring Plan

> **Goal:** Split large `mod.rs` files into logical, manageable modules
> **Principle:** Each file should be 200-400 lines max (excluding tests)

---

## 🎯 Target Structure

### Current Problems

| File | Lines | Status | Issue |
|------|-------|--------|-------|
| `element/mod.rs` | **1381** | 🔴 Too large | Mix of traits, types, impls |
| `tree/element_tree.rs` | **973** | 🟡 Large | Could split |
| `widget/mod.rs` | **820** | 🟡 Large | Mix of traits and types |

---

## 📁 Proposed Structure

### A. `element/` Module Refactoring

**Current:**
```
element/
├── mod.rs (1381 lines) ❌ TOO BIG
└── render/
    ├── mod.rs
    ├── leaf.rs
    ├── single.rs
    └── multi.rs
```

**Proposed:**
```
element/
├── mod.rs (50-100 lines)           # Re-exports only
├── traits.rs (200 lines)           # Element trait
├── lifecycle.rs (100 lines)        # ElementLifecycle enum + InactiveElements
├── component.rs (200 lines)        # ComponentElement<W>
├── stateful.rs (250 lines)         # StatefulElement
├── render_object.rs (150 lines)    # RenderObjectElement<W>
└── render/
    ├── mod.rs (50 lines)           # Re-exports
    ├── traits.rs (200 lines)       # RenderWidget traits
    ├── leaf.rs (371 lines) ✅
    ├── single.rs (448 lines) 🟡
    └── multi.rs (487 lines) 🟡
```

**Breakdown:**
- `traits.rs` - Element trait definition (~200 lines)
- `lifecycle.rs` - ElementLifecycle + InactiveElements (~100 lines)
- `component.rs` - ComponentElement implementation (~200 lines)
- `stateful.rs` - StatefulElement implementation (~250 lines)
- `render_object.rs` - RenderObjectElement implementation (~150 lines)

---

### B. `widget/` Module Refactoring

**Current:**
```
widget/
├── mod.rs (820 lines) ❌ TOO BIG
└── provider.rs (593 lines) 🟡
```

**Proposed:**
```
widget/
├── mod.rs (50-100 lines)           # Re-exports
├── traits.rs (300 lines)           # Widget, StatelessWidget, StatefulWidget, State traits
├── lifecycle.rs (150 lines)        # StateLifecycle enum
├── into_widget.rs (50 lines)       # IntoWidget trait
└── provider.rs (593 lines) 🟡      # InheritedWidget (keep as is for now)
```

**Breakdown:**
- `traits.rs` - All widget traits (~300 lines)
- `lifecycle.rs` - StateLifecycle enum (~150 lines)
- `into_widget.rs` - IntoWidget trait (~50 lines)

---

### C. `tree/` Module (Optional)

**Current:**
```
tree/
├── mod.rs (13 lines) ✅
├── element_tree.rs (973 lines) 🟡
├── build_owner.rs (476 lines) ✅
└── pipeline.rs (210 lines) ✅
```

**Proposed (if we split element_tree.rs):**
```
tree/
├── mod.rs (50 lines)
├── element_tree/
│   ├── mod.rs (100 lines)          # Core tree struct
│   ├── mount.rs (200 lines)        # Mounting operations
│   ├── update.rs (200 lines)       # Update operations
│   ├── rebuild.rs (200 lines)      # Rebuild operations
│   └── traverse.rs (200 lines)     # Tree traversal
├── build_owner.rs (476 lines) ✅
└── pipeline.rs (210 lines) ✅
```

**Note:** This is optional - 973 lines is large but manageable.

---

## 🔧 Implementation Plan

### Phase 1: Element Module (High Priority)

#### Step 1: Create new files

```bash
cd crates/flui_core/src/element

# Create new module files
touch traits.rs lifecycle.rs component.rs stateful.rs render_object.rs
```

#### Step 2: Move code

1. **`traits.rs`** - Move Element trait
   - Lines 83-303 from mod.rs
   - Element trait definition + all methods

2. **`lifecycle.rs`** - Move lifecycle types
   - Lines 41-67: ElementLifecycle enum
   - Lines 336-399: InactiveElements struct

3. **`component.rs`** - Move ComponentElement
   - Lines 404-579: ComponentElement<W>
   - All impls for ComponentElement

4. **`stateful.rs`** - Move StatefulElement
   - Lines 580-782: StatefulElement
   - All impls for StatefulElement

5. **`render_object.rs`** - Move RenderObjectElement
   - Lines 783-908: RenderObjectElement<W>
   - All impls for RenderObjectElement

#### Step 3: Update `mod.rs`

```rust
// element/mod.rs (new, ~100 lines)

//! Element tree - mutable state holders for widgets

// Submodules
mod traits;
mod lifecycle;
mod component;
mod stateful;
mod render_object;
pub mod render;

// Re-exports
pub use traits::Element;
pub use lifecycle::{ElementLifecycle, InactiveElements};
pub use component::ComponentElement;
pub use stateful::StatefulElement;
pub use render_object::RenderObjectElement;

#[cfg(test)]
mod tests {
    // Keep tests in mod.rs or move to tests/ submodule
}
```

---

### Phase 2: Widget Module (Medium Priority)

#### Step 1: Create new files

```bash
cd crates/flui_core/src/widget

touch traits.rs lifecycle.rs into_widget.rs
```

#### Step 2: Move code

1. **`traits.rs`** - All widget traits
   - Widget trait
   - StatelessWidget trait
   - StatefulWidget trait
   - State trait

2. **`lifecycle.rs`** - StateLifecycle enum
   - Lines 206-240: StateLifecycle enum

3. **`into_widget.rs`** - IntoWidget trait
   - IntoWidget trait + blanket impl

#### Step 3: Update `mod.rs`

```rust
// widget/mod.rs (new, ~100 lines)

//! Core Widget trait - the foundation of the widget system

mod traits;
mod lifecycle;
mod into_widget;
pub mod provider;

// Re-exports
pub use traits::{Widget, StatelessWidget, StatefulWidget, State};
pub use lifecycle::StateLifecycle;
pub use into_widget::IntoWidget;
pub use provider::{InheritedWidget, InheritedElement};

#[cfg(test)]
mod tests {
    // Tests
}
```

---

### Phase 3: Tree Module (Optional, Low Priority)

Only if element_tree.rs becomes too large (>1200 lines).

For now: **SKIP** - 973 lines is acceptable.

---

## 📋 File Size Targets

| Category | Target | Max |
|----------|--------|-----|
| **Trait definitions** | 150-300 lines | 400 |
| **Type implementations** | 150-250 lines | 350 |
| **Helper utilities** | 50-150 lines | 200 |
| **mod.rs (re-exports)** | 50-100 lines | 150 |

---

## ✅ Benefits

### 1. **Readability**
- Easy to find specific types
- Clear separation of concerns
- Each file has single responsibility

### 2. **Maintainability**
- Smaller files easier to edit
- Reduce merge conflicts
- Easier to review PRs

### 3. **Compilation**
- Parallel compilation of separate files
- Smaller compilation units
- Faster incremental rebuilds

### 4. **Navigation**
- Jump to traits.rs for trait definitions
- Jump to component.rs for ComponentElement
- Clear mental model

### 5. **Testing**
- Tests can be in separate test modules
- Easier to organize test helpers

---

## 🎯 Rust Best Practices

### File Naming Conventions

```rust
// ✅ GOOD
traits.rs          // Trait definitions
component.rs       // ComponentElement implementation
lifecycle.rs       // Lifecycle enums/types

// ❌ AVOID
element_trait.rs   // Redundant "trait" suffix
comp_elem.rs       // Unclear abbreviations
```

### Module Organization

```rust
// ✅ GOOD - Clear hierarchy
element/
  mod.rs           # Re-exports
  traits.rs        # Element trait
  component.rs     # ComponentElement

// ❌ AVOID - Flat structure
element.rs         # Everything in one file
element_impl.rs    # Unclear what's inside
```

### Import Patterns

```rust
// ✅ GOOD - Clean re-exports
pub use crate::element::{Element, ComponentElement, StatefulElement};

// ❌ AVOID - Exposing internal structure
pub use crate::element::component::ComponentElement;
pub use crate::element::stateful::StatefulElement;
```

---

## 🚀 Migration Strategy

### Step 1: Create branch
```bash
git checkout -b refactor/modularize-element-widget
```

### Step 2: Refactor element/ (1-2 hours)
1. Create new files
2. Move code sections
3. Update imports
4. Test compilation
5. Run tests

### Step 3: Refactor widget/ (1 hour)
1. Create new files
2. Move code sections
3. Update imports
4. Test compilation
5. Run tests

### Step 4: Update exports (30 min)
1. Update lib.rs if needed
2. Verify public API unchanged
3. Update documentation

### Step 5: Final verification (30 min)
1. Run full test suite
2. Check no warnings
3. Verify examples compile
4. Update docs if needed

**Total time:** ~4-5 hours

---

## 📊 Expected Results

### Before
```
element/mod.rs:    1381 lines ❌
widget/mod.rs:      820 lines ❌
```

### After
```
element/
  mod.rs:            100 lines ✅
  traits.rs:         200 lines ✅
  lifecycle.rs:      100 lines ✅
  component.rs:      200 lines ✅
  stateful.rs:       250 lines ✅
  render_object.rs:  150 lines ✅
  tests.rs:          381 lines ✅ (moved from mod.rs)

widget/
  mod.rs:            100 lines ✅
  traits.rs:         300 lines ✅
  lifecycle.rs:      150 lines ✅
  into_widget.rs:     50 lines ✅
  tests.rs:          220 lines ✅ (moved from mod.rs)
```

**Result:** All files < 400 lines! 🎉

---

## ✅ Checklist

- [ ] Create element/traits.rs
- [ ] Create element/lifecycle.rs
- [ ] Create element/component.rs
- [ ] Create element/stateful.rs
- [ ] Create element/render_object.rs
- [ ] Update element/mod.rs
- [ ] Create widget/traits.rs
- [ ] Create widget/lifecycle.rs
- [ ] Create widget/into_widget.rs
- [ ] Update widget/mod.rs
- [ ] Test compilation
- [ ] Run full test suite
- [ ] Update documentation
- [ ] Commit changes

---

**Ready to implement?** Let's start with element/ module!
