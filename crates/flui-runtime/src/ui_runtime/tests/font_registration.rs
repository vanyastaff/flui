//! A face registered on the app's font collection, or fed into it from the
//! host, reaches every UI runtime's layout at its next frame (ADR-0092 §2, §7).
//!
//! The registration rows register on a collection fed from a scan of the
//! host's fonts; the feed rows build the collection as the app does and feed
//! it from a host of their own. Every row names a family no other test in
//! this crate names, and the rows run in this one table.

use flui_painting::{DrawOp, FontCollection, HostFonts};

use super::*;

/// "FLUI Probe Mono" Thin: every letter it maps is one em wide.
const PROBE_MONO: &[u8] =
    include_bytes!("../../../../flui-painting/assets/fonts/probe-mono-100.ttf");

/// "FLUI Probe Sans": every letter it maps is half an em wide.
const PROBE_SANS: &[u8] =
    include_bytes!("../../../../flui-painting/assets/fonts/probe-sans-400.ttf");

fn ui_runtime_over(fonts: &FontCollection) -> UiRuntime {
    UiRuntime::new(
        test_window(),
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            fonts,
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("a ui_runtime over a headless window")
}

fn frame(ui_runtime: &UiRuntime) -> Option<flui_layer::Scene> {
    ui_runtime.draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
        400.0, 300.0,
    )))
}

/// Mounts `AAAA` in `family` at 20 px, centred so the paragraph takes its own
/// width, and draws the first frame.
fn show_probe_text(
    ui_runtime: &UiRuntime,
    family: &str,
    weight: flui_painting::typography::FontWeight,
) {
    let style = flui_painting::typography::TextStyle {
        font_family: Some(family.to_owned()),
        font_weight: Some(weight),
        font_size: Some(20.0),
        ..flui_painting::typography::TextStyle::default()
    };
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget(
                &flui_widgets::Center::new().child(flui_widgets::Text::new("AAAA").style(style)),
            )
        })
        .expect("attach succeeds");
    let _ = frame(ui_runtime);
}

/// The width the UI runtime's `RenderParagraph` was last laid out at.
fn paragraph_width(ui_runtime: &UiRuntime) -> f64 {
    ui_runtime.pipeline_for_test().with(|owner| {
        let paragraph = owner
            .render_tree()
            .iter()
            .find(|(_, node)| node.debug_name().contains("RenderParagraph"))
            .map(|(id, _)| id)
            .expect("the Text mounted a RenderParagraph");
        owner
            .box_size(paragraph)
            .expect("the paragraph was laid out")
            .width
    })
}

/// The width of every paragraph `scene` paints.
fn painted_widths(scene: &flui_layer::Scene) -> Vec<f64> {
    scene
        .tree()
        .iter()
        .filter_map(|(_, node)| match node.layer() {
            flui_layer::Layer::Picture(picture) => Some(picture),
            _ => None,
        })
        .flat_map(|picture| {
            picture
                .picture()
                .iter()
                .filter_map(|command| match &command.op {
                    DrawOp::Paragraph { paragraph, .. } => Some(paragraph.size().width),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.01,
        "{what}: expected {expected}, got {actual}"
    );
}

/// A face registered after two UI runtimes laid their text out re-lays it out in
/// both on their next frame, with nothing dirtied by hand, and the frame
/// paints it in the new face; the frame after that is idle. Fails if a
/// pipeline never learns of the change (the width stays the fallback's), or
/// if the registration reaches measurement but not paint.
fn a_face_registered_after_start_re_lays_out_text_in_every_ui_runtime_on_the_next_frame() {
    use flui_painting::typography::FontWeight;

    let fonts = FontCollection::with_host_fonts(&HostFonts::scan());
    let a = ui_runtime_over(&fonts);
    let b = ui_runtime_over(&fonts);
    for ui_runtime in [&a, &b] {
        show_probe_text(ui_runtime, "FLUI Probe Mono", FontWeight::W100);
        let before = paragraph_width(ui_runtime);
        assert!(
            (before - 80.0).abs() > 1.0,
            "before the registration the text measures in a fallback face, got {before}"
        );
        assert!(
            frame(ui_runtime).is_none(),
            "the ui_runtime is idle before the registration"
        );
    }

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");

    for (name, ui_runtime) in [("A", &a), ("B", &b)] {
        let scene = frame(ui_runtime).expect("the registration makes the next frame paint");
        assert_near(
            paragraph_width(ui_runtime),
            80.0,
            &format!("{name}'s four one-em `A`s at 20 px"),
        );
        let painted = painted_widths(&scene);
        assert_eq!(painted.len(), 1, "{name} paints its one paragraph");
        assert_near(painted[0], 80.0, &format!("{name}'s painted paragraph"));
        assert!(
            frame(ui_runtime).is_none(),
            "{name}: a change already applied lays nothing out again"
        );
    }
}

/// A face registered before a UI runtime exists measures on that UI runtime's first
/// frame. Fails if the registration does not reach the collection the text
/// is laid out and painted with.
fn a_face_registered_before_the_ui_runtime_is_built_measures_on_its_first_frame() {
    use flui_painting::typography::FontWeight;

    let fonts = FontCollection::with_host_fonts(&HostFonts::scan());
    fonts
        .register_font(PROBE_SANS)
        .expect("the probe face loads");
    let ui_runtime = ui_runtime_over(&fonts);
    show_probe_text(&ui_runtime, "FLUI Probe Sans", FontWeight::W400);
    assert_near(
        paragraph_width(&ui_runtime),
        40.0,
        "four half-em `A`s at 20 px",
    );
}

/// The first frame renders before the host's fonts are scanned: on the
/// collection the app builds, whose host feed has not run, a UI runtime draws its
/// first frame, and text in the bundled Roboto measures as on a bundled-only
/// collection. Fails if the collection is fed from the host before it is
/// handed out, which makes the first frame wait for the scan and the feed.
fn the_first_frame_renders_bundled_text_before_the_host_feed_lands() {
    let (fonts, feed) = FontCollection::with_host_feed();
    let ui_runtime = ui_runtime_over(&fonts);
    let bundled = ui_runtime_over(&FontCollection::new());
    for ui_runtime in [&ui_runtime, &bundled] {
        show_probe_text(
            ui_runtime,
            "Roboto",
            flui_painting::typography::FontWeight::W400,
        );
    }

    assert!(
        !flui_painting::testing::host_fed(&fonts),
        "no host face has been read"
    );
    assert_eq!(fonts.generation(), 0);
    let width = paragraph_width(&ui_runtime);
    assert!(
        width > 1.0,
        "the first frame laid the text out, got {width}"
    );
    assert_near(width, paragraph_width(&bundled), "the bundled Roboto");
    drop(feed);
}

/// Text styled with a family only the host carries measures in a fallback
/// face until the host feed lands, and is laid out again in that family at
/// the next frame after it lands, painted too, with nothing dirtied by hand;
/// the frame after that is idle. The feed runs on another thread, as the
/// app runs it. Fails if the feed's faces reach the collection without
/// raising its generation (the text keeps its fallback width), or never
/// reach it.
fn text_in_a_host_only_family_re_lays_out_when_the_feed_lands() {
    use flui_painting::testing::{feed_with_host, host_fonts_from};
    use flui_painting::typography::FontWeight;

    let (fonts, feed) = FontCollection::with_host_feed();
    let feed = feed_with_host(feed, host_fonts_from(&[PROBE_MONO]));
    let ui_runtime = ui_runtime_over(&fonts);
    show_probe_text(&ui_runtime, "FLUI Probe Mono", FontWeight::W100);
    let before = paragraph_width(&ui_runtime);
    assert!(
        (before - 80.0).abs() > 1.0,
        "before the feed the text measures in a fallback face, got {before}"
    );
    assert!(
        frame(&ui_runtime).is_none(),
        "the ui_runtime is idle before the feed"
    );

    std::thread::spawn(move || feed.run())
        .join()
        .expect("the feed lands");

    let scene = frame(&ui_runtime).expect("the landed feed makes the next frame paint");
    assert_near(
        paragraph_width(&ui_runtime),
        80.0,
        "four one-em `A`s at 20 px",
    );
    let painted = painted_widths(&scene);
    assert_eq!(painted.len(), 1, "the ui_runtime paints its one paragraph");
    assert_near(painted[0], 80.0, "the painted paragraph");
    assert!(
        frame(&ui_runtime).is_none(),
        "a feed already applied lays nothing out again"
    );
}

/// The UI runtime's notice wakes every presentation it hosts, not only the
/// primary: each draws its next frame, where its pipeline applies the
/// change. Fails if the notice addresses one presentation, or none.
fn a_font_change_notice_requests_a_redraw_for_every_presentation() {
    let (wake, wakes) = counting_wake();
    let mut ui_runtime = new_runtime(wake).expect("ui_runtime constructs");
    let window_b: Arc<dyn PlatformWindow> = Arc::new(crate::testing::TestWindow::new().with_id(42));
    let presentation_b = ui_runtime.assemble_presentation(window_b);
    let _b = ui_runtime.install_presentation(presentation_b);
    assert_eq!(ui_runtime.presentations.len(), 2);
    for presentation in ui_runtime.presentations.iter() {
        let _ = presentation.take_redraw_pending();
    }
    let wakes_before = wakes.load(Ordering::Relaxed);

    ui_runtime.fonts_changed();

    for presentation in ui_runtime.presentations.iter() {
        assert!(
            presentation.take_redraw_pending(),
            "every presentation draws its next frame"
        );
    }
    assert!(
        wakes.load(Ordering::Relaxed) > wakes_before,
        "the notice wakes the owner"
    );
}

#[test]
fn font_registration_matrix() {
    crate::table_test::run_table(
        "font_registration_matrix",
        &[
            (
                "a_face_registered_after_start_re_lays_out_text_in_every_ui_runtime_on_the_next_frame",
                a_face_registered_after_start_re_lays_out_text_in_every_ui_runtime_on_the_next_frame
                    as fn(),
            ),
            (
                "a_face_registered_before_the_ui_runtime_is_built_measures_on_its_first_frame",
                a_face_registered_before_the_ui_runtime_is_built_measures_on_its_first_frame
                    as fn(),
            ),
            (
                "the_first_frame_renders_bundled_text_before_the_host_feed_lands",
                the_first_frame_renders_bundled_text_before_the_host_feed_lands as fn(),
            ),
            (
                "text_in_a_host_only_family_re_lays_out_when_the_feed_lands",
                text_in_a_host_only_family_re_lays_out_when_the_feed_lands as fn(),
            ),
            (
                "a_font_change_notice_requests_a_redraw_for_every_presentation",
                a_font_change_notice_requests_a_redraw_for_every_presentation as fn(),
            ),
        ],
    );
}
