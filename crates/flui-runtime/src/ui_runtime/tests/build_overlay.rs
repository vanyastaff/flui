//! The build banner a development tool reports (`build.report`) is a node of its own after the
//! frame node and the performance overlay: published when it changes, on a frame the pipeline
//! paints nothing in, kept while it holds still, and never between the application and its
//! input.

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::{Offset, Rect};
use flui_interaction::events::{PointerKind, make_down_event, make_move_event, make_up_event};
use flui_interaction::pointer::HitTestBehavior;
use flui_layer::{Chunk, Entry, Layer, SceneNodeId, SceneVersion};
use flui_painting::{DrawOp, FontCollection};
use flui_protocol::build::{
    BuildReportParams, BuildState, Diagnostic, MAX_DIAGNOSTICS, SourceLocation,
};
use flui_protocol::error::ErrorCode;
use flui_protocol::method;
use flui_view::dev_agent::AgentWindow;
use flui_widgets::Listener;

use super::*;

const SIZE: flui_foundation::geometry::Size = flui_foundation::geometry::Size::new(400.0, 300.0);

/// Pointer contacts the root's `Listener` observed, by phase.
#[derive(Default)]
struct ContactLog {
    downs: Cell<usize>,
    moves: Cell<usize>,
    ups: Cell<usize>,
}

/// A UI runtime whose root is a `Listener` over the whole surface, logging its contacts, and the
/// agent's window onto it, held for the test: a window vended afresh asks for a frame of its own.
fn ui_runtime_at(device_pixel_ratio: f64) -> (UiRuntime, Rc<ContactLog>, AgentWindow) {
    let ui_runtime = UiRuntime::try_new(
        test_window(),
        device_pixel_ratio,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            &FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("a ui_runtime over a headless window");
    let log = Rc::new(ContactLog::default());
    let (down, moved, up) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget(
                &Listener::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_pointer_down(move |_, _| down.downs.set(down.downs.get() + 1))
                    .on_pointer_move(move |_, _| moved.moves.set(moved.moves.get() + 1))
                    .on_pointer_up(move |_, _| up.ups.set(up.ups.get() + 1))
                    .child(SizedBox::new(SIZE.width, SIZE.height)),
            )
        })
        .expect("attach succeeds");
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    (ui_runtime, log, window)
}

fn draw_at(ui_runtime: &UiRuntime, size: Size) -> Option<flui_layer::Scene> {
    ui_runtime.draw_frame(BoxConstraints::tight(size))
}

fn draw(ui_runtime: &UiRuntime) -> Option<flui_layer::Scene> {
    draw_at(ui_runtime, SIZE)
}

/// `state`, reported to the primary window through its agent and served at the owner's drain;
/// a refusal's code.
fn report(
    ui_runtime: &UiRuntime,
    window: &AgentWindow,
    state: BuildState,
) -> Result<(), ErrorCode> {
    let mut answer = window
        .call::<method::BuildReport>(BuildReportParams::new(window.id(), state))
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    answer
        .try_take()
        .expect("the report is answered at the drain")
        .map(|_| ())
        .map_err(|error| error.code)
}

fn failed(count: usize, omitted: u32) -> BuildState {
    BuildState::Failed {
        diagnostics: (0..count)
            .map(|index| {
                Diagnostic::new(
                    format!("error {index}"),
                    Some(SourceLocation::new("src/app.rs".to_owned(), 4, 9)),
                )
            })
            .collect(),
        omitted,
    }
}

/// [`report`], and whether serving it asked the primary presentation for a frame.
fn report_asking(
    ui_runtime: &UiRuntime,
    window: &AgentWindow,
    state: BuildState,
) -> (Result<(), ErrorCode>, bool) {
    let owes_frame = || {
        ui_runtime
            .take_redraw_pending_for_test(ui_runtime.presentation_id())
            .expect("the ui_runtime hosts its primary presentation")
    };
    // What an earlier frame left asked for is not the report's.
    let _ = owes_frame();
    let served = report(ui_runtime, window, state);
    let asked = owes_frame();
    if asked {
        // Asked again, for the frame the test draws next.
        ui_runtime.request_redraw();
    }
    (served, asked)
}

/// The nodes the scene's root slots, in paint order.
fn slotted(version: &SceneVersion) -> Vec<SceneNodeId> {
    version
        .node(version.root())
        .expect("a version holds its root")
        .content()
        .slots()
        .collect()
}

/// The version the primary presentation's scene holds now.
fn current(ui_runtime: &UiRuntime) -> SceneVersion {
    ui_runtime
        .presentations
        .primary()
        .scene
        .publication()
        .expect("no frame holds the scene")
        .scene
}

/// The banner's node: the last the root slots, after the frame node and any performance
/// overlay; `None` when the root slots no more than `before` nodes.
fn banner_node(version: &SceneVersion, before: usize) -> Option<SceneNodeId> {
    let slots = slotted(version);
    (slots.len() > before).then(|| slots[slots.len() - 1])
}

/// The banner's clip and its one chunk.
fn banner_parts(version: &SceneVersion, node: SceneNodeId) -> (Rect<f64>, &Chunk) {
    let content = version
        .node(node)
        .expect("the banner's node is in its version")
        .content();
    let clip = content
        .effect_layers()
        .find_map(|(_, layer)| match layer {
            Layer::ClipRect(clip) => Some(clip.clip_rect()),
            _ => None,
        })
        .expect("the banner is clipped to its bounds");
    let chunk = content
        .entries()
        .iter()
        .find_map(|entry| match entry {
            Entry::Chunk { chunk, .. } => Some(&**chunk),
            Entry::Effect { .. } | Entry::Slot { .. } => None,
        })
        .expect("the banner records one chunk");
    (clip, chunk)
}

/// The text of each paragraph `chunk` records, in order.
fn texts(chunk: &Chunk) -> Vec<String> {
    chunk
        .commands()
        .iter()
        .filter_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(paragraph.text().to_owned()),
            _ => None,
        })
        .collect()
}

/// A report shows a node after the frame node, published on a frame the pipeline paints nothing
/// in and leaving the frame node's content as it was; an equal report records nothing; a report
/// past the limit is refused `invalid_argument` and keeps what was shown; another replaces it at
/// the same node; `Succeeded` retires it. Fails if a report never reaches the scene, repaints
/// the pipeline, records again while it holds still, or a refused report changes it.
fn a_report_shows_a_node_until_it_succeeds() {
    let (ui_runtime, _log, window) = ui_runtime_at(1.0);
    let first = draw(&ui_runtime).expect("the first frame paints");
    assert_eq!(slotted(first.version()).len(), 1, "the frame node alone");
    let frame = slotted(first.version())[0];
    let frame_content = |version: &SceneVersion| {
        Arc::clone(version.node(frame).expect("the frame node stays").content())
    };

    let (served, asked) = report_asking(&ui_runtime, &window, failed(2, 3));
    served.expect("a report is served");
    assert!(asked, "a changed report asks for a frame");
    let shown = draw(&ui_runtime).expect("a changed report publishes its banner");
    assert_eq!(slotted(shown.version())[0], frame);
    let node = banner_node(shown.version(), 1).expect("the banner is slotted after the frame");
    let (clip, chunk) = banner_parts(shown.version(), node);
    assert_eq!(
        texts(chunk),
        [
            "Build failed: 2 errors and 3 more",
            "src/app.rs:4:9  error 0",
            "src/app.rs:4:9  error 1",
        ]
    );
    let surface = Rect::from_ltrb(0.0, 0.0, SIZE.width, SIZE.height);
    assert!(surface.contains_rect(&clip), "{clip:?} inside the surface");
    let inked = chunk.commands().bounds().expect("the banner inks");
    assert!(clip.contains_rect(&inked), "{inked:?} inside {clip:?}");
    assert!(
        Arc::ptr_eq(
            &frame_content(first.version()),
            &frame_content(shown.version())
        ),
        "the pipeline repaints nothing for the banner"
    );

    let (served, asked) = report_asking(&ui_runtime, &window, failed(2, 3));
    served.expect("an equal report is served");
    assert!(!asked, "an equal report asks for no frame");
    assert!(
        draw(&ui_runtime).is_none(),
        "an equal report records nothing"
    );

    let (served, asked) = report_asking(&ui_runtime, &window, failed(MAX_DIAGNOSTICS + 1, 0));
    let refused = served.expect_err("a report past the limit is refused");
    assert_eq!(refused, ErrorCode::InvalidArgument);
    assert!(!asked, "a refused report asks for no frame");
    assert!(
        draw(&ui_runtime).is_none(),
        "a refused report changes nothing"
    );
    let kept = current(&ui_runtime);
    assert_eq!(banner_node(&kept, 1), Some(node));
    assert_eq!(
        texts(banner_parts(&kept, node).1).len(),
        3,
        "the failure stays"
    );

    report(&ui_runtime, &window, BuildState::Building).expect("a report is served");
    let building = draw(&ui_runtime).expect("a new report replaces the old one");
    assert_eq!(
        banner_node(building.version(), 1),
        Some(node),
        "at the same node"
    );
    assert_eq!(
        texts(banner_parts(building.version(), node).1),
        [crate::build_overlay::BUILDING]
    );

    report(&ui_runtime, &window, BuildState::Succeeded).expect("a report is served");
    let succeeded = draw(&ui_runtime).expect("a success removes the banner");
    assert_eq!(
        slotted(succeeded.version()),
        [frame],
        "the frame node alone"
    );
    assert!(
        succeeded.version().node(node).is_none(),
        "the banner's node is retired"
    );
    assert!(
        draw(&ui_runtime).is_none(),
        "an idle frame publishes nothing"
    );
}

/// Beside the performance overlay the banner is slotted after it, under the device-pixel-ratio
/// scale, and kept clear of its bounds. Fails if the two overlap or the banner is not scaled.
fn the_banner_and_the_performance_overlay_show_apart() {
    let (ui_runtime, _log, window) = ui_runtime_at(2.0);
    ui_runtime.set_performance_overlay(true);
    let _first = draw(&ui_runtime).expect("the first frame paints");
    for state in [BuildState::Building, failed(MAX_DIAGNOSTICS, 0)] {
        report(&ui_runtime, &window, state).expect("a report is served");
        let scene = draw(&ui_runtime).expect("the banner is published");
        let version = scene.version();
        assert_eq!(slotted(version).len(), 3, "frame, readout, banner");
        let node = banner_node(version, 2).expect("the banner is slotted last");
        let layers: Vec<&Layer> = version
            .node(node)
            .expect("the banner's node is in its version")
            .content()
            .effect_layers()
            .map(|(_, layer)| layer)
            .collect();
        let [Layer::Transform(scale), Layer::ClipRect(clip)] = layers.as_slice() else {
            panic!("the banner is a scale over a clip: {layers:?}");
        };
        assert_eq!(
            *scale.transform(),
            flui_foundation::geometry::Matrix4::scaling(2.0, 2.0, 1.0)
        );
        assert!(
            !clip
                .clip_rect()
                .overlaps(&crate::performance_overlay::BOUNDS),
            "{:?} is clear of the readout",
            clip.clip_rect()
        );
    }
}

/// On a zero, tiny or small surface every clip and recording the banner publishes is finite and
/// inside the surface. Fails on a negative or non-finite rectangle.
fn zero_and_tiny_surfaces_publish_finite_geometry() {
    let (ui_runtime, _log, window) = ui_runtime_at(1.0);
    let _first = draw(&ui_runtime).expect("the first frame paints");
    let mut drawn = 0;
    for state in [BuildState::Building, failed(MAX_DIAGNOSTICS, 2)] {
        report(&ui_runtime, &window, state).expect("a report is served");
        for (width, height) in [
            (0.0, 0.0),
            (1.0, 1.0),
            (0.5, 300.0),
            (400.0, 0.5),
            (10.0, 10.0),
            (60.0, 30.0),
            (120.0, 50.0),
        ] {
            // A resize is a frame the host asks for.
            ui_runtime.request_redraw();
            let _drawn = draw_at(&ui_runtime, Size::new(width, height));
            let version = current(&ui_runtime);
            let Some(node) = banner_node(&version, 1) else {
                continue;
            };
            let (clip, chunk) = banner_parts(&version, node);
            assert!(
                clip.is_finite() && clip.left() >= 0.0 && clip.top() >= 0.0,
                "{width}x{height}: {clip:?}"
            );
            assert!(
                clip.right() <= width && clip.bottom() <= height,
                "{width}x{height}: {clip:?} inside the surface"
            );
            let inked = chunk.commands().bounds().expect("a banner inks");
            assert!(inked.is_finite(), "{width}x{height}: {inked:?}");
            drawn += 1;
        }
    }
    assert!(drawn >= 2, "the small surfaces draw each banner: {drawn}");
}

/// The primary's window, shown, running and focused, its first frame presented.
fn presented(ui_runtime: &UiRuntime) -> PresentationId {
    let id = ui_runtime.presentation_id();
    ui_runtime.synchronize_window_snapshot(
        id,
        flui_platform::WindowExecutionState::Running,
        true,
        true,
    );
    present(ui_runtime);
    id
}

fn present(ui_runtime: &UiRuntime) {
    ui_runtime.enter(|ui_runtime| ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
}

fn pointer(
    ui_runtime: &UiRuntime,
    id: PresentationId,
    event: flui_interaction::events::PointerEvent,
) {
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(id, PlatformInput::Pointer(event));
    });
}

/// A down, a move and an up at `at`.
fn tap(ui_runtime: &UiRuntime, id: PresentationId, at: Offset<f64>) {
    let kind = PointerKind::Touch;
    pointer(ui_runtime, id, make_down_event(at, kind).expect("finite"));
    pointer(ui_runtime, id, make_move_event(at, kind).expect("finite"));
    pointer(ui_runtime, id, make_up_event(at, kind).expect("finite"));
}

/// A tap inside the drawn chip or failure panel reaches the application's widget under it, its
/// down, move and up each, and the banner stays shown until the next report. Fails if the banner
/// takes any of a contact's events, or a tap on it dismisses it.
fn a_tap_on_the_banner_reaches_the_app_under_it() {
    let (ui_runtime, log, window) = ui_runtime_at(1.0);
    let id = presented(&ui_runtime);
    let mut taps = 0;
    for state in [failed(1, 0), BuildState::Building] {
        report(&ui_runtime, &window, state).expect("a report is served");
        present(&ui_runtime);
        let version = current(&ui_runtime);
        let node = banner_node(&version, 1).expect("the banner is drawn");
        let (bounds, _) = banner_parts(&version, node);
        let inside = Offset::new(
            f64::midpoint(bounds.left(), bounds.right()),
            f64::midpoint(bounds.top(), bounds.bottom()),
        );
        tap(&ui_runtime, id, inside);
        taps += 1;
        assert_eq!(
            (log.downs.get(), log.moves.get(), log.ups.get()),
            (taps, taps, taps),
            "the application sees the whole tap on the banner at {inside:?}"
        );
        present(&ui_runtime);
        let after = current(&ui_runtime);
        assert_eq!(
            banner_node(&after, 1),
            Some(node),
            "the banner stays shown after a tap on it"
        );
    }
}

#[test]
fn build_overlay_matrix() {
    crate::table_test::run_table(
        "build_overlay_matrix",
        &[
            (
                "a_report_shows_a_node_until_it_succeeds",
                a_report_shows_a_node_until_it_succeeds,
            ),
            (
                "the_banner_and_the_performance_overlay_show_apart",
                the_banner_and_the_performance_overlay_show_apart,
            ),
            (
                "zero_and_tiny_surfaces_publish_finite_geometry",
                zero_and_tiny_surfaces_publish_finite_geometry,
            ),
            (
                "a_tap_on_the_banner_reaches_the_app_under_it",
                a_tap_on_the_banner_reaches_the_app_under_it,
            ),
        ],
    );
}
