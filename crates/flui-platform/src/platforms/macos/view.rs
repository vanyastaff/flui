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

use std::sync::Weak;

use cocoa::{
    base::{BOOL, YES, id, nil},
    foundation::NSRect,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{Class, Object, Sel},
    sel, sel_impl,
};

use super::events::convert_ns_event;
use crate::shared::WindowCallbacks;

// ============================================================================
// FLUIContentView Creation
// ============================================================================

/// Create a content view for receiving input events
///
/// This view becomes the NSWindow's contentView and first responder,
/// receiving all keyboard, mouse, and scroll events.
pub fn create_content_view(
    frame: NSRect,
    scale_factor: f64,
    callbacks: Weak<WindowCallbacks>,
) -> id {
    // SAFETY: FLUIContentView is registered before alloc/init; the boxed
    // ViewContext pointer is stored in the view's ivar and released in
    // `dealloc`, so it lives exactly as long as the view.
    unsafe {
        let class = get_or_create_view_class();
        let view: id = msg_send![class, alloc];
        let view: id = msg_send![view, initWithFrame: frame];

        // Store context (scale factor + callbacks)
        let context = Box::into_raw(Box::new(ViewContext {
            scale_factor,
            callbacks,
        })) as *mut std::ffi::c_void;
        (*view).set_ivar("context_ptr", context);

        view
    }
}

/// Context stored in NSView ivar
struct ViewContext {
    scale_factor: f64,
    callbacks: Weak<WindowCallbacks>,
}

// ============================================================================
// NSView Class Definition
// ============================================================================

/// Handle an input NSEvent arriving at a FLUIContentView method.
///
/// Converts the event and dispatches it through the per-window callbacks.
extern "C" fn handle_input_event(this: &Object, _sel: Sel, event: id) {
    // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
    // live objects); `event` is a valid NSEvent* for the duration of the call;
    // `bounds` is a plain NSRect getter.
    unsafe {
        if let Some(ctx) = get_context(this) {
            let bounds: NSRect = msg_send![this, bounds];
            if let Some(input) = convert_ns_event(event, ctx.scale_factor, bounds.size.height) {
                dispatch_input_event(ctx, input);
            }
        }
    }
}

/// Dispatch a hover status change through the per-window callbacks.
fn dispatch_hover_change(this: &Object, is_hovered: bool) {
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
extern "C" fn mouse_entered(this: &Object, sel: Sel, event: id) {
    dispatch_hover_change(this, true);
    handle_input_event(this, sel, event);
}

/// mouseExited: — report hover lost, then forward the pointer event.
extern "C" fn mouse_exited(this: &Object, sel: Sel, event: id) {
    dispatch_hover_change(this, false);
    handle_input_event(this, sel, event);
}

/// drawRect: — the platform asks for content; forward as a frame request.
///
/// `request_redraw()` marks the view dirty via `setNeedsDisplay:`; AppKit then
/// calls this method on the next display pass, which is where the
/// per-window `on_request_frame` contract fires (the macOS analogue of the
/// Windows backend's WM_PAINT dispatch).
extern "C" fn draw_rect(this: &Object, _sel: Sel, dirty_rect: NSRect) {
    // SAFETY: `this` is a live FLUIContentView; the super `drawRect:` message
    // is the documented NSView teardown of the dirty region.
    unsafe {
        if let Some(ctx) = get_context(this)
            && let Some(callbacks) = ctx.callbacks.upgrade()
        {
            callbacks.dispatch_request_frame();
        }

        let superclass = class!(NSView);
        let _: () = msg_send![super(this, superclass), drawRect: dirty_rect];
    }
}

/// Get or create the FLUIContentView class
fn get_or_create_view_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = class!(NSView);
        let mut decl = ClassDecl::new("FLUIContentView", superclass)
            .expect("FLUIContentView must be registered exactly once (guarded by Once)");

        // Add ivar to store context
        decl.add_ivar::<*mut std::ffi::c_void>("context_ptr");

        // =================================================================
        // NSResponder Methods (Input Events)
        // =================================================================

        // acceptsFirstResponder - Allow view to become first responder
        extern "C" fn accepts_first_responder(_this: &Object, _sel: Sel) -> BOOL {
            YES
        }

        // becomeFirstResponder
        extern "C" fn become_first_responder(_this: &Object, _sel: Sel) -> BOOL {
            tracing::debug!("FLUIContentView became first responder");
            YES
        }

        // resignFirstResponder
        extern "C" fn resign_first_responder(_this: &Object, _sel: Sel) -> BOOL {
            tracing::debug!("FLUIContentView resigned first responder");
            YES
        }

        // flagsChanged: — modifier keys (Shift, Control, Alt, Command).
        // Modifier state is carried on every converted event, so flag-only
        // transitions are observed but not dispatched separately.
        extern "C" fn flags_changed(_this: &Object, _sel: Sel, _event: id) {
            tracing::trace!("Modifier flags changed");
        }

        // =================================================================
        // View Lifecycle
        // =================================================================

        extern "C" fn dealloc(this: &Object, _sel: Sel) {
            // SAFETY: the ivar holds either null or a Box<ViewContext> leaked
            // in `create_content_view`; reclaiming it here (exactly once, on
            // dealloc) is the matching release. The super dealloc message is
            // the mandatory NSObject teardown.
            unsafe {
                let context_ptr: *mut std::ffi::c_void = *this.get_ivar("context_ptr");
                if !context_ptr.is_null() {
                    drop(Box::from_raw(context_ptr as *mut ViewContext));
                }

                let superclass = class!(NSView);
                let _: () = msg_send![super(this, superclass), dealloc];
            }
        }

        // =================================================================
        // View Drawing (Optional)
        // =================================================================

        extern "C" fn is_opaque(_this: &Object, _sel: Sel) -> BOOL {
            YES // Our view is fully opaque
        }

        extern "C" fn accepts_touch_events(_this: &Object, _sel: Sel) -> BOOL {
            YES // Accept touch events for future trackpad gestures
        }

        // =================================================================
        // Add Methods to Class
        // =================================================================

        // SAFETY: every registered function pointer matches the Objective-C
        // method signature of its selector (`&Object, Sel` plus the declared
        // argument/return types), as required by `ClassDecl::add_method`.
        unsafe {
            // First responder
            decl.add_method(
                sel!(acceptsFirstResponder),
                accepts_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
            );
            decl.add_method(
                sel!(becomeFirstResponder),
                become_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
            );
            decl.add_method(
                sel!(resignFirstResponder),
                resign_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
            );

            // Keyboard events
            decl.add_method(
                sel!(keyDown:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(keyUp:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(flagsChanged:),
                flags_changed as extern "C" fn(&Object, Sel, id),
            );

            // Left mouse
            decl.add_method(
                sel!(mouseDown:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseUp:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseMoved:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseDragged:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );

            // Right mouse
            decl.add_method(
                sel!(rightMouseDown:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(rightMouseUp:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(rightMouseDragged:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );

            // Other mouse
            decl.add_method(
                sel!(otherMouseDown:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(otherMouseUp:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(otherMouseDragged:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );

            // Mouse enter/exit (hover status + pointer event)
            decl.add_method(
                sel!(mouseEntered:),
                mouse_entered as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseExited:),
                mouse_exited as extern "C" fn(&Object, Sel, id),
            );

            // Scroll
            decl.add_method(
                sel!(scrollWheel:),
                handle_input_event as extern "C" fn(&Object, Sel, id),
            );

            // Drawing — drawRect: drives the on_request_frame contract
            decl.add_method(
                sel!(drawRect:),
                draw_rect as extern "C" fn(&Object, Sel, NSRect),
            );

            // Lifecycle
            decl.add_method(sel!(dealloc), dealloc as extern "C" fn(&Object, Sel));

            // Properties
            decl.add_method(
                sel!(isOpaque),
                is_opaque as extern "C" fn(&Object, Sel) -> BOOL,
            );
            decl.add_method(
                sel!(acceptsTouchEvents),
                accepts_touch_events as extern "C" fn(&Object, Sel) -> BOOL,
            );
        }

        decl.register();
    });

    Class::get("FLUIContentView").expect("FLUIContentView was registered by the Once block above")
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
unsafe fn get_context(view: &Object) -> Option<&ViewContext> {
    // SAFETY: per the function contract the ivar is null or a valid
    // Box<ViewContext> pointer owned by the view; the returned shared
    // reference cannot outlive the view method invocation that holds `view`.
    unsafe {
        let context_ptr: *mut std::ffi::c_void = *view.get_ivar("context_ptr");
        if context_ptr.is_null() {
            return None;
        }
        Some(&*(context_ptr as *const ViewContext))
    }
}

/// Dispatch input event to the window's callbacks
fn dispatch_input_event(ctx: &ViewContext, input: crate::traits::PlatformInput) {
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
pub fn update_view_scale_factor(view: id, new_scale_factor: f64) {
    // SAFETY: `view` is a live FLUIContentView (callers pass the window's
    // content view); its ivar is null or a valid ViewContext owned by the
    // view, and the mutation is confined to the main thread (AppKit calls).
    unsafe {
        let context_ptr: *mut std::ffi::c_void = *(*view).get_ivar("context_ptr");
        if !context_ptr.is_null() {
            let context = &mut *(context_ptr as *mut ViewContext);
            context.scale_factor = new_scale_factor;
            tracing::debug!("Updated view scale factor to {}", new_scale_factor);
        }
    }
}

/// `NSTrackingArea` option bits (cocoa 0.26 does not bind NSTrackingArea).
///
/// Raw values per AppKit's `NSTrackingAreaOptions`.
mod tracking_area_options {
    pub const MOUSE_ENTERED_AND_EXITED: u64 = 0x01;
    pub const MOUSE_MOVED: u64 = 0x02;
    pub const ACTIVE_IN_KEY_WINDOW: u64 = 0x20;
    pub const IN_VISIBLE_RECT: u64 = 0x200;
}

/// Enable mouse tracking for mouse moved events
pub fn enable_mouse_tracking(view: id) {
    // SAFETY: `view` is a live NSView; NSTrackingArea is looked up via the
    // runtime (the class always exists in AppKit), and `addTrackingArea:`
    // retains the tracking area, so releasing our reference is handled by
    // the view's lifetime.
    unsafe {
        // Get view bounds
        let bounds: NSRect = msg_send![view, bounds];

        // Create tracking area options
        let options: u64 = tracking_area_options::MOUSE_MOVED
            | tracking_area_options::ACTIVE_IN_KEY_WINDOW
            | tracking_area_options::MOUSE_ENTERED_AND_EXITED
            | tracking_area_options::IN_VISIBLE_RECT;

        // Create tracking area
        let tracking_area: id = msg_send![class!(NSTrackingArea), alloc];
        let tracking_area: id = msg_send![tracking_area,
            initWithRect: bounds
            options: options
            owner: view
            userInfo: nil
        ];

        // Add to view
        let _: () = msg_send![view, addTrackingArea: tracking_area];

        tracing::debug!("Enabled mouse tracking for view");
    }
}
