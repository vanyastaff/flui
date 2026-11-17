//! The pointer over a caption button an application draws (ADR-0286 §7):
//! the window answers `WM_NCHITTEST` there as the system's own button
//! (`HTMAXBUTTON` shows Windows 11's snap layouts), so the pointer messages
//! that follow are non-client ones; they reach the application's button as the
//! client messages they stand for, and the default procedure, which would
//! track the system's own button and promote them to mouse messages, never
//! sees them.
//!
//! A process whose mouse is not a pointer (`EnableMouseInPointer` refused,
//! since the process had already fixed the other route) gets the legacy
//! non-client mouse messages there instead. They stand for the client mouse
//! messages the same way, so the default procedure draws no classic button
//! over the application's and tracks none. Both rules are for the
//! application's buttons only: the input asks the window whose buttons they
//! are (`owner::windows::WindowChrome`), and the messages over a caption button
//! the system draws stay the default procedure's.
//!
//! The pure half of the Win32 input: the rules need no Win32 call, so every
//! host runs their tests.

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
    if !is_caption_button(hit) {
        return None;
    }
    match message {
        WM_NCPOINTERUPDATE => Some(WM_POINTERUPDATE),
        WM_NCPOINTERDOWN => Some(WM_POINTERDOWN),
        WM_NCPOINTERUP => Some(WM_POINTERUP),
        _ => None,
    }
}

const fn is_caption_button(hit: u32) -> bool {
    matches!(hit, HT_MIN_BUTTON | HT_MAX_BUTTON | HT_CLOSE)
}

/// The pointer a non-client message names: the mouse, on the legacy route, or
/// a pointer by the id its `wParam` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptionPointer {
    Mouse,
    Pointer(u32),
}

/// The pointer whose non-client move `message` is, when the move is over none
/// of the caption buttons (the bar, a border): the hover a button saw of that
/// pointer ends there. `None` for any other message, and for a move over a
/// button, which [`client_message`] and [`client_mouse_message`] take.
pub(crate) const fn move_off_the_buttons(message: u32, wparam: usize) -> Option<CaptionPointer> {
    match message {
        WM_NCPOINTERUPDATE if !is_caption_button(((wparam >> 16) & 0xffff) as u32) => {
            Some(CaptionPointer::Pointer((wparam & 0xffff) as u32))
        }
        WM_NCMOUSEMOVE if !is_caption_button((wparam & 0xffff) as u32) => {
            Some(CaptionPointer::Mouse)
        }
        _ => None,
    }
}

/// The non-client mouse messages and the client ones they stand for
/// (`winuser.h`).
const WM_NCMOUSEMOVE: u32 = 0x00A0;
const WM_NCLBUTTONDOWN: u32 = 0x00A1;
const WM_NCLBUTTONUP: u32 = 0x00A2;
const WM_NCLBUTTONDBLCLK: u32 = 0x00A3;
const WM_NCRBUTTONDOWN: u32 = 0x00A4;
const WM_NCRBUTTONUP: u32 = 0x00A5;
const WM_NCRBUTTONDBLCLK: u32 = 0x00A6;
const WM_NCMBUTTONDOWN: u32 = 0x00A7;
const WM_NCMBUTTONUP: u32 = 0x00A8;
const WM_NCMBUTTONDBLCLK: u32 = 0x00A9;
const WM_NCXBUTTONDOWN: u32 = 0x00AB;
const WM_NCXBUTTONUP: u32 = 0x00AC;
const WM_NCXBUTTONDBLCLK: u32 = 0x00AD;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_XBUTTONDOWN: u32 = 0x020B;
const WM_XBUTTONUP: u32 = 0x020C;

/// The `MK_*` bits of a mouse message's `wParam` for its buttons (`winuser.h`).
const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
const MK_MBUTTON: usize = 0x0010;
const MK_XBUTTON1: usize = 0x0020;
const MK_XBUTTON2: usize = 0x0040;
const MK_BUTTONS: usize = MK_LBUTTON | MK_RBUTTON | MK_MBUTTON | MK_XBUTTON1 | MK_XBUTTON2;

/// A non-client mouse message over a caption button, as the client message it
/// stands for. Its position is the message's own, which the non-client message
/// carries in screen coordinates and the client one in client coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MouseStandIn {
    /// The client message.
    pub(crate) message: u32,
    /// The `MK_*` bit of the button the message presses or releases; zero
    /// for a move.
    button: usize,
    /// Whether it presses that button.
    pressed: bool,
    /// The high word of its `wParam`: an X button's number, zero for any
    /// other button.
    x_word: usize,
}

impl MouseStandIn {
    /// The client message's `wParam`, from `held`, the `MK_*` bits of the
    /// mouse buttons down in the thread's key state as the message is taken,
    /// which the non-client message does not carry: the buttons held once it
    /// is taken (a press holds its own button, a release no longer does), and
    /// an X button's number in the high word.
    pub(crate) const fn wparam(self, held: usize) -> usize {
        let held = held & MK_BUTTONS;
        let buttons = if self.pressed {
            held | self.button
        } else {
            held & !self.button
        };
        buttons | self.x_word
    }
}

/// The client mouse message the non-client one `message` stands for when it
/// is over a caption button, `None` for any other message or hit. A double
/// click on a caption button is a second press: the button decides what two
/// presses mean.
///
/// `wparam` is the non-client message's: the hit-test code in its low word
/// and, for an X button, the button's number in its high word.
pub(crate) const fn client_mouse_message(message: u32, wparam: usize) -> Option<MouseStandIn> {
    if !is_caption_button((wparam & 0xffff) as u32) {
        return None;
    }
    let x_button = (wparam >> 16) & 0xffff;
    let x_bit = match x_button {
        1 => MK_XBUTTON1,
        2 => MK_XBUTTON2,
        _ => 0,
    };
    // The client message, its own button's bit and whether it presses it.
    let (client, button, pressed) = match message {
        WM_NCMOUSEMOVE => (WM_MOUSEMOVE, 0, false),
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => (WM_LBUTTONDOWN, MK_LBUTTON, true),
        WM_NCLBUTTONUP => (WM_LBUTTONUP, MK_LBUTTON, false),
        WM_NCRBUTTONDOWN | WM_NCRBUTTONDBLCLK => (WM_RBUTTONDOWN, MK_RBUTTON, true),
        WM_NCRBUTTONUP => (WM_RBUTTONUP, MK_RBUTTON, false),
        WM_NCMBUTTONDOWN | WM_NCMBUTTONDBLCLK => (WM_MBUTTONDOWN, MK_MBUTTON, true),
        WM_NCMBUTTONUP => (WM_MBUTTONUP, MK_MBUTTON, false),
        WM_NCXBUTTONDOWN | WM_NCXBUTTONDBLCLK if x_bit != 0 => (WM_XBUTTONDOWN, x_bit, true),
        WM_NCXBUTTONUP if x_bit != 0 => (WM_XBUTTONUP, x_bit, false),
        _ => return None,
    };
    let x_word = if matches!(client, WM_XBUTTONDOWN | WM_XBUTTONUP) {
        x_button << 16
    } else {
        0
    };
    Some(MouseStandIn {
        message: client,
        button,
        pressed,
        x_word,
    })
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

    #[test]
    fn a_move_off_the_buttons_names_its_pointer() {
        let pointer = |hit: u32, id: usize| ((hit as usize) << 16) | id;
        for (name, message, wparam, moved) in [
            (
                "a pointer over the bar",
                WM_NCPOINTERUPDATE,
                pointer(HT_CAPTION, 7),
                Some(CaptionPointer::Pointer(7)),
            ),
            (
                "a pointer over a border",
                WM_NCPOINTERUPDATE,
                pointer(HT_LEFT, 1),
                Some(CaptionPointer::Pointer(1)),
            ),
            (
                "the mouse over the bar",
                WM_NCMOUSEMOVE,
                HT_CAPTION as usize,
                Some(CaptionPointer::Mouse),
            ),
            (
                "a pointer over a button",
                WM_NCPOINTERUPDATE,
                pointer(HT_MAX_BUTTON, 7),
                None,
            ),
            (
                "the mouse over a button",
                WM_NCMOUSEMOVE,
                HT_CLOSE as usize,
                None,
            ),
            (
                "a press on the bar",
                WM_NCLBUTTONDOWN,
                HT_CAPTION as usize,
                None,
            ),
            (
                "a pointer press on the bar",
                WM_NCPOINTERDOWN,
                pointer(HT_CAPTION, 7),
                None,
            ),
        ] {
            assert_eq!(move_off_the_buttons(message, wparam), moved, "{name}");
        }
    }

    #[test]
    fn mouse_messages_over_a_caption_button_become_the_applications() {
        let x_button = |number: usize, hit: u32| (number << 16) | hit as usize;
        let stand_in = |message: u32, wparam: usize| Some((message, wparam));
        for (name, message, wparam, held, client) in [
            (
                "move over maximize, nothing held",
                WM_NCMOUSEMOVE,
                HT_MAX_BUTTON as usize,
                0,
                stand_in(WM_MOUSEMOVE, 0),
            ),
            (
                "move over close with the primary held",
                WM_NCMOUSEMOVE,
                HT_CLOSE as usize,
                MK_LBUTTON,
                stand_in(WM_MOUSEMOVE, MK_LBUTTON),
            ),
            (
                "press on minimize holds its button though the key state lags",
                WM_NCLBUTTONDOWN,
                HT_MIN_BUTTON as usize,
                0,
                stand_in(WM_LBUTTONDOWN, MK_LBUTTON),
            ),
            (
                "a double click is a second press",
                WM_NCLBUTTONDBLCLK,
                HT_MAX_BUTTON as usize,
                MK_LBUTTON,
                stand_in(WM_LBUTTONDOWN, MK_LBUTTON),
            ),
            (
                "release on close no longer holds it, the other button still held",
                WM_NCLBUTTONUP,
                HT_CLOSE as usize,
                MK_LBUTTON | MK_RBUTTON,
                stand_in(WM_LBUTTONUP, MK_RBUTTON),
            ),
            (
                "secondary press",
                WM_NCRBUTTONDOWN,
                HT_MAX_BUTTON as usize,
                0,
                stand_in(WM_RBUTTONDOWN, MK_RBUTTON),
            ),
            (
                "secondary double click",
                WM_NCRBUTTONDBLCLK,
                HT_MAX_BUTTON as usize,
                0,
                stand_in(WM_RBUTTONDOWN, MK_RBUTTON),
            ),
            (
                "secondary release",
                WM_NCRBUTTONUP,
                HT_MAX_BUTTON as usize,
                MK_RBUTTON,
                stand_in(WM_RBUTTONUP, 0),
            ),
            (
                "middle press",
                WM_NCMBUTTONDOWN,
                HT_CLOSE as usize,
                0,
                stand_in(WM_MBUTTONDOWN, MK_MBUTTON),
            ),
            (
                "middle double click",
                WM_NCMBUTTONDBLCLK,
                HT_CLOSE as usize,
                0,
                stand_in(WM_MBUTTONDOWN, MK_MBUTTON),
            ),
            (
                "middle release",
                WM_NCMBUTTONUP,
                HT_CLOSE as usize,
                MK_MBUTTON,
                stand_in(WM_MBUTTONUP, 0),
            ),
            (
                "first X button press names its button",
                WM_NCXBUTTONDOWN,
                x_button(1, HT_MAX_BUTTON),
                0,
                stand_in(WM_XBUTTONDOWN, (1 << 16) | MK_XBUTTON1),
            ),
            (
                "second X button double click",
                WM_NCXBUTTONDBLCLK,
                x_button(2, HT_MAX_BUTTON),
                0,
                stand_in(WM_XBUTTONDOWN, (2 << 16) | MK_XBUTTON2),
            ),
            (
                "second X button release",
                WM_NCXBUTTONUP,
                x_button(2, HT_MIN_BUTTON),
                MK_XBUTTON2,
                stand_in(WM_XBUTTONUP, 2 << 16),
            ),
            (
                "an X button that names none",
                WM_NCXBUTTONDOWN,
                x_button(3, HT_MAX_BUTTON),
                0,
                None,
            ),
            (
                "key bits other than the buttons are not carried",
                WM_NCMOUSEMOVE,
                HT_MAX_BUTTON as usize,
                0x0004 | 0x0008,
                stand_in(WM_MOUSEMOVE, 0),
            ),
            (
                "press on the bar",
                WM_NCLBUTTONDOWN,
                HT_CAPTION as usize,
                0,
                None,
            ),
            (
                "move over a border",
                WM_NCMOUSEMOVE,
                HT_LEFT as usize,
                0,
                None,
            ),
            (
                "a pointer message",
                WM_NCPOINTERDOWN,
                HT_MAX_BUTTON as usize,
                0,
                None,
            ),
            (
                "the hit code in the high word is a pointer message's, not a mouse one's",
                WM_NCLBUTTONDOWN,
                (HT_MAX_BUTTON as usize) << 16,
                0,
                None,
            ),
        ] {
            assert_eq!(
                client_mouse_message(message, wparam)
                    .map(|stand_in| (stand_in.message, stand_in.wparam(held))),
                client,
                "{name}"
            );
        }
    }
}
