use flui_painting::FontCollection;
use flui_painting::testing::font_collection_holders;

use super::*;

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

/// Each UI runtime owns one `TextContext`, and both are built over the collection
/// the caller handed in, not over one the UI runtime made for itself. Fails if a
/// UI runtime builds its own collection (`ptr_eq`), or shares or skips a context
/// (the holder count: the caller's handle plus one per UI runtime).
fn two_ui_runtimes_hold_contexts_over_the_one_collection_they_were_given() {
    let fonts = FontCollection::new();
    let a = ui_runtime_over(&fonts);
    let b = ui_runtime_over(&fonts);

    for ui_runtime in [&a, &b] {
        assert!(
            ui_runtime
                .text_context_for_test()
                .with(|text| FontCollection::ptr_eq(text.fonts(), &fonts)),
            "a ui_runtime's text context must be built over the app's collection"
        );
    }
    assert_eq!(
        font_collection_holders(&fonts),
        3,
        "the caller's handle plus exactly one text context per ui_runtime"
    );
}

/// The context lives exactly as long as its UI runtime: dropping a UI runtime releases
/// its hold on the collection. Fails if a UI runtime leaks the context (into an
/// `Rc` cycle, a static, or anything else that outlives the UI runtime).
fn dropping_a_ui_runtime_releases_its_text_context() {
    let fonts = FontCollection::new();
    assert_eq!(font_collection_holders(&fonts), 1);
    let a = ui_runtime_over(&fonts);
    assert_eq!(font_collection_holders(&fonts), 2);
    let b = ui_runtime_over(&fonts);
    assert_eq!(font_collection_holders(&fonts), 3);

    drop(a);
    assert_eq!(font_collection_holders(&fonts), 2);
    drop(b);
    assert_eq!(font_collection_holders(&fonts), 1);
}

/// The context is per UI runtime, not per presentation: a second presentation
/// shares the UI runtime's context rather than building another.
fn a_second_presentation_adds_no_text_context() {
    let fonts = FontCollection::new();
    let mut ui_runtime = ui_runtime_over(&fonts);
    assert_eq!(font_collection_holders(&fonts), 2);

    let window_b: Arc<dyn PlatformWindow> = Arc::new(crate::testing::TestWindow::new().with_id(42));
    let presentation_b = ui_runtime.assemble_presentation(window_b);
    let _b = ui_runtime.install_presentation(presentation_b);

    assert_eq!(ui_runtime.presentations.len(), 2);
    assert_eq!(
        font_collection_holders(&fonts),
        2,
        "a presentation must not build a text context of its own"
    );
}

/// Mounts `text` as the UI runtime's root and draws one frame.
fn show_text(ui_runtime: &UiRuntime, text: &str) {
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&flui_widgets::Text::new(text)))
        .expect("attach succeeds");
    let _ = ui_runtime.draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
        400.0, 300.0,
    )));
}

/// How many measurements the UI runtime's own text context was lent for.
fn lends(ui_runtime: &UiRuntime) -> u64 {
    ui_runtime
        .text_context_for_test()
        .with(|text| flui_painting::testing::text_context_lends(text))
}

/// Marks the UI runtime's paragraph for layout, as a text change would.
fn dirty_the_paragraph(ui_runtime: &UiRuntime) {
    ui_runtime.pipeline_for_test().with_mut(|owner| {
        let paragraph = owner
            .render_tree()
            .iter()
            .find(|(_, node)| node.debug_name().contains("RenderParagraph"))
            .map(|(id, _)| id)
            .expect("the Text mounted a RenderParagraph");
        owner.mark_needs_layout(paragraph);
    });
}

/// Each UI runtime's layout measures its text through that UI runtime's own context:
/// the counts rise in both after a frame each, and a frame on A alone moves
/// only A's. Fails if a pipeline measures on a private context (neither
/// count rises) or on another UI runtime's (B's rises with A's frame).
fn two_ui_runtimes_measure_text_through_their_own_contexts() {
    let a = ui_runtime_over(&FontCollection::new());
    let b = ui_runtime_over(&FontCollection::new());
    assert_eq!((lends(&a), lends(&b)), (0, 0));

    show_text(&a, "ui_runtime a");
    show_text(&b, "ui_runtime b");
    let (after_a, after_b) = (lends(&a), lends(&b));
    assert!(after_a > 0, "A's layout measured through A's context");
    assert!(after_b > 0, "B's layout measured through B's context");

    dirty_the_paragraph(&a);
    let _ = a.draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
        400.0, 300.0,
    )));
    assert!(
        lends(&a) > after_a,
        "A's relayout measured through A's context"
    );
    assert_eq!(lends(&b), after_b, "a frame on A lends nothing of B's");
}

/// Every presentation's pipeline lends the UI runtime's one context: the first,
/// built with the UI runtime, and one assembled and installed later. Fails if a
/// presentation's pipeline is built with any context but the UI runtime's.
fn every_presentation_pipeline_holds_the_ui_runtimes_text_context() {
    let fonts = FontCollection::new();
    let mut ui_runtime = ui_runtime_over(&fonts);
    let window_b: Arc<dyn PlatformWindow> = Arc::new(crate::testing::TestWindow::new().with_id(42));
    let presentation_b = ui_runtime.assemble_presentation(window_b);
    let _b = ui_runtime.install_presentation(presentation_b);

    assert_eq!(ui_runtime.presentations.len(), 2);
    let ui_runtime_text = ui_runtime.text_context_for_test().clone();
    for presentation in ui_runtime.presentations.iter() {
        let lent = presentation
            .pipeline()
            .with(|owner| owner.text_context_for_test().clone());
        assert!(
            flui_rendering::TextContextHandle::ptr_eq(&lent, &ui_runtime_text),
            "a presentation's pipeline lends the ui_runtime's context, not one of its own"
        );
    }
}

/// A face only the probe family carries; it maps `A` one em wide.
const PROBE_MONO: &[u8] = flui_painting::testing::PROBE_MONO_100;

/// The width the UI runtime's `RenderParagraph` was laid out at, for `AAAA` in the
/// probe family at 20 px, centred so the paragraph takes its own width.
fn probe_paragraph_width(ui_runtime: &UiRuntime) -> f64 {
    let style = flui_painting::typography::TextStyle {
        font_family: Some("FLUI Probe Mono".to_owned()),
        font_weight: Some(flui_painting::typography::FontWeight::W100),
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
    let _ = ui_runtime.draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
        400.0, 300.0,
    )));
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

/// A UI runtime measures text in the faces of its own collection (ADR-0092 §10
/// step 4a): the probe face, registered only on A's collection, sizes A's
/// paragraph at four em, while B's falls back. Fails if layout measures on any
/// other collection, which never sees the face.
fn a_ui_runtime_measures_text_with_the_faces_of_its_own_collection() {
    let with_probe = FontCollection::new();
    with_probe
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    let a = ui_runtime_over(&with_probe);
    let b = ui_runtime_over(&FontCollection::new());

    let through_a = probe_paragraph_width(&a);
    let through_b = probe_paragraph_width(&b);
    assert!(
        (through_a - 80.0).abs() < 0.01,
        "four one-em `A`s at 20 px through A's collection are 80 px, got {through_a}"
    );
    assert!(
        (through_b - through_a).abs() > 1.0,
        "B's collection lacks the face, so B measures the fallback: A {through_a} vs B {through_b}"
    );
}

/// The blob ids of every run of every paragraph in `list`.
fn paragraph_blob_ids(list: &flui_painting::DisplayList) -> std::collections::BTreeSet<u64> {
    list.iter()
        .filter_map(|command| match &command.op {
            flui_painting::DrawOp::Paragraph { paragraph, .. } => Some(paragraph.clone()),
            _ => None,
        })
        .flat_map(|paragraph| {
            paragraph
                .runs()
                .map(|run| run.face().blob().id())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The performance overlay's labels are shaped at scene assembly through the
/// UI runtime's own text context (ADR-0092): the readout's runs name only faces
/// of the UI runtime's collection. Fails if the overlay is shaped over a
/// collection of its own, or carries no shaped labels for the engine to
/// shape instead.
fn the_overlay_shapes_through_the_ui_runtime_text_context() {
    let ui_runtime = ui_runtime_over(&FontCollection::new());
    ui_runtime.set_performance_overlay(true);
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&flui_widgets::Text::new("A")))
        .expect("attach succeeds");
    let scene = ui_runtime
        .draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
            400.0, 300.0,
        )))
        .expect("the first frame paints");
    let overlay = scene
        .tree()
        .iter()
        .find_map(|(_, node)| node.layer().as_performance_overlay().cloned())
        .expect("the overlay is attached");
    let overlay_ids = paragraph_blob_ids(overlay.readout());
    assert!(!overlay_ids.is_empty(), "the readout carries shaped labels");

    let ui_runtime_ids = ui_runtime.text_context_for_test().with(|text| {
        let spans = [("GPU FPS Frame ms 0123456789.=_ ".to_owned(), None)];
        let shaped = text
            .shape(&flui_painting::parley_text::ParagraphSpec {
                font_weight_adjustment: 0,
                spans: &spans,
                default_style: None,
                font_size: 11.0,
                max_width: None,
                min_width: 0.0,
                text_align: flui_painting::typography::TextAlign::Start,
                line_height: None,
                direction: flui_painting::typography::TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            })
            .expect("the overlay reference uses valid text layout inputs")
            .to_shaped(None);
        shaped
            .runs()
            .map(|run| run.face().blob().id())
            .collect::<std::collections::BTreeSet<_>>()
    });
    assert!(
        overlay_ids.is_subset(&ui_runtime_ids),
        "the overlay names blobs {overlay_ids:?}, the ui_runtime's collection {ui_runtime_ids:?}"
    );
}

#[test]
fn text_context_matrix() {
    crate::table_test::run_table(
        "text_context_matrix",
        &[
            (
                "two_ui_runtimes_hold_contexts_over_the_one_collection_they_were_given",
                two_ui_runtimes_hold_contexts_over_the_one_collection_they_were_given as fn(),
            ),
            (
                "dropping_a_ui_runtime_releases_its_text_context",
                dropping_a_ui_runtime_releases_its_text_context as fn(),
            ),
            (
                "a_second_presentation_adds_no_text_context",
                a_second_presentation_adds_no_text_context as fn(),
            ),
            (
                "two_ui_runtimes_measure_text_through_their_own_contexts",
                two_ui_runtimes_measure_text_through_their_own_contexts as fn(),
            ),
            (
                "every_presentation_pipeline_holds_the_ui_runtimes_text_context",
                every_presentation_pipeline_holds_the_ui_runtimes_text_context as fn(),
            ),
            (
                "a_ui_runtime_measures_text_with_the_faces_of_its_own_collection",
                a_ui_runtime_measures_text_with_the_faces_of_its_own_collection as fn(),
            ),
            (
                "the_overlay_shapes_through_the_ui_runtime_text_context",
                the_overlay_shapes_through_the_ui_runtime_text_context as fn(),
            ),
        ],
    );
}
