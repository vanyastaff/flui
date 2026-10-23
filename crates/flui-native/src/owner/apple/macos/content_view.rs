//! The content view of every AppKit window: `FLUIContentView`, the `NSView`
//! subclass each window installs as its content view and first responder.
//!
//! The class is the owner's, as FLUI's Win32 window class is (ADR-0151 §10):
//! several features answer what reaches it, so it names none of them. Each
//! view keeps its window's callbacks and the parts the host's table made for
//! it ([`ViewHooks`]): the input ([`ViewInput`]), which converts the mouse,
//! trackpad and keyboard events, and the text input ([`ViewText`]), which
//! keeps the composition the `NSTextInputClient` methods report and update.
//! Each part keeps its state of the window in itself, one per view, made with
//! the view and dropped with it.
//!
//! The view routes a key press between them: to the input method
//! (`interpretKeyEvents:`) while the text input composes, to the input
//! otherwise, and a key the input method declines (`doCommandBySelector:`)
//! back to the input; a release that lands inside an open composition reaches
//! neither. Its `drawRect:` asks the window for a frame inside a display pass
//! ([`display_pass`](super::display_pass)), and it reports its hover and its
//! appearance through the window's callbacks.
//!
//! The class is built with objc2's imperative `ClassBuilder` rather than the
//! declarative `define_class!` macro: its ivar holds a boxed context of Rust
//! values the macro's ivars cannot name.

// AppKit's classes are reached through Objective-C messages, which have no
// safe wrapper; every `unsafe` block states the invariant it relies on.
#![expect(unsafe_code)]

use std::cell::Cell;
use std::ffi::CStr;
use std::sync::Weak;

use flui_foundation::geometry::Bounds;
use flui_platform::{PlatformInput, WindowAppearance};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel};
use objc2::{ClassType, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSEvent,
    NSView as NSViewClass,
};
use objc2_foundation::{NSArray, NSNotFound, NSPoint, NSRange, NSRect, NSSize, NSUInteger};

use crate::owner::handlers::WindowCallbacks;

/// What the host's table makes for each content view: its input and its text
/// input, each one per view.
#[derive(Clone, Copy)]
pub(crate) struct ViewHooks {
    /// The input of a view made now.
    pub(crate) input: fn() -> Box<dyn ViewInput>,
    /// The text input of a view made now.
    pub(crate) text: fn() -> Box<dyn ViewText>,
}

impl std::fmt::Debug for ViewHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewHooks").finish_non_exhaustive()
    }
}

/// The input's part of one content view: the events AppKit delivers to it.
pub(crate) trait ViewInput {
    /// `event` reached `view` (a mouse, trackpad or keyboard event, or a key
    /// the input method declined): convert it and dispatch what it carries
    /// through the view.
    fn event(&self, view: &ContentView<'_>, event: &NSEvent);
}

/// The text input's part of one content view: the composition AppKit's input
/// method drives through the view's `NSTextInputClient` methods, and the
/// window's text-input capability (`PlatformTextInput`), which reaches it on
/// the owner lane.
pub(crate) trait ViewText {
    /// Whether a key press goes to the input method: a text input is
    /// attached and may compose. `false` until a presentation attaches one,
    /// so a window without one takes every key as a key event.
    fn takes_key_presses(&self) -> bool;

    /// Whether a key release still belongs to the input: `false` exactly
    /// while a composition is open, since the press that opened it went to
    /// the input method.
    fn reports_key_release(&self) -> bool;

    /// The input method committed `text` (`insertText:replacementRange:`).
    fn commit(&self, view: &ContentView<'_>, text: String);

    /// The composition is now `text`, with the caret at the UTF-16 range
    /// `selected` within it (`setMarkedText:selectedRange:replacementRange:`).
    fn compose(&self, view: &ContentView<'_>, text: String, selected: (usize, usize));

    /// The composition was abandoned (`unmarkText`).
    fn unmark(&self, view: &ContentView<'_>);

    /// The composition's UTF-16 range; `None` when nothing is marked.
    fn marked_range(&self) -> Option<(usize, usize)>;

    /// The caret's UTF-16 range within the composition; `None` when nothing
    /// is marked.
    fn selected_range(&self) -> Option<(usize, usize)>;

    /// Where an input method places its candidates: the caret's area in the
    /// window's logical coordinates (top-left origin), and the UTF-16 range
    /// it corresponds to; `None` while no area is set.
    fn candidate_area(&self) -> Option<(Bounds<f64>, (usize, usize))>;

    /// Whether the focused field may compose (`set_ime_allowed`); a field
    /// that may not drops its composition.
    fn set_composing(&self, view: &ContentView<'_>, allowed: bool);

    /// The caret's area, in the window's logical coordinates
    /// (`set_ime_cursor_area`).
    fn set_caret_area(&self, area: Bounds<f64>);
}

/// One live content view, as the parts it dispatches to reach it.
pub(crate) struct ContentView<'a> {
    view: &'a AnyObject,
    context: &'a ViewContext,
}

impl ContentView<'_> {
    /// Dispatch `input` through the window's callbacks: the one throat every
    /// event of the view takes, so the text input's are ordered against the
    /// pointer's and the keyboard's.
    pub(crate) fn dispatch_input(&self, input: PlatformInput) {
        if let Some(callbacks) = self.context.callbacks.upgrade() {
            let _result = callbacks.dispatch_input(input);
        } else {
            tracing::trace!("Input event received after window callbacks were dropped");
        }
    }

    /// The view's height, which AppKit's bottom-left coordinates are flipped
    /// by.
    pub(crate) fn height(&self) -> f64 {
        // SAFETY: `view` is a live FLUIContentView on the main thread; `bounds`
        // is a plain NSRect getter.
        let bounds: NSRect = unsafe { msg_send![self.view, bounds] };
        bounds.size.height
    }

    /// The view's text input.
    pub(crate) fn text(&self) -> &dyn ViewText {
        &*self.context.text
    }
}

/// The ivar name holding the boxed [`ViewContext`].
const CONTEXT_IVAR: &CStr = c"context_ptr";

/// What one content view keeps, in its ivar, from its creation to its
/// `dealloc`.
struct ViewContext {
    callbacks: Weak<WindowCallbacks>,
    input: Box<dyn ViewInput>,
    text: Box<dyn ViewText>,
    /// The `keyDown:` event inside `interpretKeyEvents:`, null otherwise: what
    /// `doCommandBySelector:`, which the input method calls during it, hands
    /// back to the input.
    pending_key_event: Cell<*mut AnyObject>,
    /// The appearance last reported to `on_appearance_changed`; AppKit also
    /// re-resolves the effective appearance for changes that keep the theme,
    /// such as the accent colour.
    appearance: Cell<WindowAppearance>,
}

/// Create the content view of a window whose callbacks are `callbacks`, with
/// the parts `hooks` make: it becomes the window's content view and first
/// responder, and receives its keyboard, mouse and scroll events.
pub(crate) fn create(
    frame: NSRect,
    callbacks: Weak<WindowCallbacks>,
    hooks: ViewHooks,
) -> *mut AnyObject {
    // SAFETY: FLUIContentView is registered before alloc/init; the boxed
    // ViewContext pointer is stored in the view's ivar and released in
    // `dealloc`, so it lives exactly as long as the view.
    unsafe {
        let class = view_class();
        let view: *mut AnyObject = msg_send![class, alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: frame];

        let context = Box::into_raw(Box::new(ViewContext {
            callbacks,
            input: (hooks.input)(),
            text: (hooks.text)(),
            pending_key_event: Cell::new(std::ptr::null_mut()),
            appearance: Cell::new(effective_appearance(as_view(&*view))),
        }))
        .cast::<std::ffi::c_void>();

        // SAFETY: `view` is a live FLUIContentView just allocated above, so it
        // carries `CONTEXT_IVAR`; the value is a pointer this function owns.
        let ivar = class
            .instance_variable(CONTEXT_IVAR)
            .expect("BUG: FLUIContentView declares the context ivar");
        let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(&*view);
        *slot = context;

        view
    }
}

/// The light/dark half of `object`'s effective appearance: the system theme,
/// unless the app or a window overrides it.
pub(crate) fn effective_appearance(
    object: &(impl NSAppearanceCustomization + objc2::Message),
) -> WindowAppearance {
    // SAFETY: AppKit exports both names as constant strings that live for the
    // process; reading the extern statics only copies those references.
    let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark]);
    match object
        .effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)
    {
        Some(name) if &*name == dark => WindowAppearance::Dark,
        _ => WindowAppearance::Light,
    }
}

/// `this`, a FLUIContentView, as the NSView it subclasses.
fn as_view(this: &AnyObject) -> &NSViewClass {
    this.downcast_ref::<NSViewClass>()
        .expect("BUG: FLUIContentView subclasses NSView")
}

/// Run `body` with the content view `this` and its context.
///
/// # Safety
///
/// `this` must be a live FLUIContentView whose `context_ptr` ivar is either
/// null or points to a `ViewContext` owned by that view: the receiver of one
/// of the class's own methods.
unsafe fn with_this<R>(this: &AnyObject, body: impl FnOnce(&ContentView<'_>) -> R) -> Option<R> {
    // SAFETY: per the function contract the ivar is null or a valid
    // Box<ViewContext> pointer owned by the view; the reference is scoped to
    // `body`, which runs inside the method invocation that holds `this`.
    unsafe {
        let class = AnyClass::get(c"FLUIContentView")?;
        let ivar = class.instance_variable(CONTEXT_IVAR)?;
        let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(this);
        let context = (*slot).cast::<ViewContext>().cast_const().as_ref()?;
        Some(body(&ContentView {
            view: this,
            context,
        }))
    }
}

/// Run `body` with the content view `view`, a view fetched from somewhere
/// other than one of the class's methods; `None` for anything that is not a
/// view this class built, so a caller needs no check of its own.
///
/// The class check is the whole point: an `NSVisualEffectView` (the window's
/// content view while Liquid Glass is applied) has no `context_ptr` ivar at
/// all, and reading it would be undefined rather than returning null. The
/// view is handed to `body` rather than returned, so the borrow cannot
/// outlive the call that established the view was alive.
///
/// # Safety
///
/// `view` must be null or a live `NSView*`, on the main thread.
pub(crate) unsafe fn with_content_view<R>(
    view: *mut AnyObject,
    body: impl FnOnce(&ContentView<'_>) -> R,
) -> Option<R> {
    // SAFETY: per the function contract `view` is null or a live NSView;
    // `isKindOfClass:` is asked before the ivar is touched, and a view that
    // answers yes is one `create` built, whose `context_ptr` is either null or
    // a `ViewContext` it owns.
    unsafe {
        let view = view.as_ref()?;
        let is_content_view: Bool = msg_send![view, isKindOfClass: view_class()];
        if is_content_view != Bool::YES {
            return None;
        }
        with_this(view, body)
    }
}

// ============================================================================
// Input routing
// ============================================================================

/// An input `NSEvent` reached the view: the input converts and dispatches it.
extern "C-unwind" fn input_event(this: &AnyObject, _sel: Sel, event: *mut AnyObject) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes a method on
    // an object that declares it); `event` is a valid NSEvent* for the
    // duration of the call.
    unsafe {
        let Some(event) = event.cast::<NSEvent>().as_ref() else {
            return;
        };
        with_this(this, |view| view.context.input.event(view, event));
    }
}

/// `keyDown:` — the input method's route while the text input composes, the
/// input's otherwise.
///
/// The gate's default is the load-bearing part: until a presentation attaches
/// a text input and lets it compose, every press goes straight to the input.
/// Only while one may compose does the press take AppKit's input method,
/// through `interpretKeyEvents:`, which comes back into the view's
/// `NSTextInputClient` methods (`setMarkedText:` while composing,
/// `insertText:` on commit); a press delivered to both routes would reach the
/// application twice. `doCommandBySelector:` is the way back for the keys the
/// input method declines.
///
/// The pending event is saved and restored rather than set and cleared: an
/// input method can route a second `keyDown:` through the view while this one
/// is inside `interpretKeyEvents:` (the character palette does), and a plain
/// clear would leave the outer event's `doCommandBySelector:` reading null and
/// dropping a command that belongs to this press.
extern "C-unwind" fn key_down(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    // SAFETY: `this` is a live FLUIContentView; `event` is a valid NSEvent* for
    // the duration of the call, and `interpretKeyEvents:` retains it for the
    // span of its own dispatch. The array is created by an NSArray class
    // constructor and autoreleased.
    unsafe {
        let composes = with_this(this, |view| view.context.text.takes_key_presses());
        if composes != Some(true) {
            input_event(this, sel, event);
            return;
        }
        let previous = with_this(this, |view| view.context.pending_key_event.replace(event));
        let events: *mut AnyObject = msg_send![objc2::class!(NSArray), arrayWithObject: event];
        let _: () = msg_send![this, interpretKeyEvents: events];
        if let Some(previous) = previous {
            with_this(this, |view| view.context.pending_key_event.set(previous));
        }
    }
}

/// `keyUp:` — the input's, except while a composition is open: the press was
/// the input method's, so a release reported then describes a key the
/// application never saw go down. A commit closes the composition before the
/// release arrives, so a typed character still reports its release.
extern "C-unwind" fn key_up(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
    // live objects).
    let reported = unsafe { with_this(this, |view| view.context.text.reports_key_release()) };
    if reported != Some(false) {
        input_event(this, sel, event);
    }
}

/// Dispatch a hover status change through the window's callbacks.
fn dispatch_hover_change(this: &AnyObject, is_hovered: bool) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
    // live objects).
    unsafe {
        with_this(this, |view| {
            if let Some(callbacks) = view.context.callbacks.upgrade() {
                callbacks.dispatch_hover_status_change(is_hovered);
            }
        });
    }
}

/// `mouseEntered:` — report hover gained, then forward the pointer event.
extern "C-unwind" fn mouse_entered(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    dispatch_hover_change(this, true);
    input_event(this, sel, event);
}

/// `mouseExited:` — report hover lost, then forward the pointer event.
extern "C-unwind" fn mouse_exited(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    dispatch_hover_change(this, false);
    input_event(this, sel, event);
}

// ============================================================================
// Drawing and appearance
// ============================================================================

/// `drawRect:` — AppKit asks for content: the window's frame request.
///
/// `request_redraw()` marks the view dirty via `setNeedsDisplay:`; AppKit then
/// calls this method on the next display pass, which is where the per-window
/// `on_request_frame` contract fires (the macOS analogue of the Win32
/// window's `WM_PAINT`). The frame runs inside this callback, so a redraw the
/// frame asks for is asked for while AppKit is displaying this view — the one
/// moment a `setNeedsDisplay:` is discarded; the display pass marks this
/// thread for the duration, so the window defers only that case.
extern "C-unwind" fn draw_rect(this: &AnyObject, _sel: Sel, dirty_rect: NSRect) {
    // SAFETY: `this` is a live FLUIContentView; the super `drawRect:` message
    // is the documented NSView handling of the dirty region.
    unsafe {
        let _display_pass = super::display_pass::DisplayPassGuard::enter();
        let callbacks = with_this(this, |view| view.context.callbacks.upgrade()).flatten();
        if let Some(callbacks) = callbacks {
            tracing::trace!("FLUIContentView drawRect: dispatching frame request");
            callbacks.dispatch_request_frame();
        } else {
            // The window's callbacks are gone (torn down, or the view outlived
            // its window). AppKit still owns this display pass, so the super
            // call below runs either way — but a missing callback here is why a
            // frame that AppKit asked for produced no frame request, which is
            // otherwise indistinguishable from AppKit never asking at all.
            tracing::trace!("FLUIContentView drawRect: no live callbacks; frame request dropped");
        }

        let _: () = msg_send![super(this, NSViewClass::class()), drawRect: dirty_rect];
    }
}

/// `viewDidChangeEffectiveAppearance` — AppKit re-resolved this view's
/// appearance: the system theme flipped, or something that keeps it did.
extern "C-unwind" fn view_did_change_effective_appearance(this: &AnyObject, _sel: Sel) {
    // SAFETY: `this` is a live FLUIContentView, messaged by AppKit on the main
    // thread; the super call is NSView's own (empty) implementation.
    unsafe {
        let _: () = msg_send![
            super(this, NSViewClass::class()),
            viewDidChangeEffectiveAppearance
        ];
        let appearance = effective_appearance(as_view(this));
        let callbacks = with_this(this, |view| {
            (view.context.appearance.replace(appearance) != appearance)
                .then(|| view.context.callbacks.upgrade())
                .flatten()
        })
        .flatten();
        if let Some(callbacks) = callbacks {
            callbacks.dispatch_appearance_changed();
        }
    }
}

// ============================================================================
// NSTextInputClient
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

/// The range AppKit is answered with for "nothing there".
fn not_found() -> NSRange {
    NSRange::new(NSNotFound as usize, 0)
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

/// `insertText:replacementRange:` — the input method committed text.
extern "C-unwind" fn insert_text(
    this: &AnyObject,
    _sel: Sel,
    string: *mut AnyObject,
    _replacement_range: NSRange,
) {
    // SAFETY: `this` is a live FLUIContentView; `string` is the live argument
    // for the duration of the call, and its bytes are copied before this
    // function returns.
    unsafe {
        let Some(text) = insert_text_argument_string(string) else {
            tracing::trace!("insertText: carried no readable string; commit dropped");
            return;
        };
        with_this(this, |view| view.context.text.commit(view, text));
    }
}

/// `setMarkedText:selectedRange:replacementRange:` — the composition changed.
/// `selectedRange` is the caret's position within the composition.
extern "C-unwind" fn set_marked_text(
    this: &AnyObject,
    _sel: Sel,
    string: *mut AnyObject,
    selected_range: NSRange,
    _replacement_range: NSRange,
) {
    // SAFETY: `this` is a live FLUIContentView; `string` is the live argument
    // for the duration of the call, copied out immediately.
    unsafe {
        let text = ns_string_to_owned(string).unwrap_or_default();
        let selected = (selected_range.location, selected_range.length);
        with_this(this, |view| view.context.text.compose(view, text, selected));
    }
}

/// `unmarkText` — the composition was abandoned; nothing is committed.
extern "C-unwind" fn unmark_text(this: &AnyObject, _sel: Sel) {
    // SAFETY: `this` is a live FLUIContentView.
    unsafe {
        with_this(this, |view| view.context.text.unmark(view));
    }
}

/// `hasMarkedText` — is a composition in progress?
extern "C-unwind" fn has_marked_text(this: &AnyObject, _sel: Sel) -> Bool {
    // SAFETY: `this` is a live FLUIContentView.
    let marked = unsafe { with_this(this, |view| view.context.text.marked_range().is_some()) };
    Bool::new(marked == Some(true))
}

/// `markedRange` — the range of the composition, in UTF-16 units.
extern "C-unwind" fn marked_range(this: &AnyObject, _sel: Sel) -> NSRange {
    // SAFETY: `this` is a live FLUIContentView.
    unsafe { with_this(this, |view| view.context.text.marked_range()) }
        .flatten()
        .map_or_else(not_found, |(location, length)| {
            NSRange::new(location, length)
        })
}

/// `selectedRange` — the caret's range within the composition; `{NSNotFound,
/// 0}` outside one: a view with no document of its own cannot truthfully name
/// a selection the application has not reported, and AppKit documents that
/// pair as "no selection".
extern "C-unwind" fn selected_range(this: &AnyObject, _sel: Sel) -> NSRange {
    // SAFETY: `this` is a live FLUIContentView.
    unsafe { with_this(this, |view| view.context.text.selected_range()) }
        .flatten()
        .map_or_else(not_found, |(location, length)| {
            NSRange::new(location, length)
        })
}

/// `validAttributesForMarkedText` — empty: the composing text is rendered by
/// the framework's own client, not by AppKit's text system, so an attributed
/// composition has nothing to describe.
extern "C-unwind" fn valid_attributes_for_marked_text(
    _this: &AnyObject,
    _sel: Sel,
) -> *mut AnyObject {
    // SAFETY: `+[NSArray array]` is a class constructor on a class AppKit
    // always provides; it returns an autoreleased empty array.
    unsafe { msg_send![objc2::class!(NSArray), array] }
}

/// `attributedSubstringForProposedRange:actualRange:` — `nil`, with
/// `actualRange` answered `{NSNotFound, 0}`: the view holds no attributed text
/// to slice, and that is the documented answer for a range that cannot be
/// satisfied.
extern "C-unwind" fn attributed_substring_for_proposed_range(
    _this: &AnyObject,
    _sel: Sel,
    _range: NSRange,
    actual_range: *mut NSRange,
) -> *mut AnyObject {
    // SAFETY: `actual_range` is AppKit's own out-parameter for this call —
    // null or a valid writable `NSRange*`.
    unsafe { write_actual_range(actual_range, not_found()) };
    std::ptr::null_mut()
}

/// `firstRectForCharacterRange:actualRange:` — where to place candidates: the
/// text input's caret area, converted to screen coordinates. The Y axis is
/// flipped on the way in: the framework's window coordinates have their
/// origin at the top-left, AppKit's at the bottom-left, and without the flip
/// the candidate window would sit mirrored about the window's vertical
/// centre.
extern "C-unwind" fn first_rect_for_character_range(
    this: &AnyObject,
    _sel: Sel,
    _range: NSRange,
    actual_range: *mut NSRange,
) -> NSRect {
    // SAFETY: `this` is a live FLUIContentView; `actual_range` is AppKit's own
    // out-parameter; `window` is AppKit's own getter on that live view, and
    // the window is nil-checked before `convertRectToScreen:` — a view with no
    // window (mid-teardown) has no screen rectangle to answer with.
    unsafe {
        write_actual_range(actual_range, not_found());
        let Some((area, corresponding, height)) = with_this(this, |view| {
            view.context
                .text
                .candidate_area()
                .map(|(area, corresponding)| (area, corresponding, view.height()))
        })
        .flatten() else {
            return zero_rect();
        };
        let flipped_y = height - (area.origin.y + area.size.height);
        let window_rect = NSRect::new(
            NSPoint::new(area.origin.x, flipped_y),
            NSSize::new(area.size.width, area.size.height),
        );
        let window: *mut AnyObject = msg_send![this, window];
        if window.is_null() {
            return zero_rect();
        }
        let screen_rect: NSRect = msg_send![window, convertRectToScreen: window_rect];
        write_actual_range(actual_range, NSRange::new(corresponding.0, corresponding.1));
        screen_rect
    }
}

/// `characterIndexForPoint:` — `NSNotFound`: the view owns no text, so it can
/// answer no index into one.
extern "C-unwind" fn character_index_for_point(
    _this: &AnyObject,
    _sel: Sel,
    _point: NSPoint,
) -> NSUInteger {
    NSNotFound as NSUInteger
}

/// `doCommandBySelector:` — a key the input method declined goes back to the
/// input, exactly the path `keyDown:` would have taken without a composing
/// text input. Outside an `interpretKeyEvents:` call (menu-key equivalents)
/// there is no key press to convert, so the command is dropped rather than
/// invented.
extern "C-unwind" fn do_command_by_selector(this: &AnyObject, _sel: Sel, _command: Sel) {
    // SAFETY: `this` is a live FLUIContentView; the pending event is either
    // null or an `NSEvent*` AppKit keeps alive across the `interpretKeyEvents:`
    // call whose dispatch is on this stack frame, the only window in which
    // this callback runs.
    unsafe {
        let pending = with_this(this, |view| view.context.pending_key_event.get())
            .unwrap_or(std::ptr::null_mut());
        if pending.is_null() {
            tracing::trace!("doCommandBySelector: outside a pending key press; command dropped");
            return;
        }
        input_event(this, sel!(keyDown:), pending);
    }
}

// ============================================================================
// The class
// ============================================================================

/// `FLUIContentView`, registered the first time a view is made.
fn view_class() -> &'static AnyClass {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = NSViewClass::class();
        let mut builder = ClassBuilder::new(c"FLUIContentView", superclass)
            .expect("FLUIContentView must be registered exactly once (guarded by Once)");

        // The boxed ViewContext, as a raw `*mut c_void`.
        builder.add_ivar::<*mut std::ffi::c_void>(CONTEXT_IVAR);

        extern "C-unwind" fn yes(_this: &AnyObject, _sel: Sel) -> Bool {
            Bool::YES
        }

        // `flagsChanged:` — modifier state is carried on every converted
        // event, so flag-only transitions are observed but not dispatched.
        extern "C-unwind" fn flags_changed(_this: &AnyObject, _sel: Sel, _event: *mut AnyObject) {
            tracing::trace!("Modifier flags changed");
        }

        extern "C-unwind" fn dealloc(this: &AnyObject, _sel: Sel) {
            // SAFETY: the ivar holds either null or a Box<ViewContext> leaked
            // in `create`; reclaiming it here (exactly once, on dealloc) is the
            // matching release. The super dealloc message is the mandatory
            // NSObject teardown.
            unsafe {
                let class = AnyClass::get(c"FLUIContentView")
                    .expect("BUG: dealloc only runs on a registered FLUIContentView");
                let ivar = class
                    .instance_variable(CONTEXT_IVAR)
                    .expect("BUG: FLUIContentView declares the context ivar");
                let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(this);
                let context = std::mem::replace(&mut *slot, std::ptr::null_mut());
                if !context.is_null() {
                    drop(Box::from_raw(context.cast::<ViewContext>()));
                }

                let _: () = msg_send![super(this, NSViewClass::class()), dealloc];
            }
        }

        // Conformance, not just the methods: AppKit's `NSTextInputContext`
        // asks `conformsToProtocol:` before it routes a key event through the
        // input method at all, so a class with these methods and no protocol
        // is never called. The protocol is declared since 10.5, below this
        // backend's floor, so a `None` here can only mean the name is wrong.
        let text_input_client = AnyProtocol::get(c"NSTextInputClient")
            .expect("BUG: NSTextInputClient is a protocol every supported macOS provides");
        builder.add_protocol(text_input_client);

        // SAFETY: every registered function pointer matches the Objective-C
        // method signature of its selector (`&AnyObject, Sel` plus the
        // declared argument/return types), as `ClassBuilder::add_method`
        // requires; the `NSTextInputClient` ones are fixed by the protocol's
        // declaration in AppKit. `hasMarkedText`'s BOOL is `bool` on arm64 and
        // `signed char` on x86_64; objc2's `Bool` encodes whichever applies.
        unsafe {
            builder.add_method(
                sel!(acceptsFirstResponder),
                yes as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(
                sel!(becomeFirstResponder),
                yes as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(
                sel!(resignFirstResponder),
                yes as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(sel!(isOpaque), yes as extern "C-unwind" fn(_, _) -> Bool);
            builder.add_method(
                sel!(acceptsTouchEvents),
                yes as extern "C-unwind" fn(_, _) -> Bool,
            );

            builder.add_method(
                sel!(keyDown:),
                key_down as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                sel!(keyUp:),
                key_up as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                sel!(flagsChanged:),
                flags_changed as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            for event in [
                sel!(mouseDown:),
                sel!(mouseUp:),
                sel!(mouseMoved:),
                sel!(mouseDragged:),
                sel!(rightMouseDown:),
                sel!(rightMouseUp:),
                sel!(rightMouseDragged:),
                sel!(otherMouseDown:),
                sel!(otherMouseUp:),
                sel!(otherMouseDragged:),
                sel!(scrollWheel:),
                sel!(magnifyWithEvent:),
                sel!(rotateWithEvent:),
                sel!(pressureChangeWithEvent:),
                sel!(beginGestureWithEvent:),
                sel!(endGestureWithEvent:),
            ] {
                builder.add_method(
                    event,
                    input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
            }
            builder.add_method(
                sel!(mouseEntered:),
                mouse_entered as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                sel!(mouseExited:),
                mouse_exited as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            builder.add_method(
                sel!(drawRect:),
                draw_rect as extern "C-unwind" fn(_, _, NSRect),
            );
            builder.add_method(sel!(dealloc), dealloc as extern "C-unwind" fn(_, _));
            builder.add_method(
                sel!(viewDidChangeEffectiveAppearance),
                view_did_change_effective_appearance as extern "C-unwind" fn(_, _),
            );

            builder.add_method(
                sel!(insertText:replacementRange:),
                insert_text as extern "C-unwind" fn(_, _, *mut AnyObject, NSRange),
            );
            builder.add_method(
                sel!(setMarkedText:selectedRange:replacementRange:),
                set_marked_text as extern "C-unwind" fn(_, _, *mut AnyObject, NSRange, NSRange),
            );
            builder.add_method(sel!(unmarkText), unmark_text as extern "C-unwind" fn(_, _));
            builder.add_method(
                sel!(hasMarkedText),
                has_marked_text as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(
                sel!(markedRange),
                marked_range as extern "C-unwind" fn(_, _) -> NSRange,
            );
            builder.add_method(
                sel!(selectedRange),
                selected_range as extern "C-unwind" fn(_, _) -> NSRange,
            );
            builder.add_method(
                sel!(validAttributesForMarkedText),
                valid_attributes_for_marked_text as extern "C-unwind" fn(_, _) -> *mut AnyObject,
            );
            builder.add_method(
                sel!(attributedSubstringForProposedRange:actualRange:),
                attributed_substring_for_proposed_range
                    as extern "C-unwind" fn(_, _, NSRange, *mut NSRange) -> *mut AnyObject,
            );
            builder.add_method(
                sel!(firstRectForCharacterRange:actualRange:),
                first_rect_for_character_range
                    as extern "C-unwind" fn(_, _, NSRange, *mut NSRange) -> NSRect,
            );
            builder.add_method(
                sel!(characterIndexForPoint:),
                character_index_for_point as extern "C-unwind" fn(_, _, NSPoint) -> NSUInteger,
            );
            builder.add_method(
                sel!(doCommandBySelector:),
                do_command_by_selector as extern "C-unwind" fn(_, _, _),
            );
        }

        builder.register();
    });

    AnyClass::get(c"FLUIContentView").expect("FLUIContentView was registered by the Once block")
}

/// `NSTrackingArea` option bits, raw values per AppKit's
/// `NSTrackingAreaOptions`.
mod tracking_area_options {
    pub(super) const MOUSE_ENTERED_AND_EXITED: usize = 0x01;
    pub(super) const MOUSE_MOVED: usize = 0x02;
    pub(super) const ACTIVE_IN_KEY_WINDOW: usize = 0x20;
    pub(super) const IN_VISIBLE_RECT: usize = 0x200;
}

/// Have AppKit deliver mouse-moved, entered and exited events to `view` while
/// its window is key.
pub(crate) fn enable_mouse_tracking(view: *mut AnyObject) {
    // SAFETY: `view` is a live NSView; NSTrackingArea is looked up via the
    // runtime (the class always exists in AppKit), and `addTrackingArea:`
    // retains the tracking area, so releasing our reference is handled by the
    // view's lifetime.
    unsafe {
        let bounds: NSRect = msg_send![view, bounds];
        let options: usize = tracking_area_options::MOUSE_MOVED
            | tracking_area_options::ACTIVE_IN_KEY_WINDOW
            | tracking_area_options::MOUSE_ENTERED_AND_EXITED
            | tracking_area_options::IN_VISIBLE_RECT;
        let cls = AnyClass::get(c"NSTrackingArea")
            .expect("BUG: NSTrackingArea is an AppKit class this backend always has");
        let tracking_area: *mut AnyObject = msg_send![cls, alloc];
        let tracking_area: *mut AnyObject = msg_send![tracking_area,
            initWithRect: bounds
            options: options
            owner: view
            userInfo: std::ptr::null_mut::<AnyObject>()
        ];
        let _: () = msg_send![view, addTrackingArea: tracking_area];
        tracing::debug!("Enabled mouse tracking for view");
    }
}
