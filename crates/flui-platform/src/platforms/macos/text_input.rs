//! macOS native text input — the `NSTextInputClient` half of the IME contract.
//!
//! AppKit has no "IME enabled" flag a backend can set: composition is a
//! *protocol the view implements*. `-[NSResponder interpretKeyEvents:]` hands
//! the key event to the view's input context, and that context calls back into
//! the view's `NSTextInputClient` methods — `setMarkedText:selectedRange:
//! replacementRange:` while composing, `insertText:replacementRange:` on commit,
//! `doCommandBySelector:` for keys it declines — and *queries* the view for
//! state it cannot know (`hasMarkedText`, `markedRange`, `selectedRange`,
//! `firstRectForCharacterRange:actualRange:` for candidate-window placement).
//! So the winit backend's shape — a thin wrapper over a platform call — does
//! not transfer: on macOS this module becomes the implementor.
//!
//! [`PlatformTextInput`]: crate::traits::PlatformTextInput
//!
//! # The two halves meet at `keyDown:`
//!
//! Routing a key event into the input context and converting it on the keyboard
//! path are mutually exclusive for one physical press: `interpretKeyEvents:`
//! answers a plain letter with `insertText:`, so a key that took both routes
//! would reach the application twice — once as `ImeEvent::Commit`, once as
//! `Key::Character`. [`TextInputState::ime_allowed`] is the switch. It defaults
//! to `false`, which is the state of every window until a presentation attaches
//! a text input and calls `set_ime_allowed(true)`; with it `false`, `keyDown:`
//! takes the keyboard path byte-identically to the way it did before this
//! module existed. `doCommandBySelector:` is the escape hatch back the other
//! way, so arrows, Escape and Return still arrive as key events when a text
//! input *is* attached and the input method declines them.
//!
//! # Coordinate space
//!
//! [`PlatformTextInput::set_ime_cursor_area`] receives the framework's window
//! coordinates — top-left origin, Y down, the convention
//! [`PlatformWindow::bounds`] and `convert_ns_event`'s Y flip establish — while
//! `convertRectToScreen:` speaks AppKit's window base coordinates, bottom-left
//! origin, Y up. `first_rect_for_character_range:actualRange:` is where the two
//! meet, and it flips the axis before converting; without that flip the
//! candidate window would sit mirrored about the window's vertical centre.
//!
//! [`PlatformWindow::bounds`]: crate::traits::PlatformWindow::bounds

use objc2::runtime::{AnyObject, Bool, ClassBuilder, Protocol, Sel};
use objc2::{msg_send, sel};
use objc2_foundation::{NSNotFound, NSPoint, NSRange, NSRect, NSSize, NSUInteger};

use flui_types::{
    ImeEvent,
    geometry::{Bounds, Pixels},
};

use super::view::{ViewContext, get_context as get_view_context};
use super::window::route_on_owner;
use crate::traits::PlatformTextInput;

// ============================================================================
// View-owned composition state
// ============================================================================

/// The composition state AppKit drives and queries through the view.
///
/// Lives in [`ViewContext`] behind a `RefCell`: every method that reads or
/// writes it is either an `NSTextInputClient` callback — which AppKit delivers
/// on the main thread by its own contract — or an owner-routed
/// [`MacOSTextInput`] body, and `get_context` hands out a *shared* reference to
/// the context, so the mutability has to be inside it.
#[derive(Debug, Default)]
pub(super) struct TextInputState {
    /// Whether `keyDown:` takes the input-context route. `false` until a
    /// presentation attaches a text input, and the reason a window with no
    /// text input behaves exactly as it did before this module existed.
    pub(super) ime_allowed: bool,

    /// The in-progress composition text, empty when nothing is marked.
    pub(super) marked_text: String,

    /// The range this view reports for `marked_text` (UTF-16 units, the
    /// `NSRange` convention). `(0, utf16_len(marked_text))` while composing:
    /// this view holds no document of its own — the application owns the text
    /// buffer — so the composition is reported as starting at the view's own
    /// origin, which is what an input method needs to place its candidates and
    /// what `replacementRange:` from AppKit is relative to.
    pub(super) marked_range: (usize, usize),

    /// The selection *within* the composition, as `setMarkedText:` delivered it
    /// (UTF-16 units) — the caret's own position, narrower than the marked
    /// range above.
    pub(super) selected_range: (usize, usize),

    /// The candidate-window rectangle from
    /// [`PlatformTextInput::set_ime_cursor_area`], in the framework's logical
    /// window coordinates (top-left origin) as the trait specifies.
    pub(super) cursor_area: Option<Bounds<Pixels>>,

    /// The `NSResponder` `keyDown:` `NSEvent*` currently inside
    /// `interpretKeyEvents:`, as a `usize` (0 when none). Non-zero only for the
    /// duration of that call, so `doCommandBySelector:` — which the input
    /// context invokes *during* it — can hand the same event back to the
    /// keyboard path when the input method declines it.
    pub(super) pending_key_event: usize,
}

impl TextInputState {
    /// Drop the marked text without emitting anything.
    ///
    /// Used by `unmarkText`, by a commit (whose text supersedes it), and by
    /// `set_ime_allowed(false)`. The last of those is the trait's own documented
    /// semantics: disabling drops an in-progress composition rather than
    /// committing it (`crate::traits::PlatformTextInput::set_ime_allowed`).
    fn clear_marked_text(&mut self) {
        self.marked_text.clear();
        self.marked_range = (0, 0);
        self.selected_range = (0, 0);
    }

    /// Whether a key *release* still belongs to the keyboard path.
    ///
    /// False exactly while a composition is open on an attached text input: the
    /// press that opened it went to the input method, so the release describes a
    /// key the application never saw go down. The view's `keyUp:` handler — the
    /// one AppKit entry point that consumes this predicate — asks before
    /// forwarding, and winit's own `keyUp:` arm queues the release only from its
    /// `Ground` and `Disabled` states, so both layers suppress the same event.
    /// Deliberately *not* a test of `ime_allowed` alone: an attached input that
    /// is not composing leaves the release on the ordinary path, which is where
    /// a typed Latin character's release happens after a commit empties
    /// [`Self::marked_text`].
    pub(super) fn reports_key_release(&self) -> bool {
        !self.ime_allowed || self.marked_text.is_empty()
    }
}

// ============================================================================
// UTF-16 → UTF-8 range conversion
// ============================================================================

/// The number of UTF-16 units in `text` — the length an `NSRange` covering all
/// of it carries.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The byte offset in `text` that UTF-16 unit index `units` lands on, or
/// `None` when that index is past the end or splits a surrogate pair.
fn byte_offset_of_utf16_boundary(text: &str, units: usize) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }
    let mut counted = 0;
    for (byte_offset, ch) in text.char_indices() {
        counted += ch.len_utf16();
        if counted >= units {
            // `>=`, not `==`: a multi-unit character (a surrogate pair) can
            // carry the count past the target, and an index inside one is not
            // a char boundary — no byte offset expresses it.
            return (counted == units).then_some(byte_offset + ch.len_utf8());
        }
    }
    None
}

/// Map an `NSString` UTF-16 `NSRange` onto byte offsets within the UTF-8
/// encoding of the same text.
///
/// AppKit speaks UTF-16 (`NSRange`, `NSTextInputClient`'s `selectedRange`),
/// `flui_types::ImeEvent` speaks bytes, and the two diverge at the first
/// non-BMP or multi-byte character. Returns `None` when the range runs past the
/// end of `text` or either end falls inside a surrogate pair — a range no byte
/// offset can express, and one the caller must not round to a nearby boundary
/// (a guessed caret is worse than none: the client hides it).
pub(super) fn utf16_range_to_byte_range(
    text: &str,
    location: usize,
    length: usize,
) -> Option<(usize, usize)> {
    let end = location.checked_add(length)?;
    Some((
        byte_offset_of_utf16_boundary(text, location)?,
        byte_offset_of_utf16_boundary(text, end)?,
    ))
}

// ============================================================================
// NSString ↔ Rust text
// ============================================================================

/// Copy the UTF-8 bytes of an `NSString` into an owned `String`.
///
/// # Safety
///
/// `string` must be null or a live `NSString*`, or an object that answers
/// `UTF8String` in its stead.
unsafe fn ns_string_to_owned(string: *mut AnyObject) -> Option<String> {
    // SAFETY: per this function's contract the message goes to a live string
    // object, and `UTF8String` returns an autoreleased buffer valid for the
    // current pool — the bytes are copied out here, so nothing borrowed
    // escapes this call.
    unsafe {
        if string.is_null() {
            return None;
        }
        let utf8: *const std::ffi::c_char = msg_send![string, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(utf8)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Read the `string` argument of `insertText:replacementRange:` as Rust text.
///
/// AppKit documents the argument as an `NSString`, but an `NSAttributedString`
/// arrives on the same path. The two are told apart by the selector
/// `NSAttributedString` alone answers — `-string` — rather than by a class
/// check, so a subclass that carries attributes without being one still
/// converts.
///
/// # Safety
///
/// `string` must be null or the live argument AppKit passed to `insertText:`.
unsafe fn insert_text_argument_string(string: *mut AnyObject) -> Option<String> {
    // SAFETY: `respondsToSelector:` runs before `-string` is sent, so the
    // message goes only to an object that declares it; both branches end in
    // `ns_string_to_owned`'s own contract.
    unsafe {
        if string.is_null() {
            return None;
        }
        let responds: Bool = msg_send![string, respondsToSelector: sel!(string)];
        let text = if responds == Bool::YES {
            msg_send![string, string]
        } else {
            string
        };
        ns_string_to_owned(text)
    }
}

// ============================================================================
// Event emission
// ============================================================================

/// Emit one IME event through the window's callbacks.
///
/// The same throat every other input event in this backend takes, so an IME
/// event is ordered against pointer and keyboard events exactly as they are
/// ordered against each other.
fn emit_ime(ctx: &ViewContext, event: ImeEvent) {
    super::view::dispatch_input_event(ctx, crate::traits::PlatformInput::Ime(event));
}

/// The empty rect AppKit is answered with when no cursor area has been set.
fn zero_rect() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
}

/// Write `range` through an `actualRange:` out-parameter.
///
/// # Safety
///
/// Either null — AppKit may pass a null `actualRange:` — or a valid, writable
/// `NSRange*` for the duration of the call, as AppKit documents.
unsafe fn write_actual_range(actual_range: *mut NSRange, range: NSRange) {
    // SAFETY: per this function's contract.
    unsafe {
        if !actual_range.is_null() {
            *actual_range = range;
        }
    }
}

// ============================================================================
// NSTextInputClient callbacks
// ============================================================================

/// `insertText:replacementRange:` — the input method committed text.
extern "C-unwind" fn insert_text(
    this: &AnyObject,
    _sel: Sel,
    string: *mut AnyObject,
    _replacement_range: NSRange,
) {
    // SAFETY: `this` is a live FLUIContentView — AppKit invokes a method only on
    // an object that declares it, and `add_text_input_methods` registered this
    // one on that class; `string` is the live argument for the duration of the
    // call, and its bytes are copied before this function returns.
    unsafe {
        let Some(ctx) = get_view_context(this) else {
            return;
        };
        let Some(text) = insert_text_argument_string(string) else {
            tracing::trace!("insertText: carried no readable string; commit dropped");
            return;
        };
        // State before announcement, as in `set_marked_text`: once `insertText:`
        // returns, AppKit requires `hasMarkedText` to answer NO, and the
        // dispatched commit runs application code that may ask.
        ctx.text_input.borrow_mut().clear_marked_text();
        emit_ime(ctx, ImeEvent::Commit(text));
    }
}

/// `setMarkedText:selectedRange:replacementRange:` — the composition changed.
///
/// `selectedRange` is the caret's position *within* the composition, and it is
/// the only range AppKit gives here that is meaningful to a view with no
/// document: the marked range this view reports is the whole composition
/// (`NSRange::new(0, ..)`), while the selection is this narrower one.
extern "C-unwind" fn set_marked_text(
    this: &AnyObject,
    _sel: Sel,
    string: *mut AnyObject,
    selected_range: NSRange,
    _replacement_range: NSRange,
) {
    // SAFETY: `this` is a live FLUIContentView (see `insert_text`); `string` is
    // the live argument for the duration of the call, copied out immediately.
    unsafe {
        let Some(ctx) = get_view_context(this) else {
            return;
        };
        let text = ns_string_to_owned(string).unwrap_or_default();
        let cursor =
            utf16_range_to_byte_range(&text, selected_range.location, selected_range.length);
        {
            let mut state = ctx.text_input.borrow_mut();
            state.marked_text.clone_from(&text);
            state.marked_range = (0, utf16_len(&text));
            state.selected_range = (selected_range.location, selected_range.length);
        }
        tracing::trace!("IME composition updated ({} bytes marked)", text.len());
        emit_ime(ctx, ImeEvent::Preedit { text, cursor });
    }
}

/// `unmarkText` — the composition was abandoned; nothing is committed.
///
/// Announced as an empty `Preedit`, the vocabulary's own spelling of "the
/// composition ended" (`flui_types::ImeEvent`'s type-level doc: a cancelled
/// composition arrives as `Preedit { text: "", cursor: None }` with no
/// following `Commit`/`Disabled`). Emitting nothing here would be the
/// previously-shipped bug class that doc records from the other side: a client
/// left holding composition state it was never told ended suppresses
/// `Key::Character` insertion for the rest of the focus session, and the
/// cancelled slice stays in its buffer. AppKit's own header is silent on
/// whether `unmarkText` is always preceded by an empty `setMarkedText:`, so
/// both paths must be covered. The event is inert when nothing is composing
/// (`TextEditingController::set_composing_text`'s empty branch strips only an
/// active span, and reports no change when there is none), which is why this
/// callback needs no `hasMarkedText` check of its own.
extern "C-unwind" fn unmark_text(this: &AnyObject, _sel: Sel) {
    // SAFETY: `this` is a live FLUIContentView (see `insert_text`).
    unsafe {
        if let Some(ctx) = get_view_context(this) {
            ctx.text_input.borrow_mut().clear_marked_text();
            emit_ime(
                ctx,
                ImeEvent::Preedit {
                    text: String::new(),
                    cursor: None,
                },
            );
        }
    }
}

/// `hasMarkedText` — is a composition in progress?
extern "C-unwind" fn has_marked_text(this: &AnyObject, _sel: Sel) -> Bool {
    // SAFETY: `this` is a live FLUIContentView (see `insert_text`).
    unsafe {
        match get_view_context(this) {
            Some(ctx) if !ctx.text_input.borrow().marked_text.is_empty() => Bool::YES,
            _ => Bool::NO,
        }
    }
}

/// `markedRange` — the range of the composition, in UTF-16 units.
extern "C-unwind" fn marked_range(this: &AnyObject, _sel: Sel) -> NSRange {
    // SAFETY: `this` is a live FLUIContentView (see `insert_text`).
    unsafe {
        let Some(ctx) = get_view_context(this) else {
            return NSRange::new(NSNotFound as usize, 0);
        };
        let state = ctx.text_input.borrow();
        if state.marked_text.is_empty() {
            NSRange::new(NSNotFound as usize, 0)
        } else {
            NSRange::new(state.marked_range.0, state.marked_range.1)
        }
    }
}

/// `selectedRange` — the caret's range within the composition.
///
/// `{NSNotFound, 0}` outside one: a view with no document of its own cannot
/// truthfully name a selection the application has not reported, and AppKit
/// documents that pair as "no selection".
extern "C-unwind" fn selected_range(this: &AnyObject, _sel: Sel) -> NSRange {
    // SAFETY: `this` is a live FLUIContentView (see `insert_text`).
    unsafe {
        let Some(ctx) = get_view_context(this) else {
            return NSRange::new(NSNotFound as usize, 0);
        };
        let state = ctx.text_input.borrow();
        if state.marked_text.is_empty() {
            NSRange::new(NSNotFound as usize, 0)
        } else {
            NSRange::new(state.selected_range.0, state.selected_range.1)
        }
    }
}

/// `validAttributesForMarkedText` — the attributes this view honours.
///
/// Empty: the composing text is rendered by the framework's own client, not by
/// AppKit's text system, so an attributed composition has nothing to describe.
extern "C-unwind" fn valid_attributes_for_marked_text(
    _this: &AnyObject,
    _sel: Sel,
) -> *mut AnyObject {
    // SAFETY: `+[NSArray array]` is a class constructor on a class AppKit
    // always provides; it returns an autoreleased empty array.
    unsafe { msg_send![objc2::class!(NSArray), array] }
}

/// `attributedSubstringForProposedRange:actualRange:` — the text in a range.
///
/// `nil`, with `actualRange` answered `{NSNotFound, 0}`: this view holds no
/// attributed text to slice, and the documented answer for a range that cannot
/// be satisfied is a nil substring plus a not-found actual range.
extern "C-unwind" fn attributed_substring_for_proposed_range(
    _this: &AnyObject,
    _sel: Sel,
    _range: NSRange,
    actual_range: *mut NSRange,
) -> *mut AnyObject {
    // SAFETY: `actual_range` is AppKit's own out-parameter for this call —
    // null or a valid writable `NSRange*` — which is what `write_actual_range`
    // requires.
    unsafe { write_actual_range(actual_range, NSRange::new(NSNotFound as usize, 0)) };
    std::ptr::null_mut()
}

/// `firstRectForCharacterRange:actualRange:` — where to place candidates.
///
/// The one consumer of [`PlatformTextInput::set_ime_cursor_area`]: the setter
/// stores the rectangle, this getter answers AppKit's query with it, converted
/// to screen coordinates. The Y axis is flipped on the way in — the framework's
/// window coordinates have their origin at the top-left, AppKit's at the
/// bottom-left — see this module's doc for why that flip is load-bearing.
extern "C-unwind" fn first_rect_for_character_range(
    this: &AnyObject,
    _sel: Sel,
    _range: NSRange,
    actual_range: *mut NSRange,
) -> NSRect {
    // SAFETY: `this` is a live FLUIContentView (see `insert_text`);
    // `actual_range` is AppKit's own out-parameter; `bounds` and `window` are
    // AppKit's own getters on that live view, and the window is nil-checked
    // before `convertRectToScreen:` — a view with no window (mid-teardown) has
    // no screen rectangle to answer with.
    unsafe {
        write_actual_range(actual_range, NSRange::new(NSNotFound as usize, 0));
        let Some(ctx) = get_view_context(this) else {
            return zero_rect();
        };
        let state = ctx.text_input.borrow();
        let Some(area) = state.cursor_area else {
            return zero_rect();
        };
        // The range that goes with the area, which the SDK's contract requires
        // of a non-null `actualRange` ("contains the character range
        // corresponding to the returned area"). The area is the composing
        // region while one is open and the caret otherwise — the only two
        // things this view can place — so it is the marked range or a
        // zero-length range at the view's origin, the same origin
        // `marked_range` reports from. `{NSNotFound, 0}` is written above only
        // for the paths that return `zero_rect()`, where there is no area for a
        // range to correspond to.
        let corresponding = if state.marked_text.is_empty() {
            NSRange::new(0, 0)
        } else {
            NSRange::new(state.marked_range.0, state.marked_range.1)
        };
        let view_bounds: NSRect = msg_send![this, bounds];
        let flipped_y = view_bounds.size.height - (area.origin.y.0 + area.size.height.0) as f64;
        let window_rect = NSRect::new(
            NSPoint::new(area.origin.x.0 as f64, flipped_y),
            NSSize::new(area.size.width.0 as f64, area.size.height.0 as f64),
        );
        let window: *mut AnyObject = msg_send![this, window];
        if window.is_null() {
            return zero_rect();
        }
        let screen_rect: NSRect = msg_send![window, convertRectToScreen: window_rect];
        write_actual_range(actual_range, corresponding);
        screen_rect
    }
}

/// `characterIndexForPoint:` — the character under a point.
///
/// `NSNotFound`: this view owns no text, so it can answer no index into one.
extern "C-unwind" fn character_index_for_point(
    _this: &AnyObject,
    _sel: Sel,
    _point: NSPoint,
) -> NSUInteger {
    NSNotFound as NSUInteger
}

/// `doCommandBySelector:` — a key the input method declined.
///
/// The escape hatch that keeps the input-context route from swallowing arrows,
/// Escape, Return and the rest: the event the caller is currently inside
/// `interpretKeyEvents:` with is re-dispatched through the ordinary keyboard
/// conversion, which is exactly the path `keyDown:` itself would have taken.
///
/// `pending_key_event` is 0 when the input method raises a command outside an
/// `interpretKeyEvents:` call (it also does so for menu-key equivalents, with no
/// event of ours on the stack); there is then no key press to convert, so the
/// command is dropped rather than invented.
extern "C-unwind" fn do_command_by_selector(this: &AnyObject, _sel: Sel, _command: Sel) {
    // SAFETY: `this` is a live FLUIContentView; `pending_key_event` is either 0
    // or an `NSEvent*` AppKit owns and keeps alive across the
    // `interpretKeyEvents:` call whose dispatch is on this stack frame, which is
    // the only window in which this callback runs.
    unsafe {
        let Some(ctx) = get_view_context(this) else {
            return;
        };
        let pending_key_event = ctx.text_input.borrow().pending_key_event;
        if pending_key_event == 0 {
            tracing::trace!("doCommandBySelector: outside a pending key press; command dropped");
            return;
        }
        super::view::handle_input_event(this, sel!(keyDown:), pending_key_event as *mut AnyObject);
    }
}

/// Register the `NSTextInputClient` callbacks on a class being declared.
///
/// Called from `get_or_create_view_class`'s `Once` block, on the
/// `FLUIContentView` declaration, before `register()`.
///
/// # Safety
///
/// `decl` must be the declaration of the class `get_context`'s ivar contract
/// applies to, and every function pointer below must keep matching its
/// selector's Objective-C signature — the signatures are fixed by
/// `NSTextInputClient`'s own declaration in AppKit.
pub(super) unsafe fn add_text_input_methods(decl: &mut ClassBuilder) {
    // Conformance, not just the methods: AppKit's `NSTextInputContext` asks
    // `conformsToProtocol:` before it will route a key event through the input
    // method at all, so a class with these methods and no protocol is never
    // called.
    //
    // `expect`, not a silent skip: registering the methods without conformance
    // would leave the class with `interpretKeyEvents:` still callable from
    // `key_down` but no client to call back into — the capability would be
    // quietly dead. The protocol is declared since 10.5 and this backend's
    // floor is 11.0 (checked against the SDK's `NSTextInputClient.h`), so a
    // `None` here can only mean this module's own name or floor is wrong.
    let protocol = Protocol::get(c"NSTextInputClient").expect(
        "BUG: NSTextInputClient is a protocol every macOS this backend supports provides; \
         a None here means the protocol name or the declared deployment floor is wrong",
    );
    decl.add_protocol(protocol);

    // SAFETY: per this function's contract — each `extern "C" fn` below matches
    // its selector's argument and return types (`v@:{_NSRange=QQ}`,
    // `{_NSRange=QQ}@:`, `@@:`, `Q@:{CGPoint=dd}`,
    // `@@:{_NSRange=QQ}^{_NSRange=QQ}`,
    // `{CGRect={CGPoint=dd}{CGSize=dd}}@:{_NSRange=QQ}^{_NSRange=QQ}`, `v@::`).
    // The one encoding that is not the same on every target is `hasMarkedText`'s
    // BOOL — `B@:` where BOOL is `bool` (arm64) and `c@:` where it is
    // `signed char` (x86_64); `objc`'s own `Encode for BOOL` is what supplies
    // whichever applies.
    unsafe {
        decl.add_method(
            sel!(insertText:replacementRange:),
            insert_text as extern "C-unwind" fn(_, _, *mut AnyObject, NSRange),
        );
        decl.add_method(
            sel!(setMarkedText:selectedRange:replacementRange:),
            set_marked_text as extern "C-unwind" fn(_, _, *mut AnyObject, NSRange, NSRange),
        );
        decl.add_method(sel!(unmarkText), unmark_text as extern "C-unwind" fn(_, _));
        decl.add_method(
            sel!(hasMarkedText),
            has_marked_text as extern "C-unwind" fn(_, _) -> Bool,
        );
        decl.add_method(
            sel!(markedRange),
            marked_range as extern "C-unwind" fn(_, _) -> NSRange,
        );
        decl.add_method(
            sel!(selectedRange),
            selected_range as extern "C-unwind" fn(_, _) -> NSRange,
        );
        decl.add_method(
            sel!(validAttributesForMarkedText),
            valid_attributes_for_marked_text as extern "C-unwind" fn(_, _) -> *mut AnyObject,
        );
        decl.add_method(
            sel!(attributedSubstringForProposedRange:actualRange:),
            attributed_substring_for_proposed_range
                as extern "C-unwind" fn(_, _, NSRange, *mut NSRange) -> *mut AnyObject,
        );
        decl.add_method(
            sel!(firstRectForCharacterRange:actualRange:),
            first_rect_for_character_range
                as extern "C-unwind" fn(_, _, NSRange, *mut NSRange) -> NSRect,
        );
        decl.add_method(
            sel!(characterIndexForPoint:),
            character_index_for_point as extern "C-unwind" fn(_, _, NSPoint) -> NSUInteger,
        );
        decl.add_method(
            sel!(doCommandBySelector:),
            do_command_by_selector as extern "C-unwind" fn(_, _, _),
        );
    }
}

// ============================================================================
// MacOSTextInput — the PlatformTextInput capability
// ============================================================================

/// One macOS window's IME capability.
///
/// The methods are the only part of this module reachable off the main thread:
/// [`PlatformTextInput`] is a public `Send + Sync` capability, so both travel
/// through [`route_on_owner`] (the standing rule for every AppKit-messaging
/// window site) even though the state they touch is main-thread-only by AppKit's
/// delivery contract.
pub(super) struct MacOSTextInput {
    /// The window whose content view owns the composition state.
    ///
    /// Not retained: the capability borrows the window it was discovered from,
    /// as every other per-window capability on this backend does. `closed` is
    /// what keeps a late call safe.
    ns_window: *mut AnyObject,

    /// Set once the window has closed (see `MacOSWindow`): the same liveness
    /// gate `HasWindowHandle::window_handle` consults, so a capability clone
    /// retained past its window's close is a no-op rather than a message to an
    /// NSWindow AppKit has already been told to tear down.
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,

    /// The owner lane every AppKit message here travels through.
    owner: &'static dispatch::Queue,

    /// True when `owner` is the main queue, so "on the main thread" is
    /// equivalent to "on the owner lane".
    owner_is_main: bool,
}

// SAFETY: the same argument as `MacOSWindow`'s own `unsafe impl Send`/`Sync`:
// `ns_window` is a raw AppKit pointer that is only ever messaged from inside
// `route_on_owner`, which runs the body on the window's owner lane; the
// remaining fields are `Arc`/`&'static`/`bool`. The residual — a capability
// clone outliving its window — is why `closed` is consulted in both bodies: the
// window's close is what makes further AppKit contact meaningless, and a
// capability whose window was dropped *without* closing retains the same
// window-clone contract every other capability in this backend has (the
// window's own `Drop` tail owns the release).
unsafe impl Send for MacOSTextInput {}
// SAFETY: see `Send` above; there is no interior mutability here at all — every
// mutation happens to the view's `RefCell` from inside the routed body.
unsafe impl Sync for MacOSTextInput {}

impl MacOSTextInput {
    pub(super) fn new(
        ns_window: *mut AnyObject,
        closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
        owner: &'static dispatch::Queue,
        owner_is_main: bool,
    ) -> Self {
        Self {
            ns_window,
            closed,
            owner,
            owner_is_main,
        }
    }

    /// Whether this capability's window has closed.
    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Run `body` against the window's live content view.
    ///
    /// The content view is fetched per call rather than cached: `liquid_glass`
    /// replaces it (and `clear_liquid_glass` replaces it again with a fresh
    /// `FLUIContentView` holding fresh state), so a cached id would be stale
    /// after either call.
    fn with_content_view<R: Send>(&self, body: impl FnOnce(&ViewContext) -> R + Send) -> Option<R> {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, move || {
            if this.is_closed() {
                tracing::trace!("text input call on a closed window; dropped");
                return None;
            }
            // SAFETY: `ns_window` is alive for the lifetime of the window this
            // capability was discovered from, and the body runs on the owner
            // thread — inline on the OS main thread for a main-lane owner, or
            // dispatched onto the lane under the reentrancy guard — before the
            // messages are sent. `content_view` is the window's own, and
            // `with_view_context` verifies it is a FLUIContentView before reading
            // the context ivar (the content view is an NSVisualEffectView while
            // liquid glass is applied, and carries no such ivar at all).
            unsafe {
                let content_view: *mut AnyObject = msg_send![this.ns_window, contentView];
                super::view::with_view_context(content_view, body)
            }
        })
    }
}

impl PlatformTextInput for MacOSTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        // The gate and the announcement travel in one owner-lane round trip:
        // `route_on_owner` blocks the caller until the body returns, so a
        // second hop here would be a second blocking round trip for no gain.
        let reached = self.with_content_view(|ctx| {
            {
                let mut state = ctx.text_input.borrow_mut();
                state.ime_allowed = allowed;
                if !allowed {
                    // The trait's documented semantics for a disable: an
                    // in-progress composition is dropped, not committed.
                    state.clear_marked_text();
                }
            }
            // Announced after the borrow is released — dispatch re-enters
            // arbitrary application code (the widget-side text input), which
            // is free to call back in. Through the same callbacks every other
            // input event takes.
            let event = if allowed {
                ImeEvent::Enabled
            } else {
                ImeEvent::Disabled
            };
            emit_ime(ctx, event);
        });
        if reached.is_none() {
            tracing::trace!("IME state changed but the window has no live content view");
        }
    }

    fn set_ime_cursor_area(&self, area: Bounds<Pixels>) {
        let stored = self.with_content_view(|ctx| {
            ctx.text_input.borrow_mut().cursor_area = Some(area);
        });
        if stored.is_none() {
            tracing::trace!("IME cursor area dropped: the window has no live content view");
        }
    }
}

impl std::fmt::Debug for MacOSTextInput {
    // Hand-written for the same reason `MacOSWindow`'s is: the NSWindow pointer
    // and the dispatch lane have no useful Debug form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOSTextInput")
            .field("ns_window", &(self.ns_window as usize))
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{NSNotFound, TextInputState, utf16_len, utf16_range_to_byte_range};

    #[test]
    fn ascii_offsets_are_their_own_byte_offsets() {
        assert_eq!(utf16_range_to_byte_range("hello", 0, 0), Some((0, 0)));
        assert_eq!(utf16_range_to_byte_range("hello", 4, 0), Some((4, 4)));
        assert_eq!(utf16_range_to_byte_range("hello", 1, 3), Some((1, 4)));
        assert_eq!(utf16_range_to_byte_range("hello", 5, 0), Some((5, 5)));
    }

    #[test]
    fn multibyte_text_diverges_from_utf16_offsets() {
        // "héllo": five UTF-16 units but six bytes — `é` is one unit, two bytes.
        assert_eq!(utf16_len("héllo"), 5);
        assert_eq!(utf16_range_to_byte_range("héllo", 2, 0), Some((3, 3)));
        assert_eq!(utf16_range_to_byte_range("héllo", 1, 2), Some((1, 4)));
        assert_eq!(utf16_range_to_byte_range("héllo", 5, 0), Some((6, 6)));
    }

    #[test]
    fn cjk_text_counts_units_not_bytes() {
        // "你好" is two units and six bytes.
        assert_eq!(utf16_range_to_byte_range("你好", 1, 1), Some((3, 6)));
        assert_eq!(utf16_range_to_byte_range("你好", 0, 2), Some((0, 6)));
    }

    #[test]
    fn an_offset_inside_a_surrogate_pair_has_no_byte_range() {
        // "\u{1F44B}" (waving hand) is one char, two UTF-16 units, four bytes,
        // so unit 1 falls inside the pair.
        assert_eq!(utf16_len("\u{1F44B}"), 2);
        assert_eq!(utf16_range_to_byte_range("\u{1F44B}", 1, 0), None);
        assert_eq!(utf16_range_to_byte_range("a\u{1F44B}b", 2, 1), None);
        // The boundaries on either side of it are still expressible.
        assert_eq!(utf16_range_to_byte_range("a\u{1F44B}b", 0, 1), Some((0, 1)));
        assert_eq!(utf16_range_to_byte_range("a\u{1F44B}b", 1, 2), Some((1, 5)));
        assert_eq!(utf16_range_to_byte_range("a\u{1F44B}b", 3, 1), Some((5, 6)));
    }

    #[test]
    fn a_range_past_the_end_is_none() {
        assert_eq!(utf16_range_to_byte_range("hello", 6, 0), None);
        assert_eq!(utf16_range_to_byte_range("hello", 0, 6), None);
        assert_eq!(utf16_range_to_byte_range("", 0, 1), None);
    }

    #[test]
    fn empty_text_has_exactly_one_boundary() {
        assert_eq!(utf16_range_to_byte_range("", 0, 0), Some((0, 0)));
        assert_eq!(utf16_range_to_byte_range("", 1, 0), None);
    }

    #[test]
    fn appkits_not_found_location_is_rejected_not_wrapped() {
        // `{NSNotFound, 0}`, as `selectedRange` reports when it has none: the
        // end offset must not overflow into a small valid one. The location is
        // the real constant AppKit sends (`NSIntegerMax`), not a convenient
        // large number — a near-`usize::MAX` value would overflow on the
        // `checked_add` in the same way but would not pin the actual wire value.
        assert_eq!(
            utf16_range_to_byte_range("hello", NSNotFound as usize, 0),
            None
        );
        assert_eq!(
            utf16_range_to_byte_range("hello", NSNotFound as usize, 1),
            None
        );
        assert_eq!(utf16_range_to_byte_range("hello", 4, usize::MAX), None);
    }

    /// The `keyUp:` gate. Each case is a state a real sequence produces, and the
    /// middle two are the pair that a gate on `ime_allowed` alone would get
    /// wrong — which is the whole reason the predicate reads the composition
    /// instead.
    #[test]
    fn a_key_release_is_suppressed_only_by_an_open_composition() {
        // No text input attached: every window until a presentation attaches
        // one, and the state `key_down` leaves byte-identical to the pre-IME
        // keyboard path.
        let idle = TextInputState::default();
        assert!(idle.reports_key_release());

        // Attached, between compositions: a committed Latin character. The
        // release must still be reported, and this is the case a gate on
        // `ime_allowed` alone would drop.
        let attached = TextInputState {
            ime_allowed: true,
            ..TextInputState::default()
        };
        assert!(attached.reports_key_release());

        // Attached, mid-composition: the press was consumed by the input
        // method, so the release describes a key the application never saw go
        // down.
        let composing = TextInputState {
            ime_allowed: true,
            marked_text: "にほ".to_string(),
            marked_range: (0, 2),
            selected_range: (2, 0),
            ..TextInputState::default()
        };
        assert!(!composing.reports_key_release());

        // A composition that ended — by commit or by `unmarkText` — puts the
        // release back on the keyboard path.
        let mut ended = composing;
        ended.clear_marked_text();
        assert!(ended.reports_key_release());
    }
}
