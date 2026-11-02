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
        Some(Box::new(move |_| {
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

/// A subscriber, scoped to the current thread, that panics when the span named `name` is created
/// (`on_exit` false) or when it exits (`on_exit` true).
struct PanicsOnSpan {
    name: &'static str,
    on_exit: bool,
}

impl tracing::Subscriber for PanicsOnSpan {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let named = attributes.metadata().name() == self.name;
        assert!(
            !named || self.on_exit,
            "a subscriber that cannot open {}",
            self.name
        );
        tracing::span::Id::from_u64(if named { 2 } else { 1 })
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, id: &tracing::span::Id) {
        assert!(
            !self.on_exit || id.into_u64() != 2,
            "a subscriber that cannot close {}",
            self.name
        );
    }
}

/// Runs `render` under `subscriber` and returns whether it panicked.
fn render_under(subscriber: PanicsOnSpan, render: impl FnOnce()) -> bool {
    // A second dispatcher keeps the callsite interest from being cached against this thread's
    // subscriber alone.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let unwound = with_quiet_panics(|| {
        tracing::subscriber::with_default(subscriber, || {
            tracing::callsite::rebuild_interest_cache();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(render))
        })
    });
    tracing::callsite::rebuild_interest_cache();
    unwound.is_err()
}

/// The sink showed the scene, so the frame is committed even when the subscriber cannot open the
/// `commit` span; the subscriber's panic then continues.
///
/// Fails when the span is opened before the commit and its panic skips it.
#[test]
fn a_subscriber_that_panics_opening_the_commit_span_does_not_skip_the_commit() {
    let ui_runtime = mount_box();
    let mut backend = ScriptedSink::always_presents();

    let panicked = render_under(
        PanicsOnSpan {
            name: "commit",
            on_exit: false,
        },
        || {
            ui_runtime.render_frame(&mut backend);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(
        primary_state(&ui_runtime),
        FrameCommitState::Committed,
        "the sink showed the scene, so the displayed tree is committed"
    );
}

/// A retryable verdict keeps its repaint obligation when the subscriber panics as the `submit`
/// span closes: the next frame still has paint work to resubmit.
///
/// Fails when the span's panic resumes before the retry work is applied.
#[test]
fn a_subscriber_that_panics_as_the_submit_span_closes_keeps_the_retry() {
    let ui_runtime = mount_box();
    let mut failing = ScriptedSink::new(|_, _| SubmitVerdict::Retry);

    let panicked = render_under(
        PanicsOnSpan {
            name: "submit",
            on_exit: true,
        },
        || {
            ui_runtime.render_frame(&mut failing);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the retry has paint work to resubmit and reaches the sink"
    );
}

/// A subscriber that cannot open the `submit` span keeps the sink from ever seeing the scene, and
/// the pipeline has already spent that scene's paint work: the repaint is owed before the panic
/// resumes, or the next frame finds nothing to draw.
///
/// Fails when only the staged agent reads are restored.
#[test]
fn a_subscriber_that_panics_opening_the_submit_span_keeps_the_repaint_owed() {
    let ui_runtime = mount_box();
    let mut backend = ScriptedSink::always_presents();

    let panicked = render_under(
        PanicsOnSpan {
            name: "submit",
            on_exit: false,
        },
        || {
            ui_runtime.render_frame(&mut backend);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(backend.submit_calls, 0, "the sink never saw the scene");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the lost scene is painted again"
    );
}

/// A withheld frame whose `commit` span fails still records the withholding and arms its retry:
/// the commit's panic waits for the verdict's own obligations.
///
/// Fails when `commit_painted_frame` unwinds before the verdict arm finishes.
#[test]
fn a_subscriber_that_panics_in_the_commit_span_of_a_withheld_frame_keeps_the_retry() {
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);

    let panicked = render_under(
        PanicsOnSpan {
            name: "commit",
            on_exit: true,
        },
        || {
            ui_runtime.render_frame(&mut withheld);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the withheld scene is painted again"
    );
}
