//! A face registered on the app's font collection reaches every realm's
//! layout at its next frame (ADR-0092 §2, §10 step 3b).
//!
//! The rows register on a collection fed from the process font system, as the
//! app's is, so each adds a family to `FONT_SYSTEM`. Every row names a family
//! no other test in this crate names, and the rows run in this one table.

use flui_painting::{DrawOp, FontCollection, shared_font_system};

use super::*;

/// "FLUI Probe Mono" Thin: every letter it maps is one em wide.
const PROBE_MONO: &[u8] =
    include_bytes!("../../../../flui-painting/assets/fonts/probe-mono-100.ttf");

/// "FLUI Probe Sans": every letter it maps is half an em wide.
const PROBE_SANS: &[u8] =
    include_bytes!("../../../../flui-painting/assets/fonts/probe-sans-400.ttf");

fn realm_over(fonts: &FontCollection) -> UiRealm {
    UiRealm::new(
        Arc::new(|| {}),
        test_window(),
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
        fonts,
        flui_scheduler::ClockSource::Platform,
    )
    .expect("a realm over a headless window")
}

fn frame(realm: &UiRealm) -> Option<flui_layer::Scene> {
    realm.draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
        400.0, 300.0,
    )))
}

/// Mounts `AAAA` in `family` at 20 px, centred so the paragraph takes its own
/// width, and draws the first frame.
fn show_probe_text(realm: &UiRealm, family: &str, weight: flui_painting::typography::FontWeight) {
    let style = flui_painting::typography::TextStyle {
        font_family: Some(family.to_owned()),
        font_weight: Some(weight),
        font_size: Some(20.0),
        ..flui_painting::typography::TextStyle::default()
    };
    realm
        .enter(|realm| {
            realm.attach_root_widget(
                &flui_widgets::Center::new().child(flui_widgets::Text::new("AAAA").style(style)),
            )
        })
        .expect("attach succeeds");
    let _ = frame(realm);
}

/// The width the realm's `RenderParagraph` was last laid out at.
fn paragraph_width(realm: &UiRealm) -> f64 {
    realm.pipeline_for_test().with(|owner| {
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

/// A face registered after two realms laid their text out re-lays it out in
/// both on their next frame, with nothing dirtied by hand, and the frame
/// paints it in the new face; the frame after that is idle. Fails if a
/// pipeline never learns of the change (the width stays the fallback's), or
/// if the registration reaches measurement but not paint.
fn a_face_registered_after_start_re_lays_out_text_in_every_realm_on_the_next_frame() {
    use flui_painting::typography::FontWeight;

    let fonts = FontCollection::with_host_faces(&shared_font_system());
    let a = realm_over(&fonts);
    let b = realm_over(&fonts);
    for realm in [&a, &b] {
        show_probe_text(realm, "FLUI Probe Mono", FontWeight::W100);
        let before = paragraph_width(realm);
        assert!(
            (before - 80.0).abs() > 1.0,
            "before the registration the text measures in a fallback face, got {before}"
        );
        assert!(
            frame(realm).is_none(),
            "the realm is idle before the registration"
        );
    }

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");

    for (name, realm) in [("A", &a), ("B", &b)] {
        let scene = frame(realm).expect("the registration makes the next frame paint");
        assert_near(
            paragraph_width(realm),
            80.0,
            &format!("{name}'s four one-em `A`s at 20 px"),
        );
        let painted = painted_widths(&scene);
        assert_eq!(painted.len(), 1, "{name} paints its one paragraph");
        assert_near(painted[0], 80.0, &format!("{name}'s painted paragraph"));
        assert!(
            frame(realm).is_none(),
            "{name}: a change already applied lays nothing out again"
        );
    }
}

/// A face registered before a realm exists measures on that realm's first
/// frame. Fails if the registration does not reach the collection the text
/// is laid out and painted with.
fn a_face_registered_before_the_realm_is_built_measures_on_its_first_frame() {
    use flui_painting::typography::FontWeight;

    let fonts = FontCollection::with_host_faces(&shared_font_system());
    fonts
        .register_font(PROBE_SANS)
        .expect("the probe face loads");
    let realm = realm_over(&fonts);
    show_probe_text(&realm, "FLUI Probe Sans", FontWeight::W400);
    assert_near(paragraph_width(&realm), 40.0, "four half-em `A`s at 20 px");
}

/// The realm's notice wakes every presentation it hosts, not only the
/// primary: each draws its next frame, where its pipeline applies the
/// change. Fails if the notice addresses one presentation, or none.
fn a_font_change_notice_requests_a_redraw_for_every_presentation() {
    let (wake, wakes) = counting_wake();
    let mut realm = new_runtime(wake).expect("realm constructs");
    let window_b: Arc<dyn PlatformWindow> = Arc::new(crate::testing::TestWindow::new().with_id(42));
    let presentation_b = realm.assemble_presentation(window_b);
    let _b = realm.install_presentation(presentation_b);
    assert_eq!(realm.presentations.len(), 2);
    for presentation in realm.presentations.iter() {
        let _ = presentation.take_redraw_pending();
    }
    let wakes_before = wakes.load(Ordering::Relaxed);

    realm.fonts_changed();

    for presentation in realm.presentations.iter() {
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
                "a_face_registered_after_start_re_lays_out_text_in_every_realm_on_the_next_frame",
                a_face_registered_after_start_re_lays_out_text_in_every_realm_on_the_next_frame
                    as fn(),
            ),
            (
                "a_face_registered_before_the_realm_is_built_measures_on_its_first_frame",
                a_face_registered_before_the_realm_is_built_measures_on_its_first_frame as fn(),
            ),
            (
                "a_font_change_notice_requests_a_redraw_for_every_presentation",
                a_font_change_notice_requests_a_redraw_for_every_presentation as fn(),
            ),
        ],
    );
}
