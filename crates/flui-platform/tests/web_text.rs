//! The decidable half of the web backend's text input (`shared::web_text`):
//! which keys belong to the input method, and the one edit between two
//! versions of a hidden textarea's text. The DOM side runs in Chrome
//! (`tests/web_browser.rs`).

use flui_platform::shared::web_text::{IME_PROCESS_KEY_CODE, TextDiff, diff, key_composition};
use flui_platform_api::keyboard::ImeComposition;

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
        ],
    );
}
