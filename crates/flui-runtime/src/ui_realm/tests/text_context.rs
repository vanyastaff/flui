use flui_painting::FontCollection;
use flui_painting::testing::font_collection_holders;

use super::*;

fn realm_over(fonts: &FontCollection) -> UiRealm {
    UiRealm::new(
        Arc::new(|| {}),
        test_window(),
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
        fonts,
    )
    .expect("a realm over a headless window")
}

/// Each realm owns one `TextContext`, and both are built over the collection
/// the caller handed in, not over one the realm made for itself. Fails if a
/// realm builds its own collection (`ptr_eq`), or shares or skips a context
/// (the holder count: the caller's handle plus one per realm).
fn two_realms_hold_contexts_over_the_one_collection_they_were_given() {
    let fonts = FontCollection::new();
    let a = realm_over(&fonts);
    let b = realm_over(&fonts);

    for realm in [&a, &b] {
        assert!(
            realm
                .text_context_for_test()
                .with(|text| FontCollection::ptr_eq(text.fonts(), &fonts)),
            "a realm's text context must be built over the app's collection"
        );
    }
    assert_eq!(
        font_collection_holders(&fonts),
        3,
        "the caller's handle plus exactly one text context per realm"
    );
}

/// The context lives exactly as long as its realm: dropping a realm releases
/// its hold on the collection. Fails if a realm leaks the context (into an
/// `Rc` cycle, a static, or anything else that outlives the realm).
fn dropping_a_realm_releases_its_text_context() {
    let fonts = FontCollection::new();
    assert_eq!(font_collection_holders(&fonts), 1);
    let a = realm_over(&fonts);
    assert_eq!(font_collection_holders(&fonts), 2);
    let b = realm_over(&fonts);
    assert_eq!(font_collection_holders(&fonts), 3);

    drop(a);
    assert_eq!(font_collection_holders(&fonts), 2);
    drop(b);
    assert_eq!(font_collection_holders(&fonts), 1);
}

/// The context is per realm, not per presentation: a second presentation
/// shares the realm's context rather than building another.
fn a_second_presentation_adds_no_text_context() {
    let fonts = FontCollection::new();
    let mut realm = realm_over(&fonts);
    assert_eq!(font_collection_holders(&fonts), 2);

    let window_b: Arc<dyn PlatformWindow> = Arc::new(crate::testing::TestWindow::new().with_id(42));
    let presentation_b = realm.assemble_presentation(window_b);
    let _b = realm.install_presentation(presentation_b);

    assert_eq!(realm.presentations.len(), 2);
    assert_eq!(
        font_collection_holders(&fonts),
        2,
        "a presentation must not build a text context of its own"
    );
}

/// Mounts `text` as the realm's root and draws one frame.
fn show_text(realm: &UiRealm, text: &str) {
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::Text::new(text)))
        .expect("attach succeeds");
    let _ = realm.draw_frame(BoxConstraints::tight(flui_foundation::geometry::Size::new(
        400.0, 300.0,
    )));
}

/// How many measurements the realm's own text context was lent for.
fn lends(realm: &UiRealm) -> u64 {
    realm
        .text_context_for_test()
        .with(|text| flui_painting::testing::text_context_lends(text))
}

/// Marks the realm's paragraph for layout, as a text change would.
fn dirty_the_paragraph(realm: &UiRealm) {
    realm.pipeline_for_test().with_mut(|owner| {
        let paragraph = owner
            .render_tree()
            .iter()
            .find(|(_, node)| node.debug_name().contains("RenderParagraph"))
            .map(|(id, _)| id)
            .expect("the Text mounted a RenderParagraph");
        owner.mark_needs_layout(paragraph);
    });
}

/// Each realm's layout measures its text through that realm's own context:
/// the counts rise in both after a frame each, and a frame on A alone moves
/// only A's. Fails if a pipeline measures on a private context (neither
/// count rises) or on another realm's (B's rises with A's frame).
fn two_realms_measure_text_through_their_own_contexts() {
    let a = realm_over(&FontCollection::new());
    let b = realm_over(&FontCollection::new());
    assert_eq!((lends(&a), lends(&b)), (0, 0));

    show_text(&a, "realm a");
    show_text(&b, "realm b");
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

/// Every presentation's pipeline lends the realm's one context: the first,
/// built with the realm, and one assembled and installed later. Fails if a
/// presentation's pipeline is left to measure on a context of its own.
fn every_presentation_pipeline_holds_the_realms_text_context() {
    let fonts = FontCollection::new();
    let mut realm = realm_over(&fonts);
    let window_b: Arc<dyn PlatformWindow> = Arc::new(crate::testing::TestWindow::new().with_id(42));
    let presentation_b = realm.assemble_presentation(window_b);
    let _b = realm.install_presentation(presentation_b);

    assert_eq!(realm.presentations.len(), 2);
    let realm_text = realm.text_context_for_test().clone();
    for presentation in realm.presentations.iter() {
        let lent = presentation
            .pipeline()
            .with(|owner| owner.text_context_for_test().cloned())
            .expect("the presentation's pipeline was given the realm's text context");
        assert!(
            flui_rendering::TextContextHandle::ptr_eq(&lent, &realm_text),
            "a presentation's pipeline lends the realm's context, not one of its own"
        );
    }
}

#[test]
fn text_context_matrix() {
    crate::table_test::run_table(
        "text_context_matrix",
        &[
            (
                "two_realms_hold_contexts_over_the_one_collection_they_were_given",
                two_realms_hold_contexts_over_the_one_collection_they_were_given as fn(),
            ),
            (
                "dropping_a_realm_releases_its_text_context",
                dropping_a_realm_releases_its_text_context as fn(),
            ),
            (
                "a_second_presentation_adds_no_text_context",
                a_second_presentation_adds_no_text_context as fn(),
            ),
            (
                "two_realms_measure_text_through_their_own_contexts",
                two_realms_measure_text_through_their_own_contexts as fn(),
            ),
            (
                "every_presentation_pipeline_holds_the_realms_text_context",
                every_presentation_pipeline_holds_the_realms_text_context as fn(),
            ),
        ],
    );
}
