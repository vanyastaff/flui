//! The Win32 input: the input messages of FLUI's window class (the mouse and
//! its buttons and wheel, the pointer messages, the keyboard and its
//! characters, the keyboard layout), their decoding (`raw`) and the precision
//! touchpad (`direct_manipulation`).
//!
//! `InputWindows` is the input's hook on the class, which the Windows host
//! registers beside the window's own: it answers the input messages and the
//! precision touchpad's hit test and pacing timer, and keeps each window's
//! input (`InputWindow`: its message clock, pointer registry, touchpad
//! viewport, hover, modifiers and the keys a keystroke leaves behind) in
//! itself, keyed by the window's identity, from the class's announcement of
//! the window to its destruction. The steps of the window's own procedure the
//! input follows (its size, focus, paint and DPI, the modal size-and-move
//! loop, a setting change) reach it as the class's steps
//! (`WindowMessages::step`). What the window, drag and drop and haptics read
//! of a window's input it answers read-only (`WindowInputs`).

// The FFI half calls Win32 directly: `unsafe` is expected on Windows only,
// where it is compiled; the pure halves forbid it.
#![cfg_attr(windows, expect(unsafe_code))]

pub mod direct_manipulation;

#[cfg(windows)]
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
        WindowsAndMessaging::{
            DM_POINTERHITTEST, DefWindowProcW, ISMEX_NOSEND, InSendMessageEx, WM_CAPTURECHANGED,
            WM_CHAR, WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
            WM_NCDESTROY, WM_POINTERCAPTURECHANGED, WM_POINTERDEVICECHANGE, WM_POINTERDOWN,
            WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE, WM_RBUTTONDOWN,
            WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP,
        },
    },
};

#[cfg(windows)]
use crate::input::assembly::ContactRegistry;
#[cfg(windows)]
use crate::owner::call_point::{Field, call};
#[cfg(windows)]
use crate::owner::windows::control::OwnerPreferences;
#[cfg(windows)]
use crate::owner::windows::{
    CreatedWindow, WindowInputs, WindowMessages, WindowRef, WindowScale, WindowStep, WindowTable,
    WindowsLoop, window_step,
};
#[cfg(windows)]
use crate::owner::{window_delivery::WindowDelivery, windows::AppHooks};
#[cfg(windows)]
use flui_platform::Modifiers;

#[cfg(windows)]
use keystroke_queue::{
    drain_dead_char, drain_translated_chars, restore_translated_sys_chars,
    take_translated_sys_chars,
};

/// The input position of the Windows host's table: the loop answers it, with
/// the active keyboard layout and the platform handler its change reaches.
#[cfg(windows)]
impl crate::host::Input<WindowsLoop> for WindowsLoop {
    fn keyboard_layout(_: &WindowsLoop) -> String {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayoutNameW;
        // SAFETY: `buffer` is a stack-local, zero-initialized `[u16; 9]`
        // sized to `KL_NAMELENGTH`, the exact size this API requires;
        // `&mut buffer` gives it a valid, correctly-sized out-parameter, and
        // it is only read (`is_ok()` branch) after the call reports success.
        unsafe {
            let mut buffer = [0u16; 9]; // KL_NAMELENGTH = 9
            if GetKeyboardLayoutNameW(&mut buffer).is_ok() {
                String::from_utf16_lossy(
                    &buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())],
                )
            } else {
                String::new()
            }
        }
    }

    fn on_keyboard_layout_change(owner: &WindowsLoop, callback: Box<dyn FnMut() + Send>) {
        owner.register_handler("on_keyboard_layout_change", callback, |hooks| {
            &mut hooks.keyboard_layout_changed
        });
    }
}

/// The input of the windows of FLUI's class on one owner thread: the input's
/// hook on the class, and each window's input, keyed by the window's identity,
/// from the class's announcement of the window ([`WindowMessages::created`])
/// to its destruction ([`WindowMessages::destroyed`], or `WM_NCDESTROY` for a
/// window whose destruction was not announced). A message, step or read for a
/// window it does not hold finds nothing and does nothing.
#[cfg(windows)]
#[derive(Default)]
pub(crate) struct InputWindows {
    windows: WindowTable<Rc<InputWindow>>,
}

#[cfg(windows)]
impl InputWindows {
    /// The input of a class on an owner thread, with no window yet.
    pub(crate) fn new() -> Rc<Self> {
        let input = Rc::new(Self::default());
        #[cfg(test)]
        testing::made(&input);
        input
    }

    /// The input of the window `hwnd` names, held for no borrow: a message
    /// that destroys the window drops the entry, not the input the message
    /// reaches.
    fn by_handle(&self, hwnd: HWND) -> Option<Rc<InputWindow>> {
        self.of(self.windows.window_of(hwnd)?)
    }

    /// The input of `window`, while its handle still names it.
    fn of(&self, window: WindowRef) -> Option<Rc<InputWindow>> {
        self.windows.read(window, Rc::clone)
    }

    /// Forget `input`'s window: it is destroyed. What the input still holds
    /// open of the touchpad ends as cancelled first, while the window's
    /// callbacks exist; the entry is dropped with no borrow held.
    fn forget(&self, input: &InputWindow) {
        input.gone.set(true);
        input.release_touchpad(input.window.hwnd());
        let removed = self.windows.take(input.window.identity());
        drop(removed);
    }
}

/// What the input keeps of one window of FLUI's class (see
/// [`InputWindows`]). Reached only on the window's thread.
#[cfg(windows)]
struct InputWindow {
    /// The window.
    window: WindowRef,
    /// The hooks of its class, which a key the application let through is
    /// told to ([`WindowStep::KeyLetThrough`]); weak, since the input is one
    /// of them.
    hooks: Weak<[Rc<dyn WindowMessages>]>,
    /// Its callbacks, which its input is dispatched to.
    delivery: Arc<WindowDelivery>,
    /// What the application gave the loop: a keyboard layout change reaches
    /// its callback.
    app_hooks: Rc<RefCell<AppHooks>>,
    /// The system preferences, which give its double-click interval and
    /// rectangle.
    preferences: Rc<dyn OwnerPreferences>,
    /// Its scale, which its procedure changes: positions arrive in device
    /// pixels.
    scale: WindowScale,
    /// The clock its messages and readings are stamped on.
    message_clock: raw::MessageClock,
    /// Whether the mouse is over its client area.
    hovered: Cell<bool>,
    /// The keyboard modifiers its last key message carried.
    modifiers: Cell<Modifiers>,
    /// Its pointer contacts and devices.
    registry: RefCell<ContactRegistry>,
    /// The precision touchpad's DirectManipulation viewport; empty when it
    /// did not initialise (the window then has no precise scrolling or
    /// trackpad gestures), after it failed, or once the window is destroyed.
    touchpad: direct_manipulation::TouchpadSlot,
    /// High half of a UTF-16 surrogate pair from an out-of-band `WM_CHAR`,
    /// held until its low half arrives in the next message (Windows splits
    /// astral-plane characters across two `WM_CHAR`s). Only the stray-char
    /// path uses this; the `WM_KEYDOWN` drain sees a whole burst at once and
    /// never needs cross-message state.
    pending_high_surrogate: Cell<Option<u16>>,
    /// Physical keys held down that were pressed as dead keys, so their
    /// release reports `Dead` like the press.
    held_dead_keys: RefCell<raw::HeldKeys>,
    /// Keys held down whose scancode names no code, with the key each was
    /// pressed as, so its repeats and release carry that key.
    unplaced_keys: RefCell<crate::input::keys::UnplacedKeys<(u16, bool)>>,
    /// Physical keys held down whose press an input method took
    /// (`VK_PROCESSKEY`): their release is no key either.
    keys_taken_by_input_method: RefCell<raw::HeldKeys>,
    /// Whether the window is destroyed: a callback can close it, and a later
    /// window can take its handle.
    gone: Cell<bool>,
}

#[cfg(windows)]
impl InputWindow {
    /// The input of `window`, which its class just announced.
    fn new(window: &CreatedWindow) -> Self {
        let message_clock = raw::MessageClock::new();
        let click_settings = raw::click_settings(&*window.preferences, window.scale.dpi());
        Self {
            window: window.window(),
            hooks: Rc::downgrade(&window.hooks),
            delivery: Arc::clone(&window.delivery),
            app_hooks: Rc::clone(&window.app_hooks),
            preferences: Rc::clone(&window.preferences),
            scale: window.scale.clone(),
            registry: RefCell::new(raw::pointer_registry(
                message_clock.native(),
                click_settings,
            )),
            message_clock,
            hovered: Cell::new(false),
            modifiers: Cell::new(Modifiers::NONE),
            touchpad: direct_manipulation::TouchpadSlot::default(),
            pending_high_surrogate: Cell::new(None),
            held_dead_keys: RefCell::default(),
            unplaced_keys: RefCell::default(),
            keys_taken_by_input_method: RefCell::default(),
            gone: Cell::new(false),
        }
    }

    /// Its scale: its DPI over 96.
    fn scale_factor(&self) -> f64 {
        self.scale.scale()
    }

    /// Whether the window is still the one this input was made for: a
    /// callback can close it and let another window take its handle.
    fn is_current(&self) -> bool {
        !self.gone.get()
    }

    /// The window's DPI changed or a system setting did: read the
    /// double-click interval and rectangle again at its DPI.
    fn measure_clicks(&self) {
        let settings = raw::click_settings(&*self.preferences, self.scale.dpi());
        self.registry.borrow_mut().set_click_settings(settings);
    }

    /// Dispatch a keyboard-layout change to the platform-level hook, through
    /// the call point: no borrow of the hooks is held while it runs.
    fn dispatch_keyboard_layout_change(&self) {
        call(
            &Field::new(&*self.app_hooks, |hooks| &mut hooks.keyboard_layout_changed),
            |handler| handler(),
        )
        .or_resume(());
    }
}

#[cfg(windows)]
impl WindowMessages for InputWindows {
    /// The window's input, with its precision touchpad's viewport; then the
    /// window's input capabilities, which say whether it has one
    /// (`input::availability::windows`).
    fn created(&self, window: &CreatedWindow) {
        let input = Rc::new(InputWindow::new(window));
        let replaced = self.windows.put(window.window(), Rc::clone(&input));
        if let Some(replaced) = replaced {
            debug_assert!(false, "BUG: a window identity is announced once");
            tracing::error!("a window identity was announced twice; its earlier input is ended");
            replaced.gone.set(true);
            replaced.release_touchpad(replaced.window.hwnd());
        }
        if let Some(host) = direct_manipulation::create_host(
            window.hwnd,
            window.client_size,
            window.apartment.clone(),
        ) {
            input.touchpad.install(host);
        }
        input
            .delivery
            .set_input_capabilities(crate::input::availability::windows(input.has_touchpad()));
    }

    fn step(&self, window: WindowRef, step: WindowStep) {
        let Some(input) = self.of(window) else {
            return;
        };
        let hwnd = window.hwnd();
        match step {
            WindowStep::Resized(size) => input.resize_touchpad(size),
            // What is held in a window that loses focus or is minimised sees
            // no release.
            WindowStep::Minimized | WindowStep::FocusLost => input.cancel_pointers(hwnd),
            // The frame follows the scrolling it shows.
            WindowStep::Painting => input.pump_touchpad_for_frame(hwnd),
            WindowStep::SizeMoveStarted => input.pause_touchpad(hwnd),
            WindowStep::SizeMoveEnded => input.resume_touchpad(),
            // The double-click rectangle is measured at the window's scale, and
            // the double-click time and size are settings too.
            WindowStep::Rescaled | WindowStep::SettingChanged => input.measure_clicks(),
            WindowStep::KeyLetThrough { .. }
            | WindowStep::ScaleApplied
            | WindowStep::SessionLocked(_)
            | WindowStep::Closed => {}
        }
    }

    /// Whatever the precision touchpad has open ends as cancelled, and its
    /// viewport is released, while the window and its callbacks still exist;
    /// then the window's input is dropped.
    fn destroyed(&self, window: WindowRef) {
        if let Some(input) = self.of(window) {
            self.forget(&input);
        }
    }

    fn answers(&self, msg: u32, wparam: WPARAM) -> bool {
        matches!(
            msg,
            WM_MOUSEMOVE
                | WM_MOUSELEAVE
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_RBUTTONDOWN
                | WM_RBUTTONUP
                | WM_MBUTTONDOWN
                | WM_MBUTTONUP
                | WM_XBUTTONDOWN
                | WM_XBUTTONUP
                | WM_POINTERDOWN
                | WM_POINTERUP
                | WM_POINTERUPDATE
                | WM_POINTERENTER
                | WM_POINTERLEAVE
                | WM_POINTERCAPTURECHANGED
                | WM_POINTERDEVICECHANGE
                | WM_CAPTURECHANGED
                | WM_MOUSEWHEEL
                | WM_MOUSEHWHEEL
                | WM_KEYDOWN
                | WM_SYSKEYDOWN
                | WM_KEYUP
                | WM_SYSKEYUP
                | WM_CHAR
                | WM_INPUTLANGCHANGE
                | DM_POINTERHITTEST
        ) || (msg == WM_TIMER && InputWindow::is_touchpad_timer(wparam.0))
    }

    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        if msg == WM_NCDESTROY {
            // The window's last message: what is left of a window whose
            // destruction was not announced goes with it. Heard, not
            // answered.
            if let Some(input) = self.by_handle(hwnd) {
                self.forget(&input);
            }
            return None;
        }
        if !self.answers(msg, wparam) {
            return None;
        }
        // Held for the whole message: a callback that closes the window
        // drops the entry, not the input this message reaches.
        let input = self.by_handle(hwnd);
        match msg {
            DM_POINTERHITTEST => {
                // The system offers a contact to the window under it; a
                // touchpad's goes to the DirectManipulation viewport.
                if let Some(input) = input {
                    input.offer_touchpad_contact(hwnd, (wparam.0 & 0xffff) as u32);
                }
                Some(LRESULT(0))
            }
            WM_TIMER => {
                if let Some(input) = input {
                    input.pump_touchpad(hwnd);
                }
                Some(LRESULT(0))
            }
            // SAFETY: the arguments as Win32 delivered them to the class's
            // procedure, which is `handle`'s contract too.
            _ => unsafe { Self::handle(hwnd, msg, wparam, lparam, input.as_deref()) },
        }
    }
}

/// What the window, drag and drop and haptics read of a window's input.
#[cfg(windows)]
impl WindowInputs for InputWindows {
    fn hovered(&self, window: WindowRef) -> Option<bool> {
        Some(self.of(window)?.hovered.get())
    }

    /// `None` too while the registry is borrowed by a message being decoded.
    fn press_kind(
        &self,
        window: WindowRef,
        pointer: flui_platform::PointerId,
    ) -> Option<flui_platform::pointer::PointerKind> {
        let input = self.of(window)?;
        let registry = input.registry.try_borrow().ok()?;
        registry.origin(pointer).map(|origin| origin.kind)
    }

    /// `None` too while the registry is borrowed by a message being decoded.
    fn pen_in_range(
        &self,
        window: WindowRef,
        device: Option<flui_platform::pointer::DeviceId>,
    ) -> Option<u64> {
        let input = self.of(window)?;
        let registry = input.registry.try_borrow().ok()?;
        registry.pen_contact(device).map(|contact| contact.0)
    }
}

#[cfg(windows)]
impl InputWindows {
    /// The input messages ([`WindowMessages::message`]), with `ctx`, the
    /// window's input, `None` for a window the input does not hold (one it
    /// was never told of, or already destroyed).
    ///
    /// # Safety
    ///
    /// Must only be called with arguments Win32 delivered to this class's
    /// `WNDPROC` for the message being handled, on the thread that owns
    /// `hwnd`.
    unsafe fn handle(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        ctx: Option<&InputWindow>,
    ) -> Option<LRESULT> {
        // SAFETY: see the `# Safety` section above. This single `unsafe`
        // block covers every Win32 call in the `match` below:
        // `TrackMouseEvent` (`WM_MOUSEMOVE`) takes a `&raw` pointer to a
        // stack-local, correctly-sized struct; `InSendMessageEx` and
        // `DefWindowProcW` take plain values.
        unsafe {
            let answer = match msg {
                WM_MOUSEMOVE => {
                    if let Some(ctx) = ctx {
                        let time = ctx.message_clock.message_time();
                        // Request WM_MOUSELEAVE notification for hover tracking
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&raw mut tme);
                        // Cleared by WM_MOUSELEAVE.
                        ctx.set_hovered(true);

                        let events = raw::mouse_move_input(
                            &ctx.registry,
                            wparam,
                            lparam,
                            ctx.scale_factor(),
                            time,
                        );
                        for event in events {
                            if ctx.registry.borrow().delivers(&event) {
                                ctx.delivery.dispatch_input(event);
                            }
                        }
                    }
                    LRESULT(0)
                }

                WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_RBUTTONDOWN
                | WM_RBUTTONUP
                | WM_MBUTTONDOWN
                | WM_MBUTTONUP
                | windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONDOWN
                | windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONUP => {
                    use raw::{
                        button_message, capture_on_press, mouse_button_input, release_capture_after,
                    };
                    let Some((button, is_down)) = button_message(msg, wparam) else {
                        return Some(LRESULT(0));
                    };
                    let timed_context = ctx.map(|ctx| (ctx, ctx.message_clock.message_time()));
                    // The capture follows the native button state even with
                    // no input to deliver to, and is settled before user
                    // code runs: a release outside the client area must
                    // still reach this window, and the sequence's own
                    // release must not read as a stolen capture.
                    if is_down {
                        capture_on_press(hwnd);
                    } else {
                        release_capture_after(hwnd, wparam);
                    }
                    if let Some((ctx, time)) = timed_context {
                        let events = mouse_button_input(
                            &ctx.registry,
                            button,
                            is_down,
                            wparam,
                            lparam,
                            ctx.scale_factor(),
                            time,
                        );
                        for event in events {
                            if ctx.registry.borrow().delivers(&event) {
                                ctx.delivery.dispatch_input(event);
                            }
                        }
                    }
                    LRESULT(isize::from(matches!(
                        msg,
                        windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONDOWN
                            | windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONUP
                    )))
                }

                windows::Win32::UI::WindowsAndMessaging::WM_POINTERDOWN
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERUP
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERUPDATE
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERENTER
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERLEAVE
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERCAPTURECHANGED => {
                    if let Some(ctx) = ctx {
                        ctx.pointer_message(hwnd, msg, wparam);
                    }
                    // Consume the native route: DefWindowProc would promote it
                    // to a second legacy mouse event.
                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_POINTERDEVICECHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.pointer_device_changed(wparam, lparam);
                    }
                    LRESULT(0)
                }

                WM_CAPTURECHANGED => {
                    // Another window, a modal loop or `WM_CANCELMODE` took
                    // the capture mid-press: this window will never see the
                    // sequence's release, so end it here.
                    if let Some(ctx) = ctx {
                        let cancels = raw::capture_changed_input(
                            hwnd,
                            lparam,
                            ctx.message_clock.message_time(),
                            &ctx.registry,
                        );
                        for cancel in cancels {
                            ctx.delivery.dispatch_input(cancel);
                        }
                    }
                    LRESULT(0)
                }

                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    if let Some(ctx) = ctx {
                        use crate::input::assembly::WheelAxis;
                        let axis = if msg == WM_MOUSEWHEEL {
                            WheelAxis::Vertical
                        } else {
                            WheelAxis::Horizontal
                        };
                        let time = ctx.message_clock.message_time();
                        let event = raw::mouse_wheel_input(
                            &ctx.registry,
                            hwnd,
                            axis,
                            wparam,
                            lparam,
                            ctx.scale_factor(),
                            time,
                        );
                        if let Some(event) = event {
                            ctx.delivery.dispatch_input(event);
                        }
                    }
                    LRESULT(0)
                }

                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let key = raw::KeyMessage::new(wparam, lparam);

                    if let Some(ctx) = ctx {
                        // Read before the character drain below retrieves
                        // other messages.
                        let time = ctx.message_clock.message_time();

                        // Track modifiers.
                        ctx.modifiers.set(raw::message_modifiers());

                        // A key the input method took is its composition,
                        // not a key: nothing is dispatched for it, and no
                        // character is drained, so what a legacy input
                        // method posts for its result arrives as text of
                        // its own (the stray `WM_CHAR` arm).
                        let mut taken = ctx.keys_taken_by_input_method.borrow_mut();
                        if crate::input::keys::taken_by_input_method(key.vk) {
                            taken.press(key.scan_code, key.extended);
                            drop(taken);
                            return Some(DefWindowProcW(hwnd, msg, wparam, lparam));
                        }
                        // A press the application gets is proof the key was
                        // released where its release went unseen.
                        taken.release(key.scan_code, key.extended);
                        drop(taken);

                        // Merge the fully-translated character(s) for this
                        // keystroke into the keydown. The message loop runs
                        // `TranslateMessage` before `DispatchMessageW` (see
                        // `run`), so the WM_CHAR burst for this keydown —
                        // shift/caps/AltGr applied, dead keys resolved — is
                        // already in the queue and is drained synchronously
                        // here. Pairing model and merge rules:
                        // `crate::input::keys` module doc.
                        //
                        // A keydown sent from another thread never passed
                        // through this thread's `TranslateMessage`, so nothing
                        // queued here is its own. It takes nothing: such a
                        // keydown is what a drain below can dispatch (`PeekMessageW`
                        // delivers pending cross-thread sent messages first), and
                        // draining from there would take the outer keystroke's
                        // characters.
                        let owns_burst = InSendMessageEx(None) == ISMEX_NOSEND;
                        let translated = if owns_burst {
                            drain_translated_chars(hwnd)
                        } else {
                            None
                        };
                        // A dead key queues `WM_DEADCHAR` (`WM_SYSDEADCHAR` under
                        // Alt) instead of a character, on the layer the held
                        // modifiers select. Taking it now leaves nothing for a
                        // nested keydown during the callback to mistake for its own.
                        let stroke =
                            if owns_burst && translated.is_none() && drain_dead_char(hwnd, msg) {
                                raw::Keystroke::Dead
                            } else {
                                raw::Keystroke::Text(translated)
                            };
                        // The default a system keydown may skip also lives in
                        // the WM_SYSCHAR `TranslateMessage` queued for it:
                        // `DefWindowProcW` turns Alt+Space's into the system
                        // menu. Hold it before user code runs, since a
                        // handler that pumps messages (a modal API) would
                        // otherwise dispatch it before the result is known.
                        let held = if owns_burst && msg == WM_SYSKEYDOWN {
                            take_translated_sys_chars(hwnd)
                        } else {
                            Vec::new()
                        };

                        // Dispatch keyboard event via per-window callback
                        use raw::key_down_event;
                        let event = {
                            let mut held = ctx.held_dead_keys.borrow_mut();
                            let event = key_down_event(key, stroke, &mut held, time);
                            ctx.settle_unplaced(key.scan_code, key.extended, event)
                        };
                        let result = ctx.delivery.dispatch_input(event);
                        // A callback can close this window and create another
                        // with a recycled HWND: only while the window is the
                        // one this input was made for does the native default
                        // still target it.
                        if result.default_prevented || !ctx.is_current() {
                            // Withdrawn with its keydown: the held
                            // characters are dropped here.
                            return Some(LRESULT(0));
                        }
                        // The window's own keys (the fullscreen hotkey) are a
                        // default the app can prevent.
                        if let Some(hooks) = ctx.hooks.upgrade() {
                            window_step(
                                &hooks,
                                ctx.window,
                                WindowStep::KeyLetThrough {
                                    virtual_key: key.vk,
                                    repeat: key.repeat,
                                },
                            );
                        }
                        let handled = DefWindowProcW(hwnd, msg, wparam, lparam);
                        restore_translated_sys_chars(hwnd, &held);
                        return Some(handled);
                    }

                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_KEYUP | WM_SYSKEYUP => {
                    if let Some(ctx) = ctx {
                        let time = ctx.message_clock.message_time();
                        // Track modifiers.
                        ctx.modifiers.set(raw::message_modifiers());

                        // The release of a key whose press the input method
                        // took is no key either, under `VK_PROCESSKEY` or its
                        // own. A press the application got is released for it,
                        // `VK_PROCESSKEY` included (the input method opened
                        // while the key was held), so no key stays down.
                        let key = raw::KeyMessage::new(wparam, lparam);
                        let taken = ctx
                            .keys_taken_by_input_method
                            .borrow_mut()
                            .release(key.scan_code, key.extended);
                        if taken {
                            return Some(DefWindowProcW(hwnd, msg, wparam, lparam));
                        }

                        use raw::key_up_event;
                        let event = {
                            let mut held = ctx.held_dead_keys.borrow_mut();
                            let event = key_up_event(key, &mut held, time);
                            ctx.settle_unplaced(key.scan_code, key.extended, event)
                        };
                        let result = ctx.delivery.dispatch_input(event);
                        if result.default_prevented || !ctx.is_current() {
                            return Some(LRESULT(0));
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_CHAR => {
                    // Normally unreachable: the WM_KEYDOWN arm drains the
                    // paired WM_CHAR burst before it would be dispatched
                    // here. What remains is an out-of-band character with no
                    // owning keydown — Alt+numpad composition (queued by
                    // TranslateMessage of the Alt KEYUP) or a directly-sent
                    // message — dispatched as its own key-down instead of
                    // being discarded, so the text lane still receives it.
                    // Astral-plane characters arrive as two WM_CHARs; the
                    // high surrogate is parked in the window's input until
                    // its low half arrives.
                    if let Some(ctx) = ctx {
                        let (pending, text) = crate::input::keys::assemble_stray_wm_char(
                            ctx.pending_high_surrogate.get(),
                            wparam.0 as u16,
                        );
                        ctx.pending_high_surrogate.set(pending);
                        if let Some(text) = text {
                            use raw::stray_char_event;
                            let time = ctx.message_clock.message_time();
                            ctx.delivery.dispatch_input(stray_char_event(text, time));
                        }
                    }
                    LRESULT(0)
                }

                // WM_MOUSELEAVE (0x02A3), requested by WM_MOUSEMOVE.
                WM_MOUSELEAVE => {
                    if let Some(ctx) = ctx {
                        ctx.set_hovered(false);
                    }
                    LRESULT(0)
                }

                // Keyboard layout change
                WM_INPUTLANGCHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.dispatch_keyboard_layout_change();
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // Not an input message: the next hook's, or the default
                // procedure's.
                _ => return None,
            };
            Some(answer)
        }
    }
}

#[cfg(windows)]
mod keystroke_queue {
    //! The keystroke's queued messages: the characters `TranslateMessage` posts
    //! for the keydown `window_proc` is handling, drained or held before user code
    //! sees the keydown.

    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            DefWindowProcW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, WM_CHAR, WM_DEADCHAR,
            WM_SYSCHAR, WM_SYSDEADCHAR, WM_SYSKEYDOWN,
        },
    };

    /// Drain the `WM_CHAR` burst `TranslateMessage` queued for the keydown
    /// currently being handled, and decode it into typeable text.
    ///
    /// Called from `window_proc`'s `WM_KEYDOWN`/`WM_SYSKEYDOWN` arm. The burst
    /// is complete at that point — the message loop runs `TranslateMessage`
    /// before `DispatchMessageW`, so every `WM_CHAR` belonging to this keystroke
    /// (two of them for an astral-plane character) is already posted. Draining
    /// filters exactly `WM_CHAR` for this window: `WM_SYSCHAR` is deliberately
    /// left queued so Alt+mnemonic accelerators still flow to `DefWindowProcW`
    /// (held across the keydown's dispatch: [`take_translated_sys_chars`]),
    /// and `WM_DEADCHAR` is taken separately by [`drain_dead_char`] (the dead-key state
    /// itself stays Windows' business). Returns `None` for keystrokes with no typeable translation
    /// (navigation keys, Ctrl chords — see `input::keys::wm_char_text`).
    pub(super) fn drain_translated_chars(hwnd: HWND) -> Option<String> {
        let mut units: Vec<u16> = Vec::new();
        let mut msg = MSG::default();

        // A plain `PM_REMOVE`, with no `PM_QS_*` mask, for this drain and the two
        // below. A `PM_QS_POSTMESSAGE` peek reads the posted queue only while its
        // `QS_POSTMESSAGE` wake bit is set, and every `GetMessage`/`PeekMessage`
        // clears that bit (`GetQueueStatus` docs), filtered or not: after the
        // retrieval of this keydown or an earlier empty drain, the masked peek
        // skips a character already queued, which then arrives as a second,
        // stray key-down. The plain peek always scans the queue. It also delivers
        // pending cross-thread sent messages first; a keydown among them owns no
        // burst and drains nothing (the `owns_burst` check in `window_proc`),
        // so it cannot take this keystroke's characters.
        // SAFETY: `msg` is a live, writable local and `PeekMessageW` writes
        // nothing else. `PM_REMOVE` only ever removes messages from this
        // thread's own queue (the wndproc runs on the queue's owning thread).
        // A sent message it delivers re-enters `window_proc`, the same
        // re-entrancy any modal Win32 call permits; nothing here holds a lock or
        // borrow across the call.
        unsafe {
            while PeekMessageW(&raw mut msg, Some(hwnd), WM_CHAR, WM_CHAR, PM_REMOVE).as_bool() {
                units.push(msg.wParam.0 as u16);
            }
        }

        crate::input::keys::wm_char_text(&units)
    }

    /// Remove the dead-character message `TranslateMessage` queued for the keydown
    /// being handled, if any: `WM_SYSDEADCHAR` for a system keydown, else
    /// `WM_DEADCHAR`. Only the notification leaves the queue; the composition
    /// itself lives in the kernel's dead-key state, which this does not touch.
    pub(super) fn drain_dead_char(hwnd: HWND, keydown: u32) -> bool {
        let kind = if keydown == WM_SYSKEYDOWN {
            WM_SYSDEADCHAR
        } else {
            WM_DEADCHAR
        };
        let mut msg = MSG::default();
        let mut found = false;
        // SAFETY: as in `drain_translated_chars`: a live writable local, and
        // `PM_REMOVE` touches only this thread's own queue.
        unsafe {
            while PeekMessageW(&raw mut msg, Some(hwnd), kind, kind, PM_REMOVE).as_bool() {
                found = true;
            }
        }
        found
    }

    /// Remove the `WM_SYSCHAR` burst `TranslateMessage` queued for a system
    /// keydown before user code sees the keydown, so a handler that pumps
    /// messages cannot hand a character to `DefWindowProcW` (which would raise
    /// `SC_KEYMENU` from it, and open the system menu for Alt+Space) before its
    /// result says whether the default runs. The same queue-ordering argument as
    /// [`drain_translated_chars`] makes the burst complete here. The burst is
    /// almost always empty or one character, so the `Vec` rarely allocates.
    pub(super) fn take_translated_sys_chars(hwnd: HWND) -> Vec<(WPARAM, LPARAM)> {
        let mut held = Vec::new();
        let mut msg = MSG::default();
        // SAFETY: as in `drain_translated_chars`: a live writable local, and
        // `PM_REMOVE` touches only this thread's own queue.
        unsafe {
            while PeekMessageW(&raw mut msg, Some(hwnd), WM_SYSCHAR, WM_SYSCHAR, PM_REMOVE)
                .as_bool()
            {
                held.push((msg.wParam, msg.lParam));
            }
        }
        held
    }

    /// Return the characters [`take_translated_sys_chars`] held for a system
    /// keydown whose default ran, to be dispatched after it as `TranslateMessage`
    /// had queued them. If the queue refuses one, it gets its default now rather
    /// than being lost.
    pub(super) fn restore_translated_sys_chars(hwnd: HWND, held: &[(WPARAM, LPARAM)]) {
        for &(wparam, lparam) in held {
            // SAFETY: posts integer character data to this thread's own live
            // window, whose wndproc is running on this thread now.
            if unsafe { PostMessageW(Some(hwnd), WM_SYSCHAR, wparam, lparam) }.is_err() {
                // SAFETY: the same live window, called synchronously on its
                // creating thread with the message `TranslateMessage` produced.
                unsafe { DefWindowProcW(hwnd, WM_SYSCHAR, wparam, lparam) };
            }
        }
    }
}

/// The window's pointer messages: `WM_POINTER*`, device arrival and removal,
/// the mouse's hover on either route, and the end of every open contact when
/// the window loses focus or is minimised.
#[cfg(windows)]
impl InputWindow {
    /// `input` of the key `(scan_code, extended)`, its key the key-down's when
    /// its scancode names no code (`KeyEvent`'s contract).
    fn settle_unplaced(
        &self,
        scan_code: u16,
        extended: bool,
        input: flui_platform::PlatformInput,
    ) -> flui_platform::PlatformInput {
        match input {
            // Scan code 0 (input injected by virtual key alone) names no key.
            flui_platform::PlatformInput::Keyboard(event) => {
                flui_platform::PlatformInput::Keyboard(
                    self.unplaced_keys
                        .borrow_mut()
                        .settle((scan_code != 0).then_some((scan_code, extended)), event),
                )
            }
            other => other,
        }
    }

    /// Record whether the mouse is over the window, raising the hover change
    /// only when it changed. Every route the mouse reaches the window by
    /// reports through here: `WM_MOUSEMOVE`/`WM_MOUSELEAVE`, and `WM_POINTER*`
    /// for a mouse under `EnableMouseInPointer`.
    fn set_hovered(&self, hovered: bool) {
        if self.hovered.replace(hovered) != hovered {
            self.delivery.report_hover(hovered);
        }
    }

    /// A `WM_POINTER*` `message` for the pointer `wparam` names.
    fn pointer_message(&self, hwnd: HWND, message: u32, wparam: WPARAM) {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP,
            WM_POINTERUPDATE,
        };

        let raw = (wparam.0 & 0xffff) as u32;
        let mouse = raw::is_mouse_pointer(raw);
        // The mouse's hover follows the messages that carry it. A capture
        // change says nothing about where it is, and no message follows it
        // (a press dragged outside sends no leave while it is held), so the
        // hover is read from the cursor once its input is delivered.
        let hover =
            (mouse && message != WM_POINTERCAPTURECHANGED).then_some(message != WM_POINTERLEAVE);
        let members = (!mouse
            && matches!(message, WM_POINTERDOWN | WM_POINTERUP | WM_POINTERUPDATE))
        .then(|| raw::pointer_frame_members(raw))
        .flatten();
        let inputs = if let Some(members) = members {
            let contacts = members.into_iter().map(|(id, message)| {
                raw::native_pointer_input(
                    hwnd,
                    message,
                    (id, false),
                    self.scale_factor(),
                    &self.message_clock,
                    &self.registry,
                )
            });
            let frame = crate::input::assembly::framed(contacts.collect::<Vec<_>>());
            raw::skip_frame_messages(raw);
            frame
        } else {
            raw::native_pointer_input(
                hwnd,
                message,
                (raw, mouse),
                self.scale_factor(),
                &self.message_clock,
                &self.registry,
            )
        };
        // The hover is settled before any of the message's input reaches user
        // code: a callback that pumps messages may see the cursor come back,
        // and nothing written after it may undo that.
        if let Some(hovered) = hover {
            self.set_hovered(hovered);
        }
        for input in inputs {
            let deliver = self.registry.borrow().delivers(&input);
            if deliver {
                self.delivery.dispatch_input(input);
            }
        }
        if mouse && message == WM_POINTERCAPTURECHANGED {
            self.reconcile_hover(hwnd);
        }
    }

    /// The window lost the mouse capture: no message may say where the
    /// cursor went, so the hover is what the cursor is over now.
    fn reconcile_hover(&self, hwnd: HWND) {
        self.set_hovered(cursor_over_client(hwnd));
    }

    /// `WM_POINTERDEVICECHANGE`: a removed device ends its contacts and
    /// leaves; an arriving one is announced.
    fn pointer_device_changed(&self, wparam: WPARAM, lparam: LPARAM) {
        use flui_platform::{EventTime, PlatformInput, pointer::PointerEvent};
        use windows::Win32::{
            Foundation::HANDLE,
            UI::WindowsAndMessaging::{PDC_ARRIVAL, PDC_REMOVAL},
        };

        let device = raw::native_device(lparam);
        let time = EventTime::from_nanos(self.message_clock.message_time());
        let mut inputs = Vec::new();
        if wparam.0 == PDC_REMOVAL as usize {
            let (ended, removed) = self.registry.borrow_mut().remove_device(device, time);
            inputs.extend(ended.into_iter().map(|(_, input)| input));
            inputs.extend(
                removed.map(|change| PlatformInput::Pointer(PointerEvent::DeviceRemoved(change))),
            );
        } else if wparam.0 == PDC_ARRIVAL as usize
            && let Some(kind) = raw::native_device_kind(HANDLE(lparam.0 as *mut core::ffi::c_void))
        {
            let added = self.registry.borrow_mut().add_device(device, kind, time);
            inputs.extend(
                added.map(|change| PlatformInput::Pointer(PointerEvent::DeviceAdded(change))),
            );
        }
        for input in inputs {
            self.delivery.dispatch_input(input);
        }
    }

    /// End every open pointer sequence and touchpad manipulation as
    /// `FocusLost`: the window lost focus or was minimised, so the releases
    /// may never reach it. What still arrives for an ended contact is an
    /// orphan and delivers nothing.
    fn cancel_pointers(&self, hwnd: HWND) {
        use crate::input::assembly::NativeCancel;
        use flui_platform::EventTime;

        // The touchpad first: its manipulation ends through the registry.
        self.cancel_touchpad(hwnd);
        let time = EventTime::from_nanos(self.message_clock.message_time());
        let cancels =
            self.registry
                .borrow_mut()
                .cancel_surface(raw::SURFACE, time, NativeCancel::FocusLost);
        for cancel in cancels {
            self.delivery.dispatch_input(cancel);
        }
    }
}

/// Whether the cursor is over `hwnd`'s client area, the window on top there;
/// `false` when the cursor cannot be read.
#[cfg(windows)]
fn cursor_over_client(hwnd: HWND) -> bool {
    use windows::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::ScreenToClient,
        UI::WindowsAndMessaging::{GetClientRect, GetCursorPos, WindowFromPoint},
    };
    let mut cursor = POINT::default();
    let mut client = RECT::default();
    // SAFETY: every out pointer addresses a live local; `hwnd` is the window
    // whose procedure is running.
    unsafe {
        if GetCursorPos(&raw mut cursor).is_err() || WindowFromPoint(cursor) != hwnd {
            return false;
        }
        ScreenToClient(hwnd, &raw mut cursor).as_bool()
            && GetClientRect(hwnd, &raw mut client).is_ok()
            && cursor.x >= client.left
            && cursor.x < client.right
            && cursor.y >= client.top
            && cursor.y < client.bottom
    }
}

#[cfg(windows)]
mod raw {
    //! Win32 message conversion to owned platform input.
    //!
    //! `window_proc` unpacks every mouse and keyboard message through these
    //! functions:
    //!
    //! - **Pointers** are copied out of each message into plain
    //!   [`NativeReading`]s and assembled by the shared
    //!   [`pointer_assembly`](crate::input::assembly), which owns contact
    //!   and device identity, held buttons, click counting and the clock choice.
    //!   This file only reads the OS.
    //! - **Modifiers** are the queue-synchronized key state (`GetKeyState`), which
    //!   Win32 advances as each input message is retrieved — the state that held
    //!   when this message was generated. `GetAsyncKeyState` would report the
    //!   keyboard at processing time instead, so a queued Shift+click whose Shift
    //!   was released before the click was processed would lose its Shift.
    //! - **Capture** belongs to a press sequence: [`capture_on_press`] captures
    //!   the mouse on a button press and [`release_capture_after`] releases it
    //!   once no button stays held, so a drag released outside the client area
    //!   still delivers its release (at client coordinates outside the client
    //!   rectangle, sign-extended by `get_x_lparam`). A capture taken away while
    //!   a button is held ends the sequence with a cancel
    //!   ([`capture_changed_input`]). That is the legacy `WM_*BUTTON*` route; on
    //!   the `WM_POINTER` route the system captures the pointer for its press.
    //! - Mouse samples retain the retrieved message's generation time, rebased
    //!   onto the shared monotonic epoch before any reentrant callbacks run.
    //!   `WM_POINTER*` readings are stamped from their performance count when
    //!   the source reports one, so a coalesced burst keeps its own spacing.

    use std::cell::RefCell;

    use flui_platform::{
        EventTime, Modifiers as KeyboardModifiers,
        keyboard::{Code, Key, KeyEvent, KeyRelease, KeyRepeat, KeyState, NamedKey},
        pointer::{PenTool, PointerButton, PointerButtons, PointerKind},
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        System::SystemInformation::GetTickCount64,
        UI::{
            Input::{
                KeyboardAndMouse::{
                    GetCapture, GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_CONTROL,
                    VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
                },
                Pointer::{
                    POINTER_BUTTON_CHANGE_TYPE, POINTER_CHANGE_FIFTHBUTTON_DOWN,
                    POINTER_CHANGE_FIFTHBUTTON_UP, POINTER_CHANGE_FIRSTBUTTON_DOWN,
                    POINTER_CHANGE_FIRSTBUTTON_UP, POINTER_CHANGE_FOURTHBUTTON_DOWN,
                    POINTER_CHANGE_FOURTHBUTTON_UP, POINTER_CHANGE_SECONDBUTTON_DOWN,
                    POINTER_CHANGE_SECONDBUTTON_UP, POINTER_CHANGE_THIRDBUTTON_DOWN,
                    POINTER_CHANGE_THIRDBUTTON_UP, POINTER_INFO, POINTER_PEN_INFO,
                    POINTER_TOUCH_INFO,
                },
            },
            WindowsAndMessaging::{
                GetMessageTime, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
                WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
            },
        },
    };

    use crate::input::assembly::{
        Assembled, ClickSettings, ClockChoice, ContactRegistry, CounterAnchor, DecodeError,
        MoveBatch, NativeCancel, NativeClock, NativeContact, NativeDevice, NativeReading,
        NativeSurface, NativeTime, PenAngles, QpcTicks, RawSensors, ReadingAction, ReadingTime,
        WheelAxis, WheelReading, decode,
    };

    /// Rebases the queue's wrapping millisecond timestamp, and pointer readings'
    /// performance-counter timestamps, onto the shared epoch.
    /// The owner snapshots a sample before callback-capable native or user calls.
    pub(crate) struct MessageClock {
        native: NativeClock,
        /// The latest stamp the window delivered, coarse or precise: a message
        /// stamp never precedes it. A key in the same system tick as a
        /// `WM_POINTER` sample before it reads an earlier millisecond tick than
        /// that sample's performance count.
        last_ns: std::cell::Cell<u64>,
    }

    /// The performance counter read now, with its frequency.
    fn performance_anchor() -> Option<CounterAnchor> {
        use windows::Win32::System::Performance::{
            QueryPerformanceCounter, QueryPerformanceFrequency,
        };
        let (mut count, mut frequency) = (0_i64, 0_i64);
        // SAFETY: both out pointers address initialized locals that outlive
        // the calls, which only write one `i64` each.
        unsafe {
            QueryPerformanceFrequency(&raw mut frequency).ok()?;
            QueryPerformanceCounter(&raw mut count).ok()?;
        }
        Some(CounterAnchor {
            count: QpcTicks(u64::try_from(count).ok()?),
            frequency: u64::try_from(frequency).ok()?.try_into().ok()?,
        })
    }

    impl MessageClock {
        pub(crate) fn new() -> Self {
            // SAFETY: argument-free monotonic system uptime query.
            let uptime_ms = unsafe { GetTickCount64() };
            // The counter and the epoch are read back to back: `Instant` is the
            // same counter on Windows, so their offset stays sub-microsecond.
            let counter = performance_anchor();
            Self::anchored(uptime_ms, counter, event_timestamp_ns())
        }

        fn anchored(uptime_ms: u64, counter: Option<CounterAnchor>, epoch_ns: u64) -> Self {
            Self {
                native: NativeClock::new(
                    EventTime::from_nanos(epoch_ns),
                    std::time::Duration::from_millis(uptime_ms),
                    counter,
                ),
                last_ns: std::cell::Cell::new(0),
            }
        }

        /// The anchors pointer readings are stamped against.
        pub(crate) const fn native(&self) -> NativeClock {
            self.native
        }

        pub(super) fn message_time(&self) -> u64 {
            // SAFETY: argument-free queries of the owner thread's retrieved
            // message and system uptime. No callback can run between them.
            let (tick, now) = unsafe { (GetMessageTime() as u32, GetTickCount64()) };
            self.stamp_at(tick, now)
        }

        fn stamp_at(&self, tick: u32, now_ms: u64) -> u64 {
            let time = ReadingTime::coarse(NativeTime::TickMillis {
                tick,
                observed_ms: now_ms,
            });
            let stamp = self
                .native
                .stamp(time, ClockChoice::Coarse)
                .expect("BUG: a millisecond tick is always a finite time")
                .as_nanos()
                .max(self.last_ns.get());
            self.last_ns.set(stamp);
            stamp
        }

        /// Raise the floor to the latest stamp `inputs` carry: pointer readings
        /// stamped from their performance count, which a later message's coarse
        /// stamp must not precede.
        pub(super) fn observe(&self, inputs: &[PlatformInput]) {
            let latest = inputs
                .iter()
                .filter_map(PlatformInput::as_pointer)
                .filter_map(pointer_time)
                .map(EventTime::as_nanos)
                .max();
            if let Some(latest) = latest {
                self.last_ns.set(self.last_ns.get().max(latest));
            }
        }
    }

    /// When `event` happened; `None` for a revision, whose time names the
    /// reading it replaces.
    fn pointer_time(event: &flui_platform::pointer::PointerEvent) -> Option<EventTime> {
        use flui_platform::pointer::{ButtonChange, PointerEvent};
        Some(match event {
            PointerEvent::Down(press)
            | PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => press.sample.time,
            PointerEvent::Up(release)
            | PointerEvent::ButtonChange(ButtonChange::Released(release)) => release.sample.time,
            PointerEvent::Move(movement) => movement.current().time,
            PointerEvent::Cancel(cancel) => cancel.time,
            PointerEvent::Enter(signal)
            | PointerEvent::Leave(signal)
            | PointerEvent::ScrollInertiaCancel(signal) => signal.time,
            PointerEvent::Scroll(scroll) => scroll.time,
            PointerEvent::PanZoom(pan_zoom) => pan_zoom.time,
            PointerEvent::PenSqueeze(squeeze) => squeeze.time,
            PointerEvent::DeviceAdded(change) | PointerEvent::DeviceRemoved(change) => change.time,
            // A revision names the reading it replaces; the vocabulary may grow.
            _ => return None,
        })
    }

    use crate::input::event_timestamp_ns;
    use crate::input::keys::{self, MOUSE_BUTTONS};
    use crate::owner::windows::control::OwnerPreferences;
    use crate::owner::windows::{get_x_lparam, get_y_lparam};
    use flui_platform::PlatformInput;

    /// Every Win32 registry serves one window.
    pub(crate) const SURFACE: NativeSurface = NativeSurface(0);

    /// The button a pointer packet's `ButtonChangeType` names and whether it
    /// went down; `None` when no button changed.
    fn button_change(change: POINTER_BUTTON_CHANGE_TYPE) -> Option<(PointerButton, bool)> {
        Some(match change {
            POINTER_CHANGE_FIRSTBUTTON_DOWN => (PointerButton::PRIMARY, true),
            POINTER_CHANGE_FIRSTBUTTON_UP => (PointerButton::PRIMARY, false),
            POINTER_CHANGE_SECONDBUTTON_DOWN => (PointerButton::SECONDARY, true),
            POINTER_CHANGE_SECONDBUTTON_UP => (PointerButton::SECONDARY, false),
            POINTER_CHANGE_THIRDBUTTON_DOWN => (PointerButton::AUXILIARY, true),
            POINTER_CHANGE_THIRDBUTTON_UP => (PointerButton::AUXILIARY, false),
            POINTER_CHANGE_FOURTHBUTTON_DOWN => (PointerButton::BACK, true),
            POINTER_CHANGE_FOURTHBUTTON_UP => (PointerButton::BACK, false),
            POINTER_CHANGE_FIFTHBUTTON_DOWN => (PointerButton::FORWARD, true),
            POINTER_CHANGE_FIFTHBUTTON_UP => (PointerButton::FORWARD, false),
            _ => return None,
        })
    }

    /// The mouse's contact, given the primary mouse identity: outside the 32-bit
    /// range of `WM_POINTER` ids. Every route a mouse reaches the window by —
    /// `WM_POINTER` under `EnableMouseInPointer`, the legacy `WM_MOUSE*`
    /// messages (a host that keeps them, or a cursor moved by `SetCursorPos`)
    /// and the wheel, which always comes as `WM_MOUSEWHEEL` — reads as this one
    /// contact, so its hover, presses and scrolls share one pointer.
    const MOUSE_CONTACT: NativeContact = NativeContact(1 << 32);

    /// The contact a `WM_POINTER` id reads as, `mouse` when it is the mouse's.
    fn pointer_contact(mouse: bool, raw: u32) -> NativeContact {
        if mouse {
            MOUSE_CONTACT
        } else {
            NativeContact(u64::from(raw))
        }
    }

    /// A window's pointer registry on `clock`, with the OS double-click
    /// thresholds `clicks`.
    pub(crate) fn pointer_registry(
        clock: NativeClock,
        clicks: Option<ClickSettings>,
    ) -> ContactRegistry {
        let mut registry = ContactRegistry::new(clock).with_reserved_contact(MOUSE_CONTACT);
        registry.set_click_settings(clicks);
        registry
    }

    /// The OS double-click interval and rectangle at `dpi`, the rectangle in
    /// device pixels, both as `preferences` read them together; `None` when the
    /// system's settings cannot be read.
    pub(crate) fn click_settings(
        preferences: &dyn OwnerPreferences,
        dpi: u32,
    ) -> Option<ClickSettings> {
        let gestures = preferences
            .mouse_gestures(dpi)
            .inspect_err(|error| tracing::debug!(%error, "double-click settings unavailable"))
            .ok()?;
        let area = gestures.native_mouse_geometry()?.double_click_area();
        Some(ClickSettings {
            interval: gestures.double_click_interval()?,
            area: (f64::from(area.width), f64::from(area.height)),
        })
    }

    /// One native pointer packet, copied out of the OS before any callback runs.
    /// A pen's carries its device's himetric mapping, when it could be read.
    #[derive(Clone, Copy)]
    enum PointerPacket {
        Pointer(POINTER_INFO),
        Pen(POINTER_PEN_INFO, Option<HimetricMap>),
        Touch(POINTER_TOUCH_INFO),
    }

    impl PointerPacket {
        fn info(self) -> POINTER_INFO {
            match self {
                Self::Pointer(info) => info,
                Self::Pen(info, _) => info.pointerInfo,
                Self::Touch(info) => info.pointerInfo,
            }
        }

        /// The packet's raw screen position in device pixels. A pen reports its
        /// position in himetric units, finer than a pixel at its 120–240 Hz, and
        /// it is mapped through its device's rectangles; every other packet, or
        /// a pen whose mapping is missing or disagrees with the pixel the system
        /// reported, has the integer pixel.
        fn screen_position(self) -> (f64, f64) {
            let pixel = self.info().ptPixelLocationRaw;
            let whole = (f64::from(pixel.x), f64::from(pixel.y));
            match self {
                Self::Pen(pen, Some(map)) => map
                    .to_screen(pen.pointerInfo.ptHimetricLocationRaw)
                    .filter(|(x, y)| (x - whole.0).abs() <= 1.0 && (y - whole.1).abs() <= 1.0)
                    .unwrap_or(whole),
                Self::Pointer(_) | Self::Pen(_, None) | Self::Touch(_) => whole,
            }
        }
    }

    /// How a pointer device's himetric coordinates map onto the screen
    /// (`GetPointerDeviceRects`): its own rectangle and the display rectangle it
    /// covers, in screen pixels.
    #[derive(Clone, Copy)]
    struct HimetricMap {
        device: RECT,
        display: RECT,
    }

    impl HimetricMap {
        /// `device`'s mapping now; `None` when the system cannot report it.
        fn of(device: windows::Win32::Foundation::HANDLE) -> Option<Self> {
            use windows::Win32::UI::Input::Pointer::GetPointerDeviceRects;
            let mut map = Self {
                device: RECT::default(),
                display: RECT::default(),
            };
            // SAFETY: both out pointers address fields of a live local; the
            // system refuses a stale or invented device handle.
            unsafe { GetPointerDeviceRects(device, &raw mut map.device, &raw mut map.display) }
                .ok()?;
            Some(map)
        }

        /// The screen pixel position of `himetric`, linearly between the two
        /// rectangles; `None` for an empty rectangle.
        fn to_screen(self, himetric: POINT) -> Option<(f64, f64)> {
            let axis =
                |value: i32, (from_start, from_end): (i32, i32), (to_start, to_end): (i32, i32)| {
                    let from = f64::from(from_end) - f64::from(from_start);
                    let to = f64::from(to_end) - f64::from(to_start);
                    (from > 0.0 && to > 0.0).then(|| {
                        f64::from(to_start) + (f64::from(value) - f64::from(from_start)) * to / from
                    })
                };
            let (device, display) = (self.device, self.display);
            Some((
                axis(
                    himetric.x,
                    (device.left, device.right),
                    (display.left, display.right),
                )?,
                axis(
                    himetric.y,
                    (device.top, device.bottom),
                    (display.top, display.bottom),
                )?,
            ))
        }
    }

    /// The plain reading a `message` packet makes. `client_offset` moves raw
    /// screen pixels into the client area; `now_ms` is the uptime its tick is
    /// placed against.
    fn pointer_reading(
        packet: PointerPacket,
        message: u32,
        client_offset: POINT,
        scale: f64,
        now_ms: u64,
        modifiers: KeyboardModifiers,
    ) -> NativeReading {
        use windows::Win32::UI::{
            Input::Pointer::{
                POINTER_FLAG_CANCELED, POINTER_FLAG_FIFTHBUTTON, POINTER_FLAG_FIRSTBUTTON,
                POINTER_FLAG_FOURTHBUTTON, POINTER_FLAG_PRIMARY, POINTER_FLAG_SECONDBUTTON,
                POINTER_FLAG_THIRDBUTTON,
            },
            WindowsAndMessaging::{
                PEN_FLAG_ERASER, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X,
                PEN_MASK_TILT_Y, PT_MOUSE, PT_TOUCHPAD, TOUCH_MASK_CONTACTAREA,
                TOUCH_MASK_PRESSURE, WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERLEAVE,
                WM_POINTERUP,
            },
        };
        let native = packet.info();
        let screen = packet.screen_position();
        let flags = native.pointerFlags.0;
        let mut sensors = RawSensors::default();
        let kind = match packet {
            PointerPacket::Pen(pen, _) => {
                if pen.penMask & PEN_MASK_PRESSURE != 0 {
                    sensors.pressure = Some(f64::from(pen.pressure) / 1024.0);
                }
                if pen.penMask & PEN_MASK_ROTATION != 0 {
                    sensors.twist_degrees = Some(f64::from(pen.rotation));
                }
                if pen.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y)
                    == (PEN_MASK_TILT_X | PEN_MASK_TILT_Y)
                {
                    sensors.angles = Some(PenAngles::TiltDegrees {
                        x: f64::from(pen.tiltX),
                        y: f64::from(pen.tiltY),
                    });
                }
                PointerKind::Pen {
                    tool: if pen.penFlags & PEN_FLAG_ERASER != 0 {
                        PenTool::Eraser
                    } else {
                        PenTool::Tip
                    },
                }
            }
            PointerPacket::Touch(touch) => {
                if touch.touchMask & TOUCH_MASK_PRESSURE != 0 {
                    sensors.pressure = Some(f64::from(touch.pressure) / 1024.0);
                }
                if touch.touchMask & TOUCH_MASK_CONTACTAREA != 0 {
                    let contact = touch.rcContactRaw;
                    sensors.contact_size = Some((
                        f64::from(contact.right) - f64::from(contact.left),
                        f64::from(contact.bottom) - f64::from(contact.top),
                    ));
                }
                PointerKind::Touch
            }
            PointerPacket::Pointer(info) => match info.pointerType {
                PT_MOUSE => PointerKind::Mouse,
                PT_TOUCHPAD => PointerKind::Trackpad,
                _ => PointerKind::Unknown,
            },
        };
        let mut buttons = PointerButtons::NONE;
        for (flag, button) in [
            (POINTER_FLAG_FIRSTBUTTON, PointerButton::PRIMARY),
            (POINTER_FLAG_SECONDBUTTON, PointerButton::SECONDARY),
            (POINTER_FLAG_THIRDBUTTON, PointerButton::AUXILIARY),
            (POINTER_FLAG_FOURTHBUTTON, PointerButton::BACK),
            (POINTER_FLAG_FIFTHBUTTON, PointerButton::FORWARD),
        ] {
            if flags & flag.0 != 0 {
                buttons = buttons.with(button);
            }
        }
        let change = button_change(native.ButtonChangeType);
        // A DOWN or UP that names no button is the primary's.
        let changed = change.map_or(PointerButton::PRIMARY, |(button, _)| button);
        // ENTER can follow DOWN while both notifications refer to the same native
        // packet: its cached ButtonChangeType is not a second press.
        let action = if message == WM_POINTERENTER {
            ReadingAction::Enter
        } else if message == WM_POINTERLEAVE {
            ReadingAction::Leave
        } else if flags & POINTER_FLAG_CANCELED.0 != 0 {
            ReadingAction::Cancel(NativeCancel::Platform)
        } else if change.is_some_and(|(_, down)| down) || message == WM_POINTERDOWN {
            ReadingAction::Press(changed)
        } else if change.is_some_and(|(_, down)| !down) || message == WM_POINTERUP {
            ReadingAction::Release(changed)
        } else {
            ReadingAction::Move
        };
        let time = ReadingTime {
            coarse: NativeTime::TickMillis {
                tick: native.dwTime,
                observed_ms: now_ms,
            },
            counter: (native.PerformanceCount != 0).then_some(QpcTicks(native.PerformanceCount)),
        };
        let mut reading = NativeReading::new(
            pointer_contact(kind == PointerKind::Mouse, native.pointerId),
            kind,
            action,
            time,
        )
        .on(SURFACE)
        .at_device(
            screen.0 + f64::from(client_offset.x),
            screen.1 + f64::from(client_offset.y),
            scale,
        )
        .with_primary(flags & POINTER_FLAG_PRIMARY.0 != 0)
        .with_buttons(buttons)
        .with_modifiers(modifiers)
        .with_sensors(sensors);
        if !native.sourceDevice.is_invalid() {
            reading = reading.from_device(NativeDevice(native.sourceDevice.0 as usize as u64));
        }
        reading
    }

    /// Native history is newest first and includes current. Read it before any
    /// callback, then decode every measured sample against one uptime snapshot.
    fn native_history(raw: u32) -> Option<Vec<PointerPacket>> {
        use windows::Win32::UI::{
            Input::Pointer::{
                GetPointerInfo, GetPointerInfoHistory, GetPointerPenInfoHistory,
                GetPointerTouchInfoHistory,
            },
            WindowsAndMessaging::{PT_PEN, PT_TOUCH},
        };
        let mut info = POINTER_INFO::default();
        // SAFETY: `info` is a live, writable local. The native ID is obtained
        // from the message; Windows rejects stale or invented IDs.
        unsafe { GetPointerInfo(raw, &raw mut info) }.ok()?;
        let count = info.historyCount.max(1);
        match info.pointerType {
            PT_PEN => read_history(count, |count, buffer: &mut [POINTER_PEN_INFO]| {
                // SAFETY: `*count` is `buffer.len()` (`read_history`), so the
                // call writes at most the entries `buffer` holds.
                unsafe { GetPointerPenInfoHistory(raw, count, Some(buffer.as_mut_ptr())) }
            })
            .map(|readings| {
                // One mapping for the message: its device is the same throughout.
                let map = HimetricMap::of(info.sourceDevice);
                readings
                    .into_iter()
                    .map(|pen| PointerPacket::Pen(pen, map))
                    .collect()
            }),
            PT_TOUCH => read_history(count, |count, buffer: &mut [POINTER_TOUCH_INFO]| {
                // SAFETY: as for the pen history.
                unsafe { GetPointerTouchInfoHistory(raw, count, Some(buffer.as_mut_ptr())) }
            })
            .map(|readings| readings.into_iter().map(PointerPacket::Touch).collect()),
            _ => read_history(count, |count, buffer: &mut [POINTER_INFO]| {
                // SAFETY: as for the pen history.
                unsafe { GetPointerInfoHistory(raw, count, Some(buffer.as_mut_ptr())) }
            })
            .map(|readings| readings.into_iter().map(PointerPacket::Pointer).collect()),
        }
    }

    /// Read up to `count` history entries: `query` is handed a buffer of
    /// `count` entries and that same count, which it rewrites to the number it
    /// filled. A count larger than the buffer is refused.
    fn read_history<T: Default + Clone>(
        count: u32,
        query: impl FnOnce(&mut u32, &mut [T]) -> windows::core::Result<()>,
    ) -> Option<Vec<T>> {
        let len = usize::try_from(count).ok()?;
        let mut values = Vec::new();
        values.try_reserve_exact(len).ok()?;
        values.resize(len, T::default());
        let mut filled = count;
        query(&mut filled, &mut values).ok()?;
        let filled = usize::try_from(filled)
            .ok()
            .filter(|filled| *filled <= len)?;
        values.truncate(filled);
        Some(values)
    }

    /// Assemble a newest-first native history: an update carries the older
    /// readings as its coalesced batch.
    fn assemble_history(
        registry: &mut ContactRegistry,
        packets: &[PointerPacket],
        message: u32,
        client_offset: POINT,
        scale: f64,
        now_ms: u64,
        modifiers: KeyboardModifiers,
    ) -> Assembled {
        use windows::Win32::UI::WindowsAndMessaging::WM_POINTERUPDATE;
        let reading =
            |packet| pointer_reading(packet, message, client_offset, scale, now_ms, modifiers);
        let Some((&current, older)) = packets.split_first() else {
            return Assembled::new();
        };
        let assembled = if message == WM_POINTERUPDATE && !older.is_empty() {
            let mut batch = MoveBatch::new();
            for &packet in older.iter().rev() {
                batch.push_coalesced(reading(packet));
            }
            batch.decode(registry, &reading(current))
        } else {
            decode(registry, &reading(current))
        };
        assembled.unwrap_or_else(DecodeError::into_inputs)
    }

    /// The pointers of the frame `raw` belongs to, each with the message its
    /// own state change reads as, when the frame holds more than one. The
    /// system sends one message per pointer of a frame; the others are
    /// skipped once the frame is read ([`skip_frame_messages`]).
    pub(super) fn pointer_frame_members(raw: u32) -> Option<Vec<(u32, u32)>> {
        use windows::Win32::UI::{
            Input::Pointer::{
                GetPointerFrameInfo, POINTER_FLAG_DOWN, POINTER_FLAG_UP, POINTER_FLAG_UPDATE,
            },
            WindowsAndMessaging::{WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE},
        };
        let mut count = 0u32;
        // SAFETY: a null buffer asks only for the count, into a live local.
        unsafe { GetPointerFrameInfo(raw, &raw mut count, None) }.ok()?;
        if count < 2 {
            return None;
        }
        let mut frame = read_history(count, |count, buffer: &mut [POINTER_INFO]| {
            // SAFETY: `*count` is `buffer.len()` (`read_history`).
            unsafe { GetPointerFrameInfo(raw, count, Some(buffer.as_mut_ptr())) }
        })?;
        // The system lists the frame newest first; deliver in sample order.
        frame.reverse();
        frame
            .iter()
            .map(|info| {
                let flags = info.pointerFlags;
                let message = if flags.contains(POINTER_FLAG_DOWN) {
                    WM_POINTERDOWN
                } else if flags.contains(POINTER_FLAG_UP) {
                    WM_POINTERUP
                } else if flags.contains(POINTER_FLAG_UPDATE) {
                    WM_POINTERUPDATE
                } else {
                    return None;
                };
                Some((info.pointerId, message))
            })
            .collect()
    }

    /// The frame `raw` belongs to was read whole: the system sends no message
    /// for its other pointers.
    pub(super) fn skip_frame_messages(raw: u32) {
        use windows::Win32::UI::Input::Pointer::SkipPointerFrameMessages;
        // SAFETY: only the id of the message being handled is passed; a refused
        // call leaves the other messages to arrive, each a frame of its own.
        if unsafe { SkipPointerFrameMessages(raw) }.is_err() {
            tracing::warn!(raw, "pointer frame messages could not be skipped");
        }
    }

    /// Whether the `WM_POINTER` id `raw` is the mouse's. A stale or invented id
    /// is not.
    pub(super) fn is_mouse_pointer(raw: u32) -> bool {
        use windows::Win32::UI::{
            Input::Pointer::GetPointerType,
            WindowsAndMessaging::{POINTER_INPUT_TYPE, PT_MOUSE},
        };
        let mut native_kind = POINTER_INPUT_TYPE::default();
        // SAFETY: an initialized out value owned by this call; Windows refuses a
        // stale or invented id.
        unsafe { GetPointerType(raw, &raw mut native_kind) }.is_ok() && native_kind == PT_MOUSE
    }

    /// The input a `WM_POINTER*` `message` for pointer `raw` makes; `mouse` is
    /// [`is_mouse_pointer`]`(raw)`.
    pub(super) fn native_pointer_input(
        hwnd: HWND,
        message: u32,
        (raw, mouse): (u32, bool),
        scale: f64,
        clock: &MessageClock,
        registry: &RefCell<ContactRegistry>,
    ) -> Assembled {
        use windows::Win32::UI::WindowsAndMessaging::WM_POINTERCAPTURECHANGED;
        let contact = pointer_contact(mouse, raw);
        let cancel = |reason| {
            let time = EventTime::from_nanos(clock.message_time());
            registry
                .borrow_mut()
                .cancel_contact(SURFACE, contact, time, reason)
                .into_iter()
                .collect()
        };
        if message == WM_POINTERCAPTURECHANGED {
            return cancel(NativeCancel::CaptureLost);
        }
        let Some(packets) = native_history(raw) else {
            tracing::warn!(raw, "pointer history unavailable; its contact is cancelled");
            return cancel(NativeCancel::InvalidInput);
        };
        let mut offset = POINT::default();
        // SAFETY: the live owner HWND and initialized client translation buffer
        // remain valid under the window procedure's ContextGuard.
        let now = unsafe {
            if !ScreenToClient(hwnd, &raw mut offset).as_bool() {
                tracing::warn!(raw, "client origin unavailable; the pointer is cancelled");
                return cancel(NativeCancel::InvalidInput);
            }
            GetTickCount64()
        };
        let modifiers = message_modifiers();
        let assembled = assemble_history(
            &mut registry.borrow_mut(),
            &packets,
            message,
            offset,
            scale,
            now,
            modifiers,
        );
        clock.observe(&assembled);
        assembled
    }

    pub(super) fn native_device_kind(
        handle: windows::Win32::Foundation::HANDLE,
    ) -> Option<PointerKind> {
        use windows::Win32::UI::{
            Controls::{
                POINTER_DEVICE_INFO, POINTER_DEVICE_TYPE_EXTERNAL_PEN,
                POINTER_DEVICE_TYPE_INTEGRATED_PEN, POINTER_DEVICE_TYPE_TOUCH,
                POINTER_DEVICE_TYPE_TOUCH_PAD,
            },
            Input::Pointer::GetPointerDevice,
        };
        let mut info = POINTER_DEVICE_INFO::default();
        // SAFETY: the notification carries the device handle and the initialized
        // output lives through the query. Stale/removed handles are refused by Windows.
        unsafe {
            GetPointerDevice(handle, &raw mut info).ok()?;
        }
        Some(match info.pointerDeviceType {
            POINTER_DEVICE_TYPE_INTEGRATED_PEN | POINTER_DEVICE_TYPE_EXTERNAL_PEN => {
                PointerKind::Pen { tool: PenTool::Tip }
            }
            POINTER_DEVICE_TYPE_TOUCH => PointerKind::Touch,
            POINTER_DEVICE_TYPE_TOUCH_PAD => PointerKind::Trackpad,
            _ => PointerKind::Unknown,
        })
    }

    /// The device a `WM_POINTERDEVICECHANGE` names.
    pub(super) fn native_device(lparam: LPARAM) -> NativeDevice {
        NativeDevice(lparam.0 as usize as u64)
    }

    // ============================================================================
    // Message-time modifiers
    // ============================================================================
    //
    // The translation decisions for keys (vk -> Key fallback, scancode -> Code,
    // the WM_CHAR merge) are pure functions in `crate::input::keys`, where their
    // tests execute on every host; this file only unpacks the Win32 message
    // parameters and feeds them through. The WM_CHAR pairing model is documented
    // on that module.

    /// Whether `key` was down when the message being processed was generated.
    fn key_down_in_queue(key: VIRTUAL_KEY) -> bool {
        // SAFETY: `GetKeyState` takes a plain virtual-key code and reads only the
        // calling thread's queue-synchronized key state; no pointers are involved.
        unsafe { GetKeyState(i32::from(key.0)) < 0 }
    }

    /// The keyboard modifiers held when the message being processed was
    /// generated (see the module doc for why this is not `GetAsyncKeyState`).
    pub(super) fn message_modifiers() -> KeyboardModifiers {
        let mut mods = KeyboardModifiers::NONE;
        for (key, modifier) in [
            (VK_SHIFT, KeyboardModifiers::SHIFT),
            (VK_CONTROL, KeyboardModifiers::CONTROL),
            (VK_MENU, KeyboardModifiers::ALT),
            (VK_LWIN, KeyboardModifiers::META),
            (VK_RWIN, KeyboardModifiers::META),
        ] {
            if key_down_in_queue(key) {
                mods |= modifier;
            }
        }
        mods
    }

    // ============================================================================
    // Pointer Event Conversion (W3C ui-events 0.3 API)
    // ============================================================================

    /// The set of mouse buttons Win32 reports as held in a message's `WPARAM`.
    ///
    /// Every mouse message (`WM_MOUSEMOVE`, `WM_*BUTTONDOWN`, `WM_*BUTTONUP`,
    /// `WM_MOUSEWHEEL`) carries the `MK_*` mask in the low word of `WPARAM`, and
    /// Win32 already applies the W3C rule for us: the bit for a button is set on
    /// its DOWN message and clear on its UP message, i.e. the set held *after*
    /// the event. That makes this stateless — unlike the winit backend, which has
    /// to track the transition itself because winit does not surface a mask.
    ///
    /// This matters beyond fidelity: the framework tells a drag-move from a hover
    /// by asking whether any button is held. A move that always reports an empty
    /// set is delivered as a hover, so no gesture recognizer ever sees it and the
    /// drag is silently re-interpreted as a tap on release.
    ///
    /// The bits are [`MOUSE_BUTTONS`]'.
    #[inline]
    fn held_buttons(wparam: WPARAM) -> PointerButtons {
        let mask = wparam.0 & 0xffff;
        MOUSE_BUTTONS
            .iter()
            .filter(|codes| mask & codes.key_state_bit as usize != 0)
            .fold(PointerButtons::NONE, |held, codes| held.with(codes.button))
    }

    /// The button a button message reports and whether it is a press, or `None`
    /// for any other message.
    pub(super) fn button_message(msg: u32, wparam: WPARAM) -> Option<(PointerButton, bool)> {
        match msg {
            WM_LBUTTONDOWN => Some((PointerButton::PRIMARY, true)),
            WM_LBUTTONUP => Some((PointerButton::PRIMARY, false)),
            WM_RBUTTONDOWN => Some((PointerButton::SECONDARY, true)),
            WM_RBUTTONUP => Some((PointerButton::SECONDARY, false)),
            WM_MBUTTONDOWN => Some((PointerButton::AUXILIARY, true)),
            WM_MBUTTONUP => Some((PointerButton::AUXILIARY, false)),
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let button = match (wparam.0 >> 16) & 0xffff {
                    1 => PointerButton::BACK,
                    2 => PointerButton::FORWARD,
                    _ => return None,
                };
                Some((button, msg == WM_XBUTTONDOWN))
            }
            _ => None,
        }
    }

    /// Captures the mouse for `hwnd` on a button press, so the rest of the press
    /// sequence — its moves and its release — reaches this window even when the
    /// cursor leaves the client area. A press while the window already holds the
    /// capture (a second button) keeps it.
    pub(super) fn capture_on_press(hwnd: HWND) {
        // SAFETY: plain handle calls on the window's owner thread, where
        // `window_proc` runs; `SetCapture` captures for this thread's own window.
        unsafe {
            if GetCapture() != hwnd {
                SetCapture(hwnd);
            }
        }
    }

    /// Releases `hwnd`'s mouse capture after a button release that leaves no
    /// button held (`wparam` is the release's `MK_*` mask, which no longer has
    /// the released button). The release itself sends `WM_CAPTURECHANGED`, which
    /// the `WM_CAPTURECHANGED` handler recognizes as the sequence's own end.
    pub(super) fn release_capture_after(hwnd: HWND, wparam: WPARAM) {
        if !held_buttons(wparam).is_empty() {
            return;
        }
        // SAFETY: plain calls on the window's owner thread; the capture is only
        // released while this window holds it, never another window's.
        unsafe {
            if GetCapture() == hwnd
                && let Err(error) = ReleaseCapture()
            {
                tracing::warn!(%error, "ReleaseCapture failed after the last button release");
            }
        }
    }

    /// The pointer cancel for a `WM_CAPTURECHANGED` that took the capture from a
    /// press sequence still in progress; nothing when the sequence ended on its
    /// own.
    ///
    /// `lparam` is the window gaining the capture. The sequence is in progress
    /// while a button is still down in the queue-synchronized key state. After
    /// the last release — the one that makes [`release_capture_after`] let go —
    /// every button there is up. Another window, a modal loop or `WM_CANCELMODE`
    /// taking the capture mid-drag leaves a button down, and without the cancel
    /// this window would never see that sequence's release.
    pub(super) fn capture_changed_input(
        hwnd: HWND,
        lparam: LPARAM,
        time: u64,
        registry: &RefCell<ContactRegistry>,
    ) -> Vec<PlatformInput> {
        let gaining = HWND(lparam.0 as *mut core::ffi::c_void);
        let held = MOUSE_BUTTONS
            .iter()
            .any(|codes| key_down_in_queue(VIRTUAL_KEY(codes.virtual_key)));
        if gaining == hwnd || !held {
            return Vec::new();
        }
        registry.borrow_mut().cancel_contact(
            SURFACE,
            MOUSE_CONTACT,
            EventTime::from_nanos(time),
            NativeCancel::CaptureLost,
        )
    }

    /// A legacy `WM_MOUSE*` message as a reading of the primary mouse.
    fn legacy_mouse_reading(
        action: ReadingAction,
        wparam: WPARAM,
        lparam: LPARAM,
        scale_factor: f64,
        time: u64,
    ) -> NativeReading {
        NativeReading::new(
            MOUSE_CONTACT,
            PointerKind::Mouse,
            action,
            ReadingTime::epoch(EventTime::from_nanos(time)),
        )
        .on(SURFACE)
        .at_device(
            f64::from(get_x_lparam(lparam)),
            f64::from(get_y_lparam(lparam)),
            scale_factor,
        )
        .with_primary(true)
        .with_buttons(held_buttons(wparam))
        .with_modifiers(message_modifiers())
    }

    /// Assemble a `WM_[LRMX]BUTTON*` (see [`button_message`]).
    pub(super) fn mouse_button_input(
        registry: &RefCell<ContactRegistry>,
        button: PointerButton,
        is_down: bool,
        wparam: WPARAM,
        lparam: LPARAM,
        scale_factor: f64,
        time: u64,
    ) -> Assembled {
        let action = if is_down {
            ReadingAction::Press(button)
        } else {
            ReadingAction::Release(button)
        };
        let reading = legacy_mouse_reading(action, wparam, lparam, scale_factor, time);
        decode(&mut registry.borrow_mut(), &reading).unwrap_or_else(DecodeError::into_inputs)
    }

    /// Assemble a `WM_MOUSEMOVE`.
    pub(super) fn mouse_move_input(
        registry: &RefCell<ContactRegistry>,
        wparam: WPARAM,
        lparam: LPARAM,
        scale_factor: f64,
        time: u64,
    ) -> Assembled {
        let reading = legacy_mouse_reading(ReadingAction::Move, wparam, lparam, scale_factor, time);
        decode(&mut registry.borrow_mut(), &reading).unwrap_or_else(DecodeError::into_inputs)
    }

    /// The signed scroll distance both wheel messages carry in the high word of
    /// `wParam` (`GET_WHEEL_DELTA_WPARAM`), in multiples of `WHEEL_DELTA` (120).
    fn wheel_distance(wparam: WPARAM) -> i16 {
        ((wparam.0 as i32) >> 16) as i16
    }

    /// A wheel message as a [`WheelReading`], assembled through `registry`.
    ///
    /// Unlike every other client-area mouse message, `WM_MOUSEWHEEL` and
    /// `WM_MOUSEHWHEEL` deliver the cursor position in SCREEN coordinates
    /// (both messages' `lParam` docs:
    /// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>),
    /// so the point is converted to client space here before the registry
    /// DPI-scales it — otherwise scroll hit-testing targets the wrong child
    /// whenever the window's client origin is not the desktop origin.
    ///
    /// Win32's vertical sign (positive = wheel rotated away from the user) is the
    /// inverse of the cross-backend convention — positive = content scrolls down
    /// — and its horizontal sign (positive = tilted right) already matches it;
    /// `crate::input::scroll` holds the sign and unit table with citations.
    pub(super) fn mouse_wheel_input(
        registry: &RefCell<ContactRegistry>,
        hwnd: HWND,
        axis: WheelAxis,
        wparam: WPARAM,
        lparam: LPARAM,
        scale_factor: f64,
        time: u64,
    ) -> Option<PlatformInput> {
        let mut point = POINT {
            x: get_x_lparam(lparam),
            y: get_y_lparam(lparam),
        };
        // SAFETY: `point` is a live, writable local; `ScreenToClient` writes
        // nothing else. On failure (invalid `hwnd`) it returns FALSE and leaves
        // `point` unchanged.
        let converted = unsafe { ScreenToClient(hwnd, &raw mut point) };
        if !converted.as_bool() {
            return None;
        }
        let reading = WheelReading {
            axis,
            distance: wheel_distance(wparam),
            position: (f64::from(point.x), f64::from(point.y)),
            scale: scale_factor,
            time: EventTime::from_nanos(time),
            modifiers: message_modifiers(),
        };
        registry.borrow().wheel(&reading)
    }

    // ============================================================================
    // Keyboard events (simple wrappers)
    // ============================================================================

    /// A key message's `WPARAM` and `LPARAM`, decoded once.
    #[derive(Clone, Copy, Debug)]
    pub struct KeyMessage {
        /// The virtual key.
        pub vk: u16,
        /// The scancode, without its extended prefix.
        pub scan_code: u16,
        /// Whether the scancode has the `E0` prefix.
        pub extended: bool,
        /// Whether the key was already down (a keydown's auto-repeat).
        pub repeat: bool,
    }

    impl KeyMessage {
        /// The key message whose parameters are `wparam` and `lparam`; a key
        /// message's `WPARAM` is a 16-bit virtual key.
        pub fn new(wparam: WPARAM, lparam: LPARAM) -> Self {
            let (scan_code, extended, repeat) = keys::parse_key_lparam(lparam.0);
            Self {
                vk: wparam.0 as u16,
                scan_code,
                extended,
                repeat,
            }
        }
    }

    /// Convert WM_KEYDOWN to W3C KeyboardEvent.
    ///
    /// `translated_text` is the drained `WM_CHAR` burst for this keydown (see
    /// `window_proc`'s `WM_KEYDOWN` arm and `crate::input::keys`'s module doc
    /// for the pairing model); when present and typeable it becomes the event's
    /// `Key::Character`, otherwise the layout-independent virtual-key fallback
    /// applies. `time` is the message's generation time on the shared epoch
    /// ([`MessageClock::message_time`]), so a key and a click queued around it
    /// keep their order.
    pub fn key_down_event(
        message: KeyMessage,
        stroke: Keystroke,
        held_dead: &mut HeldKeys,
        time: u64,
    ) -> PlatformInput {
        let KeyMessage {
            vk,
            scan_code,
            extended,
            repeat: is_repeat,
        } = message;
        let (dead, translated_text) = match stroke {
            Keystroke::Dead => (true, None),
            Keystroke::Text(text) => (false, text),
        };
        // A press is also proof the key was released before: a dead press held
        // across a focus change whose release went to another window is forgotten
        // here, when the same physical key goes down again. A key that stays held
        // across a transient focus loss keeps its identity for its release.
        if dead {
            held_dead.press(scan_code, extended);
        } else if !is_repeat {
            held_dead.release(scan_code, extended);
        }

        let modifiers = message_modifiers();
        let fallback = keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT));
        let code = keys::scancode_to_code(scan_code, extended);
        // A dead key with nothing typed yet is reported as one, not as its
        // unshifted fallback character: otherwise a shortcut bound to that
        // character fires while the user composes an accented letter. Its second
        // press (or a space after it) types the accent itself and arrives as text.
        let key = if dead {
            Key::Named(NamedKey::Dead)
        } else {
            keys::key_for_keydown(
                fallback,
                translated_text,
                modifiers.contains(KeyboardModifiers::ALT),
                modifiers.contains(KeyboardModifiers::CONTROL),
                code,
            )
        };

        PlatformInput::Keyboard(
            KeyEvent::new(KeyState::Down, key, code, EventTime::from_nanos(time))
                .with_location(keys::location_for_code(code))
                .with_modifiers(modifiers)
                .with_repeat(if is_repeat {
                    KeyRepeat::AutoRepeat
                } else {
                    KeyRepeat::First
                }),
        )
    }

    /// Convert WM_KEYUP to W3C KeyboardEvent.
    ///
    /// No `WM_CHAR` pairs with a keyup, so the key is always the virtual-key
    /// fallback — for a letter that is its shift-respecting character, for OEM
    /// punctuation the US-layout position. Consumers that type text act on
    /// `KeyState::Down` only, so this asymmetry never reaches a text field.
    /// `time` is the message's, as for [`key_down_event`].
    pub fn key_up_event(message: KeyMessage, held_dead: &mut HeldKeys, time: u64) -> PlatformInput {
        let KeyMessage {
            vk,
            scan_code,
            extended,
            ..
        } = message;

        let modifiers = message_modifiers();
        // The release of a dead key is `Dead` too (W3C UI Events dead-key
        // sequence), so a pressed-key set sees the same identity go down and up.
        // The press decided it; the layout may have changed while the key was held.
        let key = if held_dead.release(scan_code, extended) {
            Key::Named(NamedKey::Dead)
        } else {
            keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT))
        };
        let code = keys::scancode_to_code(scan_code, extended);

        PlatformInput::Keyboard(
            KeyEvent::new(KeyState::Up, key, code, EventTime::from_nanos(time))
                .with_location(keys::location_for_code(code))
                .with_modifiers(modifiers),
        )
    }

    /// Build the KeyboardEvent for an out-of-band `WM_CHAR` — one that reached
    /// `window_proc` instead of being drained by a `WM_KEYDOWN` (Alt+numpad
    /// composition, or a directly-sent message). There is no owning physical
    /// key, so it is dispatched as a key-down with `Code::Unidentified` and no
    /// paired key-up; the text lane consumes `Key::Character` on key-down only.
    /// `time` is the message's, as for [`key_down_event`].
    pub fn stray_char_event(text: String, time: u64) -> PlatformInput {
        let modifiers = message_modifiers();

        PlatformInput::Keyboard(
            KeyEvent::new(
                KeyState::Down,
                Key::character(text),
                Code::Unidentified,
                EventTime::from_nanos(time),
            )
            .with_location(keys::CHARACTER_WITHOUT_A_KEY)
            .with_release(KeyRelease::Never)
            .with_modifiers(modifiers),
        )
    }

    /// What `TranslateMessage` produced for one keydown.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Keystroke {
        /// The drained `WM_CHAR` burst, or `None` for a key with no typeable
        /// translation (navigation keys, Ctrl chords).
        Text(Option<String>),
        /// A dead key: `TranslateMessage` queued `WM_DEADCHAR` (or
        /// `WM_SYSDEADCHAR`) for it on the layer the held modifiers select.
        Dead,
    }

    /// Physical keys currently held down whose press was of one kind (a dead
    /// key, a key an input method took), by scancode and extended flag, so
    /// each one's release is reported like its press, even if the keyboard
    /// layout or the modifiers changed while it was held. Owned per window.
    #[derive(Debug, Default)]
    pub struct HeldKeys(Vec<(u16, bool)>);

    impl HeldKeys {
        pub(super) fn press(&mut self, scan_code: u16, extended: bool) {
            if !self.0.contains(&(scan_code, extended)) {
                self.0.push((scan_code, extended));
            }
        }

        /// Whether the released key was pressed as one of these; forgets it.
        pub(super) fn release(&mut self, scan_code: u16, extended: bool) -> bool {
            let before = self.0.len();
            self.0.retain(|held| *held != (scan_code, extended));
            self.0.len() != before
        }
    }

    #[cfg(test)]
    mod tests;
}

/// The input's reach into a window's input for the tests of this crate's
/// Win32 features, which decode readings into a real window's registry.
#[cfg(all(windows, test))]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::rc::{Rc, Weak};

    use windows::Win32::Foundation::HWND;

    use super::{InputWindow, InputWindows};
    use crate::input::assembly::{Assembled, DecodeError, NativeReading, decode};

    thread_local! {
        /// The input of every class made on this thread.
        static MADE: RefCell<Vec<Weak<InputWindows>>> = const { RefCell::new(Vec::new()) };
    }

    /// Record a class's input, made on this thread.
    pub(super) fn made(input: &Rc<InputWindows>) {
        MADE.with(|made| made.borrow_mut().push(Rc::downgrade(input)));
    }

    /// The input of the window `hwnd` names, from any class made on this
    /// thread.
    pub(super) fn window_input(hwnd: HWND) -> Option<Rc<InputWindow>> {
        MADE.with(|made| {
            made.borrow()
                .iter()
                .filter_map(Weak::upgrade)
                .find_map(|input| input.by_handle(hwnd))
        })
    }

    /// Decode `reading` into the registry of the window `hwnd` names, which
    /// the input holds; the window's surface is the reading's.
    pub(crate) fn decode_into(
        hwnd: HWND,
        reading: NativeReading,
    ) -> Result<Assembled, DecodeError> {
        let input = window_input(hwnd).expect("the window's input");
        decode(
            &mut input.registry.borrow_mut(),
            &reading.on(super::raw::SURFACE),
        )
    }
}

#[cfg(all(windows, test))]
mod tests;
