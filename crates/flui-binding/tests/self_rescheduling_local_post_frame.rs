//! A local post-frame callback that reschedules **itself** must succeed and
//! fire exactly once per completed frame — the load-bearing mechanism the IME
//! cursor-area tracking loop (`flui-widgets::EditableText`, ADR-0032) builds
//! on. This is the design's scheduler-level precondition: if a
//! self-rescheduling `PostFrameHandle::schedule_local` callback cannot be
//! driven cleanly through the binding's own frame pump, the loop has no
//! foundation to stand on.
//!
//! # Why through `HeadlessBinding::pump_frame`, not a bare
//! `lane.enter(|| scheduler.execute_frame())`
//!
//! `flui-scheduler`'s own unit tests already prove the primitive
//! (`PostFrameHandle::schedule_local` nests and defers correctly against a
//! bare `Scheduler`). The production question this test answers is different:
//! does the *binding's* frame-pump entry point (`pump_frame`, which every
//! runner and this crate's `UiRealm`-analog calls) keep the local lane active
//! for the *entire* drain, so a callback that reschedules itself from inside
//! the drain succeeds — not just a hand-rolled `execute_frame` call a
//! production caller never actually makes.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_scheduler::PostFrameHandle;
use flui_view::BuildOwner;

/// Schedule one tick that increments `fire_count` and immediately reschedules
/// itself for the next completed frame. Mirrors `CursorAreaLoop::schedule` /
/// `fire`'s self-rescheduling shape one to one, minus the geometry payload.
fn schedule_self_rescheduling_tick(handle: PostFrameHandle, fire_count: Rc<Cell<usize>>) {
    let handle_for_reschedule = handle.clone();
    handle
        .schedule_local(move |_timing| {
            fire_count.set(fire_count.get() + 1);
            schedule_self_rescheduling_tick(handle_for_reschedule, fire_count);
        })
        .expect(
            "scheduling must succeed while the binding's local post-frame lane is active \
             (enter_owner_scope/pump_frame)",
        );
}

/// **The acceptance test.** A self-rescheduling local post-frame callback,
/// started outside any frame (the same shape a lifecycle hook's IME-attach
/// listener uses), fires exactly once per subsequent `pump_frame` call —
/// never zero (the design would silently never track the caret) and never
/// more than one (a double-fire would send stale geometry every other
/// frame).
#[test]
fn self_rescheduling_local_post_frame_callback_fires_exactly_once_per_pumped_frame() {
    let mut binding = HeadlessBinding::new();
    let mut build_owner = BuildOwner::new();
    binding.install_build_capabilities(&mut build_owner);
    let handle = build_owner
        .post_frame_handle()
        .cloned()
        .expect("install_build_capabilities always installs a post-frame handle");

    let fire_count = Rc::new(Cell::new(0usize));

    // The initial schedule runs inside the binding's active local lane —
    // matching production, where a lifecycle hook (a focus-gain listener)
    // always fires while `pump_frame`'s (or `UiRealm::enter`'s) lane is
    // active. Scheduling with no active lane returns `InactiveLane`, not a
    // deferred success.
    binding.enter_owner_scope(|| {
        schedule_self_rescheduling_tick(handle, Rc::clone(&fire_count));
    });

    assert_eq!(
        fire_count.get(),
        0,
        "scheduling alone must not fire the callback"
    );

    binding.pump_frame(Duration::ZERO);
    assert_eq!(
        fire_count.get(),
        1,
        "the callback must fire exactly once on the next completed frame — \
         not the frame during which it was scheduled"
    );

    binding.pump_frame(Duration::ZERO);
    assert_eq!(
        fire_count.get(),
        2,
        "the self-rescheduled tick must fire again exactly once on the \
         following frame, proving the reschedule survives pump_frame's own \
         lane re-entry rather than being dropped or double-fired"
    );

    binding.pump_frame(Duration::ZERO);
    assert_eq!(
        fire_count.get(),
        3,
        "the loop keeps firing exactly once per pumped frame indefinitely"
    );
}
