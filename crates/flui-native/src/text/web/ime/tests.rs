//! The decidable half of the web backend's text input (`text::web::ime`):
//! which virtual keyboard a field asks for. Which keys belong to the input
//! method is `input::web::keys`'s; the mirror's own rules are `text::ime`'s;
//! the DOM side runs in Chrome (`tests/web_browser.rs`).

use super::input_mode;
use flui_platform::text_store::TextInputPurpose;

/// The purpose a field was built with picks the virtual keyboard the browser
/// shows; free text and a purpose this backend has no keyboard for get plain
/// text.
#[test]
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
