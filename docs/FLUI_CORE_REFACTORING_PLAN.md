# Flui-Core Refactoring Plan

## Current Issues

### 1. Flat File Structure
Current structure (14 files in src/):
```
src/
├── build_context.rs       (600+ lines)
├── constraints.rs
├── element.rs             (1000+ lines)
├── element_tree.rs        (800+ lines)
├── inherited_widget.rs
├── leaf_render_object_element.rs
├── multi_child_render_object_element.rs
├── parent_data.rs
├── pipeline_owner.rs
├── render_object.rs       (400+ lines)
├── render_object_widget.rs
├── single_child_render_object_element.rs
├── widget.rs              (500+ lines)
└── lib.rs
```

**Problems:**
- No logical grouping - everything in root
- Hard to navigate and find related code
- Large files mixing multiple concerns
- Unclear module boundaries

### 2. Missing Flutter BuildContext Methods

Current BuildContext has:
- ✅ `element_id()`, `tree()`, `parent()`
- ✅ `mark_needs_build()`
- ✅ `visit_ancestor_elements()`, `visit_child_elements()`
- ✅ `depend_on_inherited_widget()`, `get_inherited_widget()`
- ✅ `find_ancestor_widget_of_type()`, `find_ancestor_element_of_type()`
- ✅ `find_ancestor_render_object_of_type()`
- ✅ `size()`, `find_render_object()`
- ✅ `mounted()`, `is_valid()`

**Missing:**
- ❌ `widget` property (get current widget)
- ❌ `owner` property (get BuildOwner/PipelineOwner)
- ❌ `debugDoingBuild` (build phase tracking)
- ❌ `findAncestorStateOfType<T>()` - find ancestor State
- ❌ `findRootAncestorStateOfType<T>()` - find root ancestor State
- ❌ `getElementForInheritedWidgetOfExactType<T>()` - get InheritedElement
- ❌ `visitChildElements()` - already implemented but not tested enough

### 3. Missing Flutter Element Methods

Current Element trait has:
- ✅ `mount()`, `unmount()`, `update()`, `rebuild()`
- ✅ `id()`, `parent()`, `key()`
- ✅ `is_dirty()`, `mark_dirty()`
- ✅ `visit_children()`, `visit_children_mut()`
- ✅ `render_object()`, `render_object_mut()`
- ✅ `widget_type_id()`

**Missing:**
- ❌ `slot` property - element's position in parent's child list
- ❌ `depth` property - distance from root
- ❌ `widget` property - current widget configuration
- ❌ `owner` property - BuildOwner reference
- ❌ `mounted` property - is element in tree
- ❌ `debugIsDefunct`, `debugIsActive` - lifecycle state
- ❌ `reassemble()` - hot reload support
- ❌ `updateChild()` - reconciliation algorithm
- ❌ `deactivate()` - called before unmount
- ❌ `activate()` - called when reinserted
- ❌ `didChangeDependencies()` - InheritedWidget changes
- ❌ `child_ids()` - currently implemented but inconsistent

### 4. Missing Flutter Widget Methods

Current Widget trait has:
- ✅ `create_element()`
- ✅ `key()`
- ✅ `type_name()`
- ✅ `can_update()`

**Missing:**
- ❌ Better `can_update()` implementation (should check runtimeType + key)
- ❌ `toBuildOwner()` equivalent (not needed in Rust)
- ❌ Better debug formatting

### 5. Missing GlobalKey Support

Flutter's GlobalKey allows:
- Access to element from anywhere
- Access to State from outside widget tree
- Widget reparenting across tree locations

**Not implemented yet:**
- GlobalKey<T> trait/struct
- GlobalKey registry in PipelineOwner
- Element lookup by GlobalKey
- State access via GlobalKey

---

## Proposed New Structure

### Directory Organization

```
crates/flui_core/src/
├── lib.rs                      # Public API exports
│
├── foundation/                 # Core building blocks
│   ├── mod.rs
│   ├── element_id.rs          # ElementId type
│   ├── slot.rs                # Slot type for child positioning
│   └── lifecycle.rs           # Element lifecycle states
│
├── widget/                     # Widget system
│   ├── mod.rs
│   ├── widget_trait.rs        # Core Widget trait
│   ├── stateless.rs           # StatelessWidget
│   ├── stateful.rs            # StatefulWidget + State
│   └── into_widget.rs         # IntoWidget helper
│
├── element/                    # Element system
│   ├── mod.rs
│   ├── element_trait.rs       # Core Element trait
│   ├── component_element.rs   # ComponentElement (for Stateless/Stateful)
│   ├── stateful_element.rs    # StatefulElement
│   ├── render_object_element/ # RenderObject elements
│   │   ├── mod.rs
│   │   ├── base.rs            # Base RenderObjectElement
│   │   ├── leaf.rs            # LeafRenderObjectElement
│   │   ├── single_child.rs    # SingleChildRenderObjectElement
│   │   └── multi_child.rs     # MultiChildRenderObjectElement
│   └── lifecycle.rs           # Lifecycle helpers
│
├── inherited/                  # InheritedWidget system
│   ├── mod.rs
│   ├── inherited_widget.rs    # InheritedWidget trait
│   └── inherited_element.rs   # InheritedElement
│
├── render_object/             # RenderObject system
│   ├── mod.rs
│   ├── render_object_trait.rs # Core RenderObject trait
│   ├── render_object_widget.rs # RenderObjectWidget traits
│   └── parent_data.rs         # ParentData system
│
├── tree/                      # Tree management
│   ├── mod.rs
│   ├── element_tree.rs        # ElementTree
│   ├── pipeline_owner.rs      # PipelineOwner
│   └── build_context.rs       # BuildContext
│
├── constraints/               # Layout constraints
│   ├── mod.rs
│   └── box_constraints.rs     # BoxConstraints
│
└── keys/                      # Key system (future)
    ├── mod.rs
    ├── global_key.rs          # GlobalKey support
    └── local_key.rs           # LocalKey variants
```

**Benefits:**
- Clear separation of concerns
- Easy to find related code
- Better module encapsulation
- Room for future expansion
- Follows Flutter's package structure

---

## Implementation Plan

### Phase 1: Reorganize File Structure (Week 1)

**Tasks:**
1. Create new directory structure
2. Move code to appropriate modules
3. Update imports in all files
4. Fix visibility modifiers (pub vs pub(crate))
5. Update lib.rs exports
6. Run tests to ensure nothing broke

**Files to reorganize:**
- `element.rs` → split into `element/element_trait.rs` + `element/component_element.rs`
- `widget.rs` → split into `widget/widget_trait.rs` + `widget/stateless.rs` + `widget/stateful.rs`
- Element implementations → `element/render_object_element/`
- `inherited_widget.rs` → `inherited/` module

### Phase 2: Add Missing BuildContext Methods (Week 1-2)

**Priority 1 (Critical):**
1. ✅ `widget` property - return current widget
2. ✅ `owner` property - return PipelineOwner
3. ✅ `findAncestorStateOfType<T>()` - find ancestor State
4. ✅ `findRootAncestorStateOfType<T>()` - find root State

**Priority 2 (High):**
5. ✅ `getElementForInheritedWidgetOfExactType<T>()` - get InheritedElement without dependency
6. ✅ Better error messages for invalid context usage
7. ✅ Debug helpers (`debugDoingBuild`, etc.)

**Priority 3 (Medium):**
8. ✅ More convenience methods
9. ✅ Better documentation with examples

### Phase 3: Add Missing Element Methods (Week 2)

**Priority 1 (Critical):**
1. ✅ `slot` property - track position in parent
2. ✅ `depth` property - distance from root
3. ✅ `widget` property - current widget
4. ✅ `owner` property - BuildOwner/PipelineOwner
5. ✅ `mounted` property - lifecycle state
6. ✅ `child_ids()` - consistent implementation

**Priority 2 (High):**
7. ✅ `updateChild()` - proper reconciliation algorithm
8. ✅ `deactivate()` - pre-unmount lifecycle
9. ✅ `activate()` - reinsert lifecycle
10. ✅ `didChangeDependencies()` - InheritedWidget tracking

**Priority 3 (Medium):**
11. ✅ `reassemble()` - hot reload support
12. ✅ Debug lifecycle methods
13. ✅ Better lifecycle state tracking

### Phase 4: GlobalKey Support (Week 3)

**Tasks:**
1. Create `GlobalKey<T>` trait in flui-foundation
2. Add GlobalKey registry to PipelineOwner
3. Implement `currentContext`, `currentWidget`, `currentState` accessors
4. Add GlobalKey tests
5. Update documentation

**Files to create:**
- `flui_foundation/src/global_key.rs`
- `flui_core/src/keys/` module
- Tests in both crates

### Phase 5: Testing & Documentation (Week 3-4)

**Tasks:**
1. Add unit tests for all new methods
2. Add integration tests for complex scenarios
3. Update GLOSSARY with new types
4. Write migration guide for API changes
5. Update examples to use new APIs

**Target:**
- 90%+ code coverage
- All public APIs documented
- At least 2 examples per major feature

---

## Breaking Changes

### API Changes

**BuildContext:**
- Added new methods (non-breaking)
- Some methods may return different error types

**Element:**
- `child_ids()` signature may change to return `Vec<ElementId>` consistently
- New required trait methods (with default implementations where possible)

**Migration Strategy:**
- Provide default implementations for new methods
- Add deprecation warnings for old APIs
- Keep compatibility layer for 1-2 versions

### File Structure
- Import paths will change
- Old paths will be deprecated but work via re-exports
- Clear migration guide in docs

---

## Success Criteria

1. ✅ All tests passing after reorganization
2. ✅ Code coverage maintained or improved
3. ✅ No performance regressions
4. ✅ Clear module boundaries
5. ✅ Easy to navigate codebase
6. ✅ Complete Flutter API coverage for core features
7. ✅ Documentation for all public APIs
8. ✅ Examples work with new structure

---

## Timeline

| Week | Phase | Tasks | Output |
|------|-------|-------|--------|
| 1 | Phase 1 | Reorganize structure | New directory layout |
| 1-2 | Phase 2 | BuildContext methods | Complete BuildContext API |
| 2 | Phase 3 | Element methods | Complete Element API |
| 3 | Phase 4 | GlobalKey support | GlobalKey working |
| 3-4 | Phase 5 | Testing & docs | 90% coverage, docs |

**Total:** 3-4 weeks for complete refactoring

---

## Risk Assessment

**Low Risk:**
- File reorganization (can be automated)
- Adding new methods with defaults
- Documentation updates

**Medium Risk:**
- Changing Element trait (many implementors)
- GlobalKey registry (new architecture)
- Performance impact of new features

**High Risk:**
- Breaking existing user code
- Deadlocks in new tree traversal methods
- Hot reload support (complex feature)

**Mitigation:**
- Extensive testing at each phase
- Backwards compatibility where possible
- Performance benchmarks before/after
- Gradual rollout with feature flags

---

## Next Steps

1. **Get approval** for reorganization plan
2. **Create feature branch** `refactor/flui-core-structure`
3. **Start Phase 1** - file reorganization
4. **Set up CI** to track progress
5. **Regular reviews** after each phase

---

**Status:** 📋 **PLANNING**
**Owner:** TBD
**Started:** 2025-01-19
**Target Completion:** 2025-02-15 (4 weeks)

