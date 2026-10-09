//! A post-frame callback registered on the **binding's own**
//! scheduler observes THIS frame's committed layout.
//!
//! # Contract
//!
//! The post-frame phase follows the persistent phase, which is where the
//! pipeline runs. A hero transition depends on it: it forces a route offstage,
//! schedules a post-frame callback, and measures the destination hero in that
//! same frame.
//!
//! Previously, `pump_frame` never drained the post-frame queue at all, and never
//! opened a scheduler frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::prelude::*;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_view::{BuildOwner, tree::ElementTree};
use parking_lot::RwLock;

/// A leaf that lays out to a fixed size, so "did layout commit?" is observable
/// as `box_size(root) == Some(40x24)`.
///
/// `probe` is sampled **inside `perform_layout`** — the only vantage point that
/// can tell "the post-frame callback already ran" from "it has not run yet". A
/// probe placed in a persistent callback cannot: persistent callbacks run before
/// the pipeline in *both* the fixed and the broken ordering, so it would report
/// `false` either way (caught by review, and confirmed by injecting the bug).
#[derive(Default)]
struct FixedBox {
    probe: Option<Box<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for FixedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedBox").finish_non_exhaustive()
    }
}
impl flui_foundation::Diagnosticable for FixedBox {}
impl RenderBox for FixedBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok({
            if let Some(probe) = &self.probe {
                probe();
            }
            Size::new(40.0, 24.0)
        })
    }
    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
}

fn binding_with_one_box() -> (HeadlessBinding, PipelineCell, flui_foundation::RenderId) {
    binding_with_probe(FixedBox::default())
}

fn binding_with_probe(
    root_box: FixedBox,
) -> (HeadlessBinding, PipelineCell, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.insert::<BoxProtocol>(Box::new(root_box));
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

    let pipeline = PipelineCell::new(owner);
    let binding =
        HeadlessBinding::with_tree(BuildOwner::new(), ElementTree::new(), pipeline.clone());
    (binding, pipeline, root)
}

/// **The acceptance test.** The callback is never invoked by the test — it
/// runs because `pump_frame` drives a real scheduler frame — and when it runs, it
/// already sees this frame's committed geometry.
pub(crate) fn post_frame_callback_runs_after_layout_in_the_same_pumped_frame() {
    let (mut binding, pipeline, root) = binding_with_one_box();

    assert_eq!(
        pipeline.with(|owner| owner.box_size(root)),
        None,
        "nothing is laid out before the first frame"
    );

    let observed: Arc<RwLock<Option<Size>>> = Arc::new(RwLock::new(None));
    let calls = Arc::new(AtomicUsize::new(0));

    let observed_cb = Arc::clone(&observed);
    let calls_cb = Arc::clone(&calls);
    let pipeline_cb = pipeline;
    // `PipelineCell` is `!Send`, so this callback cannot go through
    // `add_post_frame_callback` (its `Box<dyn Fn() + Send + Sync>` bound is for
    // cross-thread wake, not owner-local frame callbacks). `PostFrameHandle::
    // schedule` accepts it instead, matching the `editable_text.rs` IME
    // cursor-loop pattern: the handle is `!Send` and addresses its lane directly
    // (a `Weak` pointer), so there is no "active lane" requirement to satisfy —
    // only the lane and its scheduler need to still be alive.
    let post_frame_handle = binding
        .build_owner_mut()
        .post_frame_handle()
        .expect("owner-local post-frame handle installed by with_tree/bind_tree")
        .clone();
    post_frame_handle
        .schedule(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            *observed_cb.write() = pipeline_cb.with(|owner| owner.box_size(root));
        })
        .expect("the lane outlives this call");

    binding.pump_frame(Duration::from_millis(16));

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "pump_frame must drive the post-frame queue exactly once"
    );
    assert_eq!(
        *observed.read(),
        Some(Size::new(40.0, 24.0)),
        "the post-frame callback must observe THIS frame's committed layout"
    );
}
