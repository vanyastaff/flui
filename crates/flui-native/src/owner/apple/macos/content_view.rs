//! The content view of every AppKit window: `FLUIContentView`, the `NSView`
//! subclass each window installs as its content view and first responder.
//!
//! The class is the owner's, as FLUI's Win32 window class is (ADR-0151 §10):
//! several features answer what reaches it, so it names none of them. Each
//! view keeps its window's callbacks and the parts the host's table made for
//! it ([`ViewHooks`]): the input ([`ViewInput`]), which converts the mouse,
//! trackpad and keyboard events, and the text input ([`ViewText`]), which
//! answers the `NSTextInputClient` methods from the focused field. Each part
//! keeps its state of the window in itself, one per view, made with the view
//! and dropped with it, as the class's ivars.
//!
//! The view routes a key press between them: to the input method
//! (`interpretKeyEvents:`) while a field has focus, to the input
//! otherwise, and a key the input method declines (`doCommandBySelector:`)
//! back to the input; a release that lands inside an open composition reaches
//! neither. Its `updateLayer` asks the window for a frame inside a display pass
//! ([`display_pass`](super::display_pass)), and it reports its hover and its
//! appearance through the window's callbacks. Its backing layer is a
//! `FluiMetalLayer`, which reports when its frames reached the screen.
//!
//! A click into a window that is not key first activates it; whether AppKit
//! also delivers that click (`acceptsFirstMouse:`) is the input's answer
//! ([`ViewInput::accepts_first_mouse`]).

// AppKit's classes are reached through Objective-C messages, which have no
// safe wrapper; every `unsafe` block states the invariant it relies on.
#![expect(unsafe_code)]

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Weak;

use flui_foundation::geometry::{Bounds, Point};
use flui_platform::text::text_store::TextStoreHost;
use flui_platform::text::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::{Brightness, PlatformInput};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSEvent,
    NSResponder, NSTextInputClient, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSViewLayerContentsPlacement, NSViewLayerContentsRedrawPolicy,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound, NSObject, NSObjectProtocol,
    NSPoint, NSRange, NSRect, NSSize, NSString, NSUInteger, NSUserDefaults,
};
use objc2_quartz_core::CALayer;

use crate::owner::apple::metal_layer::{FluiMetalLayer, feedback_of};
use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};
use crate::owner::window_delivery::WindowDelivery;

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

    /// Whether the press `event`, which makes the view's window key, is also
    /// delivered (`acceptsFirstMouse:`); `false` only activates the window.
    fn accepts_first_mouse(&self, view: &ContentView<'_>, event: &NSEvent) -> bool;

    /// The window's input regions, in its logical coordinates, at its `scale`:
    /// what a press on a title bar the application draws is answered from.
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input::input_regions::InputRegion],
        scale: flui_foundation::geometry::DevicePixelRatio,
    );

    /// The window holds the pointer locked (`CursorGrab::Locked`), or no
    /// longer does: while it does, the mouse's motion is raw motion, and no
    /// move is delivered.
    fn set_pointer_locked(&self, locked: bool);
}

/// The text input's part of one content view: the document AppKit's input
/// method reads and edits through the view's `NSTextInputClient` methods, and
/// the window's store host, through which the presentation names the field it
/// is. Ranges are UTF-16 code units, as `NSRange` counts; `None` stands for
/// `NSNotFound`. Coordinates are the window's logical ones (top-left origin).
pub(crate) trait ViewText {
    /// The view exists: the text input may now reach its input context. The
    /// pointer stays valid until the text input is dropped, with the view's
    /// ivars.
    fn bind(&self, view: *mut AnyObject);

    /// The window's store host, for the presentation.
    fn store_host(&self) -> Rc<dyn TextStoreHost>;

    /// Whether a key press goes to the input method: a field has focus. A
    /// window without one takes every key as a key event.
    fn takes_key_presses(&self) -> bool;

    /// Whether a key release still belongs to the input: `false` exactly
    /// while a composition is open, since the press that opened it went to
    /// the input method.
    fn reports_key_release(&self) -> bool;

    /// The view's window became or stopped being the key window.
    fn key_window_changed(&self);

    /// `insertText:replacementRange:`.
    fn insert_text(&self, text: String, replacement: Option<Utf16Range>);

    /// `setMarkedText:selectedRange:replacementRange:`; `selected` is the
    /// selection within `text`.
    fn set_marked_text(&self, text: String, selected: Utf16Range, replacement: Option<Utf16Range>);

    /// `unmarkText`.
    fn unmark_text(&self);

    /// `markedRange`; `None` when nothing is marked.
    fn marked_range(&self) -> Option<Utf16Range>;

    /// `selectedRange`; `None` when no field has focus.
    fn selected_range(&self) -> Option<Utf16Range>;

    /// `attributedSubstringForProposedRange:actualRange:`: the text and the
    /// range it covers.
    fn substring(&self, range: Utf16Range) -> Option<(String, Utf16Range)>;

    /// `firstRectForCharacterRange:actualRange:`: the rectangle and the
    /// range it covers.
    fn first_rect(&self, range: Option<Utf16Range>) -> Option<(Bounds<f64>, Utf16Range)>;

    /// `characterIndexForPoint:`, the point converted to the window.
    fn character_index(&self, point: Point<f64>) -> Option<Utf16Offset>;
}

/// One live content view, as the parts it dispatches to reach it.
pub(crate) struct ContentView<'a> {
    view: &'a FluiContentView,
}

impl ContentView<'_> {
    /// The view's context.
    fn context(&self) -> &ViewContext {
        self.view.ivars()
    }

    /// Dispatch `input` through the window's callbacks: the one throat every
    /// event of the view takes, so the text input's are ordered against the
    /// pointer's and the keyboard's.
    pub(crate) fn dispatch_input(&self, input: PlatformInput) {
        if let Some(delivery) = self.context().delivery.upgrade() {
            let _result = delivery.dispatch_input(input);
        } else {
            tracing::trace!("Input event received after window delivery were dropped");
        }
    }

    /// The view's height, which AppKit's bottom-left coordinates are flipped
    /// by.
    pub(crate) fn height(&self) -> f64 {
        self.view.bounds().size.height
    }

    /// The view's text input.
    pub(crate) fn text(&self) -> &dyn ViewText {
        &*self.context().text
    }

    /// Move the window for the press `event` starts, as its system title bar
    /// does: AppKit tracks the drag until the button goes up, snapping to the
    /// screen's edges and between Spaces. The press reaches nobody else.
    pub(crate) fn drag_window(&self, event: &NSEvent) {
        if let Some(window) = self.view.window() {
            window.performWindowDragWithEvent(event);
        }
    }

    /// Zoom the window, or put it back, as the system title bar's
    /// double-click does.
    pub(crate) fn zoom_window(&self) {
        if let Some(window) = self.view.window() {
            window.performZoom(None);
        }
    }

    /// Minimize the window into the Dock, as the system title bar's
    /// double-click does when the user set it so.
    pub(crate) fn minimize_window(&self) {
        if let Some(window) = self.view.window() {
            window.performMiniaturize(None);
        }
    }

    /// What the user set a double-click on a title bar to do, in System
    /// Settings: `AppleActionOnDoubleClick` (`Maximize`, `Minimize` or
    /// `None`), and the older `AppleMiniaturizeOnDoubleClick` where it is set.
    pub(crate) fn title_bar_double_click_setting() -> (Option<String>, Option<bool>) {
        let defaults = NSUserDefaults::standardUserDefaults();
        let action = defaults
            .stringForKey(&NSString::from_str("AppleActionOnDoubleClick"))
            .map(|action| action.to_string());
        let minimize_key = NSString::from_str("AppleMiniaturizeOnDoubleClick");
        let minimize = defaults
            .objectForKey(&minimize_key)
            .map(|_| defaults.boolForKey(&minimize_key));
        (action, minimize)
    }
}

/// What one content view keeps, as its ivars, from its creation to its
/// `dealloc`.
pub(crate) struct ViewContext {
    delivery: Weak<WindowDelivery>,
    input: Box<dyn ViewInput>,
    text: Box<dyn ViewText>,
    /// The `keyDown:` event inside `interpretKeyEvents:`, null otherwise: what
    /// `doCommandBySelector:`, which the input method calls during it, hands
    /// back to the input.
    pending_key_event: Cell<*const NSEvent>,
    /// The appearance last reported to the window's snapshot; AppKit also
    /// re-resolves the effective appearance for changes that keep the theme,
    /// such as the accent colour.
    appearance: Cell<Brightness>,
}

define_class!(
    // SAFETY: `NSView` may be subclassed; the ivars are Rust values the class
    // drops in its `dealloc` (`ContainedIvars`), and `MainThreadOnly` is
    // `NSView`'s own thread kind.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FLUIContentView"]
    #[ivars = ContainedIvars<ViewContext>]
    /// Every window's content view and first responder.
    pub(crate) struct FluiContentView;

    // SAFETY: `NSObjectProtocol` has no requirements of its own.
    unsafe impl NSObjectProtocol for FluiContentView {}

    /// Constant answers run no code, so they need no boundary.
    impl FluiContentView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsTouchEvents))]
        fn accepts_touch_events(&self) -> bool {
            true
        }

        /// The view draws by updating its layer, never through `drawRect:`:
        /// the bitmap AppKit would give a `drawRect:` view would cover the
        /// Metal drawable after every display pass.
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }
    }

    /// Input routing.
    impl FluiContentView {
        /// A click into a window that is not key: the input decides whether
        /// it is delivered as well as activating the window. After a panic
        /// AppKit's own default holds: it only activates.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, event: Option<&NSEvent>) -> bool {
            os_entry(
                "acceptsFirstMouse:",
                (),
                AfterPanic::Answer(|()| false),
                || {
                    event.is_some_and(|event| {
                        self.ivars()
                            .input
                            .accepts_first_mouse(&ContentView { view: self }, event)
                    })
                },
            )
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            os_entry("keyDown:", (), AfterPanic::Answer(|()| ()), || {
                self.route_key_down(event);
            });
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            os_entry("keyUp:", (), AfterPanic::Answer(|()| ()), || {
                // The input's, except while a composition is open: the press
                // was the input method's, so a release reported then
                // describes a key the application never saw go down. A
                // commit closes the composition before the release arrives,
                // so a typed character still reports its release.
                if self.ivars().text.reports_key_release() {
                    self.deliver(event);
                }
            });
        }

        // A modifier key is a key event of its own (the input reads whether
        // it went down from the flags).
        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            Self::input(self, "flagsChanged:", event);
        }

        // Takes a press back.
        #[unsafe(method(mouseCancelled:))]
        fn mouse_cancelled(&self, event: &NSEvent) {
            Self::input(self, "mouseCancelled:", event);
        }

        // The smart zoom.
        #[unsafe(method(smartMagnifyWithEvent:))]
        fn smart_magnify_with_event(&self, event: &NSEvent) {
            Self::input(self, "smartMagnifyWithEvent:", event);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            Self::input(self, "mouseDown:", event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            Self::input(self, "mouseUp:", event);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            Self::input(self, "mouseMoved:", event);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            Self::input(self, "mouseDragged:", event);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            Self::input(self, "rightMouseDown:", event);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            Self::input(self, "rightMouseUp:", event);
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            Self::input(self, "rightMouseDragged:", event);
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            Self::input(self, "otherMouseDown:", event);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            Self::input(self, "otherMouseUp:", event);
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            Self::input(self, "otherMouseDragged:", event);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            Self::input(self, "scrollWheel:", event);
        }

        #[unsafe(method(magnifyWithEvent:))]
        fn magnify_with_event(&self, event: &NSEvent) {
            Self::input(self, "magnifyWithEvent:", event);
        }

        #[unsafe(method(rotateWithEvent:))]
        fn rotate_with_event(&self, event: &NSEvent) {
            Self::input(self, "rotateWithEvent:", event);
        }

        #[unsafe(method(pressureChangeWithEvent:))]
        fn pressure_change_with_event(&self, event: &NSEvent) {
            Self::input(self, "pressureChangeWithEvent:", event);
        }

        #[unsafe(method(beginGestureWithEvent:))]
        fn begin_gesture_with_event(&self, event: &NSEvent) {
            Self::input(self, "beginGestureWithEvent:", event);
        }

        #[unsafe(method(endGestureWithEvent:))]
        fn end_gesture_with_event(&self, event: &NSEvent) {
            Self::input(self, "endGestureWithEvent:", event);
        }

        /// Report hover gained, then forward the pointer event.
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) {
            os_entry("mouseEntered:", (), AfterPanic::Answer(|()| ()), || {
                self.report_hover(true);
                self.deliver(event);
            });
        }

        /// Report hover lost, then forward the pointer event.
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, event: &NSEvent) {
            os_entry("mouseExited:", (), AfterPanic::Answer(|()| ()), || {
                self.report_hover(false);
                self.deliver(event);
            });
        }
    }

    /// Drawing and appearance.
    impl FluiContentView {
        /// The layer AppKit backs the view with: a [`FluiMetalLayer`], which
        /// wgpu draws into directly (it takes a view's root layer that is a
        /// `CAMetalLayer`) and whose drawables report when they reached the
        /// screen.
        #[unsafe(method_id(makeBackingLayer))]
        fn make_backing_layer(&self) -> Retained<CALayer> {
            os_entry("makeBackingLayer", (), AfterPanic::Abort, || {
                Retained::into_super(Retained::into_super(FluiMetalLayer::new()))
            })
        }

        /// AppKit asks for content: the window's frame request.
        ///
        /// `request_redraw()` marks the view dirty via `setNeedsDisplay:`;
        /// AppKit then calls this method on the next display pass, which is
        /// where the per-window `WindowHandler::frame_requested` contract
        /// fires (the macOS analogue of the Win32 window's `WM_PAINT`). The
        /// frame runs inside this callback, so a redraw the frame asks for is
        /// asked for while AppKit is displaying this view — the one moment a
        /// `setNeedsDisplay:` is discarded; the display pass marks this thread
        /// for the duration, so the window defers only that case.
        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            os_entry("updateLayer", (), AfterPanic::Answer(|()| ()), || {
                let _display_pass = super::display_pass::DisplayPassGuard::enter();
                self.present_with_transaction_while_resizing();
                if let Some(delivery) = self.ivars().delivery.upgrade() {
                    tracing::trace!("FLUIContentView updateLayer: dispatching frame request");
                    delivery.dispatch_request_frame();
                } else {
                    // The window's callbacks are gone (torn down, or the view
                    // outlived its window): why a frame that AppKit asked for
                    // produced no frame request, which is otherwise
                    // indistinguishable from AppKit never asking at all.
                    tracing::trace!(
                        "FLUIContentView updateLayer: no live delivery; frame request dropped"
                    );
                }
            });
        }

        /// AppKit re-resolved this view's appearance: the system theme
        /// flipped, or something that keeps it did.
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn view_did_change_effective_appearance(&self) {
            os_entry(
                "viewDidChangeEffectiveAppearance",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: NSView's own (empty) implementation, sent to
                    // this view on the main thread AppKit messaged it on.
                    let () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
                    let appearance = effective_appearance(self.as_super());
                    let context = self.ivars();
                    if context.appearance.replace(appearance) != appearance
                        && let Some(delivery) = context.delivery.upgrade()
                    {
                        delivery.publish_state();
                    }
                },
            );
        }
    }

    // SAFETY: each method has the signature `NSTextInputClient` declares for
    // its selector. Conformance, not just the methods: AppKit's
    // `NSTextInputContext` asks `conformsToProtocol:` before it routes a key
    // event through the input method at all.
    unsafe impl NSTextInputClient for FluiContentView {
        /// The input method committed text.
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, replacement_range: NSRange) {
            os_entry(
                "insertText:replacementRange:",
                replacement_range,
                AfterPanic::Answer(|_| ()),
                || {
                    let Some(text) = text_of(string) else {
                        tracing::trace!("insertText: carried no readable string; commit dropped");
                        return;
                    };
                    self.ivars()
                        .text
                        .insert_text(text, utf16_range(replacement_range));
                },
            );
        }

        /// The composition changed. `selectedRange` is the selection within
        /// the marked text.
        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(
            &self,
            string: &AnyObject,
            selected_range: NSRange,
            replacement_range: NSRange,
        ) {
            os_entry(
                "setMarkedText:selectedRange:replacementRange:",
                (selected_range, replacement_range),
                AfterPanic::Answer(|_| ()),
                || {
                    let text = text_of(string).unwrap_or_default();
                    let selected = utf16_range(selected_range).unwrap_or_else(|| {
                        let end = Utf16Offset::new(text.encode_utf16().count());
                        Utf16Range::collapsed(end)
                    });
                    self.ivars().text.set_marked_text(
                        text,
                        selected,
                        utf16_range(replacement_range),
                    );
                },
            );
        }

        /// The composition ends, keeping its text.
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            os_entry("unmarkText", (), AfterPanic::Answer(|()| ()), || {
                self.ivars().text.unmark_text();
            });
        }

        /// Is a composition in progress?
        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            os_entry("hasMarkedText", (), AfterPanic::Answer(|()| false), || {
                self.ivars().text.marked_range().is_some()
            })
        }

        /// The composition's span in the field, `{NSNotFound, 0}` for none.
        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            os_entry("markedRange", (), AfterPanic::Answer(|()| not_found()), || {
                ns_range(self.ivars().text.marked_range())
            })
        }

        /// The field's selection, `{NSNotFound, 0}` when no field has focus
        /// or the field cannot be read now.
        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            os_entry(
                "selectedRange",
                (),
                AfterPanic::Answer(|()| not_found()),
                || ns_range(self.ivars().text.selected_range()),
            )
        }

        /// Empty: the composing text is rendered by the framework's own
        /// client, not by AppKit's text system, so an attributed composition
        /// has nothing to describe.
        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            os_entry(
                "validAttributesForMarkedText",
                (),
                AfterPanic::Abort,
                NSArray::new,
            )
        }

        /// The field's text of `range`, as far as the field reaches, with
        /// `actualRange` the range it covers; `nil` and `{NSNotFound, 0}`
        /// when it cannot be read (a password field, no focus, a range that
        /// splits a character).
        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring_for_proposed_range(
            &self,
            range: NSRange,
            actual_range: *mut NSRange,
        ) -> Option<Retained<NSAttributedString>> {
            os_entry(
                "attributedSubstringForProposedRange:actualRange:",
                range,
                AfterPanic::Answer(|_| None),
                || {
                    let read = utf16_range(range)
                        .and_then(|range| self.ivars().text.substring(range));
                    let Some((text, covered)) = read else {
                        // SAFETY: AppKit's own out-parameter for this call.
                        unsafe { write_actual_range(actual_range, not_found()) };
                        return None;
                    };
                    // SAFETY: as above.
                    unsafe { write_actual_range(actual_range, ns_range(Some(covered))) };
                    Some(NSAttributedString::from_nsstring(&NSString::from_str(&text)))
                },
            )
        }

        /// Where to place candidates: the rectangle of `range` (the
        /// composition, else the selection, when it is `NSNotFound`),
        /// converted to screen coordinates. The Y axis is flipped on the way:
        /// the field's coordinates have their origin at the top-left,
        /// AppKit's at the bottom-left, and without the flip the candidate
        /// window would sit mirrored about the view's vertical centre. A view
        /// with no window (mid-teardown) has no screen rectangle to answer
        /// with.
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect_for_character_range(
            &self,
            range: NSRange,
            actual_range: *mut NSRange,
        ) -> NSRect {
            os_entry(
                "firstRectForCharacterRange:actualRange:",
                range,
                AfterPanic::Answer(|_| zero_rect()),
                || {
                    // SAFETY: AppKit's own out-parameter for this call.
                    unsafe { write_actual_range(actual_range, not_found()) };
                    let Some((area, covered)) = self.ivars().text.first_rect(utf16_range(range))
                    else {
                        return zero_rect();
                    };
                    let Some(window) = self.window() else {
                        return zero_rect();
                    };
                    let height = self.bounds().size.height;
                    let view_rect = NSRect::new(
                        NSPoint::new(area.origin.x, height - (area.origin.y + area.size.height)),
                        NSSize::new(area.size.width, area.size.height),
                    );
                    let window_rect = self.convertRect_toView(view_rect, None);
                    // SAFETY: as above.
                    unsafe { write_actual_range(actual_range, ns_range(Some(covered))) };
                    window.convertRectToScreen(window_rect)
                },
            )
        }

        /// The field's character boundary nearest `point`, a screen point;
        /// `NSNotFound` when no field has focus or the view has no window.
        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, point: NSPoint) -> NSUInteger {
            os_entry(
                "characterIndexForPoint:",
                (point.x, point.y),
                AfterPanic::Answer(|_| NSNotFound as NSUInteger),
                || {
                    let Some(window) = self.window() else {
                        return NSNotFound as NSUInteger;
                    };
                    let in_window = window.convertPointFromScreen(point);
                    let in_view = self.convertPoint_fromView(in_window, None);
                    let at = Point::new(in_view.x, self.bounds().size.height - in_view.y);
                    self.ivars()
                        .text
                        .character_index(at)
                        .map_or(NSNotFound as NSUInteger, |index| index.get() as NSUInteger)
                },
            )
        }

        /// A key the input method declined goes back to the input, exactly
        /// the path `keyDown:` would have taken without a composing text
        /// input. Outside an `interpretKeyEvents:` call (menu-key
        /// equivalents) there is no key press to convert, so the command is
        /// dropped rather than invented.
        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, _command: Sel) {
            os_entry("doCommandBySelector:", (), AfterPanic::Answer(|()| ()), || {
                let pending = self.ivars().pending_key_event.get();
                // SAFETY: the pending event is null or the `NSEvent` AppKit
                // keeps alive across the `interpretKeyEvents:` call whose
                // dispatch is on this stack, the only window in which this
                // runs with it set (`PendingKeyEvent`).
                let Some(event) = (unsafe { pending.as_ref() }) else {
                    tracing::trace!(
                        "doCommandBySelector: outside a pending key press; command dropped"
                    );
                    return;
                };
                self.deliver(event);
            });
        }
    }
);

impl std::fmt::Debug for FluiContentView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluiContentView").finish_non_exhaustive()
    }
}

impl FluiContentView {
    /// The view as the parts it dispatches to reach it.
    pub(crate) fn content(&self) -> ContentView<'_> {
        ContentView { view: self }
    }

    /// An input event AppKit sent as `selector`, delivered to the input
    /// inside the panic boundary.
    fn input(&self, selector: &'static str, event: &NSEvent) {
        os_entry(selector, (), AfterPanic::Answer(|()| ()), || {
            self.deliver(event);
        });
    }

    /// The input converts `event` and dispatches what it carries.
    fn deliver(&self, event: &NSEvent) {
        self.ivars().input.event(&self.content(), event);
    }

    /// `keyDown:` — the input method's route while a field has focus, the
    /// input's otherwise.
    ///
    /// The gate's default is the load-bearing part: until a presentation
    /// focuses a field, every press goes straight to the input. Only while
    /// one has focus does the press take AppKit's input method, through
    /// `interpretKeyEvents:`, which comes back into the view's
    /// `NSTextInputClient` methods (`setMarkedText:` while composing,
    /// `insertText:` on commit); a press delivered to both routes would reach
    /// the field twice. `doCommandBySelector:` is the way back for the keys
    /// the input method declines.
    ///
    /// The pending event is saved and restored rather than set and cleared:
    /// an input method can route a second `keyDown:` through the view while
    /// this one is inside `interpretKeyEvents:` (the character palette does),
    /// and a plain clear would leave the outer event's `doCommandBySelector:`
    /// reading null and dropping a command that belongs to this press. It is
    /// restored by a guard, so a panic unwinding out of `interpretKeyEvents:`
    /// leaves no event a later `doCommandBySelector:` would read once AppKit
    /// has let it go.
    fn route_key_down(&self, event: &NSEvent) {
        let context = self.ivars();
        if !context.text.takes_key_presses() {
            self.deliver(event);
            return;
        }
        let _restore = PendingKeyEvent::hold(&context.pending_key_event, event);
        self.interpretKeyEvents(&NSArray::from_slice(&[event]));
    }

    /// Dispatch a hover status change through the window's callbacks.
    fn report_hover(&self, is_hovered: bool) {
        if let Some(delivery) = self.ivars().delivery.upgrade() {
            delivery.report_hover(is_hovered);
        }
    }

    /// The view's backing layer, a [`FluiMetalLayer`] (`makeBackingLayer`).
    fn backing_layer(&self) -> Option<Retained<AnyObject>> {
        // SAFETY: `layer` takes no argument and answers the view's backing
        // layer or nil.
        unsafe { msg_send![self, layer] }
    }

    /// Through a live resize the layer presents inside the Core Animation
    /// transaction that carries the new window frame, so the edge never
    /// shows a stretched old frame or a strip of background; otherwise it
    /// presents as soon as Metal is done.
    ///
    /// The frame renders on this thread, inside this display pass (the raster
    /// lane is inline on macOS), and wgpu reads the flag when it takes the
    /// drawable: with it set, it waits until the command buffer is scheduled
    /// and presents the drawable in the current transaction. That wait holds
    /// the main thread, which is why the flag is set only while the user
    /// drags an edge.
    fn present_with_transaction_while_resizing(&self) {
        let resizing = self.inLiveResize();
        if let Some(layer) = self
            .backing_layer()
            .and_then(|layer| layer.downcast::<FluiMetalLayer>().ok())
        {
            layer.setPresentsWithTransaction(resizing);
        }
    }
}

/// The `keyDown:` event `interpretKeyEvents:` is dispatching, until it
/// returns or unwinds: the event before it is put back on drop.
struct PendingKeyEvent<'a> {
    slot: &'a Cell<*const NSEvent>,
    previous: *const NSEvent,
}

impl<'a> PendingKeyEvent<'a> {
    fn hold(slot: &'a Cell<*const NSEvent>, event: &NSEvent) -> Self {
        Self {
            previous: slot.replace(std::ptr::from_ref(event)),
            slot,
        }
    }
}

impl Drop for PendingKeyEvent<'_> {
    fn drop(&mut self) {
        self.slot.set(self.previous);
    }
}

/// Create the content view of a window whose callbacks are `delivery`, with
/// the parts `hooks` make, on the main thread `main` proves: it becomes the
/// window's content view and first responder, and receives its keyboard,
/// mouse and scroll events. The caller owns the view; dropping the last
/// reference frees it, and its context with it.
pub(crate) fn create(
    frame: NSRect,
    delivery: Weak<WindowDelivery>,
    hooks: ViewHooks,
    main: MainThreadMarker,
) -> Retained<FluiContentView> {
    let reports = delivery
        .upgrade()
        .map(|delivery| delivery.presentation_reports());
    let context = ViewContext {
        delivery,
        input: (hooks.input)(),
        text: (hooks.text)(),
        pending_key_event: Cell::new(std::ptr::null()),
        // Read from the view once it exists, below.
        appearance: Cell::new(Brightness::Light),
    };
    let this = FluiContentView::alloc(main).set_ivars(ContainedIvars::new(context));
    // SAFETY: `initWithFrame:` is `NSView`'s designated initializer, sent once
    // to the instance just allocated with its ivars set.
    let view: Retained<FluiContentView> = unsafe { msg_send![super(this), initWithFrame: frame] };
    view.ivars()
        .appearance
        .set(effective_appearance(view.as_super()));
    view.ivars()
        .text
        .bind(Retained::as_ptr(&view).cast_mut().cast());
    // Layer-backed from the start, so the view's `FluiMetalLayer` is there
    // before a renderer builds a surface on the view, and it reports to this
    // window. A view that makes its own backing layer is not redrawn unless it
    // says when: on `setNeedsDisplay:` and through a live resize
    // (`NSViewLayerContentsRedrawDuringViewResize`), as a `drawRect:` view is.
    view.setWantsLayer(true);
    view.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::DuringViewResize);
    // A drawable of another size than the layer (a frame not drawn in time
    // for a step of a live resize) keeps its own size at the view's top-left
    // corner, clipped or with a strip beside it, where AppKit's default
    // stretches it to the layer and the next frame snaps it back. The
    // placement is AppKit's name for the backing layer's `contentsGravity`,
    // which it owns on a layer-backed view. Contents that are not stretched
    // are laid out by the layer's `contentsScale`, which the window keeps at
    // its backing scale (`set_layer_contents_scale`).
    view.setLayerContentsPlacement(NSViewLayerContentsPlacement::TopLeft);
    if let (Some(feedback), Some(reports)) = (
        view.backing_layer().as_deref().and_then(feedback_of),
        reports,
    ) {
        feedback.report_to(reports);
    }
    view
}

/// Run `body` with the content view `view`, a view reached through a raw
/// handle (a feature's [`ContentViewRef`](super::super::macos::ContentViewRef));
/// `None` for anything that is not a view this class built, so a caller needs
/// no check of its own. The view is handed to `body` rather than returned, so
/// the borrow cannot outlive the call that established the view was alive.
///
/// # Safety
///
/// `view` must be null or a live `NSView*`, on the main thread.
pub(crate) unsafe fn with_content_view<R>(
    view: *mut AnyObject,
    body: impl FnOnce(&ContentView<'_>) -> R,
) -> Option<R> {
    // SAFETY: per the function contract `view` is null or a live object.
    let view = unsafe { view.as_ref() }?;
    view.downcast_ref::<FluiContentView>()
        .map(|view| body(&view.content()))
}

/// Hand the input of `view` the window's input regions in its logical
/// coordinates, at the window's `scale`.
pub(crate) fn set_input_regions(
    view: &FluiContentView,
    regions: &[flui_platform::input::input_regions::InputRegion],
    scale: flui_foundation::geometry::DevicePixelRatio,
) {
    view.ivars().input.set_input_regions(regions, scale);
}

/// Tell the input of `view` whether its window holds the pointer locked.
pub(crate) fn set_pointer_locked(view: &FluiContentView, locked: bool) {
    view.ivars().input.set_pointer_locked(locked);
}

/// Tell the layer of `view` that the drawable it handed out last is about to
/// be presented showing `frame` (`None` for a present that shows no frame of
/// the window), so the drawable's report names it.
pub(crate) fn layer_presenting(view: &FluiContentView, frame: Option<flui_foundation::FrameStamp>) {
    if let Some(feedback) = view.backing_layer().as_deref().and_then(feedback_of) {
        feedback.presenting(frame);
    }
}

/// Tell the layer of `view` the refresh interval of the display its window is
/// on; `None` when variable or unknown.
pub(crate) fn set_layer_refresh(view: &FluiContentView, refresh: Option<std::time::Duration>) {
    if let Some(feedback) = view.backing_layer().as_deref().and_then(feedback_of) {
        feedback.set_refresh(refresh);
    }
}

/// Lay the contents of `view`'s layer out at `scale`, the scale its window's
/// drawables are sized by: device pixels per point.
///
/// The layer places its contents at their own size (`create`), which is the
/// drawable's pixels over this scale; at another scale a frame would show
/// larger or smaller than the view. The window calls this when it is made and
/// whenever its backing scale changes; wgpu sizes the drawable and leaves the
/// scale of a view's own layer alone.
pub(crate) fn set_layer_contents_scale(
    view: &FluiContentView,
    scale: flui_foundation::geometry::DevicePixelRatio,
) {
    if let Some(layer) = view
        .backing_layer()
        .and_then(|layer| layer.downcast::<FluiMetalLayer>().ok())
    {
        layer.setContentsScale(scale.get());
    }
}

/// The light/dark half of `object`'s effective appearance: the system theme,
/// unless the app or a window overrides it.
pub(crate) fn effective_appearance(
    object: &(impl NSAppearanceCustomization + objc2::Message),
) -> Brightness {
    // SAFETY: AppKit exports both names as constant strings that live for the
    // process; reading the extern statics only copies those references.
    let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark]);
    match object
        .effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)
    {
        Some(name) if &*name == dark => Brightness::Dark,
        _ => Brightness::Light,
    }
}

/// Have AppKit deliver mouse-moved, entered and exited events to `view` while
/// its window is key. The view retains the tracking area for its life.
pub(crate) fn enable_mouse_tracking(view: &NSView) {
    let options = NSTrackingAreaOptions::MouseMoved
        | NSTrackingAreaOptions::ActiveInKeyWindow
        | NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::InVisibleRect;
    // SAFETY: the owner is the view the area is added to, which outlives it,
    // and no user info is passed.
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            <NSTrackingArea as objc2::AnyThread>::alloc(),
            view.bounds(),
            options,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&area);
    tracing::debug!("Enabled mouse tracking for view");
}

// ============================================================================
// NSTextInputClient arguments and answers
// ============================================================================

/// The `string` argument of `insertText:replacementRange:` and
/// `setMarkedText:…` as Rust text. AppKit documents it as an `NSString`, but
/// an `NSAttributedString` arrives on the same path; anything else carries no
/// text.
fn text_of(string: &AnyObject) -> Option<String> {
    if let Some(attributed) = string.downcast_ref::<NSAttributedString>() {
        Some(attributed.string().to_string())
    } else {
        string.downcast_ref::<NSString>().map(NSString::to_string)
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
    if let Some(slot) = unsafe { actual_range.as_mut() } {
        *slot = range;
    }
}

/// `range` as a span of the document; `None` for `NSNotFound` or a range
/// whose end overflows.
fn utf16_range(range: NSRange) -> Option<Utf16Range> {
    if range.location == NSNotFound as usize {
        return None;
    }
    let end = range.location.checked_add(range.length)?;
    Utf16Range::try_new(Utf16Offset::new(range.location), Utf16Offset::new(end))
}

/// `range` as AppKit's, `{NSNotFound, 0}` for none.
fn ns_range(range: Option<Utf16Range>) -> NSRange {
    range.map_or_else(not_found, |range| {
        NSRange::new(range.start().get(), range.len())
    })
}
