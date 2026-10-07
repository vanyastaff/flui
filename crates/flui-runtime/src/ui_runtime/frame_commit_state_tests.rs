use std::cell::Cell;
use std::rc::Rc;

use flui_widgets::SizedBox;

use super::{SegmentPhase, UiRuntime};
use crate::epoch::{FrameCommitState, TreeRevision};
use crate::sink::SubmitVerdict;
use crate::testing::ScriptedSink;

fn with_quiet_panics<R>(f: impl FnOnce() -> R) -> R {
    type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;

    struct HookRestore(Option<PanicHook>);

    impl Drop for HookRestore {
        fn drop(&mut self) {
            if let Some(hook) = self.0.take() {
                std::panic::set_hook(hook);
            }
        }
    }

    let _restore = HookRestore(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));
    f()
}

fn mount_box() -> UiRuntime {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&SizedBox::new(20.0, 20.0))
        .expect("root attaches");
    ui_runtime
}

fn primary_state(ui_runtime: &UiRuntime) -> FrameCommitState {
    ui_runtime.presentations.primary().frame_commit_state()
}

fn assert_uncommitted_since(ui_runtime: &UiRuntime, since: TreeRevision) {
    assert_eq!(
        primary_state(ui_runtime),
        FrameCommitState::Uncommitted { since }
    );
}

fn arm_one_shot_build_panic(ui_runtime: &UiRuntime) {
    let is_armed = Rc::new(Cell::new(true));
    let probe_is_armed = Rc::clone(&is_armed);
    ui_runtime.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(move || {
            assert!(
                !probe_is_armed.replace(false),
                "build probe — intentional one-shot panic"
            );
        })),
    );
}

pub(crate) fn errored_frame_is_uncommitted() {
    let ui_runtime = mount_box();
    arm_one_shot_build_panic(&ui_runtime);
    let mut backend = ScriptedSink::always_presents();

    assert!(!with_quiet_panics(|| ui_runtime.render_frame(&mut backend)));
    assert_uncommitted_since(&ui_runtime, TreeRevision::ZERO.next());
    assert_eq!(backend.submit_calls, 0);
}

pub(crate) fn deferred_painted_frame_waits_for_the_later_present_to_commit() {
    let ui_runtime = mount_box();
    ui_runtime.defer_first_frame();
    let mut backend = ScriptedSink::always_presents();

    assert!(!ui_runtime.render_frame(&mut backend));
    assert_uncommitted_since(&ui_runtime, TreeRevision::ZERO.next());
    assert_eq!(backend.submit_calls, 0);

    ui_runtime.allow_first_frame();
    assert!(ui_runtime.render_frame(&mut backend));
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
}

/// The withheld retry is BOUNDED: a drawable that never comes back stops
/// being retried instead of spinning at the fallback pace forever.
///
/// This is the one rule separating "ride out a transient" from "loop
/// forever", and AppKit's occlusion gate does not provide it — that gate keys
/// off `occlusionState`, which the cold-start trace behind this arm has
/// reporting the window VISIBLE for the whole ~132 ms the drawable was
/// unavailable. Retrying is itself what re-dirties the presentation, so with
/// no cap a surface that stays withdrawn produces one frame per fallback
/// period indefinitely.
pub(crate) fn the_withheld_retry_is_bounded_and_then_parks() {
    let budget = super::MAX_NOT_SHOWN_RETRIES;
    let ui_runtime = mount_box();
    // Every attempt is withheld, including the one that exhausts the budget,
    // so what the test measures is WHERE the loop stopped rather than that it
    // stopped only because a frame finally succeeded.
    let mut backend = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);

    for _ in 0..=budget {
        assert!(
            !ui_runtime.render_frame(&mut backend),
            "a withheld frame never reports a present"
        );
    }
    assert_eq!(
        backend.submit_calls,
        budget + 1,
        "the budget is spent by RE-ARMING: {budget} retained attempts, then \
         the attempt that exhausts it and runs without arming another"
    );

    for _ in 0..8 {
        let _ = ui_runtime.render_frame(&mut backend);
    }
    assert_eq!(
        backend.submit_calls,
        budget + 1,
        "once the budget is spent and nothing else is dirty the loop must \
         park: no further frame may reach the backend"
    );
}
