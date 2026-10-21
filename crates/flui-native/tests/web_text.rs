//! The decidable half of the web backend's text input (`shared::web_text`):
//! which keys belong to the input method, and the one edit between two
//! versions of a hidden textarea's text. The DOM side runs in Chrome
//! (`tests/web_browser.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use flui_native::shared::ime_text::{PROTECTED_UNIT, document_text, scalar_spans};
use flui_native::shared::web_text::{
    IME_PROCESS_KEY_CODE, TextDiff, diff, input_mode, key_composition,
};
use flui_platform::keyboard::ImeComposition;
use flui_platform::text_store::{
    InMemoryTextStore, LockGrant, LockTiming, TextInputPurpose, TextStore,
};

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

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn a_textarea_change_is_one_edit() {
    let edit = |old: &str, new: &str| diff(&units(old), &units(new));
    let at = |start, old_end, new_end| {
        Some(TextDiff {
            start,
            old_end,
            new_end,
        })
    };
    assert_eq!(edit("hello", "hello"), None);
    assert_eq!(edit("hello", "hello world"), at(5, 5, 11), "appended");
    assert_eq!(edit("hello world", "hello"), at(5, 11, 5), "deleted");
    assert_eq!(edit("abc", "aXc"), at(1, 2, 2), "replaced");
    assert_eq!(edit("aaa", "aaaa"), at(3, 3, 4), "the prefix wins a tie");
    assert_eq!(edit("", "に"), at(0, 0, 1), "a composition's first text");
    // 😀 and 😁 share their high surrogate: the edit takes the pair.
    assert_eq!(edit("a😀b", "a😁b"), at(1, 3, 3), "a pair is never split");
    // U+1F600 and U+10600 share their low surrogate (DE00): the edit takes
    // the pair from the other side too.
    assert_eq!(
        edit("x\u{1F600}", "x\u{10600}"),
        at(1, 3, 3),
        "nor from its end"
    );
}

/// A protected field reads as bullets of the field's length, so the browser's
/// surface starts with the offsets the field has instead of empty.
fn a_protected_field_is_mirrored_as_bullets() {
    let store = InMemoryTextStore::new("secret");
    store.set_protected(true);
    let read: Rc<RefCell<String>> = Rc::default();
    let out = Rc::clone(&read);
    store
        .request_lock(
            LockGrant::read(move |session| *out.borrow_mut() = document_text(session)),
            LockTiming::Sync,
        )
        .map(drop)
        .expect("a free store grants a read");
    assert_eq!(*read.borrow(), PROTECTED_UNIT.to_string().repeat(6));
}

/// A character-bounds request names code units; each lands on the scalar
/// that holds it, and a surrogate pair is one scalar however it is cut.
fn character_bounds_follow_scalars() {
    let span = |start, end| (start, end);
    let spans = |text: &str, start, end| {
        scalar_spans(text, start, end)
            .into_iter()
            .map(|range| (range.start().get(), range.end().get()))
            .collect::<Vec<_>>()
    };
    // "😀か" is units [D83D, DE00, 304B].
    assert_eq!(
        spans("😀か", 0, 3),
        [span(0, 2), span(2, 3)],
        "both scalars"
    );
    assert_eq!(spans("😀か", 0, 1), [span(0, 2)], "half a pair is the pair");
    assert_eq!(
        spans("😀か", 1, 3),
        [span(0, 2), span(2, 3)],
        "from the low half"
    );
    assert_eq!(spans("😀か", 2, 3), [span(2, 3)]);
    assert_eq!(
        spans("ab", 1, 9),
        [span(1, 2)],
        "beyond the text is not listed"
    );
    assert_eq!(
        spans("ab", 1, 1),
        Vec::<(usize, usize)>::new(),
        "an empty request"
    );
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
    super::run_table(
        "web_text_table",
        &[
            (
                "a_key_of_the_input_method_is_marked_composing",
                a_key_of_the_input_method_is_marked_composing,
            ),
            (
                "a_textarea_change_is_one_edit",
                a_textarea_change_is_one_edit,
            ),
            (
                "a_protected_field_is_mirrored_as_bullets",
                a_protected_field_is_mirrored_as_bullets,
            ),
            (
                "character_bounds_follow_scalars",
                character_bounds_follow_scalars,
            ),
            (
                "the_purpose_picks_the_input_mode",
                the_purpose_picks_the_input_mode,
            ),
        ],
    );
}
