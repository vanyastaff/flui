//! Unit tests for `RenderState<P>` storage primitives.
//!
//! The propagation tests + `MockTree` helper that previously lived here were
//! deleted during the flui-rendering Phase 1 zombie cleanup
//! (`docs/plans/2026-05-20-005-refactor-flui-rendering-zombie-cleanup-plan.md`)
//! because the `RenderState::mark_needs_*` methods they exercised were
//! unreachable in production. Production dirty marking goes through
//! `PipelineOwner::add_node_needing_layout / mark_needs_paint` invoked
//! from `flui-view` and `flui-hot-reload`. Coverage of the real production
//! path is tracked separately under Mythos audit Step 4 item 13.

// ========================================================================
// Static memory-footprint assertions
// ========================================================================
//
// These tests guard the data-oriented design budgets documented in
// `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` Section 9.
// If a future change blows up the per-node size, these tests fail
// loudly rather than the regression sneaking in unobserved.

// ========================================================================
// Construction, parent data, and layout-cache accessors
// ========================================================================

// ========================================================================
// Box- and Sliver-protocol geometry convenience methods
// ========================================================================
