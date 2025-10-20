# Flui Core Roadmap

> Comprehensive implementation roadmap based on Flutter's framework.dart architecture

## Current Status

**flui-core** currently implements the foundation of the three-tree architecture:

### ✅ Implemented
- **Widget trait system**
  - Base `Widget` trait with `create_element()`, `key()`, `can_update()`
  - `StatelessWidget` trait for stateless components
  - `StatefulWidget` trait with associated `State` type
  - `IntoWidget` helper trait

- **Element system**
  - Base `Element` trait with lifecycle methods
  - `ComponentElement<W>` for StatelessWidget
  - `StatefulElement` for StatefulWidget
  - `RenderObjectElement<W>` for RenderObjectWidget
  - Specialized render elements: `LeafRenderObjectElement`, `SingleChildRenderObjectElement`, `MultiChildRenderObjectElement`

- **Context system**
  - `BuildContext` (aliased as `Context`) for tree navigation
  - Ancestor traversal with iterators
  - `InheritedWidget` and `InheritedElement` for state propagation

- **Foundation**
  - `ElementId` for unique element identification
  - `Slot` for child positioning
  - Lifecycle tracking

- **Tree management**
  - `ElementTree` for element storage and traversal
  - `PipelineOwner` (basic structure)

---

## Roadmap

Based on Flutter's framework.dart (7,461 lines), here's what needs to be implemented:

### Phase 1: Key System Enhancement 🔑

**Priority: HIGH** | **Complexity: MEDIUM**

Flutter has sophisticated key types for widget identification and state preservation:

#### 1.1 Expand Key Types
- [ ] **ObjectKey** - Uses object identity (`identical()`) for equality
- [ ] **GlobalKey<T>** - Unique across entire app
  - [ ] `current_context()` - Get BuildContext at this key
  - [ ] `current_widget()` - Get Widget at this key
  - [ ] `current_state()` - Get State object (for StatefulWidget)
  - [ ] Global key registry in BuildOwner
- [ ] **LabeledGlobalKey<T>** - GlobalKey with debug label
- [ ] **GlobalObjectKey<T>** - GlobalKey using object identity
- [ ] **UniqueKey** - Always unique, never matches any other key

**Files to create:**
- `crates/flui_foundation/src/key.rs` (expand existing)

**Current location:** `flui_foundation::Key` trait exists but only supports basic keys

---

### Phase 2: State Lifecycle Enhancement 🔄

**Priority: HIGH** | **Complexity: HIGH**

Flutter has a sophisticated state lifecycle with multiple callback points:

#### 2.1 State Lifecycle Tracking
- [ ] **State lifecycle enum**
  ```rust
  enum StateLifecycle {
      Created,      // After creation
      Initialized,  // After initState()
      Ready,        // Ready to build
      Defunct,      // After dispose()
  }
  ```

#### 2.2 Additional State Callbacks
Currently we have: `init_state()`, `did_update_widget()`, `dispose()`, `build()`

Need to add:
- [ ] **`did_change_dependencies()`** - Called when InheritedWidget dependencies change
- [ ] **`reassemble()`** - Hot reload support
- [ ] **`deactivate()`** - Called when removed from tree (before dispose)
- [ ] **`activate()`** - Called when reinserted into tree after deactivate

#### 2.3 Mounted State Tracking
- [ ] Add `mounted` property to State
- [ ] Enforce lifecycle rules (e.g., can't call `set_state()` when unmounted)
- [ ] Better error messages for lifecycle violations

**Files to modify:**
- `crates/flui_core/src/widget/mod.rs` (State trait)
- `crates/flui_core/src/element/mod.rs` (StatefulElement)

---

### Phase 3: Enhanced Element Lifecycle 🌳

**Priority: HIGH** | **Complexity: HIGH**

Flutter's Element has a complex lifecycle with inactive/active states:

#### 3.1 Element Lifecycle States
```rust
enum ElementLifecycle {
    Initial,   // Created, not mounted
    Active,    // Mounted in tree
    Inactive,  // Removed, waiting for reactivation
    Defunct,   // Permanently unmounted
}
```

#### 3.2 Inactive Element Management
- [ ] **`_InactiveElements`** class - holds deactivated elements
- [ ] **`deactivate()`** method - remove from tree but keep element
- [ ] **`activate()`** method - reinsert element into tree
- [ ] Support for element reparenting with GlobalKeys
- [ ] Deactivation/reactivation within same frame

#### 3.3 Enhanced Element Methods
Currently missing from Element trait:
- [ ] **`update_child()`** - Smart child update algorithm
  - [ ] Handles null → widget, widget → null, widget → widget
  - [ ] Reuses elements when possible
  - [ ] Creates new elements when needed
- [ ] **`inflate_widget()`** - Create and mount new element from widget
- [ ] **`deactivate_child()`** - Remove child to inactive list
- [ ] **`forget_child()`** - Forget child (for GlobalKey reparenting)
- [ ] **`did_change_dependencies()`** - Propagate dependency changes
- [ ] **`update_slot_for_child()`** - Update child's slot position

**Files to modify:**
- `crates/flui_core/src/element/mod.rs`

---

### Phase 4: BuildOwner & Build Scheduling 🏗️

**Priority: CRITICAL** | **Complexity: HIGH**

Currently `PipelineOwner` exists but needs full BuildOwner functionality:

#### 4.1 Core BuildOwner Features
- [ ] **Dirty element tracking**
  - [ ] `_dirty_elements` list
  - [ ] `schedule_build_for(element)` - Mark element dirty
  - [ ] `build_scope()` - Execute build pass
  - [ ] Sort dirty elements by depth before building

- [ ] **Global key registry**
  - [ ] `_global_key_registry: HashMap<GlobalKey, ElementId>`
  - [ ] `register_global_key()` / `unregister_global_key()`
  - [ ] Enforce uniqueness (panic on duplicate keys)
  - [ ] Support for key reparenting

- [ ] **Build phases**
  - [ ] `build_scope(callback)` - Execute build with callback
  - [ ] `finalize_tree()` - End of build cleanup
  - [ ] `lock_state(callback)` - Prevent setState during callback
  - [ ] `on_build_scheduled` callback

#### 4.2 Focus Management
- [ ] **FocusManager** integration
  - [ ] Track focus state
  - [ ] Focus traversal
  - [ ] Focus scope management

**Files to modify:**
- `crates/flui_core/src/tree/pipeline.rs` → rename to `build_owner.rs`
- Create `crates/flui_core/src/tree/build_scope.rs`

---

### Phase 5: ProxyWidget Hierarchy 🎭

**Priority: MEDIUM** | **Complexity: MEDIUM**

Flutter has proxy widgets that wrap children and provide services:

#### 5.1 ProxyWidget Base
- [ ] **ProxyWidget** trait
  - [ ] Single `child` field
  - [ ] Creates `ProxyElement`

- [ ] **ProxyElement**
  - [ ] `updated(old_widget)` callback
  - [ ] `notify_clients(old_widget)` - notify dependents

#### 5.2 ParentDataWidget
- [ ] **ParentDataWidget<T: ParentData>** trait
  - [ ] `apply_parent_data(render_object, parent_data)`
  - [ ] `debug_typical_ancestor_widget_class()`
  - [ ] `debug_can_apply_out_of_turn()`

- [ ] **ParentDataElement<T>**
  - [ ] Efficient parent data application
  - [ ] Out-of-turn application optimization

**Current status:** `InheritedWidget` and `InheritedElement` exist in `widget/provider.rs`

**Files to create:**
- `crates/flui_core/src/widget/proxy.rs`
- `crates/flui_core/src/widget/parent_data.rs`
- `crates/flui_core/src/element/proxy.rs`

---

### Phase 6: Enhanced InheritedWidget System 📡

**Priority: HIGH** | **Complexity: MEDIUM**

Current `InheritedWidget` is basic. Need to add:

#### 6.1 Dependency Tracking
- [ ] **InheritedElement enhancements**
  - [ ] `_dependents: HashMap<ElementId, DependencyInfo>` - track who depends on this
  - [ ] `update_dependencies(dependent, aspect)` - register dependency
  - [ ] `notify_dependent(old_widget, dependent)` - notify single dependent
  - [ ] `notify_clients(old_widget)` - notify all dependents
  - [ ] Support for aspect-based dependencies (partial rebuilds)

#### 6.2 BuildContext Dependency Methods
Current BuildContext has basic support. Need:
- [ ] **`depend_on_inherited_element()`** - Low-level dependency creation
- [ ] **`depend_on_inherited_widget_of_exact_type<T>()`** - Create dependency + return widget
- [ ] **`get_inherited_widget_of_exact_type<T>()`** - Get without dependency
- [ ] **`get_element_for_inherited_widget_of_exact_type<T>()`** - Get element directly

#### 6.3 InheritedModel (Optional - Advanced)
- [ ] **InheritedModel<T>** - Aspect-based inherited widgets
- [ ] Partial rebuilds based on which aspect changed
- [ ] More granular control than InheritedWidget

**Files to modify:**
- `crates/flui_core/src/widget/provider.rs`
- `crates/flui_core/src/context/inherited.rs`

---

### Phase 7: Enhanced Context Methods 🧭

**Priority: MEDIUM** | **Complexity: LOW-MEDIUM**

BuildContext needs more navigation and query methods:

#### 7.1 Tree Navigation
Current: Basic ancestor iteration exists

Need to add:
- [ ] **`find_ancestor_widget_of_exact_type<T>()`**
- [ ] **`find_ancestor_state_of_type<T>()`**
- [ ] **`find_root_ancestor_state_of_type<T>()`**
- [ ] **`find_ancestor_render_object_of_type<T>()`**
- [ ] **`visit_child_elements(visitor)`** - Walk children

#### 7.2 Layout & Rendering Queries
- [ ] **`find_render_object()`** - Get this element's RenderObject
- [ ] **`size()`** - Get widget size (after layout)
- [ ] **`owner()`** - Get BuildOwner reference
- [ ] **`mounted()`** - Check if still in tree

#### 7.3 Notifications
- [ ] **`dispatch_notification(notification)`** - Bubble notification up tree
- [ ] Notification system with NotificationListener

**Files to modify:**
- `crates/flui_core/src/context/mod.rs`

---

### Phase 8: Multi-Child Element Management 👨‍👩‍👧‍👦

**Priority: CRITICAL** | **Complexity: VERY HIGH**

This is one of Flutter's most complex algorithms:

#### 8.1 Enhanced MultiChildRenderObjectElement
Current implementation is basic. Need:

- [ ] **Keyed child update algorithm**
  - [ ] Build key → element map for old children
  - [ ] Build key → widget map for new children
  - [ ] Three-phase update:
    1. Update keyed children in-place
    2. Remove old unkeyed children
    3. Insert new unkeyed children
  - [ ] Handle moved keyed children efficiently
  - [ ] Maintain render tree consistency

- [ ] **`update_children()`** method
  - [ ] Compare old child elements with new child widgets
  - [ ] Reuse elements when `can_update()` returns true
  - [ ] Create new elements for new widgets
  - [ ] Remove elements for removed widgets
  - [ ] Handle slot updates efficiently

#### 8.2 IndexedSlot
- [ ] **IndexedSlot<T>** for tracking child positions
  - [ ] `index` - position in list
  - [ ] `value` - previous sibling element reference

**Files to modify:**
- `crates/flui_core/src/element/render/multi.rs`

**Reference:** Flutter's `updateChildren()` is ~200 lines of complex list diffing

---

### Phase 9: RenderObject Enhancement 🎨

**Priority: HIGH** | **Complexity: MEDIUM**

Current RenderObject is minimal. Need Flutter's full feature set:

#### 9.1 RenderObject Lifecycle
- [ ] **Layout tracking**
  - [ ] `needs_layout` flag
  - [ ] `mark_needs_layout()` - propagate up to root
  - [ ] Layout dirty tracking in PipelineOwner

- [ ] **Paint tracking**
  - [ ] `needs_paint` flag
  - [ ] `mark_needs_paint()` - propagate to compositing layer
  - [ ] Paint dirty tracking

- [ ] **Compositing**
  - [ ] `needs_compositing_bits_update` flag
  - [ ] Layer tree management
  - [ ] Compositing dirty tracking

#### 9.2 ParentData System
Current: Basic `ParentData`, `BoxParentData`, `ContainerParentData` exist

Need to add:
- [ ] **`setup_parent_data(child)`** - Initialize child's parent data
- [ ] **`adopt_child(child)`** / **`drop_child(child)`** - Lifecycle
- [ ] **`attach(owner)`** / **`detach()`** - Connect to pipeline
- [ ] **`redepth_child(child)`** - Update child depth

#### 9.3 Hit Testing
- [ ] **`hit_test(result, position)`** - Pointer hit detection
- [ ] **`hit_test_children()`** - Recursive hit testing
- [ ] **`hit_test_self()`** - Test this render object

**Files to modify:**
- `crates/flui_core/src/render/mod.rs`

---

### Phase 10: Error Handling & Debugging 🐛

**Priority: MEDIUM** | **Complexity: LOW-MEDIUM**

Flutter has extensive error handling:

#### 10.1 ErrorWidget
- [ ] **ErrorWidget** - Displays exceptions in debug mode
  - [ ] `message` - error message
  - [ ] `builder` - configurable error widget builder
  - [ ] Red screen of death in debug, gray in release

#### 10.2 Debug Tools
- [ ] **DebugCreator** - Track element creation for debugging
- [ ] **Element diagnostic tree** - Debug print element tree
- [ ] **Widget inspector support** - DevTools integration
- [ ] **Debug flags**
  - [ ] `debug_print_build_scope`
  - [ ] `debug_print_mark_needs_build_stacks`
  - [ ] `debug_print_global_key_event_info`

#### 10.3 Assertions & Validation
- [ ] Lifecycle state validation
- [ ] Global key uniqueness enforcement
- [ ] Better error messages with context

**Files to create:**
- `crates/flui_core/src/error_widget.rs`
- `crates/flui_core/src/debug/mod.rs`

---

### Phase 11: Notification System 📣

**Priority: LOW-MEDIUM** | **Complexity: MEDIUM**

Flutter's notification bubbling system:

#### 11.1 Notification Infrastructure
- [ ] **Notification** trait - Base for all notifications
- [ ] **NotificationListener** widget - Catches bubbling notifications
- [ ] **`dispatch_notification()`** on BuildContext
- [ ] **_NotificationNode** - Tree structure for efficient bubbling

#### 11.2 Built-in Notifications
- [ ] **ScrollNotification** - Scroll events
- [ ] **LayoutChangedNotification** - Layout changes
- [ ] **SizeChangedLayoutNotification** - Size changes
- [ ] **KeepAliveNotification** - Keep-alive requests

**Files to create:**
- `crates/flui_core/src/notification/mod.rs`

---

### Phase 12: Advanced Widget Types 🎯

**Priority: LOW** | **Complexity: LOW-MEDIUM**

Additional widget patterns from Flutter:

#### 12.1 Special Element Types
- [ ] **RenderTreeRootElement** - Marks root of render tree
- [ ] **RootElementMixin** - For root elements
- [ ] **NotifiableElementMixin** - Elements that respond to notifications

#### 12.2 Widget Utilities
- [ ] **Widget.can_update()** static method (currently per-instance)
- [ ] **const widget** optimization support (if possible in Rust)
- [ ] **Widget equality** optimization

**Files to modify:**
- `crates/flui_core/src/widget/mod.rs`
- `crates/flui_core/src/element/mod.rs`

---

### Phase 13: Performance Optimizations ⚡

**Priority: MEDIUM** | **Complexity: VARIES**

Flutter has many performance optimizations:

#### 13.1 Build Scope Optimization
- [ ] **Dirty element sorting** - Build by depth (parents before children)
- [ ] **Build batching** - Batch multiple setState calls
- [ ] **Inactive element pool** - Reuse deactivated elements
- [ ] **Element reuse** - Minimize element creation/destruction

#### 13.2 Layout Optimization
- [ ] **Relayout boundaries** - Isolate layout changes
- [ ] **Repaint boundaries** - Isolate paint changes
- [ ] **Layer caching** - Cache rendered layers
- [ ] **Dirty-only layout** - Skip clean subtrees

#### 13.3 Memory Optimization
- [ ] **Weak references** for parent pointers (where safe)
- [ ] **Element pooling** - Reuse element allocations
- [ ] **Smart Arc usage** - Minimize clone overhead

---

### Phase 14: Hot Reload Support 🔥

**Priority: LOW** | **Complexity: MEDIUM**

Support for Flutter-style hot reload:

#### 14.1 Reassemble Infrastructure
- [ ] **`reassemble()`** on all elements
- [ ] **`reassemble()`** on State objects
- [ ] Preserve state across hot reloads
- [ ] Clear caches on reassemble

#### 14.2 Development Tools
- [ ] **Hot reload trigger** API
- [ ] **Widget tree diffing** for reload
- [ ] **State preservation** during reload

---

### Phase 15: Testing Infrastructure 🧪

**Priority: LOW** | **Complexity: LOW**

Tools for testing widgets:

#### 15.1 Test Utilities
- [ ] **PumpWidget** - Mount widget for testing
- [ ] **Widget tester** - Simulate interactions
- [ ] **Find** - Locate widgets in tree
- [ ] **Mock BuildContext** - Testing without full tree

---

## Implementation Priority Summary

### 🔴 Critical (Must Have)
1. **BuildOwner & Build Scheduling** (Phase 4) - Core infrastructure
2. **Multi-Child Element Management** (Phase 8) - Essential for layouts
3. **Enhanced Element Lifecycle** (Phase 3) - Fundamental correctness

### 🟠 High Priority (Should Have Soon)
4. **State Lifecycle Enhancement** (Phase 2) - Better state management
5. **Key System Enhancement** (Phase 1) - State preservation
6. **Enhanced InheritedWidget** (Phase 6) - Efficient state propagation
7. **RenderObject Enhancement** (Phase 9) - Layout and paint pipeline

### 🟡 Medium Priority (Nice to Have)
8. **ProxyWidget Hierarchy** (Phase 5) - Widget composition patterns
9. **Enhanced Context Methods** (Phase 7) - Better tree navigation
10. **Error Handling & Debugging** (Phase 10) - Developer experience
11. **Notification System** (Phase 11) - Event bubbling
12. **Performance Optimizations** (Phase 13) - Production readiness

### 🟢 Low Priority (Future)
13. **Advanced Widget Types** (Phase 12) - Additional patterns
14. **Hot Reload Support** (Phase 14) - Development experience
15. **Testing Infrastructure** (Phase 15) - Test support

---

## File Organization Roadmap

Suggested file structure to match Flutter's organization:

```
crates/flui_core/src/
├── foundation/
│   ├── mod.rs           ✅ (exists)
│   ├── id.rs            ✅ (exists)
│   ├── lifecycle.rs     ✅ (exists)
│   ├── slot.rs          ✅ (exists)
│   └── key.rs           �� EXPAND (add GlobalKey, ObjectKey, etc.)
│
├── widget/
│   ├── mod.rs           ✅ (exists - Widget, StatelessWidget, StatefulWidget)
│   ├── provider.rs      ✅ (exists - InheritedWidget)
│   ├── proxy.rs         ⚪ NEW (ProxyWidget)
│   ├── parent_data.rs   ⚪ NEW (ParentDataWidget)
│   └── error.rs         ⚪ NEW (ErrorWidget)
│
├── element/
│   ├── mod.rs           ✅ (exists - Element, ComponentElement, StatefulElement)
│   ├── proxy.rs         ⚪ NEW (ProxyElement, ParentDataElement)
│   └── render/
│       ├── mod.rs       ✅ (exists - RenderObjectElement)
│       ├── leaf.rs      ✅ (exists - LeafRenderObjectElement)
│       ├── single.rs    ✅ (exists - SingleChildRenderObjectElement)
│       └── multi.rs     🔴 ENHANCE (add keyed child algorithm)
│
├── context/
│   ├── mod.rs           ✅ (exists - BuildContext/Context)
│   ├── iterators.rs     ✅ (exists - ancestor iteration)
│   └── inherited.rs     ✅ (exists - inherited widget access)
│
├── tree/
│   ├── mod.rs           ✅ (exists - exports)
│   ├── element_tree.rs  ✅ (exists - ElementTree)
│   ├── pipeline.rs      🔴 EXPAND → rename to build_owner.rs
│   ├── build_owner.rs   ⚪ NEW (BuildOwner - replaces pipeline.rs)
│   ├── build_scope.rs   ⚪ NEW (BuildScope)
│   └── inactive.rs      ⚪ NEW (InactiveElements)
│
├── render/
│   ├── mod.rs           ✅ (exists - RenderObject trait)
│   ├── widget.rs        ✅ (exists - RenderObjectWidget variants)
│   ├── parent_data.rs   ✅ (exists - ParentData types)
│   ├── layer.rs         ⚪ NEW (Layer system)
│   └── hit_test.rs      ⚪ NEW (Hit testing)
│
├── notification/
│   ├── mod.rs           ⚪ NEW (Notification trait)
│   └── listener.rs      ⚪ NEW (NotificationListener)
│
├── debug/
│   ├── mod.rs           ⚪ NEW (Debug utilities)
│   └── inspector.rs     ⚪ NEW (Widget inspector support)
│
├── constraints.rs       ✅ (exists - BoxConstraints)
├── error.rs             ✅ (exists - CoreError)
└── lib.rs               ✅ (exists - re-exports)
```

**Legend:**
- ✅ Exists and functional
- 🔴 Needs major enhancement/expansion
- ⚪ Needs to be created

---

## Testing Strategy

Each phase should include:

1. **Unit tests** - Test individual components
2. **Integration tests** - Test component interaction
3. **Widget tests** - Test widget behavior in tree
4. **Performance benchmarks** - Measure optimization impact

Example test structure:
```rust
#[cfg(test)]
mod tests {
    // Unit tests for individual methods
    #[test] fn test_element_mount() { }
    #[test] fn test_element_update() { }

    // Integration tests for complex scenarios
    #[test] fn test_keyed_child_reordering() { }
    #[test] fn test_inherited_widget_propagation() { }

    // Performance tests
    #[bench] fn bench_large_tree_rebuild() { }
}
```

---

## Migration Notes

### Breaking Changes to Expect

1. **Phase 4 (BuildOwner)** - `PipelineOwner` → `BuildOwner` rename
2. **Phase 6 (InheritedWidget)** - API changes for dependency tracking
3. **Phase 8 (Multi-child)** - Element update algorithm changes

### Backward Compatibility Strategy

- Use `#[deprecated]` attributes for old APIs
- Provide migration guides for each phase
- Maintain compatibility shims for at least 2 versions

---

## Resources & References

- **Flutter framework.dart**: 7,461 lines of reference implementation
- **Flutter rendering library**: Additional render object patterns
- **Flutter widget catalog**: Real-world widget examples
- **Rust UI framework comparisons**: Druid, Iced, Dioxus for Rust-specific patterns

---

## Success Metrics

By completion of all phases, flui-core should:

1. ✅ Support all basic Flutter widget patterns
2. ✅ Handle complex widget trees (10,000+ widgets)
3. ✅ Efficient rebuild (< 16ms for typical updates)
4. ✅ Complete state lifecycle management
5. ✅ GlobalKey support with reparenting
6. ✅ InheritedWidget with efficient dependency tracking
7. ✅ Multi-child keyed updates with minimal churn
8. ✅ Comprehensive error handling and debugging tools

---

## Contributing

To contribute to flui-core development:

1. Pick a phase from this roadmap
2. Create an issue for discussion
3. Implement with tests
4. Submit PR with reference to this roadmap
5. Update roadmap with ✅ when complete

---

**Last Updated:** 2025-10-19
**Document Version:** 1.0
**Status:** Living document - will be updated as implementation progresses
