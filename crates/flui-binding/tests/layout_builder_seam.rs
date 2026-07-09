//! `HeadlessBinding::pump_frame` runs the shared layout↔build fixpoint
//! (ADR-0017 U1).
//!
//! These plant a registry entry by hand rather than mounting a real
//! `LayoutBuilder`, so they stay pure wiring tests of the frame path. `flui-app` carries the mirror-image test for `draw_frame`; if
//! either binding stopped calling
//! `BuildOwner::run_frame_with_layout_builders`, exactly one of the two would
//! fail, which is the divergence this pair exists to catch.

use std::sync::Arc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use flui_view::{BuildOwner, tree::ElementTree};
use parking_lot::RwLock;

/// A frame must drive `service_layout_builders`, which prunes entries whose
/// element and render node do not exist.
///
/// Pruning is the observable side effect available without a real
/// `RenderLayoutBuilder`: it happens on every pass, before anything is built.
/// If `pump_frame` called the plain `PipelineOwner::run_frame` instead of the
/// fixpoint helper, the stale entry would survive.
#[test]
fn headless_pump_frame_runs_the_layout_builder_seam() {
    let mut build_owner = BuildOwner::new();
    let cell = build_owner.register_layout_builder_for_test(RenderId::new(1), ElementId::new(1));
    assert_eq!(build_owner.layout_builder_count(), 1);
    drop(cell);

    let mut binding = HeadlessBinding::with_tree(
        build_owner,
        ElementTree::new(),
        Arc::new(RwLock::new(PipelineOwner::new())),
    );

    binding.pump_frame(Duration::from_millis(16));

    assert_eq!(
        binding.build_owner_mut().layout_builder_count(),
        0,
        "pump_frame must run service_layout_builders (via the shared \
         run_frame_with_layout_builders helper), which prunes the stale entry"
    );
}

/// A frame over an empty registry is a plain `run_frame`: it must not panic, and
/// the fixpoint must converge on its first pass.
#[test]
fn headless_pump_frame_with_no_layout_builders_is_inert() {
    let mut binding = HeadlessBinding::with_tree(
        BuildOwner::new(),
        ElementTree::new(),
        Arc::new(RwLock::new(PipelineOwner::new())),
    );

    binding.pump_frame(Duration::from_millis(16));
    binding.pump_frame(Duration::from_millis(16));

    assert_eq!(binding.build_owner_mut().layout_builder_count(), 0);
}
