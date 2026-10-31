//! The decidable half of the web backend's text input (`text::web::ime`):
//! which virtual keyboard a field asks for. Which keys belong to the input
//! method is `input::web::keys`'s; the mirror's own rules are `text::ime`'s;
//! the DOM side runs in Chrome (`tests/web_browser.rs`).

use super::{autocomplete, enter_key_hint, input_mode};
use flui_platform::text_store::{TextInputPurpose, TextStoreStatus};

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

/// The hidden editor offers the browser nothing to fill, and a protected one
/// is no login field; the semantics host's field is filled as a native field
/// of its purpose is.
#[test]
fn the_status_picks_what_the_browser_fills_in() {
    let status = |purpose, protected| {
        TextStoreStatus::EDITABLE_SINGLE_LINE
            .with_purpose(purpose)
            .with_protected(protected)
    };
    let rows = [
        (TextInputPurpose::Text, false, "off", "off"),
        (TextInputPurpose::Email, false, "off", "email"),
        (TextInputPurpose::Url, false, "off", "url"),
        (TextInputPurpose::Phone, false, "off", "tel"),
        (TextInputPurpose::Number, false, "off", "off"),
        (TextInputPurpose::Search, false, "off", "off"),
        (
            TextInputPurpose::Text,
            true,
            "new-password",
            "current-password",
        ),
    ];
    for (purpose, protected, hidden, native) in rows {
        let status = status(purpose, protected);
        assert_eq!(autocomplete(&status, false), hidden, "{purpose:?} hidden");
        assert_eq!(autocomplete(&status, true), native, "{purpose:?} native");
    }
}

/// A query's Enter key searches and a field of many lines' breaks the line;
/// any other single-line field keeps the browser's own label.
#[test]
fn the_status_labels_the_enter_key() {
    let single = TextStoreStatus::EDITABLE_SINGLE_LINE;
    assert_eq!(enter_key_hint(&single), None);
    assert_eq!(
        enter_key_hint(&single.with_purpose(TextInputPurpose::Search)),
        Some("search")
    );
    let mut multi = single;
    multi.single_line = false;
    assert_eq!(enter_key_hint(&multi), Some("enter"));
}
