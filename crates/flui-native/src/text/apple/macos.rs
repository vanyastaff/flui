//! macOS native text input: the content view's `NSTextInputClient` methods,
//! served from the focused field's store.
//!
//! AppKit has no "IME enabled" flag a backend can set: composition is a
//! *protocol the view implements*. `-[NSResponder interpretKeyEvents:]` hands
//! the key event to the view's input context, and that context calls back into
//! the view's `NSTextInputClient` methods — `setMarkedText:selectedRange:
//! replacementRange:` while composing, `insertText:replacementRange:` on commit,
//! `doCommandBySelector:` for keys it declines — and *queries* the view for the
//! document it composes in (`selectedRange`, a substring for reconversion, the
//! index under a point, `firstRectForCharacterRange:actualRange:` for
//! candidate-window placement). The view is the owner's
//! (`owner::apple::macos::content_view`), which turns those methods into Rust
//! calls on each view's [`TextView`]; this module answers them from the
//! field's store through the view's [`TextClient`], the window's
//! [`TextStoreHost`] (ADR-0090).
//!
//! # Where the key press goes
//!
//! Routing a key event into the input context and converting it on the
//! keyboard path are mutually exclusive for one physical press:
//! `interpretKeyEvents:` answers a plain letter with `insertText:`, so a key
//! that took both routes would reach the field twice. The input context takes
//! every press while a field has focus ([`TextClient::takes_key_presses`]) and
//! none otherwise, so a window with no focused field takes every key as a key
//! event.
//!
//! # Coordinate space
//!
//! A store speaks the window's logical coordinates — top-left origin, Y down,
//! the convention [`WindowState`] and AppKit pointer conversion's Y flip
//! establish; the view flips the axis as it converts to and from the screen.
//!
//! [`WindowState`]: flui_platform::WindowState

// The content view is reached through its raw AppKit pointer, and the
// capability that reaches it is shared across threads: the module is one of
// the crate's `unsafe` islands, opted out at its boundary.
#![expect(unsafe_code)]

use std::rc::Rc;

use flui_foundation::geometry::{Bounds, Point};
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{TextStoreHost, TextStoreStatus};
use objc2::msg_send;
use objc2::runtime::AnyObject;

use super::client::{DocumentChange, InputContext, TextClient};
use super::secure_input::SecureHold;
use crate::owner::apple::macos::content_view::{ViewText, with_content_view};
use crate::owner::apple::macos::{ContentViewRef, on_owner_thread};

// ============================================================================
// TextView — one content view's text input
// ============================================================================

/// The text input of one content view: the client that serves the focused
/// field to its input context, made with the view and retired with it. Every
/// call reaches it on the main thread, from the view's `NSTextInputClient`
/// methods (AppKit's delivery contract) or from the presentation through the
/// window's store host.
#[derive(Debug)]
pub(crate) struct TextView {
    client: Rc<TextClient>,
}

impl Default for TextView {
    fn default() -> Self {
        Self {
            client: TextClient::new(),
        }
    }
}

/// The view is deallocated with its context: the client lets the field go
/// and never messages the view again, though the presentation may still
/// hold it as the window's host.
impl Drop for TextView {
    fn drop(&mut self) {
        self.client.retire();
    }
}

// HIToolbox's secure event input, which `NSSecureTextField` turns on while it
// edits; the Carbon umbrella framework exports it.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// Take one hold on the process's secure event input; `noErr` (0) on
    /// success.
    fn EnableSecureEventInput() -> i32;
    /// Give one hold back; `noErr` (0) on success.
    fn DisableSecureEventInput() -> i32;
}

/// A content view's `NSTextInputContext`, reached through the view.
struct ViewInputContext {
    /// The view, alive while the client holds this: the client drops it as
    /// it retires, which the view's `dealloc` causes.
    view: *mut AnyObject,
    /// Whether this view holds secure event input.
    secure: SecureHold,
}

impl ViewInputContext {
    /// The view's input context; `None` when it has none.
    ///
    /// # Safety
    ///
    /// `self.view` is a live `NSView` on the main thread.
    unsafe fn input_context(&self) -> Option<&AnyObject> {
        // SAFETY: per the function contract; `inputContext` is NSView's own
        // getter, answering an object the view keeps alive or nil.
        unsafe {
            let context: *mut AnyObject = msg_send![self.view, inputContext];
            context.as_ref()
        }
    }
}

impl InputContext for ViewInputContext {
    fn discard_marked_text(&self) {
        // SAFETY: the client reaches this only while its view is alive (see
        // `view`), on the main thread its view's methods and the
        // presentation's calls run on; `discardMarkedText` is
        // `NSTextInputContext`'s own method.
        unsafe {
            if let Some(context) = self.input_context() {
                let _: () = msg_send![context, discardMarkedText];
            }
        }
    }

    fn invalidate_character_coordinates(&self) {
        // SAFETY: as for `discard_marked_text`;
        // `invalidateCharacterCoordinates` is `NSTextInputContext`'s own.
        unsafe {
            if let Some(context) = self.input_context() {
                let _: () = msg_send![context, invalidateCharacterCoordinates];
            }
        }
    }

    /// Secure event input follows the field: held while a protected one has
    /// focus in the key window.
    fn field_changed(&self, status: Option<TextStoreStatus>) {
        let wanted = status.is_some_and(|status| status.protected) && self.is_key_window();
        self.secure.sync(wanted, Self::set_secure_input);
    }

    /// AppKit asks its client for the document whenever it needs it, so an
    /// application's edit needs no notice beyond the marked text it ended
    /// and the coordinates it moved.
    fn document_changed(&self, _change: DocumentChange) {}
}

impl ViewInputContext {
    /// Take (`true`) or give back (`false`) one hold on the process's
    /// secure event input; `false` when the system refused.
    fn set_secure_input(secure: bool) -> bool {
        // SAFETY: both functions take no arguments and only move the
        // process's hold count; `SecureHold` pairs each hold it took with one
        // give-back, so the count never goes below what others hold.
        let status = unsafe {
            if secure {
                EnableSecureEventInput()
            } else {
                DisableSecureEventInput()
            }
        };
        if status != 0 {
            tracing::warn!(
                target: "flui.text.macos",
                status,
                secure,
                "the system refused to change secure event input"
            );
        }
        status == 0
    }

    fn is_key_window(&self) -> bool {
        // SAFETY: as for `discard_marked_text`; `window` is NSView's own
        // getter, answering the window the view is in or nil, and
        // `isKeyWindow` is NSWindow's.
        unsafe {
            let window: *mut AnyObject = msg_send![self.view, window];
            match window.as_ref() {
                Some(window) => {
                    let key: objc2::runtime::Bool = msg_send![window, isKeyWindow];
                    key.as_bool()
                }
                None => false,
            }
        }
    }
}

impl ViewText for TextView {
    fn bind(&self, view: *mut AnyObject) {
        self.client.bind(Rc::new(ViewInputContext {
            view,
            secure: SecureHold::default(),
        }));
    }

    fn store_host(&self) -> Rc<dyn TextStoreHost> {
        Rc::clone(&self.client) as Rc<dyn TextStoreHost>
    }

    fn takes_key_presses(&self) -> bool {
        self.client.takes_key_presses()
    }

    fn reports_key_release(&self) -> bool {
        self.client.reports_key_release()
    }

    fn key_window_changed(&self) {
        self.client.key_window_changed();
    }

    fn insert_text(&self, text: String, replacement: Option<Utf16Range>) {
        self.client.insert_text(text, replacement);
    }

    fn set_marked_text(&self, text: String, selected: Utf16Range, replacement: Option<Utf16Range>) {
        self.client.set_marked_text(text, selected, replacement);
    }

    fn unmark_text(&self) {
        self.client.unmark_text();
    }

    fn marked_range(&self) -> Option<Utf16Range> {
        self.client.marked_range()
    }

    fn selected_range(&self) -> Option<Utf16Range> {
        self.client.selected_range()
    }

    fn substring(&self, range: Utf16Range) -> Option<(String, Utf16Range)> {
        self.client.substring(range)
    }

    fn first_rect(&self, range: Option<Utf16Range>) -> Option<(Bounds<f64>, Utf16Range)> {
        self.client.first_rect(range)
    }

    fn character_index(&self, point: Point<f64>) -> Option<Utf16Offset> {
        self.client.character_index(point)
    }
}

// ============================================================================
// MacOSTextInput — the window's text position
// ============================================================================

/// One macOS window's text position: its content view's [`TextClient`],
/// handed to the presentation as the window's store host.
pub struct MacOSTextInput {
    /// The window's content view, with its owner lane and whether it has
    /// closed.
    ///
    /// The view is not retained: the position borrows the window it was made
    /// for, as every other per-window capability on this backend does, and
    /// the view lives as long as that window. `closed`, the same liveness
    /// gate the window's surface target consults, is what keeps a late call
    /// safe: a position read past its window's close is answered with nothing
    /// rather than a look into a view AppKit has torn down.
    window: ContentViewRef,
}

// SAFETY: the position holds the window's content-view address, the window's
// `closed` flag and its owner lane (a `&'static` queue and a `bool`). The view
// is read only on the owner thread, checked before it is touched, and only
// after `closed` reads unset there. The window sets `closed` on that same
// thread, in `windowWillClose:` and in its `Drop` before it releases the
// NSWindow whose hierarchy holds the view, so no release of the view can fall
// between the check and the read, and a position that outlives its window
// reaches nothing.
unsafe impl Send for MacOSTextInput {}
// SAFETY: see `Send` above; a shared reference reads the flag and copies the
// address and the lane, and the view is read only on the owner thread.
unsafe impl Sync for MacOSTextInput {}

/// A window's text position is made for it as it opens; the content view's
/// `NSTextInputClient` conformance is the input method's reach, so detaching
/// it leaves nothing behind (the view retires its client as it goes) and
/// sizing has no native converter.
impl crate::host::Text<ContentViewRef> for MacOSTextInput {
    fn for_window(window: &ContentViewRef) -> std::sync::Arc<dyn crate::text::TextInput> {
        std::sync::Arc::new(Self {
            window: window.clone(),
        })
    }
}

impl MacOSTextInput {
    /// Whether this position's window has closed.
    fn is_closed(&self) -> bool {
        self.window.closed.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl crate::text::TextInput for MacOSTextInput {
    /// The content view's client, on the owner thread while the window is
    /// open; `None` otherwise.
    fn window_text_input(&self) -> Option<crate::text::WindowTextInput> {
        if self.is_closed() || !on_owner_thread(self.window.owner, self.window.owner_is_main) {
            return None;
        }
        // SAFETY: the view is alive for the lifetime of the window this
        // position was made for, which has not closed, and this runs on the
        // owner thread, where the window sets `closed` before releasing it.
        // `with_content_view` checks the class before reading the view's
        // context.
        unsafe { with_content_view(self.window.view, |view| view.text().store_host()) }
            .map(crate::text::WindowTextInput::Store)
    }

    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        // AppKit has no system text-size converter a window could capture.
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    fn invalidate_sizing(&self) {}

    fn fence_sizing(&self) {}

    fn detach(&self) {}
}

impl std::fmt::Debug for MacOSTextInput {
    // Hand-written: the NSView pointer and the dispatch lane have no useful
    // Debug form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOSTextInput")
            .field("view", &self.window.view.addr())
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}
