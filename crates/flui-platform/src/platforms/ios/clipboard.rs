//! iOS clipboard (`UIPasteboard`).
//!
//! `+[UIPasteboard generalPasteboard]` is a process-wide system service;
//! reading and writing it are the whole implementation. UIKit is
//! main-thread-only, so every access is checked for a `MainThreadMarker`
//! before messaging the pasteboard rather than trusting a caller to be on
//! the right thread.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSString;
use objc2_ui_kit::UIPasteboard;

use crate::traits::Clipboard;

/// iOS clipboard backed by `UIPasteboard.generalPasteboard`.
pub struct IOSClipboard;

impl std::fmt::Debug for IOSClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSClipboard").finish_non_exhaustive()
    }
}

impl IOSClipboard {
    /// Create the clipboard handle. Constructing it is infallible; every
    /// operation checks the thread instead, so a handle can be created
    /// anywhere and only refuses when used off-main.
    pub fn new() -> Self {
        Self
    }

    /// The general pasteboard, or `None` off the main thread.
    fn pasteboard() -> Option<Retained<UIPasteboard>> {
        MainThreadMarker::new()?;
        Some(UIPasteboard::generalPasteboard())
    }
}

impl Default for IOSClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for IOSClipboard {
    fn read_text(&self) -> Option<String> {
        let pasteboard = Self::pasteboard()?;
        // SAFETY: the pasteboard is main-thread-affine and this runs after
        // the `MainThreadMarker` check above; `string` is a documented getter.
        let string = unsafe { pasteboard.string() }?;
        Some(string.to_string())
    }

    fn write_text(&self, text: String) {
        let Some(pasteboard) = Self::pasteboard() else {
            tracing::warn!("clipboard write dropped: UIPasteboard is main-thread-only");
            return;
        };
        let string = NSString::from_str(&text);
        // SAFETY: as above — main thread established, `setString:` is the
        // documented setter.
        unsafe { pasteboard.setString(Some(&string)) };
    }

    fn has_text(&self) -> bool {
        // SAFETY: as above — main thread established, `hasStrings` is a
        // documented getter.
        Self::pasteboard().is_some_and(|p| unsafe { p.hasStrings() })
    }
}
