//! Unit tests for `RenderState<P>` storage primitives.
//!
//! The propagation tests + `MockTree` helper that previously lived here were
//! deleted in U3 of the flui-rendering Phase 1 zombie cleanup
//! (`docs/plans/2026-05-20-005-refactor-flui-rendering-zombie-cleanup-plan.md`)
//! because the `RenderState::mark_needs_*` methods they exercised were
//! unreachable in production. Production dirty marking goes through
//! `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked
//! from `flui-view` and `flui-hot-reload`. Coverage of the real production
//! path is tracked separately under Mythos audit Step 4 item 13.

use std::mem::size_of;

use flui_types::{Offset, geometry::px};

use super::*;
use crate::protocol::{BoxProtocol, SliverProtocol};

// ========================================================================
// Mythos Step 14 -- static memory-footprint assertions
// ========================================================================
//
// These tests guard the data-oriented design budgets documented in
// `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` Section 9.
// If a future change blows up the per-node size, these tests fail
// loudly rather than the regression sneaking in unobserved.

#[test]
fn render_state_box_fits_budget() {
    // RenderState<BoxProtocol> = AtomicRenderFlags(4) + OnceCell<Size>
    // + OnceCell<BoxConstraints> + AtomicOffset(8) + PhantomData(0).
    // Documented estimate: 44-60 bytes. Cap at 128 to leave room for
    // future fields without forcing a re-budget on every commit.
    let actual = size_of::<RenderState<BoxProtocol>>();
    assert!(
        actual <= 128,
        "RenderState<BoxProtocol> grew beyond budget: {actual} bytes (cap 128)"
    );
}

#[test]
fn render_state_sliver_fits_budget() {
    let actual = size_of::<RenderState<SliverProtocol>>();
    assert!(
        actual <= 192,
        "RenderState<SliverProtocol> grew beyond budget: {actual} bytes (cap 192)"
    );
}

#[test]
fn test_geometry_set_is_idempotent() {
    // D-block PR-A1 U14: previously `set_geometry` panicked on second
    // invocation (OnceCell-backed). Re-layout now overwrites cleanly
    // mirroring Flutter `_size = size` straight assignment.
    let mut state = BoxRenderState::new();
    let size1 = flui_types::Size::new(px(100.0), px(50.0));
    let size2 = flui_types::Size::new(px(200.0), px(100.0));

    // First set establishes geometry.
    state.set_geometry(size1);
    assert_eq!(state.geometry(), Some(size1));

    // Second set overwrites with no panic — re-layout safe.
    state.set_geometry(size2);
    assert_eq!(state.geometry(), Some(size2));

    // Clear resets to None.
    state.clear_geometry();
    assert_eq!(state.geometry(), None);
}

#[test]
fn test_atomic_offset() {
    let state = BoxRenderState::new();
    let offset = Offset::new(px(10.0), px(20.0));

    state.set_offset(offset);
    assert_eq!(state.offset(), offset);

    // Can update multiple times
    let offset2 = Offset::new(px(30.0), px(40.0));
    state.set_offset(offset2);
    assert_eq!(state.offset(), offset2);
}

#[test]
fn test_boundary_flags() {
    let state = BoxRenderState::new();

    assert!(!state.is_relayout_boundary());
    assert!(!state.is_repaint_boundary());

    state.set_relayout_boundary(true);
    assert!(state.is_relayout_boundary());

    state.set_repaint_boundary(true);
    assert!(state.is_repaint_boundary());

    state.set_relayout_boundary(false);
    assert!(!state.is_relayout_boundary());
    assert!(state.is_repaint_boundary());
}

/// Copilot P1 regression test (PR #139): a non-root node with loose
/// (non-tight) constraints and `sized_by_parent = false` must NOT default
/// to a relayout boundary. Pre-fix the U17 bootstrap passed
/// `parent_uses_size = false` which made `!parent_uses_size = true`,
/// flipping every Box node into a boundary and silently blocking
/// `PipelineOwner::mark_needs_layout` propagation at the leaf.
#[test]
fn compute_relayout_boundary_non_tight_non_root_is_not_boundary_by_default() {
    use crate::constraints::BoxConstraints;
    use flui_types::Size;

    let mut state = BoxRenderState::new();
    // Loose constraints (not tight) — typical layout-from-parent case.
    let loose = BoxConstraints::loose(Size::new(px(200.0), px(100.0)));
    state.set_constraints(loose);

    // Bootstrap as if running under the U17 wiring with the BoxProtocol
    // override: parent_uses_size=true (conservative), sized_by_parent=false,
    // has_parent=true (non-root).
    state.compute_relayout_boundary(true, false, true);

    assert!(
        !state.is_relayout_boundary(),
        "non-tight, non-root, non-sized-by-parent node MUST default to non-boundary so mark_needs_layout propagates up to a real boundary",
    );
}

/// Companion test: tight constraints still mark as boundary (Flutter parity:
/// when parent forces a single valid size, the node can re-layout in
/// isolation).
#[test]
fn compute_relayout_boundary_tight_constraints_is_boundary() {
    use crate::constraints::BoxConstraints;
    use flui_types::Size;

    let mut state = BoxRenderState::new();
    let tight = BoxConstraints::tight(Size::new(px(50.0), px(50.0)));
    state.set_constraints(tight);

    state.compute_relayout_boundary(true, false, true);

    assert!(
        state.is_relayout_boundary(),
        "tight constraints mean parent forces a single valid size — node is a boundary",
    );
}

/// Companion test: root (no parent) is always a boundary regardless of
/// other signals — propagation cannot escape the root.
#[test]
fn compute_relayout_boundary_root_is_always_boundary() {
    let state = BoxRenderState::new();
    state.compute_relayout_boundary(true, false, /* has_parent = */ false);
    assert!(
        state.is_relayout_boundary(),
        "root must always be a boundary"
    );
}
