# Phase 6: Enhanced InheritedWidget System - Design Document

**Date:** 2025-10-20
**Status:** 🚧 In Progress
**Priority:** HIGH
**Complexity:** MEDIUM

---

## Overview

This phase enhances the existing `InheritedWidget` system with proper dependency tracking, enabling efficient state propagation and selective rebuilds. This matches Flutter's InheritedWidget behavior where only widgets that actually depend on inherited data are rebuilt when it changes.

### Current State

✅ **Already Implemented:**
- Basic `InheritedWidget` trait with `update_should_notify()`
- `InheritedElement` with widget storage
- Basic `Context::depend_on()` method
- ProxyWidget hierarchy (Phase 5)

❌ **Missing:**
- Dependency tracking (who depends on this InheritedWidget?)
- Selective notification (only notify actual dependents)
- Aspect-based dependencies (partial rebuilds)
- Complete BuildContext API for inherited widgets

### Goals

1. **Track Dependencies**: InheritedElement tracks which elements depend on it
2. **Selective Notification**: Only notify elements that registered dependencies
3. **Efficient Updates**: Avoid rebuilding entire subtrees unnecessarily
4. **Complete API**: Full Flutter-compatible BuildContext methods
5. **Zero Breaking Changes**: Maintain backward compatibility

---

## Architecture

### 1. Dependency Tracking System

```rust
/// Information about a dependency on an InheritedWidget
#[derive(Debug, Clone)]
pub struct DependencyInfo {
    /// The element that depends on the InheritedWidget
    pub dependent_id: ElementId,

    /// Optional aspect for partial dependencies (future enhancement)
    pub aspect: Option<Box<dyn Any + Send + Sync>>,
}

/// Tracks dependencies for an InheritedElement
pub struct DependencyTracker {
    /// Map from dependent element ID to dependency info
    dependents: HashMap<ElementId, DependencyInfo>,
}

impl DependencyTracker {
    pub fn new() -> Self {
        Self {
            dependents: HashMap::new(),
        }
    }

    /// Register a dependency
    pub fn add_dependent(&mut self, dependent_id: ElementId) {
        self.dependents.insert(dependent_id, DependencyInfo {
            dependent_id,
            aspect: None,
        });
    }

    /// Remove a dependency (when element is unmounted)
    pub fn remove_dependent(&mut self, dependent_id: ElementId) {
        self.dependents.remove(&dependent_id);
    }

    /// Get all dependents
    pub fn dependents(&self) -> impl Iterator<Item = &DependencyInfo> + '_ {
        self.dependents.values()
    }

    /// Check if an element depends on this
    pub fn has_dependent(&self, dependent_id: ElementId) -> bool {
        self.dependents.contains_key(&dependent_id)
    }

    /// Get count of dependents
    pub fn dependent_count(&self) -> usize {
        self.dependents.len()
    }
}
```

### 2. Enhanced InheritedElement

```rust
pub struct InheritedElement<W: InheritedWidget> {
    // ... existing fields
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    child: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    tree: Option<Arc<RwLock<ElementTree>>>,

    // NEW: Dependency tracking
    dependencies: DependencyTracker,
}

impl<W: InheritedWidget> InheritedElement<W> {
    /// Register a dependency from another element
    pub fn update_dependencies(&mut self, dependent_id: ElementId) {
        self.dependencies.add_dependent(dependent_id);
        tracing::trace!(
            "InheritedElement({:?}): Added dependency from {:?}",
            self.id,
            dependent_id
        );
    }

    /// Notify a single dependent that the widget changed
    fn notify_dependent(&mut self, old_widget: &W, dependent_id: ElementId) {
        if !self.widget.update_should_notify(old_widget) {
            return;
        }

        // Mark the dependent as dirty
        if let Some(tree) = &self.tree {
            let tree_guard = tree.read();
            if let Some(element) = tree_guard.get(dependent_id) {
                // The dependent element will rebuild on next frame
                element.mark_needs_build();
            }
        }
    }

    /// Notify all dependents that the widget changed
    pub fn notify_clients(&mut self, old_widget: &W) {
        if !self.widget.update_should_notify(old_widget) {
            tracing::trace!(
                "InheritedElement({:?}): update_should_notify = false, skipping notifications",
                self.id
            );
            return;
        }

        let dependent_count = self.dependencies.dependent_count();
        tracing::info!(
            "InheritedElement({:?}): Notifying {} dependents",
            self.id,
            dependent_count
        );

        // Collect dependent IDs to avoid borrow checker issues
        let dependent_ids: Vec<ElementId> = self.dependencies
            .dependents()
            .map(|info| info.dependent_id)
            .collect();

        for dependent_id in dependent_ids {
            self.notify_dependent(old_widget, dependent_id);
        }
    }
}
```

### 3. Enhanced Context Methods

Add these methods to `Context` in `context/mod.rs`:

```rust
impl<'a> Context<'a> {
    /// Create a dependency on an InheritedElement
    ///
    /// Low-level method, prefer using typed methods like
    /// `depend_on_inherited_widget_of_exact_type<T>()`.
    pub fn depend_on_inherited_element(
        &self,
        ancestor_id: ElementId,
        aspect: Option<Box<dyn Any + Send + Sync>>,
    ) {
        if let Some(tree) = &self.tree {
            let mut tree_guard = tree.write();
            if let Some(ancestor) = tree_guard.get_mut(ancestor_id) {
                // Call update_dependencies on the InheritedElement
                // This requires downcasting to InheritedElement<W>
                // which is handled via the AnyElement trait
                ancestor.register_dependency(self.id, aspect);
            }
        }
    }

    /// Get and depend on an InheritedWidget of exact type T
    ///
    /// This creates a dependency, so the current element will rebuild
    /// when the InheritedWidget changes.
    ///
    /// Returns None if no ancestor of type T is found.
    pub fn depend_on_inherited_widget_of_exact_type<T: InheritedWidget>(
        &self,
    ) -> Option<&T> {
        // Find the InheritedWidget ancestor
        let ancestor_id = self.find_ancestor_inherited_element_of_type::<T>()?;

        // Register dependency
        self.depend_on_inherited_element(ancestor_id, None);

        // Return the widget
        if let Some(tree) = &self.tree {
            let tree_guard = tree.read();
            if let Some(element) = tree_guard.get(ancestor_id) {
                return element.widget_as::<T>();
            }
        }

        None
    }

    /// Get InheritedWidget without creating dependency
    ///
    /// This does NOT cause rebuilds when the widget changes.
    /// Use this when you only need to read the value once.
    pub fn get_inherited_widget_of_exact_type<T: InheritedWidget>(
        &self,
    ) -> Option<&T> {
        let ancestor_id = self.find_ancestor_inherited_element_of_type::<T>()?;

        if let Some(tree) = &self.tree {
            let tree_guard = tree.read();
            if let Some(element) = tree_guard.get(ancestor_id) {
                return element.widget_as::<T>();
            }
        }

        None
    }

    /// Get the InheritedElement of exact type T
    ///
    /// Returns the element ID, useful for advanced scenarios.
    pub fn get_element_for_inherited_widget_of_exact_type<T: InheritedWidget>(
        &self,
    ) -> Option<ElementId> {
        self.find_ancestor_inherited_element_of_type::<T>()
    }

    /// Helper: Find ancestor InheritedElement of type T
    fn find_ancestor_inherited_element_of_type<T: InheritedWidget>(
        &self,
    ) -> Option<ElementId> {
        if let Some(tree) = &self.tree {
            let tree_guard = tree.read();

            // Walk up the tree looking for InheritedElement<T>
            let mut current = self.parent;
            while let Some(parent_id) = current {
                if let Some(element) = tree_guard.get(parent_id) {
                    // Check if this is an InheritedElement with widget type T
                    if element.widget_is::<T>() {
                        return Some(parent_id);
                    }
                    current = element.parent();
                } else {
                    break;
                }
            }
        }

        None
    }
}
```

### 4. AnyElement Trait Extensions

Add to `AnyElement` trait to support dependency registration:

```rust
pub trait AnyElement: DowncastSync + fmt::Debug {
    // ... existing methods

    /// Register a dependency on this element (for InheritedElement)
    fn register_dependency(
        &mut self,
        dependent_id: ElementId,
        aspect: Option<Box<dyn Any + Send + Sync>>,
    ) {
        // Default: no-op (only InheritedElement implements this)
    }

    /// Get widget as specific type (for Context methods)
    fn widget_as<T: InheritedWidget>(&self) -> Option<&T> {
        None
    }

    /// Check if widget is specific type (for Context methods)
    fn widget_is<T: InheritedWidget>(&self) -> bool {
        false
    }
}
```

Then implement in `InheritedElement`:

```rust
impl<W: InheritedWidget> AnyElement for InheritedElement<W> {
    // ... existing implementations

    fn register_dependency(
        &mut self,
        dependent_id: ElementId,
        _aspect: Option<Box<dyn Any + Send + Sync>>,
    ) {
        self.update_dependencies(dependent_id);
    }

    fn widget_as<T: InheritedWidget>(&self) -> Option<&T> {
        // Attempt downcast to T
        if TypeId::of::<W>() == TypeId::of::<T>() {
            // SAFETY: We just checked the type IDs match
            unsafe {
                Some(&*(&self.widget as *const W as *const T))
            }
        } else {
            None
        }
    }

    fn widget_is<T: InheritedWidget>(&self) -> bool {
        TypeId::of::<W>() == TypeId::of::<T>()
    }
}
```

---

## Implementation Plan

### Step 1: Create DependencyTracker ✅
- [ ] Create `context/dependency.rs` with `DependencyInfo` and `DependencyTracker`
- [ ] Add comprehensive unit tests
- [ ] Export from `context/mod.rs`

### Step 2: Enhance InheritedElement ✅
- [ ] Add `dependencies: DependencyTracker` field
- [ ] Implement `update_dependencies()`
- [ ] Implement `notify_dependent()`
- [ ] Enhance `notify_clients()` to use tracker
- [ ] Update `unmount()` to clean up dependencies

### Step 3: Extend AnyElement Trait ✅
- [ ] Add `register_dependency()` method
- [ ] Add `widget_as<T>()` method
- [ ] Add `widget_is<T>()` method
- [ ] Implement in all element types (no-op for most)
- [ ] Implement properly in `InheritedElement`

### Step 4: Enhanced Context Methods ✅
- [ ] Add `depend_on_inherited_element()`
- [ ] Add `depend_on_inherited_widget_of_exact_type<T>()`
- [ ] Add `get_inherited_widget_of_exact_type<T>()`
- [ ] Add `get_element_for_inherited_widget_of_exact_type<T>()`
- [ ] Add `find_ancestor_inherited_element_of_type<T>()` helper

### Step 5: Testing ✅
- [ ] Test dependency registration
- [ ] Test selective notification (only dependents rebuild)
- [ ] Test multiple dependents
- [ ] Test dependency cleanup on unmount
- [ ] Test nested InheritedWidgets
- [ ] Test `update_should_notify()` integration
- [ ] Performance test with large trees

### Step 6: Documentation ✅
- [ ] Update `InheritedWidget` trait docs
- [ ] Add examples for new Context methods
- [ ] Document dependency tracking behavior
- [ ] Create completion document

---

## Performance Considerations

### Before Phase 6
```rust
// Update InheritedWidget
theme.set_color(Color::RED);

// PROBLEM: Entire subtree rebuilds, even if only 1 widget depends on theme!
// With 1000 descendants, all 1000 rebuild
```

### After Phase 6
```rust
// Update InheritedWidget
theme.set_color(Color::RED);

// SOLUTION: Only widgets that called depend_on() rebuild!
// With 1000 descendants but only 5 dependents, only 5 rebuild
// ~200x faster for typical apps!
```

### Expected Performance Impact

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| Theme change (1 dependent out of 1000) | 1000 rebuilds | 1 rebuild | **1000x** |
| Localization change (100 dependents out of 1000) | 1000 rebuilds | 100 rebuilds | **10x** |
| Nested inherited widgets (10 levels deep) | Full tree rebuild | Targeted rebuilds | **50-100x** |

---

## Example Usage

### Before Phase 6 (Basic)
```rust
// Create theme
let theme = Theme::new(Color::BLUE);

// Build widget tree
let app = InheritedWidget::new(theme, MyApp);

// In MyApp.build():
let context: Context = ...;
let theme = context.depend_on::<Theme>(); // Basic, no tracking
```

### After Phase 6 (Enhanced)
```rust
// Create theme
let theme = Theme::new(Color::BLUE);

// Build widget tree
let app = InheritedWidget::new(theme, MyApp);

// In MyApp.build():
let context: Context = ...;

// Option 1: Create dependency (rebuilds when theme changes)
let theme = context.depend_on_inherited_widget_of_exact_type::<Theme>();

// Option 2: No dependency (doesn't rebuild)
let theme = context.get_inherited_widget_of_exact_type::<Theme>();

// Option 3: Get element directly
let theme_element = context.get_element_for_inherited_widget_of_exact_type::<Theme>();
```

---

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_dependency_tracker_add_remove() {
    let mut tracker = DependencyTracker::new();
    let id1 = ElementId::new();

    tracker.add_dependent(id1);
    assert_eq!(tracker.dependent_count(), 1);

    tracker.remove_dependent(id1);
    assert_eq!(tracker.dependent_count(), 0);
}

#[test]
fn test_multiple_dependents() {
    let mut tracker = DependencyTracker::new();
    let id1 = ElementId::new();
    let id2 = ElementId::new();

    tracker.add_dependent(id1);
    tracker.add_dependent(id2);

    assert_eq!(tracker.dependent_count(), 2);
    assert!(tracker.has_dependent(id1));
    assert!(tracker.has_dependent(id2));
}
```

### Integration Tests
```rust
#[test]
fn test_selective_rebuild() {
    // Create tree with InheritedWidget
    let mut tree = ElementTree::new();
    let theme = TestTheme { color: Color::BLUE };

    // Mount widgets that depend on theme
    let dependent1 = /* ... */;
    let dependent2 = /* ... */;
    let non_dependent = /* ... */;

    // Update theme
    let new_theme = TestTheme { color: Color::RED };

    // Verify: Only dependent1 and dependent2 rebuild
    // non_dependent should NOT rebuild
}

#[test]
fn test_nested_inherited_widgets() {
    // Create tree with nested InheritedWidgets
    // Theme -> Locale -> MediaQuery

    // Update Theme
    // Verify only Theme dependents rebuild

    // Update Locale
    // Verify only Locale dependents rebuild
}
```

---

## Breaking Changes

**None!** This is a backward-compatible enhancement.

Existing code using `context.depend_on()` continues to work, but new code can use the enhanced API for better performance.

---

## Files to Create/Modify

### New Files
1. `crates/flui_core/src/context/dependency.rs` (~150 lines)
   - `DependencyInfo` struct
   - `DependencyTracker` struct with methods

### Modified Files
1. `crates/flui_core/src/widget/provider.rs` (~+80 lines)
   - Add `dependencies` field to `InheritedElement`
   - Implement `update_dependencies()`, `notify_dependent()`, `notify_clients()`

2. `crates/flui_core/src/element/mod.rs` (~+20 lines)
   - Add `register_dependency()` to `AnyElement` trait
   - Add `widget_as()` and `widget_is()` to `AnyElement` trait

3. `crates/flui_core/src/context/mod.rs` (~+100 lines)
   - Add `depend_on_inherited_element()`
   - Add `depend_on_inherited_widget_of_exact_type<T>()`
   - Add `get_inherited_widget_of_exact_type<T>()`
   - Add `get_element_for_inherited_widget_of_exact_type<T>()`

4. `crates/flui_core/src/context/inherited.rs` (if needed, ~+50 lines)
   - Helper methods for inherited widget lookup

### Test Files
1. `crates/flui_core/tests/dependency_tracking_tests.rs` (new, ~400 lines)
   - Comprehensive dependency tracking tests

---

## Success Criteria

✅ **Phase 6 is complete when:**

1. [ ] `DependencyTracker` implemented with full tests
2. [ ] `InheritedElement` tracks dependencies
3. [ ] `notify_clients()` only notifies actual dependents
4. [ ] All new Context methods work correctly
5. [ ] 20+ comprehensive tests passing
6. [ ] Zero breaking changes to existing code
7. [ ] Performance improvement measured (100x+ for typical apps)
8. [ ] Full documentation complete

---

## Comparison with Flutter

| Feature | Flutter | Flui (Phase 6) | Status |
|---------|---------|----------------|--------|
| `dependOnInheritedElement()` | ✅ | ✅ | Implementing |
| `dependOnInheritedWidgetOfExactType<T>()` | ✅ | ✅ | Implementing |
| `getInheritedWidgetOfExactType<T>()` | ✅ | ✅ | Implementing |
| `getElementForInheritedWidgetOfExactType<T>()` | ✅ | ✅ | Implementing |
| Dependency tracking | ✅ | ✅ | Implementing |
| Selective notification | ✅ | ✅ | Implementing |
| Aspect-based dependencies | ✅ | ⏸️ | Future |
| InheritedModel | ✅ | ⏸️ | Future |

**Result:** Core dependency tracking **100% Flutter-compatible**!

---

## Next Steps After Phase 6

Once Phase 6 is complete, the next priorities are:

1. **Phase 7**: Enhanced Context Methods (tree navigation)
2. **Phase 10**: Error Handling & Debugging
3. **Phase 11**: Notification System

---

**Last Updated:** 2025-10-20
**Status:** 🚧 Design Complete, Ready for Implementation
**Estimated Time:** 4-6 hours
