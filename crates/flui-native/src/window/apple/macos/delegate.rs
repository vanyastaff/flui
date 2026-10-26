//! What AppKit tells a window: its `NSWindowDelegate` class, built once per
//! process, and the handlers of what the delegate reports (resize, move,
//! focus, occlusion, close, backing scale and screen changes).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use flui_foundation::geometry::{Point, Size};
use flui_platform::DisplayId;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{ClassType, class, msg_send, sel};
use objc2_foundation::NSRect;

use super::{BObjC, Class, MacOSWindow, NIL, NO, ObjcId, Object, YES};
use crate::owner::apple::macos::HostDisplays;
use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// The display a raw `NSScreen*` names, or `None` when the pointer is nil (a
/// window not on any screen) or the source leaves that screen out.
///
/// # Safety
///
/// `screen` must be null or a live `NSScreen*`, and the call must run on the
/// owner lane (this messages the screen).
pub(super) unsafe fn display_of_screen(
    displays: &dyn HostDisplays,
    screen: ObjcId,
) -> Option<DisplayId> {
    // SAFETY: the caller's contract — nil or a live `NSScreen*` on the owner
    // lane; the borrow is scoped to the `and_then` closure.
    let screen = unsafe { screen.as_ref() }?;
    // SAFETY: `NSScreen` and `AnyObject` share the Objective-C object layout,
    // and the caller guarantees the pointee really is an `NSScreen`.
    let screen: &objc2_app_kit::NSScreen =
        unsafe { &*std::ptr::from_ref::<AnyObject>(screen).cast() };
    displays.id_of(screen)
}

/// Create a window delegate for lifecycle events
pub(super) fn create_window_delegate(window: Weak<MacOSWindow>) -> ObjcId {
    // SAFETY: the delegate class is registered before alloc/init; the boxed
    // Weak pointer stored in the ivar is reclaimed in the delegate's
    // `dealloc`, so it lives exactly as long as the delegate.
    unsafe {
        // Get or create delegate class
        let class = get_or_create_delegate_class();
        let delegate: ObjcId = msg_send![class, alloc];
        let delegate: ObjcId = msg_send![delegate, init];

        // Store weak pointer to window
        let window_ptr = Box::into_raw(Box::new(window)).cast::<std::ffi::c_void>();
        let ivar = class
            .instance_variable(c"window_ptr")
            .expect("BUG: FLUIWindowDelegate declares the window_ptr ivar");
        let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(&*delegate);
        *slot = window_ptr;

        delegate
    }
}

/// Get or create the NSWindowDelegate class
fn get_or_create_delegate_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = objc2_foundation::NSObject::class();
        let mut decl = ClassBuilder::new(c"FLUIWindowDelegate", superclass)
            .expect("FLUIWindowDelegate must be registered exactly once (guarded by Once)");

        // Add ivar to store window pointer
        decl.add_ivar::<*mut std::ffi::c_void>(c"window_ptr");

        // windowDidResize:
        // Each method runs its body in the panic boundary: a panic answers
        // AppKit as if the window were gone (`()`, or the default of a
        // `windowShould…:` question) rather than unwinding through it.

        extern "C-unwind" fn window_did_resize(this: &Object, _sel: Sel, _notification: ObjcId) {
            os_entry("windowDidResize:", (), AfterPanic::Answer(|()| ()), || {
                // SAFETY: AppKit invokes delegate methods on live delegate objects.
                if let Some(window) = unsafe { get_window_from_delegate(this) } {
                    window.handle_resize();
                }
            });
        }

        // windowDidMove:
        extern "C-unwind" fn window_did_move(this: &Object, _sel: Sel, _notification: ObjcId) {
            os_entry("windowDidMove:", (), AfterPanic::Answer(|()| ()), || {
                // SAFETY: AppKit invokes delegate methods on live delegate objects.
                if let Some(window) = unsafe { get_window_from_delegate(this) } {
                    window.handle_move();
                }
            });
        }

        // windowDidBecomeKey:
        extern "C-unwind" fn window_did_become_key(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowDidBecomeKey:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: AppKit invokes delegate methods on live delegate objects.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_focus_gained();
                    }
                },
            );
        }

        // windowDidResignKey:
        extern "C-unwind" fn window_did_resign_key(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowDidResignKey:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: AppKit invokes delegate methods on live delegate objects.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_focus_lost();
                    }
                },
            );
        }

        // The delegate callbacks below share one invariant: they run on the
        // main thread, on a live `FLUIWindowDelegate` whose `window_ptr` ivar
        // holds the `Box<Weak<MacOSWindow>>` that `create_window_delegate`
        // stored, which is what `get_window_from_delegate` requires.

        // windowWillMiniaturize: comes before the window leaves normal mode,
        // while its frame is still the one it returns to.
        extern "C-unwind" fn window_will_miniaturize(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowWillMiniaturize:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: see the shared invariant above.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.record_normal();
                        window.handle_transition_started();
                    }
                },
            );
        }

        // windowWillEnterFullScreen: likewise; the full-screen bit may already
        // be set while the frame is still the normal one.
        extern "C-unwind" fn window_will_enter_fullscreen(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowWillEnterFullScreen:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: see the shared invariant above.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.record_normal_entering_fullscreen();
                        window.handle_transition_started();
                    }
                },
            );
        }

        // windowWillExitFullScreen:
        extern "C-unwind" fn window_will_exit_fullscreen(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowWillExitFullScreen:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: see the shared invariant above.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_transition_started();
                    }
                },
            );
        }

        // windowShouldZoom:toFrame: is asked before a zoom, and a programmatic
        // `zoom:` asks it too. It never vetoes.
        extern "C-unwind" fn window_should_zoom(
            this: &Object,
            _sel: Sel,
            _window: ObjcId,
            _new_frame: NSRect,
        ) -> BObjC {
            os_entry(
                "windowShouldZoom:toFrame:",
                (),
                AfterPanic::Answer(|()| YES),
                || {
                    // SAFETY: see the shared invariant above.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.record_normal();
                    }
                    YES
                },
            )
        }

        // windowDidMiniaturize:, windowDidDeminiaturize:,
        // windowDidEnterFullScreen:, windowDidExitFullScreen: and the two
        // windowDidFailTo…FullScreen: end the step that was animating.
        extern "C-unwind" fn window_transition_ended(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "a window transition's end",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: see the shared invariant above.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_transition_ended();
                    }
                },
            );
        }

        // windowShouldClose: — a close request whose handling panicked is
        // refused: the window stays, and the next request asks again.
        extern "C-unwind" fn window_should_close(
            this: &Object,
            _sel: Sel,
            _sender: ObjcId,
        ) -> BObjC {
            os_entry(
                "windowShouldClose:",
                (),
                AfterPanic::Answer(|()| NO),
                || {
                    // SAFETY: AppKit invokes delegate methods on live delegate objects.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        if window.handle_close_request() {
                            YES
                        } else {
                            NO
                        }
                    } else {
                        YES
                    }
                },
            )
        }

        // windowWillClose:
        extern "C-unwind" fn window_will_close(this: &Object, _sel: Sel, _notification: ObjcId) {
            os_entry("windowWillClose:", (), AfterPanic::Answer(|()| ()), || {
                // SAFETY: AppKit invokes delegate methods on live delegate objects.
                if let Some(window) = unsafe { get_window_from_delegate(this) } {
                    // SAFETY: the wrapper retains this live NSWindow. Pin it before
                    // user callbacks can drop the final wrapper; transferring that
                    // extra retain to AppKit's surrounding autorelease pool keeps
                    // the receiver alive until the native close stack has returned.
                    // Nested callback pools cannot drain this already-enqueued pin.
                    if let Some(pin) = unsafe { objc2::rc::Retained::retain(window.ns_window) } {
                        let _ = objc2::rc::Retained::autorelease_ptr(pin);
                    }
                    window.handle_close();
                }
            });
        }

        // windowDidChangeBackingProperties: (Retina/DPI change)
        extern "C-unwind" fn window_did_change_backing_properties(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowDidChangeBackingProperties:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: AppKit invokes delegate methods on live delegate
                    // objects.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_backing_properties_changed();
                    }
                },
            );
        }

        // windowDidChangeScreen: (moved to different monitor)
        extern "C-unwind" fn window_did_change_screen(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowDidChangeScreen:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: AppKit invokes delegate methods on live delegate objects.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_screen_changed();
                    }
                },
            );
        }

        // windowDidChangeOcclusionState: (hidden-surface gating — fires on
        // full occlusion by other windows, miniaturization, hide/unhide,
        // and space switches; the visible bit is the surface-composed
        // claim the runtime's FrameClock gate consumes)
        extern "C-unwind" fn window_did_change_occlusion_state(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            os_entry(
                "windowDidChangeOcclusionState:",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: AppKit invokes delegate methods on live delegate
                    // objects.
                    if let Some(window) = unsafe { get_window_from_delegate(this) } {
                        window.handle_occlusion_state_changed();
                    }
                },
            );
        }

        // dealloc — reclaim the boxed Weak<MacOSWindow>
        extern "C-unwind" fn delegate_dealloc(this: &Object, _sel: Sel) {
            os_entry(
                "FLUIWindowDelegate's dealloc",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: the ivar holds either null or a Box<Weak<MacOSWindow>>
                    // leaked in `create_window_delegate`; reclaiming it exactly once
                    // on dealloc is the matching release. The super dealloc message
                    // is the mandatory NSObject teardown.
                    unsafe {
                        let window_ptr: *mut std::ffi::c_void = *this.get_ivar("window_ptr");
                        if !window_ptr.is_null() {
                            drop(Box::from_raw(window_ptr.cast::<Weak<MacOSWindow>>()));
                        }
                        let superclass = class!(NSObject);
                        let _: () = msg_send![super(this, superclass), dealloc];
                    }
                },
            );
        }

        // Add methods
        // SAFETY: every registered function pointer matches the Objective-C
        // method signature of its selector, as required by
        // `ClassDecl::add_method`.
        unsafe {
            decl.add_method(
                sel!(windowDidResize:),
                window_did_resize as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidMove:),
                window_did_move as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidBecomeKey:),
                window_did_become_key as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidResignKey:),
                window_did_resign_key as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowWillMiniaturize:),
                window_will_miniaturize as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowWillEnterFullScreen:),
                window_will_enter_fullscreen as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowWillExitFullScreen:),
                window_will_exit_fullscreen as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowShouldZoom:toFrame:),
                window_should_zoom as extern "C-unwind" fn(_, _, _, _) -> Bool,
            );
            for selector in [
                sel!(windowDidMiniaturize:),
                sel!(windowDidDeminiaturize:),
                sel!(windowDidEnterFullScreen:),
                sel!(windowDidExitFullScreen:),
                sel!(windowDidFailToEnterFullScreen:),
                sel!(windowDidFailToExitFullScreen:),
            ] {
                decl.add_method(
                    selector,
                    window_transition_ended as extern "C-unwind" fn(_, _, _),
                );
            }
            decl.add_method(
                sel!(windowShouldClose:),
                window_should_close as extern "C-unwind" fn(_, _, _) -> Bool,
            );
            decl.add_method(
                sel!(windowWillClose:),
                window_will_close as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidChangeBackingProperties:),
                window_did_change_backing_properties as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidChangeScreen:),
                window_did_change_screen as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidChangeOcclusionState:),
                window_did_change_occlusion_state as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(dealloc),
                delegate_dealloc as extern "C-unwind" fn(_, _),
            );
        }

        decl.register();
    });

    AnyClass::get(c"FLUIWindowDelegate")
        .expect("FLUIWindowDelegate was registered by the Once block above")
}

/// Get window from delegate
///
/// # Safety
///
/// `delegate` must be a live FLUIWindowDelegate whose `window_ptr` ivar is
/// either null or points to a `Weak<MacOSWindow>` owned by that delegate.
unsafe fn get_window_from_delegate(delegate: &Object) -> Option<Arc<MacOSWindow>> {
    // SAFETY: per the function contract the ivar is null or a valid
    // Box<Weak<MacOSWindow>> pointer owned by the delegate.
    unsafe {
        let window_ptr: *mut std::ffi::c_void = *delegate.get_ivar("window_ptr");
        let weak_ptr = window_ptr.cast::<Weak<MacOSWindow>>();
        if weak_ptr.is_null() {
            return None;
        }
        (*weak_ptr).upgrade()
    }
}

// ============================================================================
// Window Event Handlers
// ============================================================================

impl MacOSWindow {
    /// A step that animates has started, from this backend or from the user.
    fn handle_transition_started(&self) {
        self.state
            .lock()
            .sequence
            .step_started(std::time::Instant::now());
    }

    /// A step that animates is over (or failed): publish the window, then send
    /// the mode held back for it, if there is one. The held mode is taken after
    /// the publish, since a subscriber that runs in it may command a newer one.
    fn handle_transition_ended(&self) {
        self.state.lock().sequence.step_ended();
        self.publish_state();
        let held = self.state.lock().sequence.take_held();
        if let Some(mode) = held
            && let Err(error) = self.change_mode(mode)
        {
            tracing::debug!(%error, "a window could not finish changing mode");
        }
    }

    /// Handle window resize event
    fn handle_resize(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let content_rect: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];

            let new_size = Size::new(
                content_rect.size.width as f64,
                content_rect.size.height as f64,
            );

            // Update state
            let scale = {
                let mut state = self.state.lock();
                state.bounds.size = new_size;
                state.scale_factor
            };

            // Notify per-window callbacks
            self.publish_state();
            self.callbacks
                .dispatch_logical_resize(new_size, scale as f64);

            tracing::debug!("Window resized to {}x{}", new_size.width, new_size.height);
        }
    }

    /// Handle window move event
    fn handle_move(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];

            let new_origin = Point::new(frame.origin.x as f64, frame.origin.y as f64);

            // Update state
            {
                let mut state = self.state.lock();
                state.bounds.origin = new_origin;
            }

            self.publish_state();

            tracing::debug!("Window moved to ({}, {})", new_origin.x, new_origin.y);
        }
    }

    /// Handle focus gained event
    fn handle_focus_gained(&self) {
        self.publish_state();
        self.callbacks.dispatch_active_status_change(true);
        // Key-window status is what VoiceOver treats as view focus; the
        // delegate delivers this on the main thread.
        if let Some(bridge) = self.accessibility.get() {
            bridge.focus_changed(true);
        }
        tracing::debug!("Window gained focus");
    }

    /// Handle focus lost event
    fn handle_focus_lost(&self) {
        self.publish_state();
        self.callbacks.dispatch_active_status_change(false);
        if let Some(bridge) = self.accessibility.get() {
            bridge.focus_changed(false);
        }
        tracing::debug!("Window lost focus");
    }

    /// Handle occlusion state change (hidden-surface gating).
    ///
    /// Reads the window's current `occlusionState` and forwards the visible
    /// bit through `dispatch_visibility_status_change` — the wire the
    /// runtime's per-presentation `FrameClock` gate and `AppLifecycleState`
    /// derivation hang off. The decidable half is host-tested: the bit test
    /// and its polarity in `crate::window::apple::geometry::surface`, the edge
    /// filter in `crate::window::visibility`; only the `occlusionState` read
    /// is AppKit-bound.
    fn handle_occlusion_state_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`;
        // `occlusionState` returns a plain NSUInteger bitmask.
        let raw: u64 = unsafe {
            let state: usize = msg_send![self.ns_window, occlusionState];
            state as u64
        };
        let visible = crate::window::apple::geometry::surface::occlusion_state_is_visible(raw);
        let edge = self.state.lock().occlusion_visible.report(visible);
        // Dispatch outside the state lock, like every other handler here.
        if let Some(visible) = edge {
            tracing::debug!(?visible, "Window occlusion state changed");
            self.publish_state();
            self.callbacks.dispatch_visibility_status_change(visible);
        }
    }

    /// Handle close request event
    ///
    /// Returns true to allow close, false to prevent
    fn handle_close_request(&self) -> bool {
        let should_close = self.callbacks.dispatch_should_close();
        tracing::debug!("Window close requested: {}", should_close);
        should_close
    }

    /// Handle window close event
    ///
    /// Fires from `windowWillClose:` while `ns_window` is still valid — the
    /// delegate notification AppKit posts for every route through
    /// `-[NSWindow close]`, vetoed or not (see [`close`](Self::close)'s own
    /// doc). Order matters: `closed` is set BEFORE `dispatch_close` so that
    /// a frame callback the close dispatch itself triggers observes the
    /// window as already gone, and `callbacks.clear()` runs AFTER, so the
    /// `on_close` callback registered above still fires normally before its
    /// slot — and every other slot — is released.
    fn handle_close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }

        // Untrack from the platform's window map here, on the owner thread the
        // delegate notification runs on, rather than from `Drop` — `Drop` is the
        // only other removal site and is unreachable while the map itself pins a
        // wrapper clone (issue #1147). Removing the entry lets the wrapper reach
        // `Drop`'s last-clone gate (and so the NSWindow release + a11y teardown
        // tail) as soon as the application drops its final external handle;
        // without it, every closed window stays pinned for the process lifetime.
        //
        // Removal precedes `dispatch_close()` so the closing window is already
        // untracked when user close callbacks run. Unlike the repo's winit
        // backend — which removes from tracking only AFTER its own close
        // callbacks (owner/winit.rs dispatch_close before windows.remove) —
        // this ordering is safe here because the macOS map is private with no
        // content readers: the early removal is observable only as the intended
        // lifetime change. `Drop`'s removal stays as an idempotent safety net.
        let _prev = self.windows_map.lock().remove(&(self.ns_window as u64));

        self.callbacks.dispatch_close();
        tracing::debug!("Window closed");

        // Release every remaining registered callback now, while
        // `ns_window` is still valid (this delegate method runs before
        // AppKit tears the window down further). Without this, the frame
        // callback registered via `on_request_frame` — which in
        // `flui-app`'s wiring owns this window's GPU renderer, whose
        // `wgpu::Surface` is built from the `Arc<dyn PlatformWindow>` clone
        // the renderer owns (ADR-0063) — stays pinned forever: window
        // (through its callback slots) → frame closure → raster lane →
        // renderer → surface → `Arc<MacOSWindow>`, a cycle nothing else
        // here breaks.
        self.callbacks.clear();

        // Match winit's own `windowWillClose:` handling: NIL the delegate
        // so no further delegate method can fire against a window this
        // wrapper now treats as closed. Harmless even without this today
        // (the delegate holds only a `Weak<MacOSWindow>`), but this is the
        // documented AppKit-recommended cleanup, not just a FLUI habit.
        //
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, setDelegate: NIL];
        }
        if let Some(control) = self.loop_control.get().and_then(std::sync::Weak::upgrade) {
            control.request(false);
        }
    }

    /// Handle backing properties changed (Retina/DPI change)
    fn handle_backing_properties_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // content view is NIL-checked before use.
        unsafe {
            let new_scale: f64 = msg_send![self.ns_window, backingScaleFactor];

            // Update window state
            let (changed, size) = {
                let mut state = self.state.lock();
                let changed = (state.scale_factor - new_scale).abs() > 0.01;
                if changed {
                    state.scale_factor = new_scale;
                    tracing::info!("Window scale factor changed to {}", new_scale);
                }
                (changed, state.bounds.size)
            };

            // Re-read the display in the same pass: this handler is reached on
            // both a backing-properties change and a screen change
            // (`handle_screen_changed` delegates here), and the display belongs
            // to the window exactly as the scale does. A change of display or
            // of its scale is a hint to re-read the snapshot; it dispatches no
            // resize, because a refresh period does not invalidate layout.
            let moved = self.refresh_display();
            self.publish_state();

            if moved || changed {
                // The snapshot carries each display's scale, and a window that
                // moved may sit on a display the last sample did not hold.
                // Invalidated before any callback so a resize handler that
                // reads the snapshot sees the new scale.
                if let Some(displays) = self.displays.get() {
                    displays.invalidate();
                }
            }

            // A scale change invalidates layout: notify as a resize
            if changed {
                self.callbacks.dispatch_logical_resize(size, new_scale);
            }
            if moved || changed {
                self.callbacks.dispatch_display_changed();
            }
        }
    }

    /// Handle screen changed (moved to different monitor)
    fn handle_screen_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // screen object is NIL-checked before messaging.
        unsafe {
            let screen: ObjcId = msg_send![self.ns_window, screen];
            if screen != NIL {
                let scale: f64 = msg_send![screen, backingScaleFactor];
                tracing::debug!("Window moved to screen with scale factor {}", scale);

                // Update scale factor (will trigger backing properties changed)
                self.handle_backing_properties_changed();
            } else if self.refresh_display() {
                // Off every screen: the window names no display until it is
                // on one again, and its scale is left as it was.
                self.callbacks.dispatch_display_changed();
            }
        }
    }
}
