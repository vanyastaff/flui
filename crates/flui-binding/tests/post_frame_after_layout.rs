//! A post-frame callback registered on the **binding's own**
//! scheduler observes THIS frame's committed layout.
//!
//! # Parity oracle
//!
//! `.flutter/packages/flutter/lib/src/scheduler/binding.dart:1338-1358` — the
//! post-frame phase follows the persistent phase, which is where the pipeline
//! runs. `heroes.dart:966-971` is the caller that depends on it: it forces a
//! route offstage, schedules a post-frame callback, and measures the destination
//! hero in that same frame.
//!
//! Previously, `pump_frame` never drained the post-frame queue at all, and never
//! opened a scheduler frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::HasInstance;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::prelude::*;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Size, geometry::px};
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
    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        if let Some(probe) = &self.probe {
            probe();
        }
        Size::new(px(40.0), px(24.0))
    }
    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
}

fn binding_with_one_box() -> (
    HeadlessBinding,
    Arc<RwLock<PipelineOwner>>,
    flui_foundation::RenderId,
) {
    binding_with_probe(FixedBox::default())
}

fn binding_with_probe(
    root_box: FixedBox,
) -> (
    HeadlessBinding,
    Arc<RwLock<PipelineOwner>>,
    flui_foundation::RenderId,
) {
    let mut owner = PipelineOwner::new();
    let root = owner.insert::<BoxProtocol>(Box::new(root_box));
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let pipeline = Arc::new(RwLock::new(owner));
    let binding =
        HeadlessBinding::with_tree(BuildOwner::new(), ElementTree::new(), Arc::clone(&pipeline));
    (binding, pipeline, root)
}

/// **The acceptance test.** The callback is never invoked by the test — it
/// runs because `pump_frame` drives a real scheduler frame — and when it runs, it
/// already sees this frame's committed geometry.
#[test]
fn post_frame_callback_runs_after_layout_in_the_same_pumped_frame() {
    let (mut binding, pipeline, root) = binding_with_one_box();

    assert_eq!(
        pipeline.read().box_size(root),
        None,
        "nothing is laid out before the first frame"
    );

    let observed: Arc<RwLock<Option<Size>>> = Arc::new(RwLock::new(None));
    let calls = Arc::new(AtomicUsize::new(0));

    let observed_cb = Arc::clone(&observed);
    let calls_cb = Arc::clone(&calls);
    let pipeline_cb = Arc::clone(&pipeline);
    binding
        .scheduler()
        .add_post_frame_callback(Box::new(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            *observed_cb.write() = pipeline_cb.read().box_size(root);
        }));

    binding.pump_frame(Duration::from_millis(16));

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "pump_frame must drive the post-frame queue exactly once"
    );
    assert_eq!(
        *observed.read(),
        Some(Size::new(px(40.0), px(24.0))),
        "the post-frame callback must observe THIS frame's committed layout"
    );
}

/// The negative half, observed from **inside layout**: while the pipeline runs,
/// the post-frame callback has not fired yet.
///
/// The previous production order (drain, then pipeline) makes this fail — the
/// probe would see `fired == true` while laying out. An earlier version of this
/// test sampled from a *persistent* callback and passed under the bug, because
/// persistent callbacks precede the pipeline in both orderings. Red-checked.
#[test]
fn the_post_frame_callback_has_not_run_while_layout_is_still_uncommitted() {
    let fired = Arc::new(AtomicBool::new(false));
    let seen_during_layout = Arc::new(AtomicBool::new(false));
    let laid_out = Arc::new(AtomicBool::new(false));

    let fired_probe = Arc::clone(&fired);
    let seen_probe = Arc::clone(&seen_during_layout);
    let laid_out_probe = Arc::clone(&laid_out);
    let root_box = FixedBox {
        probe: Some(Box::new(move || {
            laid_out_probe.store(true, Ordering::SeqCst);
            seen_probe.store(fired_probe.load(Ordering::SeqCst), Ordering::SeqCst);
        })),
    };

    let (mut binding, pipeline, root) = binding_with_probe(root_box);

    let fired_cb = Arc::clone(&fired);
    binding
        .scheduler()
        .add_post_frame_callback(Box::new(move |_| {
            fired_cb.store(true, Ordering::SeqCst);
        }));

    binding.pump_frame(Duration::from_millis(16));

    assert!(laid_out.load(Ordering::SeqCst), "the probe must have run");
    assert!(
        !seen_during_layout.load(Ordering::SeqCst),
        "the post-frame callback ran before layout committed"
    );
    assert!(fired.load(Ordering::SeqCst), "but it did run by frame end");
    assert_eq!(
        pipeline.read().box_size(root),
        Some(Size::new(px(40.0), px(24.0)))
    );
}

/// The real invariant, preserved: exactly one async-driver poll per frame,
/// on the binding's **own** scheduler, before `build_scope`. The poll moved from
/// `pump_frame` into `Scheduler::handle_begin_frame` — it must
/// still happen, and still happen once.
#[test]
fn pump_frame_still_polls_the_async_driver_exactly_once_per_frame() {
    let (mut binding, _pipeline, _root) = binding_with_one_box();

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_task = Arc::clone(&polls);
    let _token = binding.scheduler().spawn_local(Box::pin(async move {
        polls_task.fetch_add(1, Ordering::SeqCst);
    }));

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::SeqCst), 1);

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(
        polls.load(Ordering::SeqCst),
        1,
        "the task completed; no re-poll"
    );
}

/// The binding drives its **own** scheduler, never the `Scheduler::instance()`
/// singleton. A post-frame callback parked on the singleton must not fire here —
/// otherwise a headless test would silently "prove" things about production.
#[test]
fn pump_frame_drives_the_binding_local_scheduler_not_the_singleton() {
    let (mut binding, _pipeline, _root) = binding_with_one_box();

    let singleton_fired = Arc::new(AtomicBool::new(false));
    let singleton_cb = Arc::clone(&singleton_fired);
    flui_scheduler::Scheduler::instance().add_post_frame_callback(Box::new(move |_| {
        singleton_cb.store(true, Ordering::SeqCst);
    }));

    let local_fired = Arc::new(AtomicBool::new(false));
    let local_cb = Arc::clone(&local_fired);
    binding
        .scheduler()
        .add_post_frame_callback(Box::new(move |_| {
            local_cb.store(true, Ordering::SeqCst);
        }));

    binding.pump_frame(Duration::from_millis(16));

    assert!(
        local_fired.load(Ordering::SeqCst),
        "the binding's own queue drains"
    );
    assert!(
        !singleton_fired.load(Ordering::SeqCst),
        "pump_frame must not drive the global singleton's queue"
    );
}
