//! A frame its surface withheld (`SubmitVerdict::NotShown`) is retried by publishing the version
//! the scene already holds, with nothing painted (ADR-0304 §7), unless that version is no longer
//! what the screen should show: then the retry repaints in full and publishes a new one.

use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::{FrameEpoch, PresentationAddress};
use flui_layer::SceneVersionId;
use flui_painting::styling::Color;
use flui_runtime::sink::{FrameSettlement, FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::UiRuntime;
use flui_testing::UnshownFrame;
use flui_testing::widgets::{lay_out, tight};
use flui_widgets::{ColoredBox, SizedBox};

/// A sink that answers the verdicts of its script in turn, then presents, and keeps the version
/// and epoch of every frame it was handed.
struct Scripted {
    size: (u32, u32),
    limit: NonZeroUsize,
    script: VecDeque<SubmitVerdict>,
    frames: Vec<(FrameEpoch, SceneVersionId)>,
}

impl Scripted {
    fn new(script: impl IntoIterator<Item = SubmitVerdict>) -> Self {
        Self {
            size: (400, 400),
            limit: NonZeroUsize::MIN,
            script: script.into_iter().collect(),
            frames: Vec::new(),
        }
    }

    fn versions(&self) -> Vec<SceneVersionId> {
        self.frames.iter().map(|&(_, version)| version).collect()
    }
}

impl FrameSink for Scripted {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn in_flight_limit(&mut self) -> NonZeroUsize {
        self.limit
    }

    fn submit(
        &mut self,
        epoch: FrameEpoch,
        publication: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        self.frames.push((epoch, publication.scene.id()));
        self.script.pop_front().unwrap_or(SubmitVerdict::Presented)
    }
}

fn host_turn(runtime: &mut UiRuntime, sink: &mut Scripted) {
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
        sink,
    );
}

fn mounted() -> UiRuntime {
    let runtime = crate::owner_publication::runtime_with_wake(Arc::new(|| {}));
    runtime
        .attach_root_widget_with_size(&ColoredBox::new(Color::rgb(40, 80, 120)), 400.0, 400.0)
        .expect("mount a box");
    runtime
}

fn address(runtime: &UiRuntime) -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: runtime.id(),
        presentation_id: runtime.presentation_id(),
    }
}

/// The retry of a withheld frame paints nothing and publishes the withheld version again; the
/// frame it shows draws what a frame painted for the same tree draws.
///
/// Fails when the retry repaints the tree, which paints its nodes again and publishes a new
/// version, as ADR-0068 §3's retry did.
#[test]
fn a_frame_not_shown_is_retried_from_the_held_version() {
    let tree = |rgb: (u8, u8, u8)| {
        SizedBox::new(40.0, 20.0).child(ColoredBox::new(Color::rgb(rgb.0, rgb.1, rgb.2)))
    };
    let mut laid = lay_out(tree((10, 20, 30)), tight(200.0, 100.0));
    let shown = laid.painted_frame_count();

    laid.leave_unshown(1, UnshownFrame::Withdrawn);
    laid.pump_widget(tree((200, 40, 40)));
    assert_eq!(
        laid.painted_frame_count(),
        shown,
        "the frame of the changed tree is withheld"
    );

    let retry = laid.pump_report(Duration::from_millis(16));
    assert_eq!(
        laid.painted_frame_count(),
        shown + 1,
        "the retry shows the withheld frame"
    );
    assert_eq!(
        (retry.pipeline.nodes_painted, retry.pipeline.scene_ops),
        (0, 0),
        "the retry paints nothing and applies no patch: {retry:?}"
    );
    let reference = lay_out(tree((200, 40, 40)), tight(200.0, 100.0));
    assert!(
        !reference.draw_ops().is_empty(),
        "the tree draws something to compare"
    );
    assert_eq!(
        format!("{:?}", laid.draw_ops()),
        format!("{:?}", reference.draw_ops()),
        "the retry shows the changed tree"
    );
}

/// The retry republishes the withheld version only while it is what the screen should show. A
/// retry that finds another surface size, a repaint the host asked for (a device rebuilt, a
/// surface restored or a display changed), or a newer version applied since the withheld one,
/// repaints in full and publishes a new version; a retry that finds none of them republishes.
///
/// Fails when a retry republishes a version drawn for another size, ignores a repaint asked for,
/// or republishes a version newer than the one withheld in place of the newest tree.
#[test]
fn a_not_shown_retry_repaints_when_the_held_version_is_stale() {
    #[derive(Debug, Clone, Copy)]
    enum Between {
        Nothing,
        Resize,
        RepaintAsked,
        NewerVersion,
    }

    for (between, republished) in [
        (Between::Nothing, true),
        (Between::Resize, false),
        (Between::RepaintAsked, false),
        (Between::NewerVersion, false),
    ] {
        let mut runtime = mounted();
        let (withheld, retried) = if matches!(between, Between::NewerVersion) {
            // Two frames in flight: the older one is withheld once the newer one is queued.
            let mut sink = Scripted::new([SubmitVerdict::Queued, SubmitVerdict::Queued]);
            sink.limit = NonZeroUsize::new(2).expect("BUG: two is not zero");
            host_turn(&mut runtime, &mut sink);
            runtime.mark_primary_needs_full_repaint();
            host_turn(&mut runtime, &mut sink);
            let [(older, withheld), (_, newer)] = sink.frames[..] else {
                panic!("{between:?}: two frames were queued: {:?}", sink.versions());
            };
            assert_ne!(
                withheld, newer,
                "{between:?}: the second frame is a new version"
            );
            let _ = runtime.settle_frame(FrameSettlement::new(
                address(&runtime),
                older,
                SubmitVerdict::NotShown,
            ));
            host_turn(&mut runtime, &mut sink);
            let retried = *sink.versions().last().expect("a frame was submitted");
            assert_ne!(
                retried, newer,
                "{between:?}: the retry draws the newest tree, not the queued version again"
            );
            (withheld, retried)
        } else {
            let mut sink = Scripted::new([SubmitVerdict::NotShown]);
            host_turn(&mut runtime, &mut sink);
            match between {
                Between::Resize => sink.size = (300, 200),
                Between::RepaintAsked => runtime.mark_primary_needs_full_repaint(),
                Between::Nothing | Between::NewerVersion => {}
            }
            host_turn(&mut runtime, &mut sink);
            let [withheld, retried] = sink.versions()[..] else {
                panic!(
                    "{between:?}: the withheld frame and its retry: {:?}",
                    sink.versions()
                );
            };
            (withheld, retried)
        };
        assert_eq!(
            retried == withheld,
            republished,
            "{between:?}: the retry republishes the withheld version: {republished}"
        );
    }
}
