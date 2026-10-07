use std::cell::RefCell;

use super::*;

/// Test-local view that captures its `RebuildHandle` and the UI runtime's
/// `AsyncDriver` in `init_state` into a shared slot the test reads
/// back — the exact seam `flui_view::element::future_builder::
/// FutureBuilder` (the framework's own production async widget) uses,
/// minus the `AsyncSnapshot` state machine this test does not need.
#[derive(Clone)]
struct AsyncCaptureProbeView {
    captured: Rc<RefCell<Option<(flui_view::RebuildHandle, flui_scheduler::AsyncDriver)>>>,
}

struct AsyncCaptureProbeState {
    captured: Rc<RefCell<Option<(flui_view::RebuildHandle, flui_scheduler::AsyncDriver)>>>,
}

impl StatefulView for AsyncCaptureProbeView {
    type State = AsyncCaptureProbeState;

    fn create_state(&self) -> Self::State {
        AsyncCaptureProbeState {
            captured: Rc::clone(&self.captured),
        }
    }
}

impl ViewState<AsyncCaptureProbeView> for AsyncCaptureProbeState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        if let Some(driver) = ctx.async_driver() {
            let _prev = self
                .captured
                .borrow_mut()
                .replace((ctx.rebuild_handle(), driver));
        }
    }

    fn build(
        &self,
        _view: &AsyncCaptureProbeView,
        _ctx: &dyn flui_view::BuildContext,
    ) -> impl IntoView {
        flui_widgets::SizedBox::new(0.0, 0.0)
    }
}

impl flui_view::View for AsyncCaptureProbeView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// The mandatory async-task-disposition audit: `AsyncDriver` is
/// UI runtime-level (shared by every presentation in a UI runtime), so an
/// in-flight task spawned from a presentation that later closes is
/// NOT cancelled — it keeps getting polled until it completes. Its
/// completion reaches back into a tree through a `RebuildHandle`,
/// which routes through `ExternalBuildScheduler` — an
/// `Arc<Mutex<HashMap<ElementId, _>>>` inbox minted fresh per
/// `BuildOwner` at construction, never shared or reused across
/// owners (`crates/flui-view/src/owner/build_owner.rs`'s
/// `external_scheduler`). A stale completion's `schedule()` call
/// therefore writes into ITS OWN (orphaned, since that `BuildOwner`
/// already dropped) inbox — there is no shared table keyed only by
/// raw `ElementId` for it to alias a live sibling's entry in, even
/// when the two owners' trees happen to reuse the same numeral (the
/// identical-numeral hazard `GlobalKeyRegistryComposite` exists for
/// is a DIFFERENT registry; this one was never shared to begin
/// with). This test proves it end to end rather than resting on
/// that code reading alone: no fix was needed here, and this test is
/// the evidence for that, not a description of one.
pub(crate) fn async_completion_after_presentation_teardown_fails_closed_no_sibling_reach() {
    let mut ui_runtime = UiRuntime::for_test();
    // `ui_runtime` starts with exactly one presentation; call it "A" and
    // install a second, "B", to observe.
    let a_id = ui_runtime.presentation_id();
    let b_id = ui_runtime.install_second_presentation_for_test();

    let captured = Rc::new(RefCell::new(None));
    let probe = AsyncCaptureProbeView {
        captured: Rc::clone(&captured),
    };
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&probe))
        .expect("A mounts the capture probe");
    // `attach_root_widget` only creates the root element and
    // schedules the first build -- `init_state` (where the probe
    // captures its handles) does not run until that build pass
    // actually happens, exactly like `a1_autowrap_causes_
    // registration_after_build_pass` above.
    let _ = ui_runtime.enter(|ui_runtime| {
        ui_runtime.draw_frame_entered(BoxConstraints::tight(flui_foundation::geometry::Size::new(
            20.0, 20.0,
        )))
    });

    let (rebuild_handle, driver) = captured
        .borrow_mut()
        .take()
        .expect("init_state must have captured both handles");

    // Spawn on the UI runtime's shared driver — exactly what a real
    // async widget on presentation A would have done. Nothing
    // drives it to completion until explicitly polled below, well
    // after A has already closed.
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran_marker = Arc::clone(&ran);
    let _token = driver.spawn_local(Box::pin(async move {
        rebuild_handle.schedule(flui_foundation::RebuildReason::AsyncCompletion);
        ran_marker.store(true, Ordering::Relaxed);
    }));

    // Close A through the real consolidated teardown path —
    // production never cancels this ui_runtime-level `AsyncDriver`'s
    // in-flight tasks (see this test's own doc and
    // `UiRuntime::close_presentation_entered`'s).
    assert!(
        ui_runtime.close_presentation_entered(a_id),
        "A was installed"
    );

    let b_has_pending_builds = || {
        ui_runtime
            .presentations
            .get(b_id)
            .expect("B still installed")
            .widgets()
            .has_pending_builds()
    };
    assert!(
        !b_has_pending_builds(),
        "precondition: B has no pending build before A's stale task runs"
    );

    // Poll the driver to completion — A's task runs, its captured
    // `RebuildHandle` schedules against A's own (now-orphaned)
    // inbox.
    ui_runtime.scheduler().drive_frame(
        ui_runtime.owner_frame(),
        flui_scheduler::Instant::now(),
        flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
        || {},
    );
    assert!(
        ran.load(Ordering::Relaxed),
        "A's stale task must still run to completion"
    );

    assert!(
        !b_has_pending_builds(),
        "A's stale completion must not have reached B's build inbox"
    );
}
