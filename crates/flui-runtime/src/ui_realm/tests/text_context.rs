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
            FontCollection::ptr_eq(realm.text_context_for_test().fonts(), &fonts),
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
        ],
    );
}
