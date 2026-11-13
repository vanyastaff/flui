//! A local post-frame callback that reschedules **itself** must succeed and
//! fire exactly once per completed frame — the load-bearing mechanism the IME
//! cursor-area tracking loop (`flui-widgets::EditableText`, ADR-0030) builds
//! on.
//!
//! `flui-scheduler`'s own unit tests prove the primitive against a bare
//! `UpdateScheduler`. This one asks the production question: does the frame
//! every runner runs (`UiRuntime::pump`, here through [`HeadlessHost`]) drain
//! the same owner frame the handle addresses for the whole frame, so a
//! callback that reschedules itself from inside the drain succeeds?

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_scheduler::PostFrameHandle;
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_widgets::SizedBox;

/// Schedule one tick that increments `fire_count` and immediately reschedules
/// itself for the next completed frame. Mirrors `CursorAreaLoop::schedule` /
/// `fire`'s self-rescheduling shape one to one, minus the geometry payload.
fn schedule_self_rescheduling_tick(handle: PostFrameHandle, fire_count: Rc<Cell<usize>>) {
    let handle_for_reschedule = handle.clone();
    handle
        .schedule(move |_timing| {
            fire_count.set(fire_count.get() + 1);
            schedule_self_rescheduling_tick(handle_for_reschedule, fire_count);
        })
        .expect("scheduling must succeed while the UI runtime's local lane is alive");
}

/// **The acceptance test.** A self-rescheduling local post-frame callback,
/// started outside any frame (the same shape a lifecycle hook's IME-attach
/// listener uses), fires exactly once per subsequent frame — never zero (the
/// design would silently never track the caret) and never more than one (a
/// double-fire would send stale geometry every other frame).
pub(crate) fn self_rescheduling_local_post_frame_callback_fires_exactly_once_per_pumped_frame() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10));
    host.attach(&SizedBox::new(10.0, 10.0))
        .expect("a fresh host has no root yet");
    let _outcome = host.pump(Duration::ZERO);

    let fire_count = Rc::new(Cell::new(0usize));
    // The handle addresses its lane directly (a `Weak` pointer), so no
    // "active lane" scope is required to register.
    schedule_self_rescheduling_tick(host.post_frame_handle(), Rc::clone(&fire_count));
    assert_eq!(
        fire_count.get(),
        0,
        "scheduling alone must not fire the callback"
    );

    // A post-frame callback does not ask for a frame, so each frame is
    // requested, as a widget's state change would.
    for expected in 1..=3 {
        host.request_frame();
        let _outcome = host.pump(Duration::ZERO);
        assert_eq!(
            fire_count.get(),
            expected,
            "the tick fires exactly once per completed frame, its reschedule \
             surviving the drain it ran in"
        );
    }
}
