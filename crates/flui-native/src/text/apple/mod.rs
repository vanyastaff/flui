//! The Apple hosts' text: AppKit's text input (`macos`), the
//! `NSTextInputClient` half of the IME contract the window's view implements,
//! UIKit's keyboard responder and Dynamic Type sizing (`ios`), and the
//! document that responder edits (`composition`).
//!
//! Both input methods speak UTF-16, as `NSString` counts, and
//! `flui_platform::ImeEvent` speaks UTF-8 bytes; [`ime_cursor`] is the one
//! conversion between them.

use flui_platform::text_store::utf16::{self, Utf16Offset, Utf16Range};

#[cfg(any(target_os = "ios", test))]
pub(crate) mod composition;
#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;

/// The UTF-8 byte range of `text`'s UTF-16 units `from..to`, as
/// `ImeEvent::Preedit`'s cursor carries it.
///
/// `None` when the range is inverted, runs past the end of `text` or an end
/// falls inside a surrogate pair: a range no byte offset can express, and
/// one the caller must not round to a nearby boundary (a guessed caret is
/// worse than none: the client hides it).
pub(crate) fn ime_cursor(text: &str, from: Utf16Offset, to: Utf16Offset) -> Option<(usize, usize)> {
    let range = Utf16Range::new(from, to)?;
    let bytes = utf16::byte_range(text, range).ok()?;
    Some((bytes.start, bytes.end))
}

#[cfg(test)]
mod tests {
    use flui_platform::text_store::utf16::Utf16Offset;

    use super::ime_cursor;

    fn at(units: usize) -> Utf16Offset {
        Utf16Offset::new(units)
    }

    #[test]
    fn an_ime_cursor_is_counted_in_bytes() {
        // "héllo": five UTF-16 units but six bytes; `é` is one unit, two bytes.
        assert_eq!(ime_cursor("héllo", at(2), at(2)), Some((3, 3)));
        assert_eq!(ime_cursor("héllo", at(1), at(3)), Some((1, 4)));
        assert_eq!(ime_cursor("héllo", at(5), at(5)), Some((6, 6)));
        assert_eq!(ime_cursor("héllo", at(3), at(1)), None, "inverted");
        assert_eq!(ime_cursor("héllo", at(6), at(6)), None, "past the end");
        // "a😀": the emoji is a surrogate pair, units 1 and 2.
        assert_eq!(ime_cursor("a😀", at(2), at(2)), None, "inside the pair");
        assert_eq!(ime_cursor("a😀", at(3), at(3)), Some((5, 5)));
    }
}
