//! macOS native text input — the composition behind the content view's
//! `NSTextInputClient` methods.
//!
//! AppKit has no "IME enabled" flag a backend can set: composition is a
//! *protocol the view implements*. `-[NSResponder interpretKeyEvents:]` hands
//! the key event to the view's input context, and that context calls back into
//! the view's `NSTextInputClient` methods — `setMarkedText:selectedRange:
//! replacementRange:` while composing, `insertText:replacementRange:` on commit,
//! `doCommandBySelector:` for keys it declines — and *queries* the view for
//! state it cannot know (`hasMarkedText`, `markedRange`, `selectedRange`,
//! `firstRectForCharacterRange:actualRange:` for candidate-window placement).
//! The view is the owner's (`owner::apple::macos::content_view`), which turns
//! those methods into Rust calls on each view's [`TextView`]; this module
//! keeps the composition they read and change, and announces it as
//! [`ImeEvent`]s.
//!
//! # Where the key press goes
//!
//! Routing a key event into the input context and converting it on the
//! keyboard path are mutually exclusive for one physical press:
//! `interpretKeyEvents:` answers a plain letter with `insertText:`, so a key
//! that took both routes would reach the application twice — once as
//! `ImeEvent::Commit`, once as `Key::Character`. [`TextInputState::ime_allowed`]
//! is the switch the view asks. It defaults to `false`, the state of every
//! window until a presentation attaches a text input and calls
//! `set_ime_allowed(true)`; with it `false`, every key takes the keyboard path.
//!
//! # Coordinate space
//!
//! [`PlatformTextInput::set_ime_cursor_area`] receives the framework's window
//! coordinates — top-left origin, Y down, the convention [`WindowState`] and
//! AppKit pointer conversion's Y flip establish; the view flips the axis as it
//! converts the area to the screen for `firstRectForCharacterRange:`.
//!
//! [`WindowState`]: flui_platform::WindowState

// The content view is reached through its raw AppKit pointer, and the
// capability that reaches it is shared across threads: the module is one of
// the crate's `unsafe` islands, opted out at its boundary.
#![expect(unsafe_code)]

use std::cell::RefCell;

use flui_foundation::geometry::Bounds;
use flui_platform::text_store::utf16::{self, Utf16Offset};
use flui_platform::{ImeEvent, PlatformInput, PlatformTextInput};

use crate::owner::apple::macos::content_view::{ContentView, ViewText, with_content_view};
use crate::owner::apple::macos::{ContentViewRef, exec_on_owner};

// ============================================================================
// The composition one view keeps
// ============================================================================

/// The composition AppKit drives and queries through one view.
#[derive(Debug, Default)]
pub(crate) struct TextInputState {
    /// Whether a key press takes the input method's route. `false` until a
    /// presentation attaches a text input, and the reason a window with no
    /// text input takes every key as a key event.
    pub(crate) ime_allowed: bool,

    /// The in-progress composition text, empty when nothing is marked.
    pub(crate) marked_text: String,

    /// The range this view reports for `marked_text` (UTF-16 units, the
    /// `NSRange` convention). `(0, utf16_len(marked_text))` while composing:
    /// this view holds no document of its own — the application owns the text
    /// buffer — so the composition is reported as starting at the view's own
    /// origin, which is what an input method needs to place its candidates and
    /// what `replacementRange:` from AppKit is relative to.
    pub(crate) marked_range: (usize, usize),

    /// The selection *within* the composition, as `setMarkedText:` delivered it
    /// (UTF-16 units) — the caret's own position, narrower than the marked
    /// range above.
    pub(crate) selected_range: (usize, usize),

    /// The candidate-window rectangle from
    /// [`PlatformTextInput::set_ime_cursor_area`], in the framework's logical
    /// window coordinates (top-left origin) as the trait specifies.
    pub(crate) cursor_area: Option<Bounds<f64>>,
}

impl TextInputState {
    /// Drop the marked text without emitting anything.
    ///
    /// Used by `unmarkText`, by a commit (whose text supersedes it), and by
    /// `set_ime_allowed(false)`. The last of those is the trait's own documented
    /// semantics: disabling drops an in-progress composition rather than
    /// committing it (`flui_platform::PlatformTextInput::set_ime_allowed`).
    fn clear_marked_text(&mut self) {
        self.marked_text.clear();
        self.marked_range = (0, 0);
        self.selected_range = (0, 0);
    }

    /// Whether a key *release* still belongs to the keyboard path.
    ///
    /// False exactly while a composition is open on an attached text input: the
    /// press that opened it went to the input method, so the release describes a
    /// key the application never saw go down. winit's own `keyUp:` arm queues
    /// the release only from its `Ground` and `Disabled` states, so both layers
    /// suppress the same event. Deliberately *not* a test of `ime_allowed`
    /// alone: an attached input that is not composing leaves the release on the
    /// ordinary path, which is where a typed Latin character's release happens
    /// after a commit empties [`Self::marked_text`].
    pub(crate) fn reports_key_release(&self) -> bool {
        !self.ime_allowed || self.marked_text.is_empty()
    }
}

// ============================================================================
// TextView — one content view's text input
// ============================================================================

/// The text input of one content view: its composition, made with the view
/// and dropped with it. Every call reaches it on the main thread, from the
/// view's `NSTextInputClient` methods (AppKit's delivery contract) or from a
/// [`MacOSTextInput`] body routed onto the owner lane.
#[derive(Debug, Default)]
pub(crate) struct TextView {
    state: RefCell<TextInputState>,
}

/// Announce `event` through the view: the same throat every other input
/// event of the view takes, so an IME event is ordered against pointer and
/// keyboard events exactly as they are ordered against each other.
fn announce(view: &ContentView<'_>, event: ImeEvent) {
    view.dispatch_input(PlatformInput::Ime(event));
}

impl ViewText for TextView {
    fn takes_key_presses(&self) -> bool {
        self.state.borrow().ime_allowed
    }

    fn reports_key_release(&self) -> bool {
        self.state.borrow().reports_key_release()
    }

    /// State before announcement, as in [`Self::compose`]: once `insertText:`
    /// returns, AppKit requires `hasMarkedText` to answer NO, and the
    /// dispatched commit runs application code that may ask.
    fn commit(&self, view: &ContentView<'_>, text: String) {
        self.state.borrow_mut().clear_marked_text();
        announce(view, ImeEvent::Commit(text));
    }

    /// `selected` is the caret's position *within* the composition, the only
    /// range AppKit gives here that is meaningful to a view with no document:
    /// the marked range this view reports is the whole composition.
    fn compose(&self, view: &ContentView<'_>, text: String, selected: (usize, usize)) {
        let cursor = selected.0.checked_add(selected.1).and_then(|end| {
            super::ime_cursor(&text, Utf16Offset::new(selected.0), Utf16Offset::new(end))
        });
        {
            let mut state = self.state.borrow_mut();
            state.marked_text.clone_from(&text);
            state.marked_range = (0, utf16::utf16_len(&text).get());
            state.selected_range = selected;
        }
        tracing::trace!("IME composition updated ({} bytes marked)", text.len());
        announce(view, ImeEvent::Preedit { text, cursor });
    }

    /// Announced as an empty `Preedit`, the vocabulary's own spelling of "the
    /// composition ended" (`flui_platform::ImeEvent`: a cancelled composition
    /// arrives as `Preedit { text: "", cursor: None }` with no following
    /// `Commit`/`Disabled`). Announcing nothing would leave a client holding
    /// composition state it was never told ended, suppressing
    /// `Key::Character` insertion for the rest of the focus session. The event
    /// is inert when nothing is composing, so no `hasMarkedText` check is
    /// needed.
    fn unmark(&self, view: &ContentView<'_>) {
        self.state.borrow_mut().clear_marked_text();
        announce(
            view,
            ImeEvent::Preedit {
                text: String::new(),
                cursor: None,
            },
        );
    }

    fn marked_range(&self) -> Option<(usize, usize)> {
        let state = self.state.borrow();
        (!state.marked_text.is_empty()).then_some(state.marked_range)
    }

    fn selected_range(&self) -> Option<(usize, usize)> {
        let state = self.state.borrow();
        (!state.marked_text.is_empty()).then_some(state.selected_range)
    }

    /// The range that goes with the area, which the SDK's contract requires of
    /// a non-null `actualRange`: the area is the composing region while one is
    /// open and the caret otherwise — the only two things this view can place
    /// — so it is the marked range or a zero-length range at the view's
    /// origin, the same origin `markedRange` reports from.
    fn candidate_area(&self) -> Option<(Bounds<f64>, (usize, usize))> {
        let state = self.state.borrow();
        let area = state.cursor_area?;
        let corresponding = if state.marked_text.is_empty() {
            (0, 0)
        } else {
            state.marked_range
        };
        Some((area, corresponding))
    }

    /// Announced after the borrow is released: dispatch re-enters application
    /// code (the widget-side text input), which is free to call back in.
    fn set_composing(&self, view: &ContentView<'_>, allowed: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.ime_allowed = allowed;
            if !allowed {
                // The trait's documented semantics for a disable: an
                // in-progress composition is dropped, not committed.
                state.clear_marked_text();
            }
        }
        announce(
            view,
            if allowed {
                ImeEvent::Enabled
            } else {
                ImeEvent::Disabled
            },
        );
    }

    fn set_caret_area(&self, area: Bounds<f64>) {
        self.state.borrow_mut().cursor_area = Some(area);
    }
}

// ============================================================================
// MacOSTextInput — the PlatformTextInput capability
// ============================================================================

/// One macOS window's IME capability.
///
/// [`PlatformTextInput`] is a public `Send + Sync` capability, so both of its
/// methods travel through [`exec_on_owner`] (the standing rule for every
/// AppKit-messaging window site) to the view's [`TextView`], whose state is
/// main-thread-only by AppKit's delivery contract.
pub struct MacOSTextInput {
    /// The window's content view, with its owner lane and whether it has
    /// closed.
    ///
    /// The view is not retained: the capability borrows the window it was
    /// made for, as every other per-window capability on this backend does,
    /// and the view lives as long as that window. `closed`, the same liveness
    /// gate the window's surface target consults, is what keeps a late call
    /// safe: a capability retained past its window's close or drop is a no-op
    /// rather than a message to a view AppKit has torn down.
    window: ContentViewRef,
}

// SAFETY: the capability holds the window's content-view address, the
// window's `closed` flag and its owner lane (a `&'static` queue and a `bool`).
// The view is messaged only inside `exec_on_owner`, which runs the body on the
// owner lane, and only after `closed` reads unset there. The window sets
// `closed` on that same thread, in `windowWillClose:` and in its `Drop` before
// it releases the NSWindow whose hierarchy holds the view, so no release of
// the view can fall between the check and the message, and a capability that
// outlives its window reaches nothing.
unsafe impl Send for MacOSTextInput {}
// SAFETY: see `Send` above; a shared reference reads the flag and copies the
// address and the lane, and every mutation happens to the view's text input
// inside the routed body.
unsafe impl Sync for MacOSTextInput {}

/// A window's IME capability is made for it as it opens; the content view's
/// `NSTextInputClient` conformance is the input method's reach, so detaching
/// it leaves nothing behind and sizing has no native converter.
impl crate::host::Text<ContentViewRef> for MacOSTextInput {
    fn for_window(window: &ContentViewRef) -> std::sync::Arc<dyn crate::text::TextInput> {
        std::sync::Arc::new(std::sync::Arc::new(Self {
            window: window.clone(),
        }))
    }
}

impl MacOSTextInput {
    /// Whether this capability's window has closed.
    fn is_closed(&self) -> bool {
        self.window.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Run `body` against the window's content view, on its owner lane.
    fn with_view<R: Send>(&self, body: impl FnOnce(&ContentView<'_>) -> R + Send) -> Option<R> {
        let owner = self.window.owner;
        let owner_is_main = self.window.owner_is_main;
        let this = self;
        exec_on_owner(owner, owner_is_main, move || {
            if this.is_closed() {
                tracing::trace!("text input call on a closed window; dropped");
                return None;
            }
            // SAFETY: the view is alive for the lifetime of the window this
            // capability was made for, which has not closed, and the body runs
            // on the owner thread — inline on the OS main thread for a
            // main-lane owner, or dispatched onto the lane under the reentrancy
            // guard — before the view is touched. `with_content_view` checks
            // the class before reading the view's context.
            unsafe { with_content_view(this.window.view, body) }
        })
    }
}

impl PlatformTextInput for MacOSTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        // The gate and the announcement travel in one owner-lane round trip:
        // `exec_on_owner` blocks the caller until the body returns, so a
        // second hop here would be a second blocking round trip for no gain.
        let reached = self.with_view(|view| view.text().set_composing(view, allowed));
        if reached.is_none() {
            tracing::trace!("IME state changed but the window has no live content view");
        }
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        let stored = self.with_view(|view| view.text().set_caret_area(area));
        if stored.is_none() {
            tracing::trace!("IME cursor area dropped: the window has no live content view");
        }
    }
}

impl crate::text::TextInput for std::sync::Arc<MacOSTextInput> {
    fn window_text_input(&self) -> Option<crate::text::WindowTextInput> {
        Some(crate::text::WindowTextInput::Ime(
            std::sync::Arc::clone(self) as std::sync::Arc<dyn PlatformTextInput>,
        ))
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
