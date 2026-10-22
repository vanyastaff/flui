//! The decidable half of the web backend's text input (`text::web::ime`):
//! which keys belong to the input method, and which virtual keyboard a field
//! asks for. The mirror's own rules are `text::ime`'s; the DOM side runs in
//! Chrome (`tests/web_browser.rs`).

use super::{IME_PROCESS_KEY_CODE, input_mode, key_composition};
use flui_platform::keyboard::ImeComposition;
use flui_platform::text_store::TextInputPurpose;

const ESCAPE: u32 = 27;

/// Escape that cancels a composition is the input method's however the
/// browser reports it: Chromium marks it composing, WebKit sends it after
/// `compositionend` with only the input method's key code, and a surface
/// that still composes claims it whatever the event says.
fn a_key_of_the_input_method_is_marked_composing() {
    let rows = [
        (
            "chromium, mid-composition",
            true,
            ESCAPE,
            false,
            ImeComposition::Active,
        ),
        (
            "webkit, after compositionend",
            false,
            IME_PROCESS_KEY_CODE,
            false,
            ImeComposition::Active,
        ),
        (
            "the surface composes",
            false,
            ESCAPE,
            true,
            ImeComposition::Active,
        ),
        (
            "no composition",
            false,
            ESCAPE,
            false,
            ImeComposition::Inactive,
        ),
    ];
    for (case, is_composing, key_code, host, expected) in rows {
        assert_eq!(
            key_composition(is_composing, key_code, host),
            expected,
            "{case}"
        );
    }
}

/// The purpose a field was built with picks the virtual keyboard the browser
/// shows; free text and a purpose this backend has no keyboard for get plain
/// text.
fn the_purpose_picks_the_input_mode() {
    let rows = [
        (TextInputPurpose::Text, "text"),
        (TextInputPurpose::Email, "email"),
        (TextInputPurpose::Url, "url"),
        (TextInputPurpose::Phone, "tel"),
        (TextInputPurpose::Number, "numeric"),
        (TextInputPurpose::Search, "search"),
    ];
    for (purpose, expected) in rows {
        assert_eq!(input_mode(purpose), expected, "{purpose:?}");
    }
}

#[test]
fn web_text_table() {
    crate::table_test::run_table(
        "web_text_table",
        &[
            (
                "a_key_of_the_input_method_is_marked_composing",
                a_key_of_the_input_method_is_marked_composing,
            ),
            (
                "the_purpose_picks_the_input_mode",
                the_purpose_picks_the_input_mode,
            ),
        ],
    );
}
