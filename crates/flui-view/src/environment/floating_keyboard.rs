//! [`FloatingKeyboard`]: the soft keyboard floating over the window.

use flui_foundation::geometry::Rect;

/// The soft keyboard floating over the window, a value published to a
/// subtree (ADR-0322 §5, ADR-0373): its frame in logical pixels in the
/// window's content coordinates.
///
/// The runtime publishes one from each window's root while a keyboard floats
/// over the window and none otherwise, so a window with no floating keyboard
/// adds nothing to its tree's inherited scope. A keyboard that starts or
/// stops floating re-derives the scope below the root once; one the user
/// drags rebuilds only the widgets that read it. A test or a preview
/// publishes its own from a view's `provide` hook. A widget reads it with
/// [`MediaQuery::floating_keyboard_of`](super::MediaQuery::floating_keyboard_of).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatingKeyboard {
    frame: Rect,
}

impl FloatingKeyboard {
    /// The keyboard floating at `frame`.
    #[must_use]
    pub const fn new(frame: Rect) -> Self {
        Self { frame }
    }

    /// Where the keyboard floats.
    #[must_use]
    pub const fn frame(&self) -> Rect {
        self.frame
    }
}

impl crate::ProvidedValue for FloatingKeyboard {
    fn update_should_notify(&self, old: &Self) -> bool {
        self != old
    }
}
