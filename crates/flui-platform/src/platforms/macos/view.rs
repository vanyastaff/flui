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
//! dispatch_input_event()
//!     ↓
//! PlatformHandlers.on_input
//! ```

use super::events::convert_ns_event;
use crate::shared::PlatformHandlers;
use crate::traits::input::PlatformInput;
use cocoa::appkit::{NSEvent, NSView};
use cocoa::base::{id, nil, BOOL, YES, NO};
use cocoa::foundation::NSRect;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use parking_lot::Mutex;
use std::sync::{Arc, Weak};

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
    handlers: Weak<Mutex<PlatformHandlers>>,
) -> id {
    unsafe {
        let class = get_or_create_view_class();
        let view: id = msg_send![class, alloc];
        let view: id = msg_send![view, initWithFrame: frame];

        // Store context (scale factor + handlers)
        let context = Box::into_raw(Box::new(ViewContext {
            scale_factor,
            handlers,
        })) as *mut std::ffi::c_void;
        (*view).set_ivar("context_ptr", context);

        view
    }
}

/// Context stored in NSView ivar
struct ViewContext {
    scale_factor: f64,
    handlers: Weak<Mutex<PlatformHandlers>>,
}

// ============================================================================
// NSView Class Definition
// ============================================================================

/// Get or create the FLUIContentView class
fn get_or_create_view_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = class!(NSView);
        let mut decl = ClassDecl::new("FLUIContentView", superclass).unwrap();

        // Add ivar to store context
        decl.add_ivar::<*mut std::ffi::c_void>("context_ptr");

        // =================================================================
        // NSResponder Methods (Input Events)
        // =================================================================

        // acceptsFirstResponder - Allow view to become first responder
        unsafe extern "C" fn accepts_first_responder(_this: &Object, _sel: Sel) -> BOOL {
            YES
        }

        // becomeFirstResponder
        unsafe extern "C" fn become_first_responder(this: &Object, _sel: Sel) -> BOOL {
            tracing::debug!("FLUIContentView became first responder");
            YES
        }

        // resignFirstResponder
        unsafe extern "C" fn resign_first_responder(this: &Object, _sel: Sel) -> BOOL {
            tracing::debug!("FLUIContentView resigned first responder");
            YES
        }

        // =================================================================
        // Keyboard Events
        // =================================================================

        unsafe extern "C" fn key_down(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn key_up(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn flags_changed(this: &Object, _sel: Sel, event: id) {
            // Modifier keys (Shift, Control, Alt, Command) changed
            // For now, we handle modifiers in key events themselves
            tracing::trace!("Modifier flags changed");
        }

        // =================================================================
        // Mouse Events
        // =================================================================

        // Left mouse button
        unsafe extern "C" fn mouse_down(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn mouse_up(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn mouse_moved(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn mouse_dragged(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        // Right mouse button
        unsafe extern "C" fn right_mouse_down(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn right_mouse_up(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn right_mouse_dragged(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        // Other mouse button (middle, etc.)
        unsafe extern "C" fn other_mouse_down(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn other_mouse_up(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn other_mouse_dragged(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        // Mouse enter/exit
        unsafe extern "C" fn mouse_entered(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        unsafe extern "C" fn mouse_exited(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        // =================================================================
        // Scroll Events
        // =================================================================

        unsafe extern "C" fn scroll_wheel(this: &Object, _sel: Sel, event: id) {
            if let Some(ctx) = get_context(this) {
                if let Some(input) = convert_ns_event(event, ctx.scale_factor) {
                    dispatch_input_event(&ctx, input);
                }
            }
        }

        // =================================================================
        // View Lifecycle
        // =================================================================

        unsafe extern "C" fn dealloc(this: &Object, _sel: Sel) {
            // Clean up context
            let context_ptr: *mut std::ffi::c_void = *this.get_ivar("context_ptr");
            if !context_ptr.is_null() {
                let _context = Box::from_raw(context_ptr as *mut ViewContext);
                // Drop context
            }

            // Call super dealloc
            let superclass = class!(NSView);
            let dealloc: extern "C" fn(&Object, Sel) = std::mem::transmute(
                msg_send![super(this, superclass), dealloc]
            );
        }

        // =================================================================
        // View Drawing (Optional)
        // =================================================================

        unsafe extern "C" fn is_opaque(_this: &Object, _sel: Sel) -> BOOL {
            YES // Our view is fully opaque
        }

        unsafe extern "C" fn accepts_touch_events(_this: &Object, _sel: Sel) -> BOOL {
            YES // Accept touch events for future trackpad gestures
        }

        // =================================================================
        // Add Methods to Class
        // =================================================================

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
                key_down as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(keyUp:),
                key_up as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(flagsChanged:),
                flags_changed as extern "C" fn(&Object, Sel, id),
            );

            // Left mouse
            decl.add_method(
                sel!(mouseDown:),
                mouse_down as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseUp:),
                mouse_up as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseMoved:),
                mouse_moved as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(mouseDragged:),
                mouse_dragged as extern "C" fn(&Object, Sel, id),
            );

            // Right mouse
            decl.add_method(
                sel!(rightMouseDown:),
                right_mouse_down as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(rightMouseUp:),
                right_mouse_up as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(rightMouseDragged:),
                right_mouse_dragged as extern "C" fn(&Object, Sel, id),
            );

            // Other mouse
            decl.add_method(
                sel!(otherMouseDown:),
                other_mouse_down as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(otherMouseUp:),
                other_mouse_up as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(otherMouseDragged:),
                other_mouse_dragged as extern "C" fn(&Object, Sel, id),
            );

            // Mouse enter/exit
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
                scroll_wheel as extern "C" fn(&Object, Sel, id),
            );

            // Lifecycle
            decl.add_method(
                sel!(dealloc),
                dealloc as extern "C" fn(&Object, Sel),
            );

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

    Class::get("FLUIContentView").unwrap()
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get view context from ivar
unsafe fn get_context(view: &Object) -> Option<&ViewContext> {
    let context_ptr: *mut std::ffi::c_void = *view.get_ivar("context_ptr");
    if context_ptr.is_null() {
        return None;
    }
    Some(&*(context_ptr as *const ViewContext))
}

/// Dispatch input event to platform handlers
fn dispatch_input_event(ctx: &ViewContext, input: PlatformInput) {
    if let Some(handlers) = ctx.handlers.upgrade() {
        let handlers = handlers.lock();
        if let Some(handler) = &handlers.on_input {
            handler(input);
        } else {
            tracing::trace!("Input event received but no handler set: {:?}", input);
        }
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Update view scale factor (called when window moves to different display)
pub fn update_view_scale_factor(view: id, new_scale_factor: f64) {
    unsafe {
        let context_ptr: *mut std::ffi::c_void = (*view).get_ivar("context_ptr");
        if !context_ptr.is_null() {
            let context = &mut *(context_ptr as *mut ViewContext);
            context.scale_factor = new_scale_factor;
            tracing::debug!("Updated view scale factor to {}", new_scale_factor);
        }
    }
}

/// Enable mouse tracking for mouse moved events
pub fn enable_mouse_tracking(view: id) {
    unsafe {
        use cocoa::appkit::NSTrackingArea;
        use cocoa::foundation::NSRect;

        // Get view bounds
        let bounds: NSRect = msg_send![view, bounds];

        // Create tracking area options
        let options = cocoa::appkit::NSTrackingAreaOptions::NSTrackingMouseMoved
            | cocoa::appkit::NSTrackingAreaOptions::NSTrackingActiveInKeyWindow
            | cocoa::appkit::NSTrackingAreaOptions::NSTrackingMouseEnteredAndExited
            | cocoa::appkit::NSTrackingAreaOptions::NSTrackingInVisibleRect;

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
