//! The decidable half of the web backend's text input (`text::web::ime`):
//! which virtual keyboard a field asks for, and what a field's element shows
//! of a store's text. Which keys belong to the input method is
//! `input::web::keys`'s; the mirror's own rules are `text::ime`'s; the DOM
//! side runs in Chrome (`tests/web_browser.rs`).

use super::{FieldValue, autocomplete, enter_key_hint, input_mode};
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

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// A single-line input shows the store's text without its line feeds and
/// carriage returns, a textarea with a line feed for each line break; each
/// shown offset is a store offset and back, an edit next to a dropped unit
/// keeping it.
#[test]
fn a_field_shows_the_store_s_text_as_the_browser_holds_it() {
    let store = units("a\nb\r\nc\rd");
    let line = FieldValue::of(&store, true);
    let area = FieldValue::of(&store, false);
    assert_eq!(line.shown(), units("abcd"));
    assert_eq!(area.shown(), units("a\nb\nc\nd"));

    let rows = [
        ((0, 0), (0, 0), "the caret at the start"),
        ((1, 1), (2, 2), "the caret past the dropped line feed"),
        ((0, 1), (0, 1), "the unit before a dropped one, without it"),
        ((1, 2), (2, 3), "the unit after a dropped one, without it"),
        ((0, 4), (0, 8), "everything"),
        ((2, 1), (2, 3), "ordered"),
        ((9, 9), (8, 8), "clamped to the end"),
    ];
    for ((start, end), store_range, why) in rows {
        assert_eq!(line.store_range(start, end), store_range, "{why}");
    }
    let back = [
        (0, 0),
        (1, 1),
        (2, 1),
        (3, 2),
        (5, 2),
        (6, 3),
        (7, 3),
        (8, 4),
    ];
    for (offset, shown) in back {
        assert_eq!(line.shown_offset(offset), shown, "store offset {offset}");
    }
    // The textarea drops only a CRLF's carriage return.
    assert_eq!(area.store_range(3, 4), (4, 5), "the CRLF's line feed");
    assert_eq!(area.shown_offset(4), 3);
}

/// An edit of the shown text is the edit of the store's units it covers,
/// after which the value shows the edited store.
#[test]
fn an_edit_of_the_shown_text_is_an_edit_of_the_store() {
    let rows = [
        (
            "a\nb",
            (1, 2, "x"),
            (2, 3),
            "a\nx",
            "replacing the unit after the line feed",
        ),
        (
            "a\nb",
            (0, 1, ""),
            (0, 1),
            "\nb",
            "deleting the unit before it",
        ),
        ("a\nb", (1, 1, "x"), (2, 2), "a\nxb", "inserting at it"),
        ("a\nb", (0, 2, ""), (0, 3), "", "deleting across it"),
        (
            "\nab",
            (0, 0, "x"),
            (1, 1),
            "\nxab",
            "inserting past a leading one",
        ),
        (
            "ab\n",
            (2, 2, "x"),
            (3, 3),
            "ab\nx",
            "inserting past a trailing one",
        ),
    ];
    for (text, (start, old_end, inserted), replaced, edited, why) in rows {
        let mut value = FieldValue::of(&units(text), true);
        assert_eq!(
            value.replace(start, old_end, &units(inserted)),
            replaced,
            "{why}"
        );
        assert_eq!(value, FieldValue::of(&units(edited), true), "{why}");
    }
}
