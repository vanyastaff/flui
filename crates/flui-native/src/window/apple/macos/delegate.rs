//! What AppKit tells a window: its `NSWindowDelegate`, one per window, and the
//! handlers of what the delegate reports (resize, move, focus, occlusion,
//! close, backing scale and screen changes).

use std::rc::{Rc, Weak};
use std::sync::atomic::Ordering;

use flui_foundation::geometry::{Point, Size};
use flui_platform::DisplayId;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSScreen, NSWindow, NSWindowDelegate, NSWindowStyleMask};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSRect};

use super::MacOSWindow;
use crate::owner::apple::macos::HostDisplays;
use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};
use crate::window::apple::geometry::mode::{ANIMATION_LIMIT, HeldCommand};
use crate::window::state::MODE_ANSWER_LIMIT;

/// The display `screen` names, or `None` when there is none (a window not on
/// any screen) or the source leaves that screen out.
pub(super) fn display_of_screen(
    displays: &dyn HostDisplays,
    screen: Option<&NSScreen>,
) -> Option<DisplayId> {
    displays.id_of(screen?)
}

/// A delegate for `window`, made on the main thread `main` proves. AppKit
/// holds a window's delegate weakly, so the window keeps it
/// ([`MacOSWindow`]'s `delegate`) until it lets go of the `NSWindow`.
pub(super) fn create_window_delegate(
    window: Weak<MacOSWindow>,
    main: MainThreadMarker,
) -> Retained<WindowDelegate> {
    let this = WindowDelegate::alloc(main).set_ivars(ContainedIvars::new(window));
    // SAFETY: `init` is `NSObject`'s designated initializer, sent once to the
    // instance just allocated with its ivars set.
    unsafe { msg_send![super(this), init] }
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements; the ivars are Rust
    // values the class drops in its `dealloc` (`ContainedIvars`), and
    // `MainThreadOnly` matches AppKit, which sends a window delegate's
    // methods on the main thread only.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FLUIWindowDelegate"]
    #[ivars = ContainedIvars<Weak<MacOSWindow>>]
    /// A window's `NSWindowDelegate`: it holds its window weakly, and each
    /// method reaches the window only while it lives.
    ///
    /// Each method runs its body in the panic boundary: a panic answers
    /// AppKit as if the window were gone (nothing, or the default of a
    /// `windowShould…:` question) rather than unwinding through it.
    pub(super) struct WindowDelegate;

    // SAFETY: `NSObjectProtocol` has no requirements of its own.
    unsafe impl NSObjectProtocol for WindowDelegate {}

    // SAFETY: each method has the signature `NSWindowDelegate` declares for
    // its selector.
    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidResize:", |window| window.handle_resize());
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidMove:", |window| window.handle_move());
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidBecomeKey:", |window| window.handle_focus_gained());
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidResignKey:", |window| window.handle_focus_lost());
        }

        // Comes before the window leaves normal mode, while its frame is
        // still the one it returns to.
        #[unsafe(method(windowWillMiniaturize:))]
        fn window_will_miniaturize(&self, _notification: &NSNotification) {
            Self::notice(self, "windowWillMiniaturize:", |window| {
                window.record_normal();
                window.handle_transition_started();
                // A window in the Dock holds no pointer, and its snapshot
                // says so.
                if window.let_pointer_go() {
                    window.publish_state();
                }
            });
        }

        // This and `windowDidEndLiveResize:` bracket the user's resize by the
        // frame.
        #[unsafe(method(windowWillStartLiveResize:))]
        fn window_will_start_live_resize(&self, _notification: &NSNotification) {
            Self::notice(self, "windowWillStartLiveResize:", |window| {
                window.handle_live_resize(true);
            });
        }

        #[unsafe(method(windowDidEndLiveResize:))]
        fn window_did_end_live_resize(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidEndLiveResize:", |window| {
                window.handle_live_resize(false);
            });
        }

        // The full-screen bit may already be set while the frame is still the
        // normal one.
        #[unsafe(method(windowWillEnterFullScreen:))]
        fn window_will_enter_full_screen(&self, _notification: &NSNotification) {
            Self::notice(self, "windowWillEnterFullScreen:", |window| {
                window.record_normal_entering_fullscreen();
                // Full screen wins over a level while it lasts; the level
                // comes back when the window leaves it.
                window.apply_level(true);
                window.handle_fullscreen_transition_started(true);
            });
        }

        #[unsafe(method(windowWillExitFullScreen:))]
        fn window_will_exit_full_screen(&self, _notification: &NSNotification) {
            Self::notice(self, "windowWillExitFullScreen:", |window| {
                window.handle_fullscreen_transition_started(false);
            });
        }

        // Asked before a zoom, a programmatic `zoom:` included. It never
        // vetoes.
        #[unsafe(method(windowShouldZoom:toFrame:))]
        fn window_should_zoom(&self, _window: &NSWindow, _new_frame: NSRect) -> bool {
            os_entry(
                "windowShouldZoom:toFrame:",
                (),
                AfterPanic::Answer(|()| true),
                || {
                    if let Some(window) = self.window() {
                        window.record_normal();
                    }
                    true
                },
            )
        }

        // The six below end the step that was animating.
        #[unsafe(method(windowDidMiniaturize:))]
        fn window_did_miniaturize(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidMiniaturize:", MacOSWindow::handle_transition_ended);
        }

        #[unsafe(method(windowDidDeminiaturize:))]
        fn window_did_deminiaturize(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidDeminiaturize:", MacOSWindow::handle_transition_ended);
        }

        #[unsafe(method(windowDidEnterFullScreen:))]
        fn window_did_enter_full_screen(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidEnterFullScreen:", MacOSWindow::handle_transition_ended);
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidExitFullScreen:", MacOSWindow::handle_transition_ended);
        }

        #[unsafe(method(windowDidFailToEnterFullScreen:))]
        fn window_did_fail_to_enter_full_screen(&self, _window: &NSWindow) {
            Self::notice(self, "windowDidFailToEnterFullScreen:", MacOSWindow::handle_transition_ended);
        }

        #[unsafe(method(windowDidFailToExitFullScreen:))]
        fn window_did_fail_to_exit_full_screen(&self, _window: &NSWindow) {
            Self::notice(self, "windowDidFailToExitFullScreen:", MacOSWindow::handle_transition_ended);
        }

        // A close request whose handling panicked is refused: the window
        // stays, and the next request asks again. A window already gone
        // closes.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            os_entry(
                "windowShouldClose:",
                (),
                AfterPanic::Answer(|()| false),
                || self.window().is_none_or(|window| window.handle_close_request()),
            )
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            Self::notice(self, "windowWillClose:", |window| {
                // Pin the NSWindow before user callbacks can drop the final
                // wrapper; transferring that extra retain to AppKit's
                // surrounding autorelease pool keeps the receiver alive until
                // the native close stack has returned. Nested callback pools
                // cannot drain this already-enqueued pin.
                let _ = Retained::autorelease_ptr(Retained::clone(&window.ns_window));
                window.handle_close();
            });
        }

        // A Retina or scale change.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing_properties(&self, _notification: &NSNotification) {
            Self::notice(self,
                "windowDidChangeBackingProperties:",
                |window| window.handle_backing_properties_changed(),
            );
        }

        // The window moved to another screen.
        #[unsafe(method(windowDidChangeScreen:))]
        fn window_did_change_screen(&self, _notification: &NSNotification) {
            Self::notice(self, "windowDidChangeScreen:", |window| window.handle_screen_changed());
        }

        // Full occlusion by other windows, miniaturization, hide and unhide,
        // and Space switches: the visible bit is the claim the runtime's
        // frame clock gate consumes.
        #[unsafe(method(windowDidChangeOcclusionState:))]
        fn window_did_change_occlusion_state(&self, _notification: &NSNotification) {
            Self::notice(self,
                "windowDidChangeOcclusionState:",
                |window| window.handle_occlusion_state_changed(),
            );
        }
    }
);

impl WindowDelegate {
    /// The window, while it lives. AppKit sends the delegate's methods on the
    /// main thread, where the window and its reference counts live.
    fn window(&self) -> Option<Rc<MacOSWindow>> {
        self.ivars().upgrade()
    }

    /// A notification AppKit sent as `selector`: `handle` runs with the
    /// window, if it still lives, inside the panic boundary.
    fn notice(&self, selector: &'static str, handle: impl FnOnce(&Rc<MacOSWindow>)) {
        os_entry(selector, (), AfterPanic::Answer(|()| ()), || {
            if let Some(window) = self.window() {
                handle(&window);
            }
        });
    }
}

impl std::fmt::Debug for WindowDelegate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowDelegate").finish_non_exhaustive()
    }
}

// ============================================================================
// Window Event Handlers
// ============================================================================

impl MacOSWindow {
    /// A step that animates has started, from this backend or from the user.
    fn handle_transition_started(&self) {
        self.state
            .borrow_mut()
            .sequence
            .step_started(std::time::Instant::now());
    }

    /// A full-screen transition has started, `entering` full screen or
    /// leaving it, from a command or from the user (the green button, the
    /// system's shortcut): the user's is the user's own request through the
    /// system's chrome, which the window carries out and reports, as a window
    /// manager's maximize button is on Linux. The snapshot keeps the mode the
    /// window left until the transition lands
    /// (`ModeSequence::fullscreen_transition`).
    fn handle_fullscreen_transition_started(&self, entering: bool) {
        let flags = self.read_flags();
        let lands_in = self.state.borrow_mut().sequence.fullscreen_transition(
            std::time::Instant::now(),
            entering,
            flags,
        );
        self.wait_out_mode(lands_in, ANIMATION_LIMIT);
    }

    /// A step that animates is over (or failed): publish the window, then send
    /// the mode held back for it, if there is one.
    ///
    /// The held mode is planned on the owner lane's next turn. AppKit puts back
    /// the frame a window had before full screen only after
    /// `windowDidExitFullScreen:` returns, so inside it a window that went full
    /// screen zoomed still reads as unzoomed, and a held `Normal` would plan no
    /// un-zoom and leave it maximized. It is also taken after the publish,
    /// since a subscriber that runs in it may command a newer one. A window
    /// dropped before that turn has no mode left to take. The mode the
    /// snapshot waits for is waited for a short while more, past the frame
    /// AppKit puts back and the held mode's step.
    fn handle_transition_ended(self: &Rc<Self>) {
        self.state.borrow_mut().sequence.step_ended();
        if self.snapshot.mode_deadline().is_some() {
            self.snapshot
                .wait_longer(std::time::Instant::now() + MODE_ANSWER_LIMIT);
            self.read_again_after(MODE_ANSWER_LIMIT);
        }
        // A window that left full screen, or failed to enter it, stands at
        // its own level again; one still full screen stays at the normal one.
        let fullscreen = self.read_flags().fullscreen;
        self.apply_level(fullscreen);
        self.publish_state();
        let window = crate::owner::slot::OwnerWeak::new(self);
        crate::owner::apple::macos::exec_async_on_owner(
            crate::owner::apple::macos::owner_queue(),
            move || {
                let Some(window) = window.upgrade() else {
                    return;
                };
                // The held mode first, then the held commands in arrival order: a
                // fixed size meant for the normal window the held mode leads to
                // lands after it, held again if that mode set off another step.
                let after = window.state.borrow_mut().sequence.take_after_step();
                if let Some(mode) = after.mode
                    && let Err(error) = window.change_mode(mode)
                {
                    tracing::debug!(%error, "a window could not finish changing mode");
                }
                for command in after.commands {
                    match command {
                        HeldCommand::FixedSize => {
                            match window.set_style_bit(NSWindowStyleMask::Resizable, false) {
                                Ok(()) => window.publish_state(),
                                Err(error) => {
                                    tracing::debug!(%error, "a window could not be made fixed-size");
                                }
                            }
                        }
                    }
                }
            },
        );
    }

    /// Handle window resize event
    fn handle_resize(&self) {
        let content_rect = self
            .ns_window
            .contentRectForFrameRect(self.ns_window.frame());

        let new_size = Size::new(content_rect.size.width, content_rect.size.height);

        let moved_controls = {
            let mut state = self.state.borrow_mut();
            state.bounds.size = new_size;
            state.moves_system_controls()
        };
        // AppKit lays the traffic lights out again as it resizes the
        // window, and as it leaves full screen.
        if moved_controls {
            self.place_system_controls();
        }
        // A locked pointer rests over the content wherever it went.
        self.keep_lock_point();
        self.publish_state();

        tracing::trace!(
            handle = ?self.ns_window(),
            width = %new_size.width,
            height = %new_size.height,
            "window resized"
        );
    }

    /// Handle window move event
    fn handle_move(&self) {
        let frame = self.ns_window.frame();

        let new_origin = Point::new(frame.origin.x, frame.origin.y);

        // Update state
        {
            let mut state = self.state.borrow_mut();
            state.bounds.origin = new_origin;
        }

        // A locked pointer rests over the content wherever it went.
        self.keep_lock_point();
        self.publish_state();

        tracing::debug!(
            handle = ?self.ns_window(),
            x = %new_origin.x,
            y = %new_origin.y,
            "window moved"
        );
    }

    /// Handle focus gained event
    fn handle_focus_gained(&self) {
        self.text_key_window_changed();
        self.publish_state();
        // Key-window status is what VoiceOver treats as view focus; the
        // delegate delivers this on the main thread.
        self.facets.accessibility.focus_changed(true);
        tracing::debug!("Window gained focus");
    }

    /// Handle focus lost event
    fn handle_focus_lost(&self) {
        self.text_key_window_changed();
        // A grab ends with the focus, before the snapshot says so.
        self.let_pointer_go();
        self.publish_state();
        self.facets.accessibility.focus_changed(false);
        tracing::debug!("Window lost focus");
    }

    /// The content view's text hears the window's key status changed, first
    /// of all: a protected field holds secure event input only while its
    /// window is key, so another application's keyboard utilities work the
    /// moment this window resigns.
    fn text_key_window_changed(&self) {
        self.view.content().text().key_window_changed();
    }

    /// Handle occlusion state change (hidden-surface gating).
    ///
    /// Reads the window's current `occlusionState` into the snapshot's
    /// visibility — what the runtime's per-presentation `FrameClock` gate and
    /// `AppLifecycleState` derivation read. The bit test and its polarity are
    /// host-tested in `crate::window::apple::geometry::surface`; only the
    /// `occlusionState` read is AppKit-bound. AppKit may post the notification
    /// for reasons other than the visible bit flipping: an unchanged snapshot
    /// publishes nothing.
    fn handle_occlusion_state_changed(&self) {
        let raw = self.ns_window.occlusionState().0 as u64;
        let visible = crate::window::apple::geometry::surface::occlusion_state_is_visible(raw);
        self.state.borrow_mut().occlusion_visible = visible;
        // Published with the state not borrowed, like every other handler here.
        self.publish_state();
    }

    /// Handle close request event
    ///
    /// Returns true to allow close, false to prevent. Asked behind a running
    /// handler method (a `performClose:` sent from inside `input`), the
    /// window is kept for now and closed later if the handler grants it.
    fn handle_close_request(self: &Rc<Self>) -> bool {
        let window = crate::owner::slot::OwnerWeak::new(self);
        let decision = self.delivery.dispatch_close_request(move || {
            if let Some(window) = window.upgrade() {
                window.close_window();
            }
        });
        tracing::debug!(?decision, "Window close requested");
        decision == flui_platform::CloseDecision::Close
    }

    /// Handle window close event
    ///
    /// Fires from `windowWillClose:` while `ns_window` is still valid — the
    /// delegate notification AppKit posts for every route through
    /// `-[NSWindow close]`, vetoed or not (see the comment in
    /// [`close_window`](Self::close_window)). Order matters: `closed` is set BEFORE `dispatch_close` so that
    /// a frame callback the close dispatch itself triggers observes the
    /// window as already gone, and the delivery's `clear()` runs AFTER, so
    /// the handler's `closed` is still called before the window lets go of
    /// the handler.
    fn handle_close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // Every step below runs whichever one before it panics (`closed`, a
        // delivery queued ahead of it, the release, a tracing subscriber):
        // the window's `Drop` finds `closed` set and releases nothing, so a
        // close once begun always finishes here. The first panic resumes at
        // the end.
        let mut failure = flui_foundation::panic::FirstPanic::new();
        // A closed window holds no pointer: the mouse is attached again
        // before anyone hears of the close.
        failure.contain(|| self.let_pointer_go());

        // Untrack from the platform's window map here, on the owner thread the
        // delegate notification runs on: the map holds the window strongly, so
        // removing the entry lets the window drop (and so release its NSWindow
        // and detach its bridge) as soon as the application drops its last
        // handle; without it, every closed window stays pinned for the process
        // lifetime.
        //
        // Removal precedes `dispatch_close()` so the closing window is already
        // untracked when user close callbacks run. This ordering is safe here
        // because the macOS map is private with no content readers: the early
        // removal is observable only as the intended lifetime change. The
        // removed entry drops at the end of this statement's scope, and the
        // delegate holds the window meanwhile.
        let _prev = self.windows_map.remove(&self.native_key());

        failure.contain(|| {
            self.delivery.dispatch_close();
            tracing::debug!("Window closed");
        });

        // Release every remaining registered callback now, while
        // `ns_window` is still valid (this delegate method runs before
        // AppKit tears the window down further): the handler owns, in
        // `flui-app`'s wiring, this window's GPU renderer, whose
        // `wgpu::Surface` is built from the window's surface target, and that
        // surface goes while the window's view is still there (ADR-0063).
        // The clear also retires the delivery, so the surface target answers
        // `Unavailable` from here on. A keep recorded for the window before
        // the handler let go, usually while it was open, keeps its view past
        // the close.
        failure.contain(|| self.clear_then_keep_if_kept());

        // Match winit's own `windowWillClose:` handling: clear the delegate
        // so no further delegate method can fire against a window this
        // wrapper now treats as closed. Harmless even without this today
        // (the delegate holds only a weak reference to the window), but this
        // is the documented AppKit-recommended cleanup, not just a FLUI habit.
        self.ns_window.setDelegate(None);
        if let Some(control) = self.loop_control.get().and_then(std::sync::Weak::upgrade) {
            failure.contain(|| control.request(self.main, false));
        }
        failure.resume();
    }

    /// Handle backing properties changed (Retina/DPI change)
    fn handle_backing_properties_changed(&self) {
        let reported = self.ns_window.backingScaleFactor();

        // Update window state, by the hosts' one rule; the refusal and
        // the change are reported once the state is let go.
        let (changed, new_scale, refused) = {
            let mut state = self.state.borrow_mut();
            let (new_scale, refused) = state
                .scale_streak
                .read(reported, "NSWindow backingScaleFactor");
            let changed = (state.scale_factor.get() - new_scale.get()).abs() > 0.01;
            if changed {
                state.scale_factor = new_scale;
            }
            (changed, new_scale, refused)
        };
        if let Some(refused) = refused {
            refused.report();
        }
        if changed {
            tracing::info!(handle = ?self.ns_window(), scale = new_scale.get(), "window scale changed");
            // An image cursor is measured in points at the scale.
            self.rescale_cursor();
        }

        // Re-read the display in the same pass: this handler is reached on
        // both a backing-properties change and a screen change
        // (`handle_screen_changed` delegates here), and the display belongs
        // to the window exactly as the scale does.
        let moved = self.refresh_display();
        if moved || changed {
            // The display snapshot carries each display's scale, and a
            // window that moved may sit on a display the last sample did
            // not hold. Invalidated before the window's snapshot is
            // published, so a consumer it wakes reads the displays again.
            if let Some(displays) = self.displays.get() {
                displays.invalidate();
            }
        }
        // After the invalidation, so the snapshot read is the one with the
        // display the window moved to.
        self.sync_layer_refresh();
        // On another screen a locked pointer is put back over the content.
        self.keep_lock_point();
        self.publish_state();
    }

    /// Handle screen changed (moved to different monitor)
    fn handle_screen_changed(&self) {
        if let Some(screen) = self.ns_window.screen() {
            let scale = screen.backingScaleFactor();
            tracing::debug!(handle = ?self.ns_window(), scale, "window moved to another screen");

            // Update scale factor (will trigger backing properties changed)
            self.handle_backing_properties_changed();
        } else if self.refresh_display() {
            // Off every screen: the window names no display until it is
            // on one again, and its scale is left as it was.
            self.publish_state();
        }
    }
}
