# Rebuild Logic Refactoring Proposal

## Current Issues

The `rebuild_component()` method in `build_pipeline.rs` (lines 513-661) has several areas for improvement:

1. **Code Duplication**: The pattern of inserting, mounting, and updating parent is repeated 3 times
2. **Complex Match Statement**: 4-case match with nested logic is hard to follow
3. **Long Method**: ~150 lines doing multiple responsibilities
4. **Unclear Comments**: "Phase 1/2/3" doesn't clearly communicate intent

## Proposed Refactoring

### 1. Extract Helper Methods

```rust
/// Insert element into tree and mount it to parent
fn insert_and_mount_child(
    tree_guard: &mut ElementTree,
    element: Element,
    parent_id: ElementId,
) -> ElementId {
    let new_id = tree_guard.insert(element);

    if let Some(child) = tree_guard.get_mut(new_id) {
        child.mount(Some(parent_id), None);
    }

    new_id
}

/// Update ComponentElement's child reference
fn update_component_child_reference(
    tree_guard: &mut ElementTree,
    parent_id: ElementId,
    child_id: Option<ElementId>,
) {
    if let Some(Element::Component(component)) = tree_guard.get_mut(parent_id) {
        match child_id {
            Some(id) => component.set_child(id),
            None => component.clear_child(),
        }
    }
}

/// Reconcile child element: replace old with new, handling Slab ID reuse
///
/// CRITICAL: Inserts new element BEFORE removing old to prevent Slab ID reuse.
/// This ensures new_element's children (already inserted during build) remain valid.
fn reconcile_child(
    tree_guard: &mut ElementTree,
    parent_id: ElementId,
    old_child_id: Option<ElementId>,
    new_element: Option<Element>,
) {
    match (old_child_id, new_element) {
        // Replace existing child with new one
        (Some(old_id), Some(new_element)) => {
            // Insert-before-remove pattern prevents Slab ID reuse
            let new_id = insert_and_mount_child(tree_guard, new_element, parent_id);
            update_component_child_reference(tree_guard, parent_id, Some(new_id));
            let _ = tree_guard.remove(old_id);

            // NOTE: We don't schedule new_id for rebuild because:
            // - It was just created and is already "fresh"
            // - RenderElements don't rebuild (only layout/paint)
            // - ComponentElements will be scheduled when they become dirty
        }

        // Add new child (no previous child)
        (None, Some(new_element)) => {
            let new_id = insert_and_mount_child(tree_guard, new_element, parent_id);
            update_component_child_reference(tree_guard, parent_id, Some(new_id));
        }

        // Remove old child (no new child)
        (Some(old_id), None) => {
            let _ = tree_guard.remove(old_id);
            update_component_child_reference(tree_guard, parent_id, None);
        }

        // No child before or after - nothing to do
        (None, None) => {}
    }
}
```

### 2. Simplified rebuild_component()

```rust
/// Rebuild a ComponentElement
///
/// Three-stage process:
/// 1. Extract component data (view, child, hooks)
/// 2. Build new child element (outside locks)
/// 3. Reconcile old/new children in tree
fn rebuild_component(
    &mut self,
    tree: &Arc<parking_lot::RwLock<ElementTree>>,
    element_id: ElementId,
    _depth: usize,
) -> bool {
    #[cfg(debug_assertions)]
    tracing::debug!("Rebuilding component element {:?}", element_id);

    // Stage 1: Extract component data (minimize lock time)
    let (view, old_child_id, hook_context) = {
        let mut tree_guard = tree.write();
        let component = match tree_guard.get_mut(element_id)?.as_component_mut() {
            Some(c) => c,
            None => return false,
        };

        let view = component.view().clone_box();
        let old_child = component.child();
        let hook_context = extract_or_create_hook_context(component);

        (view, old_child, hook_context)
    };

    // Stage 2: Build new child view (outside locks - this is the expensive part)
    let new_element = build_with_hooks(
        tree.clone(),
        element_id,
        hook_context,
        self.rebuild_queue.clone(),
        view,
    );

    // Stage 3: Reconcile old/new children (write lock)
    {
        let mut tree_guard = tree.write();
        reconcile_child(&mut tree_guard, element_id, old_child_id, Some(new_element));
    }

    true
}
```

### 3. Additional Helper Functions

```rust
/// Extract existing HookContext or create new one
fn extract_or_create_hook_context(
    component: &mut ComponentElement,
) -> Arc<Mutex<HookContext>> {
    if let Some(ctx) = component
        .state_mut()
        .downcast_mut::<Arc<Mutex<HookContext>>>()
    {
        // Reuse existing HookContext (preserves hook state across rebuilds!)
        ctx.clone()
    } else {
        // First build - create new HookContext and store it
        let ctx = Arc::new(Mutex::new(HookContext::new()));
        component.set_state(Box::new(ctx.clone()));
        ctx
    }
}

/// Build view with hook context setup
fn build_with_hooks(
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
    hook_context: Arc<Mutex<HookContext>>,
    rebuild_queue: RebuildQueue,
    view: Box<dyn AnyView>,
) -> Element {
    // Create BuildContext
    let ctx = BuildContext::with_hook_context_and_queue(
        tree,
        element_id,
        hook_context.clone(),
        rebuild_queue,
    );

    // Set up ComponentId for hooks
    let component_id = ComponentId(element_id.get() as u64);

    // Begin component rendering
    {
        let mut hook_ctx = hook_context.lock();
        hook_ctx.begin_component(component_id);
    }

    // Build with thread-local BuildContext
    let element = with_build_context(&ctx, || view.build_any());

    // End component rendering
    {
        let mut hook_ctx = hook_context.lock();
        hook_ctx.end_component();
    }

    element
}
```

## Benefits

1. ✅ **Reduced Duplication**: Insert/mount/update pattern extracted to single function
2. ✅ **Clearer Intent**: Each function has a single, well-documented responsibility
3. ✅ **Shorter Methods**: Main method is ~30 lines instead of ~150
4. ✅ **Better Names**: "Extract/Build/Reconcile" instead of "Phase 1/2/3"
5. ✅ **Easier Testing**: Helper functions can be unit tested independently
6. ✅ **Maintainability**: Critical insert-before-remove logic isolated in one place

## Implementation Notes

- All helper functions should be `impl BuildPipeline` methods
- Keep existing tracing/debug statements
- Preserve exact behavior (just reorganized)
- Add comprehensive doc comments explaining the insert-before-remove pattern

## Migration Path

1. ✅ Add helper methods to BuildPipeline
2. ✅ Refactor rebuild_component() to use helpers
3. ✅ Test with counter example
4. ✅ Remove old implementation once confirmed working
5. ⏭️ Apply same pattern to rebuild_provider() if applicable (future work)

## Implementation Results

**Status**: ✅ **Successfully Implemented and Tested**

### Code Metrics

**Before Refactoring:**
- `rebuild_component()`: ~150 lines
- Code duplication: 3x repeated insert/mount/update pattern
- Comments: Vague "Phase 1/2/3"
- Helper methods: 0

**After Refactoring:**
- `rebuild_component()`: ~60 lines (60% reduction)
- Code duplication: 0 (extracted to helper methods)
- Comments: Clear "Stage 1/2/3" with explanations
- Helper methods: 4 well-documented functions

### Performance Testing

Tested with `examples/counter.rs`:

```
Performance: 540 frames | Rebuilds: 14 (2.6%) | Layouts: 14 (2.6%) | Paints: 540 (100.0%)
```

✅ Each button click triggers exactly +1 rebuild (optimal)
✅ No errors or panics
✅ Signal persistence working correctly
✅ Insert-before-remove pattern prevents ElementId conflicts

### Key Improvements

1. **Reduced Duplication**: Insert/mount/update pattern extracted to `insert_and_mount_child()`
2. **Clearer Intent**: `reconcile_child()` documents the critical insert-before-remove pattern
3. **Better Separation**: `extract_or_create_hook_context()` isolates HookContext management
4. **Easier Maintenance**: Critical logic (Slab ID reuse handling) now in one well-documented place
5. **Improved Readability**: Main method flow is now obvious at a glance

### Files Modified

- `crates/flui_core/src/pipeline/build_pipeline.rs`
  - Added 4 helper methods (lines 510-613)
  - Simplified `rebuild_component()` (lines 615-688)
  - Total: ~150 lines replaced with ~180 lines (net +30, but much cleaner)

### Future Work

- Apply same refactoring pattern to `rebuild_provider()` if it has similar duplication
- Consider extracting the hook setup/teardown logic into a helper method
- Add unit tests for the helper methods
