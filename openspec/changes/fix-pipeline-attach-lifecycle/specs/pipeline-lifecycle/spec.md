# Spec: Pipeline Lifecycle - Root Widget Attachment

**Capability:** `pipeline-lifecycle`
**Change ID:** `fix-pipeline-attach-lifecycle`
**Status:** ✅ Implemented

## MODIFIED Requirements

### Requirement: PipelineOwner must extend BuildContext guard lifetime during attachment

**Priority:** Critical
**Type:** Bug Fix

#### Context

BuildContext is accessed via thread-local storage managed by `BuildContextGuard` (RAII pattern). During View → Element conversion, child widgets recursively call `into_element()`, which accesses BuildContext from thread-local storage. The guard must stay alive for the entire conversion process.

#### Scenario: Root widget with nested children builds successfully

**Given** a root widget with nested children (e.g., Container → Center → Text)

**When** `PipelineOwner::attach()` converts the widget to an element tree

**Then** the BuildContextGuard must remain alive during:
- Root widget's `into_element()` call
- All child widget `build()` calls
- All recursive `into_element()` calls

**And** no "No BuildContext available!" panic occurs

**Implementation:**
```rust
// crates/flui_core/src/pipeline/pipeline_owner.rs:367
let element = crate::view::with_build_context(&ctx, || widget.into_element());
```

**Verification:**
- Run hello_world_view demo with nested widgets
- Confirm no BuildContext panics
- Check logs: "Build complete count=1"

---

### Requirement: PipelineOwner must request initial layout after root attachment

**Priority:** Critical
**Type:** Bug Fix

#### Context

Flutter's rendering pipeline has three phases that must execute sequentially: Build → Layout → Paint. Setting a root element only completes the Build phase. Without an explicit layout request, the Layout and Paint phases never execute, resulting in a blank screen.

#### Scenario: Newly attached root widget triggers complete render pipeline

**Given** a widget tree has been converted to an element tree

**When** `PipelineOwner::attach()` sets the root element

**Then** the system must:
1. Call `set_root(element)` to update the element tree
2. Call `request_layout(root_id)` to mark root as needing layout
3. Log the attachment with root_id

**And** subsequent `flush_layout()` and `flush_paint()` calls will execute

**Implementation:**
```rust
// crates/flui_core/src/pipeline/pipeline_owner.rs:377-378
let root_id = self.set_root(element);
self.request_layout(root_id);
```

**Verification:**
- Run hello_world_view demo
- Confirm UI renders (blue background + white text)
- Check logs: "Layout complete count=1", "Paint complete count=1"

#### Scenario: Layout phase processes root element on first frame

**Given** `attach()` has requested layout for root element

**When** the first frame render calls `flush_layout(constraints)`

**Then** the layout pipeline must:
1. Find root element in dirty set
2. Call root RenderObject's `layout()` method
3. Propagate constraints down the tree
4. Return computed size

**And** layout phase logs "Layout complete count=1"

**Verification:**
- Enable RUST_LOG=debug
- Observe layout phase execution
- Verify RenderObject::layout() calls for root and children

---

## ADDED Requirements

### Requirement: BuildContext guard lifetime management must use closure-based scoping

**Priority:** High
**Type:** Best Practice

#### Context

Direct guard creation in block scopes is error-prone because the guard can be dropped before asynchronous or recursive operations complete. Using a closure-based helper function enforces correct lifetime management through Rust's borrow checker.

#### Scenario: with_build_context() ensures guard outlives closure execution

**Given** a widget needs to build its children recursively

**When** `with_build_context(&ctx, || operation())` is called

**Then** the BuildContextGuard must:
1. Be created before closure execution
2. Remain alive during entire closure (including recursion)
3. Be dropped only after closure returns

**And** the borrow checker enforces this at compile time

**Implementation:**
```rust
// crates/flui_core/src/view/build_context.rs:621-627
pub fn with_build_context<F, R>(context: &BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()
}
```

**Verification:**
- Code compiles (borrow checker validates lifetime)
- No runtime panics during nested View builds
- All children can access BuildContext

---

### Requirement: BuildContext must not be allocated per-frame

**Priority:** High
**Type:** Performance

#### Context

Creating BuildContext on every frame would waste CPU time and memory. The architecture must reuse BuildContext instances across frames, only creating new ones during component rebuilds when necessary.

#### Scenario: Startup creates BuildContext once

**Given** the application is starting up

**When** `PipelineOwner::attach()` is called for the root widget

**Then** exactly one BuildContext must be created via `BuildContext::new()`

**And** this BuildContext is used for the initial View → Element conversion

**Implementation:**
```rust
// crates/flui_core/src/pipeline/pipeline_owner.rs:359
let ctx = BuildContext::new(self.tree.clone(), ElementId::new(ROOT_PLACEHOLDER));
```

**Verification:**
- Add tracing to BuildContext::new()
- Run demo and count allocations
- Confirm only 1 allocation during startup

#### Scenario: Frame rendering reuses HookContext

**Given** a component has been built before and has stored HookContext in its state

**When** the component is marked dirty and rebuilt during a frame

**Then** the system must:
1. Extract existing HookContext from component state (Arc clone)
2. Create BuildContext with existing HookContext (no new allocation)
3. Build widget tree with reused context

**And** no new HookContext allocations occur

**Implementation:**
```rust
// crates/flui_core/src/pipeline/build_pipeline.rs:596-612
fn extract_or_create_hook_context(component: &mut ComponentElement)
    -> Arc<Mutex<HookContext>>
{
    if let Some(ctx) = component.state_mut()
        .downcast_mut::<Arc<Mutex<HookContext>>>()
    {
        // Reuse existing (cheap Arc clone)
        ctx.clone()
    } else {
        // First build only
        let ctx = Arc::new(Mutex::new(HookContext::new()));
        component.set_state(Box::new(ctx.clone()));
        ctx
    }
}
```

**Verification:**
- Add tracing to HookContext::new()
- Run animated demo (many rebuilds)
- Confirm HookContext created once per component (not per rebuild)

---

## Related Specs

- **`window-events`**: Window resize event handling (triggers layout)
- **`build-pipeline`**: Build phase execution (uses BuildContext)
- **`layout-pipeline`**: Layout phase execution (triggered by attach)

## Implementation Notes

### Files Modified

1. **`crates/flui_core/src/pipeline/pipeline_owner.rs`**
   - Lines 367: Use `with_build_context()` closure
   - Lines 377-378: Add `request_layout()` after `set_root()`

2. **`crates/flui_core/src/view/build_context.rs`**
   - Lines 621-627: `with_build_context()` helper (already existed, now documented)

3. **`crates/flui_core/src/pipeline/build_pipeline.rs`**
   - Lines 596-612: `extract_or_create_hook_context()` (already existed, now documented)

### Breaking Changes

None. These are internal implementation fixes.

### Performance Impact

- ✅ **Positive**: BuildContext allocated once (not per-frame)
- ✅ **Positive**: HookContext reused across rebuilds (Arc clone is cheap)
- ✅ **Neutral**: Closure-based scoping has zero runtime cost
- ✅ **Neutral**: Explicit layout request adds ~1 function call (negligible)

### Testing Strategy

**Unit Tests:**
- Test `with_build_context()` ensures guard lifetime
- Test `attach()` calls `request_layout()`

**Integration Tests:**
- Test nested widgets can all access BuildContext
- Test initial render executes all three phases
- Test BuildContext allocation count during animation

**Manual Verification:**
- Run hello_world_view demo (visual confirmation)
- Enable RUST_LOG=debug and check logs
- Profile memory usage during sustained operation

## Validation Checklist

- [x] BuildContext panic resolved
- [x] Initial render works (blue background + text)
- [x] Layout phase executes on first frame
- [x] Paint phase executes on first frame
- [x] BuildContext created once at startup
- [x] HookContext reused across rebuilds
- [x] No performance regressions
- [x] Nested widgets build successfully
- [x] Logs show correct phase execution
- [x] Demo runs without errors

## References

- Flutter BuildContext: https://api.flutter.dev/flutter/widgets/BuildContext-class.html
- Flutter RenderView.scheduleInitialLayout(): https://api.flutter.dev/flutter/rendering/RenderView/scheduleInitialLayout.html
- Rust RAII: https://doc.rust-lang.org/rust-by-example/scope/raii.html
