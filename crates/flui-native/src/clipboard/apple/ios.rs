//! iOS clipboard (`UIPasteboard`).
//!
//! `+[UIPasteboard generalPasteboard]` is a process-wide system service;
//! reading and writing it are the whole implementation. UIKit is
//! main-thread-only, so every access is checked for a `MainThreadMarker`
//! before messaging the pasteboard rather than trusting a caller to be on
//! the right thread. A read or write off the main thread answers
//! [`ClipboardError::Gone`]: retrying on that thread cannot succeed.

#![expect(
    unsafe_code,
    reason = "calls the operating system directly, which has no safe wrapper"
)]

use flui_platform::{
    ClipboardEntryId, ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment,
    ExpiryClearing,
};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDate, NSDictionary, NSNumber, NSString};
use objc2_ui_kit::{
    UIPasteboard, UIPasteboardOption, UIPasteboardOptionExpirationDate, UIPasteboardOptionLocalOnly,
};

use crate::host::{
    ClearDecision, ExpiredClear, ExpiredEntry, ExpiringRecord, NativeClipboard, SyncClipboard,
    admit_text, host_text,
};

use crate::host;
use crate::owner::apple::IOSLoop;

/// iOS clipboard backed by `UIPasteboard.generalPasteboard`.
///
/// An expiring write is set with `UIPasteboardOptionExpirationDate`: UIKit
/// itself removes the item at its time, even after the process ended, and
/// only that item, so a later copy is never taken for it (ADR-0363). The
/// loop's own clear at the time compares the pasteboard's `changeCount` with
/// the one recorded after the write and clears in a second call, a window a
/// copy made between the two can still fall into; the expiration date stands
/// behind it.
pub struct IOSClipboard {
    /// The last expiring entry this clipboard landed, with the pasteboard's
    /// `changeCount` after it; the main thread alone reaches it.
    expiring: std::cell::RefCell<ExpiringRecord<isize>>,
}

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
        Self {
            expiring: std::cell::RefCell::default(),
        }
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
        // Every handle reads and writes the general pasteboard; each records
        // the expiring entries written through it.
        std::rc::Rc::new(IOSClipboard::new())
    }
}

impl SyncClipboard for IOSClipboard {
    fn read_now(&self) -> ClipboardRead {
        let pasteboard = Self::pasteboard().ok_or(ClipboardError::Gone)?;
        // SAFETY: the pasteboard is main-thread-affine and this runs after
        // the `MainThreadMarker` check above; `string` is a documented getter.
        let Some(string) = (unsafe { pasteboard.string() }) else {
            return Ok(None);
        };
        // Measured in UTF-16 units before the copy into Rust.
        admit_text(string.length())?;
        host_text(string.to_string())
    }

    fn write_now(&self, write: ClipboardWrite) -> ClipboardWritten {
        let Some(pasteboard) = Self::pasteboard() else {
            tracing::warn!("clipboard write refused: UIPasteboard is main-thread-only");
            return Err(ClipboardError::Gone);
        };
        let string = NSString::from_str(write.as_text());
        let entry = write.entry();
        if !write.is_concealed() {
            // SAFETY: as above — main thread established, `setString:` is the
            // documented setter.
            unsafe { pasteboard.setString(Some(&string)) };
            self.landed(&pasteboard, None);
            return Ok(());
        }
        // A concealed write stays on this device: iOS keeps no clipboard
        // history, and `UIPasteboardOptionLocalOnly` keeps the item off the
        // universal clipboard of the user's other devices (ADR-0152 §6). An
        // expiring one is removed by UIKit at its time (ADR-0363).
        let kind = NSString::from_str("public.utf8-plain-text");
        let text: &AnyObject = &string;
        let item = NSDictionary::<NSString, AnyObject>::from_slices(&[&*kind], &[text]);
        let items = NSArray::from_retained_slice(&[item]);
        let local = NSNumber::numberWithBool(true);
        let local: &AnyObject = &local;
        let expires = write
            .expiry()
            .map(|after| NSDate::dateWithTimeIntervalSinceNow(after.get().as_secs_f64()));
        // SAFETY: `UIPasteboardOptionLocalOnly` and
        // `UIPasteboardOptionExpirationDate` are immutable `NSString`
        // constants UIKit exports, which reading never races. The main thread
        // is established above; `setItems:options:` takes an array of
        // dictionaries from a type identifier to its value and a dictionary
        // of `UIPasteboardOption`s to their values (a boolean `NSNumber`, an
        // `NSDate`), which both are.
        unsafe {
            let options = match &expires {
                Some(date) => {
                    let date: &AnyObject = date;
                    NSDictionary::<UIPasteboardOption, AnyObject>::from_slices(
                        &[
                            UIPasteboardOptionLocalOnly,
                            UIPasteboardOptionExpirationDate,
                        ],
                        &[local, date],
                    )
                }
                None => NSDictionary::<UIPasteboardOption, AnyObject>::from_slices(
                    &[UIPasteboardOptionLocalOnly],
                    &[local],
                ),
            };
            pasteboard.setItems_options(&items, &options);
        }
        self.landed(&pasteboard, entry);
        Ok(())
    }

    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }

    /// UIKit removes an expiring item at its time; iOS gives an application
    /// no notice of the device locking.
    fn expiry_clearing(&self) -> ExpiryClearing {
        ExpiryClearing::AtTime
    }

    /// The change count is read and the items cleared in two calls (see the
    /// type's docs).
    fn clear_expired_now(&self, entry: ExpiredEntry) -> ExpiredClear {
        let Some(pasteboard) = Self::pasteboard() else {
            return ExpiredClear::Failed(ClipboardError::Gone);
        };
        // SAFETY: main thread established above; `changeCount` is a
        // documented getter.
        let count = unsafe { pasteboard.changeCount() };
        let decision = self.expiring.borrow_mut().decide(entry, &count);
        match decision {
            ClearDecision::NotHeld => ExpiredClear::NotHeld,
            ClearDecision::Clear => {
                // The record forgets the entry before UIKit is asked, with
                // its borrow ended: `setItems:` posts
                // `UIPasteboardChangedNotification` at once, and an observer
                // that writes records its own entry.
                self.expiring.borrow_mut().landed(None, count);
                let none = NSArray::<NSDictionary<NSString, AnyObject>>::new();
                // SAFETY: main thread established above; `setItems:` takes an
                // array of item dictionaries, here none.
                unsafe { pasteboard.setItems(&none) };
                ExpiredClear::Cleared
            }
        }
    }
}

impl IOSClipboard {
    /// A write landed on `pasteboard`: record `entry` with the change count
    /// it left, or forget the earlier entry for a write that does not expire.
    fn landed(&self, pasteboard: &UIPasteboard, entry: Option<ClipboardEntryId>) {
        // SAFETY: called on the main thread right after a write to the
        // pasteboard; `changeCount` is a documented getter.
        let count = unsafe { pasteboard.changeCount() };
        self.expiring.borrow_mut().landed(entry, count);
    }
}
