//! macOS NSView implementation for input events
//!
//! Creates a custom NSView subclass (FLUIContentView) that receives keyboard,
//! mouse, and scroll events through the NSResponder chain.
//!
//! # Architecture
//!
//! ```text
//! NSEvent (OS)
//!     ↓
//! NSApplication.sendEvent:
//!     ↓
//! NSWindow.sendEvent:
//!     ↓
//! FLUIContentView (first responder)
//!     ↓
//! keyDown:/mouseDown:/etc.
//!     ↓
//! convert_ns_event()
//!     ↓
//! WindowCallbacks::dispatch_input
//! ```
//!
//! # Migration note
//!
//! The class is built with objc2's imperative `ClassBuilder` (the successor to
//! `objc` 0.2's `ClassDecl`) rather than the declarative `define_class!` macro,
//! because the `NSTextInputClient` methods are registered by a *separate*
//! function in [`super::text_input`] — the macro wants all methods in one
//! block, and the split is what keeps the IME conformance's eleven callbacks
//! beside the state they read.

use std::cell::RefCell;
use std::ffi::CStr;
use std::sync::Weak;

use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Sel};
use objc2::{ClassType, msg_send};
use objc2_app_kit::{NSResponder, NSView as NSViewClass};
use objc2_foundation::{NSRect, NSRectEdge};

use super::events::convert_ns_event;
use super::text_input::{TextInputState, add_text_input_methods};
use crate::shared::WindowCallbacks;

// ============================================================================
// FLUIContentView Creation
// ============================================================================

/// The ivar name holding the boxed [`ViewContext`].
const CONTEXT_IVAR: &CStr = c"context_ptr";

/// Create a content view for receiving input events
///
/// This view becomes the NSWindow's contentView and first responder,
/// receiving all keyboard, mouse, and scroll events.
pub fn create_content_view(
    frame: NSRect,
    scale_factor: f64,
    callbacks: Weak<WindowCallbacks>,
) -> *mut AnyObject {
    // SAFETY: FLUIContentView is registered before alloc/init; the boxed
    // ViewContext pointer is stored in the view's ivar and released in
    // `dealloc`, so it lives exactly as long as the view.
    unsafe {
        let class = get_or_create_view_class();
        let view: *mut AnyObject = msg_send![class, alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: frame];

        // Store context (scale factor + callbacks + composition state)
        let context = Box::into_raw(Box::new(ViewContext {
            scale_factor,
            callbacks,
            text_input: RefCell::new(TextInputState::default()),
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

/// Context stored in NSView ivar
pub(super) struct ViewContext {
    scale_factor: f64,
    pub(super) callbacks: Weak<WindowCallbacks>,

    /// The `NSTextInputClient` composition state this view is driven through
    /// (see [`super::text_input`]). A `RefCell` because the AppKit callbacks and
    /// the routed [`super::text_input::MacOSTextInput`] bodies both reach it
    /// through a shared `&ViewContext`, and every one of them either runs on the
    /// main thread (AppKit's delivery contract) or under the owner-lane guard.
    pub(super) text_input: RefCell<TextInputState>,
}

// ============================================================================
// NSView Class Definition
// ============================================================================

/// Handle an input NSEvent arriving at a FLUIContentView method.
///
/// Converts the event and dispatches it through the per-window callbacks.
///
/// `pub(super)` because `super::text_input`'s `doCommandBySelector:` is the
/// second caller: the key event an input method declined is handed back here to
/// take the ordinary keyboard path.
pub(super) extern "C-unwind" fn handle_input_event(
    this: &AnyObject,
    _sel: Sel,
    event: *mut AnyObject,
) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
    // live objects); `event` is a valid NSEvent* for the duration of the call;
    // `bounds` is a plain NSRect getter.
    unsafe {
        if let Some(ctx) = get_context(this) {
            let bounds: NSRect = msg_send![this, bounds];
            if let Some(input) = convert_ns_event(
                event.cast::<std::ffi::c_void>(),
                ctx.scale_factor,
                bounds.size.height,
            ) {
                dispatch_input_event(ctx, input);
            }
        }
    }
}

/// `keyDown:` — the input-context route while a text input is attached, the
/// keyboard route otherwise.
///
/// The gate is load-bearing and its default is the load-bearing part: with
/// `ime_allowed == false` — the state of every window until a presentation
/// attaches a text input and enables composition — this is byte-identical to
/// wiring `keyDown:` straight to [`handle_input_event`], which is what it did
/// before the `NSTextInputClient` conformance existed. Only while a text input
/// IS attached does the event take AppKit's input method instead, through
/// `interpretKeyEvents:`: that call comes back into this view's
/// `NSTextInputClient` methods (`setMarkedText:` while composing, `insertText:`
/// on commit), so a key delivered to both routes would reach the application
/// twice from one physical press — an `ImeEvent::Commit` *and* a
/// `Key::Character`. `doCommandBySelector:` is the way back for the keys the
/// input method declines.
///
/// `pending_key_event` is set for exactly the duration of the
/// `interpretKeyEvents:` call, which is the only window in which the input
/// context can call `doCommandBySelector:` back for this event.
extern "C-unwind" fn key_down(this: &AnyObject, _sel: Sel, event: *mut AnyObject) {
    // SAFETY: `this` is a live FLUIContentView; `event` is a valid NSEvent* for
    // the duration of the call, and `interpretKeyEvents:` retains it for the
    // span of its own dispatch. The array is created by an NSArray class
    // constructor and autoreleased.
    unsafe {
        let Some(ctx) = get_context(this) else {
            return;
        };
        // The gate, and the whole of the change this callback makes to a window
        // that never attaches a text input: while no input context is attached
        // the key takes the keyboard path below, byte for byte as `keyDown:`
        // did before `NSTextInputClient` existed. Only an attached text input
        // makes AppKit's input method a *second* producer for the same press,
        // and a press must reach the application exactly once — either as a
        // composition/commit through the input context, or as a key event
        // through the keyboard conversion, never both (see the module doc of
        // `super::text_input`).
        let ime_allowed = ctx.text_input.borrow().ime_allowed;
        if !ime_allowed {
            handle_input_event(this, objc2::sel!(keyDown:), event);
            return;
        }

        // Saved and restored rather than set and cleared: an input method can
        // route a *second* `keyDown:` through the view while this one is inside
        // `interpretKeyEvents:` (the character palette does), and a plain clear
        // would leave the outer event's `doCommandBySelector:` reading zero and
        // dropping a command that belongs to this press.
        let previous_pending_key_event = ctx.text_input.borrow().pending_key_event;
        ctx.text_input.borrow_mut().pending_key_event = event as usize;
        let events: *mut AnyObject = msg_send![
            objc2::class!(NSArray),
            arrayWithObject: event
        ];
        let _: () = msg_send![this, interpretKeyEvents: events];
        ctx.text_input.borrow_mut().pending_key_event = previous_pending_key_event;
    }
}

/// `keyUp:` — the keyboard route, except while a composition is in flight.
///
/// Gated on the *composition* rather than on `ime_allowed`, which is the
/// distinction winit draws in its own `keyUp:` arm: it queues the release only
/// from its `Ground` and `Disabled` states, so an active preedit suppresses it.
/// The reason is the same one [`key_down`] exists for, on the opposite edge —
/// the press was consumed by the input method, so a release reported while the
/// composition is still open describes a key the application never saw go down.
///
/// A commit returns the state to `Ground` before the release arrives (winit
/// resets it in `keyDown:`, and [`TextInputState::clear_marked_text`] empties
/// the composition here), so an ordinary typed character still reports its
/// release normally. What is suppressed is exactly the release that lands
/// inside an open composition.
extern "C-unwind" fn key_up(this: &AnyObject, _sel: Sel, event: *mut AnyObject) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
    // live objects); `event` is a valid NSEvent* for the duration of the call.
    let reported = unsafe {
        get_context(this).is_none_or(|ctx| ctx.text_input.borrow().reports_key_release())
    };
    if !reported {
        return;
    }
    handle_input_event(this, objc2::sel!(keyUp:), event);
}

/// Dispatch a hover status change through the per-window callbacks.
fn dispatch_hover_change(this: &AnyObject, is_hovered: bool) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
    // live objects); `get_context`'s ivar contract holds for views created by
    // `create_content_view`.
    unsafe {
        if let Some(ctx) = get_context(this)
            && let Some(callbacks) = ctx.callbacks.upgrade()
        {
            callbacks.dispatch_hover_status_change(is_hovered);
        }
    }
}

/// mouseEntered: — report hover gained, then forward the pointer event.
extern "C-unwind" fn mouse_entered(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    dispatch_hover_change(this, true);
    handle_input_event(this, sel, event);
}

/// mouseExited: — report hover lost, then forward the pointer event.
extern "C-unwind" fn mouse_exited(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    dispatch_hover_change(this, false);
    handle_input_event(this, sel, event);
}

/// drawRect: — the platform asks for content; forward as a frame request.
///
/// `request_redraw()` marks the view dirty via `setNeedsDisplay:`; AppKit then
/// calls this method on the next display pass, which is where the
/// per-window `on_request_frame` contract fires (the macOS analogue of the
/// Windows backend's WM_PAINT dispatch).
///
/// The frame runs inside this callback, so any redraw the frame asks for is
/// asked for *while AppKit is displaying this view* — the one moment a
/// `setNeedsDisplay:` is discarded. [`DisplayPassGuard`] marks this thread for
/// the duration so `request_redraw` can tell that case apart from a request
/// made at any other moment and defer only the former.
///
/// [`DisplayPassGuard`]: super::display_pass::DisplayPassGuard
extern "C-unwind" fn draw_rect(this: &AnyObject, _sel: Sel, dirty_rect: NSRect) {
    // SAFETY: `this` is a live FLUIContentView; the super `drawRect:` message
    // is the documented NSView teardown of the dirty region.
    unsafe {
        let _display_pass = super::display_pass::DisplayPassGuard::enter();
        if let Some(callbacks) = get_context(this).and_then(|ctx| ctx.callbacks.upgrade()) {
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

/// Get or create the FLUIContentView class
fn get_or_create_view_class() -> &'static AnyClass {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = NSViewClass::class();
        let mut builder = objc2::runtime::ClassBuilder::new(c"FLUIContentView", superclass)
            .expect("FLUIContentView must be registered exactly once (guarded by Once)");

        // Add ivar to store context (a raw `*mut c_void` boxed ViewContext).
        builder.add_ivar::<*mut std::ffi::c_void>(CONTEXT_IVAR);

        // =================================================================
        // NSResponder Methods (Input Events)
        // =================================================================

        // acceptsFirstResponder - Allow view to become first responder
        extern "C-unwind" fn accepts_first_responder(_this: &AnyObject, _sel: Sel) -> Bool {
            Bool::YES
        }

        // becomeFirstResponder
        extern "C-unwind" fn become_first_responder(_this: &AnyObject, _sel: Sel) -> Bool {
            tracing::debug!("FLUIContentView became first responder");
            Bool::YES
        }

        // resignFirstResponder
        extern "C-unwind" fn resign_first_responder(_this: &AnyObject, _sel: Sel) -> Bool {
            tracing::debug!("FLUIContentView resigned first responder");
            Bool::YES
        }

        // flagsChanged: — modifier keys (Shift, Control, Alt, Command).
        // Modifier state is carried on every converted event, so flag-only
        // transitions are observed but not dispatched separately.
        extern "C-unwind" fn flags_changed(_this: &AnyObject, _sel: Sel, _event: *mut AnyObject) {
            tracing::trace!("Modifier flags changed");
        }

        // =================================================================
        // View Lifecycle
        // =================================================================

        extern "C-unwind" fn dealloc(this: &AnyObject, _sel: Sel) {
            // SAFETY: the ivar holds either null or a Box<ViewContext> leaked
            // in `create_content_view`; reclaiming it here (exactly once, on
            // dealloc) is the matching release. The super dealloc message is
            // the mandatory NSObject teardown.
            unsafe {
                let class = AnyClass::get(c"FLUIContentView")
                    .expect("BUG: dealloc only runs on a registered FLUIContentView");
                let ivar = class
                    .instance_variable(CONTEXT_IVAR)
                    .expect("BUG: FLUIContentView declares the context ivar");
                let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(this);
                let context_ptr = *slot;
                if !context_ptr.is_null() {
                    drop(Box::from_raw(context_ptr.cast::<ViewContext>()));
                }

                let _: () = msg_send![super(this, NSViewClass::class()), dealloc];
            }
        }

        // =================================================================
        // View Drawing (Optional)
        // =================================================================

        extern "C-unwind" fn is_opaque(_this: &AnyObject, _sel: Sel) -> Bool {
            Bool::YES // Our view is fully opaque
        }

        extern "C-unwind" fn accepts_touch_events(_this: &AnyObject, _sel: Sel) -> Bool {
            Bool::YES // Accept touch events for future trackpad gestures
        }

        // =================================================================
        // Add Methods to Class
        // =================================================================

        // SAFETY: every registered function pointer matches the Objective-C
        // method signature of its selector (`&AnyObject, Sel` plus the declared
        // argument/return types), as required by `ClassBuilder::add_method`.
        unsafe {
            // First responder
            builder.add_method(
                objc2::sel!(acceptsFirstResponder),
                accepts_first_responder as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(
                objc2::sel!(becomeFirstResponder),
                become_first_responder as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(
                objc2::sel!(resignFirstResponder),
                resign_first_responder as extern "C-unwind" fn(_, _) -> Bool,
            );

            // Keyboard events
            builder.add_method(
                objc2::sel!(keyDown:),
                key_down as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(keyUp:),
                key_up as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(flagsChanged:),
                flags_changed as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            // Left mouse
            builder.add_method(
                objc2::sel!(mouseDown:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(mouseUp:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(mouseMoved:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(mouseDragged:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            // Right mouse
            builder.add_method(
                objc2::sel!(rightMouseDown:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(rightMouseUp:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(rightMouseDragged:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            // Other mouse
            builder.add_method(
                objc2::sel!(otherMouseDown:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(otherMouseUp:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(otherMouseDragged:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            // Mouse enter/exit (hover status + pointer event)
            builder.add_method(
                objc2::sel!(mouseEntered:),
                mouse_entered as extern "C-unwind" fn(_, _, *mut AnyObject),
            );
            builder.add_method(
                objc2::sel!(mouseExited:),
                mouse_exited as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            // Scroll
            builder.add_method(
                objc2::sel!(scrollWheel:),
                handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
            );

            // Drawing — drawRect: drives the on_request_frame contract
            builder.add_method(
                objc2::sel!(drawRect:),
                draw_rect as extern "C-unwind" fn(_, _, NSRect),
            );

            // Lifecycle
            builder.add_method(objc2::sel!(dealloc), dealloc as extern "C-unwind" fn(_, _));

            // Properties
            builder.add_method(
                objc2::sel!(isOpaque),
                is_opaque as extern "C-unwind" fn(_, _) -> Bool,
            );
            builder.add_method(
                objc2::sel!(acceptsTouchEvents),
                accepts_touch_events as extern "C-unwind" fn(_, _) -> Bool,
            );

            // The NSTextInputClient conformance and its callbacks —
            // `interpretKeyEvents:` reaches this view's composition state, and
            // the input method reaches it back, only through these.
            add_text_input_methods(&mut builder);
        }

        builder.register();
    });

    AnyClass::get(c"FLUIContentView").expect("FLUIContentView was registered by the Once block")
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get view context from ivar
///
/// # Safety
///
/// `view` must be a live FLUIContentView whose `context_ptr` ivar is either
/// null or points to a `ViewContext` owned by that view.
pub(super) unsafe fn get_context(view: &AnyObject) -> Option<&ViewContext> {
    // SAFETY: per the function contract the ivar is null or a valid
    // Box<ViewContext> pointer owned by the view; the returned shared
    // reference cannot outlive the view method invocation that holds `view`.
    unsafe {
        let class = AnyClass::get(c"FLUIContentView")?;
        let ivar = class.instance_variable(CONTEXT_IVAR)?;
        let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(view);
        let context_ptr = *slot;
        if context_ptr.is_null() {
            return None;
        }
        Some(&*(context_ptr as *const ViewContext))
    }
}

/// The `context_ptr` of `view`, or `None` unless `view` is really one of ours.
///
/// The class check is the whole point of this function. [`get_context`] may
/// index the `context_ptr` ivar directly only because its callers are
/// `FLUIContentView` methods — AppKit invokes a method on an object that
/// declares it. A view obtained from `-[NSWindow contentView]` carries no such
/// guarantee: `liquid_glass` replaces the content view with an
/// `NSVisualEffectView`, which has no `context_ptr` ivar at all, and reading it
/// would be undefined rather than returning null. So every read of that ivar
/// from a view the crate did not itself receive as `self` goes through here,
/// where the class is checked first.
///
/// # Safety
///
/// `view` must be null or a live `NSView*`.
unsafe fn content_view_context_ptr(view: *mut AnyObject) -> Option<*mut ViewContext> {
    // SAFETY: per the function contract `view` is null or a live NSView;
    // `isKindOfClass:` is asked before the ivar is touched, and a view that
    // answers yes to it is one `create_content_view` built, whose
    // `context_ptr` is either null or a `ViewContext` it owns.
    unsafe {
        if view.is_null() {
            return None;
        }
        let is_content_view: Bool = msg_send![view, isKindOfClass: get_or_create_view_class()];
        if is_content_view != Bool::YES {
            return None;
        }
        let ivar = get_or_create_view_class().instance_variable(CONTEXT_IVAR)?;
        let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(&*view);
        let context_ptr = *slot;
        if context_ptr.is_null() {
            return None;
        }
        Some(context_ptr.cast::<ViewContext>())
    }
}

/// Reach the [`ViewContext`] of a view fetched from somewhere other than a
/// `FLUIContentView` method, scoped to `body`.
///
/// Answers `None` for anything that is not a view this crate built, so a caller
/// that obtained its view from `-[NSWindow contentView]` needs no check of its
/// own. The context is handed to `body` rather than returned, so the borrow
/// cannot outlive the call that established the view was alive.
///
/// # Safety
///
/// `view` must be null or a live `NSView*`, and `body` must not hold on to what
/// it is given past its own return (it may not: the reference is scoped to it).
pub(super) unsafe fn with_view_context<R>(
    view: *mut AnyObject,
    body: impl FnOnce(&ViewContext) -> R,
) -> Option<R> {
    // SAFETY: `content_view_context_ptr` establishes that the pointer is a live
    // `ViewContext` owned by `view`; the reference is scoped to `body`, so it
    // cannot outlive the call that established the view was alive.
    unsafe { content_view_context_ptr(view).map(|ptr| body(&*ptr)) }
}

/// [`with_view_context`], handing the context out mutably.
///
/// # Safety
///
/// Same contract as [`with_view_context`]: `view` must be null or a live
/// `NSView*`, and `body` must not hold on to what it is given past its own
/// return.
pub(super) unsafe fn with_view_context_mut<R>(
    view: *mut AnyObject,
    body: impl FnOnce(&mut ViewContext) -> R,
) -> Option<R> {
    // SAFETY: as `with_view_context`, plus: the `&mut` is sound because the
    // pointer is the view's own ivar and AppKit delivers these callbacks
    // serially on the main thread, so no other borrow of this context is live.
    unsafe { content_view_context_ptr(view).map(|ptr| body(&mut *ptr)) }
}

/// Dispatch input event to the window's callbacks
///
/// `pub(super)` for [`super::text_input`], whose IME events take this same
/// throat so they are ordered against pointer and keyboard events.
pub(super) fn dispatch_input_event(ctx: &ViewContext, input: crate::traits::PlatformInput) {
    if let Some(callbacks) = ctx.callbacks.upgrade() {
        let _result = callbacks.dispatch_input(input);
    } else {
        tracing::trace!("Input event received after window callbacks were dropped");
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Update view scale factor (called when window moves to different display)
///
/// A no-op on any view that is not a `FLUIContentView`: the caller reaches this
/// through `-[NSWindow contentView]`, and `liquid_glass` leaves an
/// `NSVisualEffectView` there.
pub fn update_view_scale_factor(view: *mut AnyObject, new_scale_factor: f64) {
    // SAFETY: `view` is null or a live NSView* (callers pass the window's
    // content view); `with_view_context_mut` checks the class before touching
    // the ivar and scopes the borrow to the closure, and the mutation is
    // confined to the main thread (AppKit calls).
    let updated = unsafe {
        with_view_context_mut(view, |context| {
            context.scale_factor = new_scale_factor;
        })
        .is_some()
    };
    if updated {
        tracing::debug!("Updated view scale factor to {}", new_scale_factor);
    }
}

/// `NSTrackingArea` option bits (cocoa 0.26 does not bind NSTrackingArea).
///
/// Raw values per AppKit's `NSTrackingAreaOptions`.
mod tracking_area_options {
    pub const MOUSE_ENTERED_AND_EXITED: usize = 0x01;
    pub const MOUSE_MOVED: usize = 0x02;
    pub const ACTIVE_IN_KEY_WINDOW: usize = 0x20;
    pub const IN_VISIBLE_RECT: usize = 0x200;
}

/// Enable mouse tracking for mouse moved events
pub fn enable_mouse_tracking(view: *mut AnyObject) {
    // SAFETY: `view` is a live NSView; NSTrackingArea is looked up via the
    // runtime (the class always exists in AppKit), and `addTrackingArea:`
    // retains the tracking area, so releasing our reference is handled by
    // the view's lifetime.
    unsafe {
        // Get view bounds
        let bounds: NSRect = msg_send![view, bounds];

        // Create tracking area options
        let options: usize = tracking_area_options::MOUSE_MOVED
            | tracking_area_options::ACTIVE_IN_KEY_WINDOW
            | tracking_area_options::MOUSE_ENTERED_AND_EXITED
            | tracking_area_options::IN_VISIBLE_RECT;

        // Create tracking area
        let cls = AnyClass::get(c"NSTrackingArea")
            .expect("BUG: NSTrackingArea is an AppKit class this backend always has");
        let tracking_area: *mut AnyObject = msg_send![cls, alloc];
        let tracking_area: *mut AnyObject = msg_send![tracking_area,
            initWithRect: bounds
            options: options
            owner: view
            userInfo: std::ptr::null_mut::<AnyObject>()
        ];

        // Add to view
        let _: () = msg_send![view, addTrackingArea: tracking_area];

        tracing::debug!("Enabled mouse tracking for view");
    }
}

/// Unused-import guard: `NSResponder`/`AnyProtocol`/`NSRectEdge` are named by
/// the conformance and superclass chain objc2's typed AppKit API relies on.
#[allow(dead_code)]
fn _typed_appkit_is_used(_r: Option<&NSResponder>, _p: Option<&AnyProtocol>, _e: NSRectEdge) {}
