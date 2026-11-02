//! iOS clipboard (`UIPasteboard`).
//!
//! `+[UIPasteboard generalPasteboard]` is a process-wide system service;
//! reading and writing it are the whole implementation. UIKit is
//! main-thread-only, so every access is checked for a `MainThreadMarker`
//! before messaging the pasteboard rather than trusting a caller to be on
//! the right thread.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use flui_platform::{ClipboardError, ClipboardRead, ClipboardWrite, Concealment};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use objc2_ui_kit::{UIPasteboard, UIPasteboardOption, UIPasteboardOptionLocalOnly};

use crate::host::{NativeClipboard, SyncClipboard, host_text};

use crate::host;
use crate::owner::apple::IOSLoop;

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

impl host::Clipboard<IOSLoop> for IOSClipboard {
    fn clipboard(_: &IOSLoop) -> std::rc::Rc<dyn NativeClipboard> {
        // Stateless: every handle reads and writes the general pasteboard.
        std::rc::Rc::new(IOSClipboard::new())
    }
}

impl SyncClipboard for IOSClipboard {
    fn read_now(&self) -> ClipboardRead {
        let pasteboard = Self::pasteboard().ok_or(ClipboardError::Unavailable)?;
        // SAFETY: the pasteboard is main-thread-affine and this runs after
        // the `MainThreadMarker` check above; `string` is a documented getter.
        let Some(string) = (unsafe { pasteboard.string() }) else {
            return Ok(None);
        };
        host_text(string.to_string())
    }

    fn write_now(&self, write: ClipboardWrite) {
        let Some(pasteboard) = Self::pasteboard() else {
            tracing::warn!("clipboard write dropped: UIPasteboard is main-thread-only");
            return;
        };
        let string = NSString::from_str(write.as_text());
        if !write.is_concealed() {
            // SAFETY: as above — main thread established, `setString:` is the
            // documented setter.
            unsafe { pasteboard.setString(Some(&string)) };
            return;
        }
        // A concealed write stays on this device: iOS keeps no clipboard
        // history, and `UIPasteboardOptionLocalOnly` keeps the item off the
        // universal clipboard of the user's other devices (ADR-0152 §6).
        let kind = NSString::from_str("public.utf8-plain-text");
        let text: &AnyObject = &string;
        let item = NSDictionary::<NSString, AnyObject>::from_slices(&[&*kind], &[text]);
        let items = NSArray::from_retained_slice(&[item]);
        let local = NSNumber::numberWithBool(true);
        let local: &AnyObject = &local;
        // SAFETY: `UIPasteboardOptionLocalOnly` is an immutable `NSString`
        // constant UIKit exports, which reading never races. The main thread
        // is established above; `setItems:options:` takes an array of
        // dictionaries from a type identifier to its value and a dictionary
        // of `UIPasteboardOption`s to their values, which both are.
        unsafe {
            let options = NSDictionary::<UIPasteboardOption, AnyObject>::from_slices(
                &[UIPasteboardOptionLocalOnly],
                &[local],
            );
            pasteboard.setItems_options(&items, &options);
        }
    }

    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }
}
