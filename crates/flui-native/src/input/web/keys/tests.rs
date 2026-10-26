//! The decidable half of the web backend's key translation
//! (`input::web::keys`): which keys belong to the input method. The DOM side
//! runs in Chrome (`tests/web_browser.rs`).

use crate::input::web::keys::{IME_PROCESS_KEY_CODE, key_composition};
use flui_platform::keyboard::ImeComposition;

const ESCAPE: u32 = 27;

/// Escape that cancels a composition is the input method's however the
/// browser reports it: Chromium marks it composing, WebKit sends it after
/// `compositionend` with only the input method's key code, and a surface
/// that still composes claims it whatever the event says.
#[test]
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
