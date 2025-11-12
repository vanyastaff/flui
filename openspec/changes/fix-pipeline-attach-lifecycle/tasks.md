# Tasks: Fix PipelineOwner Root Widget Attachment Lifecycle

**Change ID:** `fix-pipeline-attach-lifecycle`
**Status:** ✅ All tasks completed

## Task Breakdown

### Phase 1: Investigate and Diagnose (✅ Completed)

**Task 1.1: Reproduce the blank screen issue** (✅ Done)
- Run hello_world_view demo
- Observe that window opens but nothing renders
- Check background processes for panic messages
- **Result:** Found panic "No BuildContext available!"

**Task 1.2: Analyze BuildContext lifecycle** (✅ Done)
- Read `pipeline_owner.rs` attach() implementation
- Identify BuildContextGuard lifetime issue (block scope problem)
- Compare with old FluiApp::build_root() code (commit ed498f9)
- **Result:** Guard dropped too early

**Task 1.3: Check git history for missing logic** (✅ Done)
- User hint: "ты можешь глянуть в истории как раньше был app.rs"
- Examine commit ed498f9 FluiApp::build_root()
- Identify missing `request_layout()` call
- **Result:** Found the lost initialization step

### Phase 2: Fix BuildContext Guard Lifetime (✅ Completed)

**Task 2.1: Attempt direct guard fix** (✅ Done - Initial attempt)
- Move BuildContextGuard outside block scope
- Test with hello_world_view
- **Result:** Still didn't work, needed closure approach

**Task 2.2: Implement with_build_context() closure** (✅ Done - Final fix)
- Use `crate::view::with_build_context(&ctx, || widget.into_element())`
- Ensures guard lives for entire closure including recursive calls
- Verify thread-local storage works correctly
- **Result:** BuildContext panic resolved (commit 8c42968)

**Task 2.3: Verify nested View builds work** (✅ Done)
- Test with hello_world_view (has nested Container → Center → Text)
- Confirm no panics during recursive into_element() calls
- Check logs for successful build phase
- **Result:** "Build complete count=1" ✓

### Phase 3: Add Missing Layout Request (✅ Completed)

**Task 3.1: Add request_layout() to attach()** (✅ Done)
- Modify `PipelineOwner::attach()` in `pipeline_owner.rs:377-378`
- Add `self.request_layout(root_id);` after `set_root()`
- Add tracing log for visibility
- **Result:** Commit afc8c1d

**Task 3.2: Verify initial render works** (✅ Done)
- Run hello_world_view demo
- Confirm blue background appears
- Confirm white "Hello, World!" text appears
- Check logs: "Layout complete count=1", "Paint complete count=1"
- **Result:** Full rendering pipeline working ✓

**Task 3.3: Test with multiple widgets** (✅ Done)
- Verify Container, Center, Text all render correctly
- Check logs for drawing commands: "text_count=1 rects=1"
- **Result:** All widgets rendering correctly ✓

### Phase 4: Fix Window Resize Behavior (✅ Completed)

**Task 4.1: Identify resize issue** (✅ Done)
- User feedback: "ну так размеры жкрана меняю и ничего не происходит"
- Check `wgpu.rs` WindowEvent::Resized handler
- Identify missing layout request
- **Result:** Handler only resizes GPU surface, not UI tree

**Task 4.2: Add layout request on resize** (✅ Done)
- Modify `wgpu.rs:257-264`
- Add pipeline access and request_layout() call
- Add tracing for debugging
- **Result:** Commit 6cbf49d

**Task 4.3: Verify resize responsiveness** (✅ Done)
- Run hello_world_view
- Resize window manually
- Check logs: "Layout complete count=1 constraints=801.0x600.0"
- Observe UI adapting to new window size
- **Result:** UI now responsive to resize ✓

### Phase 5: Performance Verification (✅ Completed)

**Task 5.1: Verify BuildContext not created per-frame** (✅ Done)
- User concern: "а у нас там нет проблем того что каждый фремйм создает build context"
- Analyze `pipeline_owner.rs` and `build_pipeline.rs`
- Confirm BuildContext::new() only in attach() (once at startup)
- Confirm rebuilds use with_hook_context_and_queue() (reuses HookContext)
- **Result:** No per-frame allocation ✓

**Task 5.2: Verify HookContext persistence** (✅ Done)
- Read `build_pipeline.rs:596-612` extract_or_create_hook_context()
- Confirm Arc<Mutex<HookContext>> stored in component state
- Confirm reuse on subsequent rebuilds (no re-allocation)
- **Result:** HookContext persists across rebuilds ✓

**Task 5.3: Measure frame timing** (✅ Done)
- Run demo and observe tracing logs
- Verify no unusual spikes in frame time
- Confirm Layout/Paint phases execute only when dirty
- **Result:** Performance matches expectations ✓

### Phase 6: Documentation and Cleanup (✅ Completed)

**Task 6.1: Create OpenSpec proposal** (✅ Done)
- Document all three issues and fixes
- Include code examples (before/after)
- Add verification logs
- Create `proposal.md` in `openspec/changes/fix-pipeline-attach-lifecycle/`
- **Result:** This document

**Task 6.2: Clean up obsolete files** (✅ Done)
- Remove Python migration scripts (migrate_slivers.py, fix_sliver_layouts.py)
- Remove duplicate AGENTS.md
- Remove outdated architecture docs (APP_ARCHITECTURE.md, flui_app_wgpu_architecture.md, HIT_TEST_IMPLEMENTATION_PLAN.md)
- **Result:** Commit 00fc1c5 "chore: Remove obsolete Python scripts and duplicate documentation"

**Task 6.3: Push all changes** (✅ Done)
- Verify git status clean
- Push to origin/main
- **Result:** All commits successfully pushed ✓

## Task Dependencies

```
Phase 1 (Investigate)
    ↓
Phase 2 (Fix BuildContext) ──────┐
    ↓                            │
Phase 3 (Add Layout Request)     │
    ↓                            │
Phase 4 (Fix Resize) ────────────┤
    ↓                            │
Phase 5 (Verify Performance) ────┤
    ↓                            │
Phase 6 (Document) ←─────────────┘
```

## Validation Checklist

- [x] hello_world_view demo renders blue background
- [x] hello_world_view demo renders white text "Hello, World!"
- [x] No BuildContext panics during startup
- [x] Layout phase executes on first frame
- [x] Paint phase executes on first frame
- [x] Window resize triggers UI layout updates
- [x] Logs show correct layout constraints after resize
- [x] BuildContext only created once at startup
- [x] HookContext persists across rebuilds
- [x] No performance regressions
- [x] All code changes committed
- [x] All code changes pushed to remote
- [x] Documentation created (this OpenSpec proposal)

## Success Metrics

| Metric | Before | After | Status |
|--------|--------|-------|--------|
| BuildContext panics | Yes | No | ✅ Fixed |
| Initial render works | No | Yes | ✅ Fixed |
| Resize responsive | No | Yes | ✅ Fixed |
| BuildContext allocations/frame | Unknown | 0 | ✅ Verified |
| HookContext allocations/rebuild | Unknown | 0 (reused) | ✅ Verified |

## Related Commits

- `5c478ab` - Initial BuildContext guard fix attempt
- `afc8c1d` - Added request_layout() after root attachment
- `8c42968` - Used with_build_context() closure approach
- `6cbf49d` - Fixed window resize behavior
- `00fc1c5` - Cleaned up obsolete files

## Time Investment

- Investigation: ~30 minutes
- Fix implementation: ~1 hour
- Verification: ~20 minutes
- Documentation: ~30 minutes
- **Total: ~2 hours 20 minutes**

## Lessons Learned

1. **Always verify complete lifecycle sequences** when refactoring
2. **RAII guard lifetimes are critical** for thread-local storage
3. **Git history is invaluable** for finding lost logic during refactoring
4. **User feedback accelerates debugging** - "look at git history" hint was key
5. **Separation of concerns must preserve behavior** - clean architecture doesn't mean dropping steps
