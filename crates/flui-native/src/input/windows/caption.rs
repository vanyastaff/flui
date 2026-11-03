//! The pointer over a caption button an application draws (ADR-0286 §7):
//! the window answers `WM_NCHITTEST` there as the system's own button
//! (`HTMAXBUTTON` shows Windows 11's snap layouts), so the pointer messages
//! that follow are non-client ones; they reach the application's button as the
//! client messages they stand for, and the default procedure, which would
//! track the system's own button and promote them to mouse messages, never
//! sees them.
//!
//! The pure half of the Win32 input: the rule needs no Win32 call, so every
//! host runs its test.

#![forbid(unsafe_code)]
#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "off Windows the caption rule is compiled for its test"
    )
)]

/// The caption buttons' `WM_NCHITTEST` answers (`winuser.h`).
const HT_MIN_BUTTON: u32 = 8;
const HT_MAX_BUTTON: u32 = 9;
const HT_CLOSE: u32 = 20;

/// The non-client pointer messages and the client ones they stand for
/// (`winuser.h`).
const WM_NCPOINTERUPDATE: u32 = 0x0241;
const WM_NCPOINTERDOWN: u32 = 0x0242;
const WM_NCPOINTERUP: u32 = 0x0243;
const WM_POINTERUPDATE: u32 = 0x0245;
const WM_POINTERDOWN: u32 = 0x0246;
const WM_POINTERUP: u32 = 0x0247;

/// The client pointer message `message` stands for when it is a non-client
/// one over a caption button, `None` for any other message or hit. `hit` is
/// the hit-test code the message carries (the high word of its `wParam`).
pub(crate) const fn client_message(message: u32, hit: u32) -> Option<u32> {
    if !matches!(hit, HT_MIN_BUTTON | HT_MAX_BUTTON | HT_CLOSE) {
        return None;
    }
    match message {
        WM_NCPOINTERUPDATE => Some(WM_POINTERUPDATE),
        WM_NCPOINTERDOWN => Some(WM_POINTERDOWN),
        WM_NCPOINTERUP => Some(WM_POINTERUP),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HT_CAPTION: u32 = 2;
    const HT_LEFT: u32 = 10;

    #[test]
    fn only_pointer_messages_over_a_caption_button_become_the_applications() {
        for (name, message, hit, client) in [
            (
                "move over maximize",
                WM_NCPOINTERUPDATE,
                HT_MAX_BUTTON,
                Some(WM_POINTERUPDATE),
            ),
            (
                "press on close",
                WM_NCPOINTERDOWN,
                HT_CLOSE,
                Some(WM_POINTERDOWN),
            ),
            (
                "release on minimize",
                WM_NCPOINTERUP,
                HT_MIN_BUTTON,
                Some(WM_POINTERUP),
            ),
            ("press on the bar", WM_NCPOINTERDOWN, HT_CAPTION, None),
            ("press on a border", WM_NCPOINTERDOWN, HT_LEFT, None),
            ("a mouse message", 0x00A1, HT_MAX_BUTTON, None),
        ] {
            assert_eq!(client_message(message, hit), client, "{name}");
        }
    }
}
