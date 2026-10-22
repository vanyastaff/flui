//! The input messages of FLUI's window class: the mouse and its buttons and
//! wheel, the pointer messages, the keyboard and its characters, and the
//! keyboard layout.
//!
//! [`InputMessages`] is the hook the Windows host registers on the class
//! beside the window's own ([`WindowProc`](crate::window::windows::proc::WindowProc))
//! and the precision touchpad's; each message reaches one of them.

// The FFI half calls Win32 directly: `unsafe` is expected on Windows only,
// where it is compiled; the pure halves forbid it.
#![cfg_attr(windows, expect(unsafe_code))]

pub mod direct_manipulation;
#[cfg(windows)]
pub(crate) mod raw;

#[cfg(windows)]
pub(crate) use direct_manipulation::Touchpad;

#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
        WindowsAndMessaging::{
            DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, ISMEX_NOSEND, InSendMessageEx,
            WM_CAPTURECHANGED, WM_CHAR, WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
            WM_MOUSEWHEEL, WM_POINTERCAPTURECHANGED, WM_POINTERDEVICECHANGE, WM_POINTERDOWN,
            WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE, WM_RBUTTONDOWN,
            WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
        },
    },
};

#[cfg(windows)]
use crate::owner::windows::{WindowMessages, WindowsLoop};
#[cfg(windows)]
use crate::window::windows::proc::{WindowContext, with_message_context};

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
        owner.register_handler("on_keyboard_layout_change", callback, |handlers| {
            &mut handlers.keyboard_layout_changed
        });
    }
}

/// The input part of FLUI's window procedure: it answers the input messages
/// and passes every other message on.
#[cfg(windows)]
pub(crate) struct InputMessages;

#[cfg(windows)]
impl WindowMessages for InputMessages {
    fn answers(&self, msg: u32, _wparam: WPARAM) -> bool {
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
        )
    }

    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        if !self.answers(msg, wparam) {
            return None;
        }
        // SAFETY: the arguments as Win32 delivered them to the class's
        // procedure, which is `with_message_context`'s and `handle`'s
        // contract too.
        unsafe {
            with_message_context(hwnd, |ctx, slot| {
                Self::handle(hwnd, msg, wparam, lparam, ctx, slot)
            })
        }
    }
}

#[cfg(windows)]
impl InputMessages {
    /// The input messages ([`WindowMessages::message`]), with `ctx`, the
    /// window's context borrowed for the message, and `slot`, the value its
    /// `GWLP_USERDATA` slot held when the borrow began.
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
        ctx: Option<&WindowContext>,
        slot: isize,
    ) -> Option<LRESULT> {
        // SAFETY: see the `# Safety` section above. This single `unsafe`
        // block covers every Win32 call in the `match` below:
        // `GetWindowLongPtrW` takes `hwnd` and a plain index, no pointers;
        // `TrackMouseEvent` (`WM_MOUSEMOVE`) takes a `&raw` pointer to a
        // stack-local, correctly-sized struct; `PostMessageW` and
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
                            &ctx.pointer_registry,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                            time,
                        );
                        for event in events {
                            if ctx.pointer_registry.borrow().delivers(&event) {
                                ctx.callbacks.dispatch_input(event);
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
                    // no context to deliver to, and is settled before user
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
                            &ctx.pointer_registry,
                            button,
                            is_down,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                            time,
                        );
                        for event in events {
                            if ctx.pointer_registry.borrow().delivers(&event) {
                                ctx.callbacks.dispatch_input(event);
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
                            &ctx.pointer_registry,
                        );
                        for cancel in cancels {
                            ctx.callbacks.dispatch_input(cancel);
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
                            &ctx.pointer_registry,
                            hwnd,
                            axis,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                            time,
                        );
                        if let Some(event) = event {
                            ctx.callbacks.dispatch_input(event);
                        }
                    }
                    LRESULT(0)
                }

                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let vk = wparam.0 as u16;
                    let is_repeat = (lparam.0 & (1 << 30)) != 0;

                    if let Some(ctx) = ctx {
                        // Read before the character drain below retrieves
                        // other messages.
                        let time = ctx.message_clock.message_time();

                        // Track modifiers.
                        ctx.modifiers.set(raw::message_modifiers());

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
                            key_down_event(wparam, lparam, stroke, &mut held, time)
                        };
                        let result = ctx.callbacks.dispatch_input(event);
                        // A callback can close this window and create another
                        // with a recycled HWND. The entry guard pins the old
                        // context, so matching userdata proves the native
                        // default still targets this exact live window.
                        if result.default_prevented
                            || GetWindowLongPtrW(hwnd, GWLP_USERDATA) != slot
                        {
                            // Withdrawn with its keydown: the held
                            // characters are dropped here.
                            return Some(LRESULT(0));
                        }
                        // The window's own keys (the fullscreen hotkey) are a
                        // default the app can prevent.
                        ctx.apply_window_hotkey(hwnd, vk, is_repeat);
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

                        use raw::key_up_event;
                        let event = {
                            let mut held = ctx.held_dead_keys.borrow_mut();
                            key_up_event(wparam, lparam, &mut held, time)
                        };
                        let result = ctx.callbacks.dispatch_input(event);
                        if result.default_prevented
                            || GetWindowLongPtrW(hwnd, GWLP_USERDATA) != slot
                        {
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
                    // high surrogate is parked in the context until its low
                    // half arrives.
                    if let Some(ctx) = ctx {
                        let (pending, text) = crate::input::keys::assemble_stray_wm_char(
                            ctx.pending_high_surrogate.get(),
                            wparam.0 as u16,
                        );
                        ctx.pending_high_surrogate.set(pending);
                        if let Some(text) = text {
                            use raw::stray_char_event;
                            let time = ctx.message_clock.message_time();
                            ctx.callbacks.dispatch_input(stray_char_event(text, time));
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

#[cfg(windows)]
mod pointer_messages {
    //! The window's pointer messages: `WM_POINTER*`, device arrival and removal,
    //! the mouse's hover on either route, and the end of every open contact when
    //! the window loses focus or is minimised.

    use flui_platform::{EventTime, pointer::PointerEvent};
    use windows::Win32::{
        Foundation::{HANDLE, HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            PDC_ARRIVAL, PDC_REMOVAL, WM_POINTERCAPTURECHANGED, WM_POINTERLEAVE,
        },
    };

    use crate::input::assembly::NativeCancel;
    use crate::input::windows::raw as events;
    use crate::window::windows::proc::WindowContext;
    use flui_platform::PlatformInput;

    impl WindowContext {
        /// Record whether the mouse is over the window, raising the hover change
        /// only when it changed. Every route the mouse reaches the window by
        /// reports through here: `WM_MOUSEMOVE`/`WM_MOUSELEAVE`, and `WM_POINTER*`
        /// for a mouse under `EnableMouseInPointer`.
        pub(crate) fn set_hovered(&self, hovered: bool) {
            if self.is_hovered.replace(hovered) != hovered {
                self.callbacks.dispatch_hover_status_change(hovered);
            }
        }

        /// A `WM_POINTER*` `message` for the pointer `wparam` names.
        pub(crate) fn pointer_message(&self, hwnd: HWND, message: u32, wparam: WPARAM) {
            let raw = (wparam.0 & 0xffff) as u32;
            let mouse = events::is_mouse_pointer(raw);
            // The mouse's hover follows the messages that carry it. A capture
            // change says nothing about where it is, and no message follows it
            // (a press dragged outside sends no leave while it is held), so the
            // hover is read from the cursor once its input is delivered.
            let hover = (mouse && message != WM_POINTERCAPTURECHANGED)
                .then_some(message != WM_POINTERLEAVE);
            let inputs = events::native_pointer_input(
                hwnd,
                message,
                (raw, mouse),
                self.scale_factor.get(),
                &self.message_clock,
                &self.pointer_registry,
            );
            // The hover is settled before any of the message's input reaches user
            // code: a callback that pumps messages may see the cursor come back,
            // and nothing written after it may undo that.
            if let Some(hovered) = hover {
                self.set_hovered(hovered);
            }
            for input in inputs {
                let deliver = self.pointer_registry.borrow().delivers(&input);
                if deliver {
                    self.callbacks.dispatch_input(input);
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
        pub(crate) fn pointer_device_changed(&self, wparam: WPARAM, lparam: LPARAM) {
            let device = events::native_device(lparam);
            let time = EventTime::from_nanos(self.message_clock.message_time());
            let mut inputs = Vec::new();
            if wparam.0 == PDC_REMOVAL as usize {
                let (ended, removed) = self
                    .pointer_registry
                    .borrow_mut()
                    .remove_device(device, time);
                inputs.extend(ended.into_iter().map(|(_, input)| input));
                inputs.extend(
                    removed
                        .map(|change| PlatformInput::Pointer(PointerEvent::DeviceRemoved(change))),
                );
            } else if wparam.0 == PDC_ARRIVAL as usize
                && let Some(kind) =
                    events::native_device_kind(HANDLE(lparam.0 as *mut core::ffi::c_void))
            {
                let added = self
                    .pointer_registry
                    .borrow_mut()
                    .add_device(device, kind, time);
                inputs.extend(
                    added.map(|change| PlatformInput::Pointer(PointerEvent::DeviceAdded(change))),
                );
            }
            for input in inputs {
                self.callbacks.dispatch_input(input);
            }
        }

        /// End every open pointer sequence and touchpad manipulation as
        /// `FocusLost`: the window lost focus or was minimised, so the releases
        /// may never reach it. What still arrives for an ended contact is an
        /// orphan and delivers nothing.
        pub(crate) fn cancel_pointers(&self, hwnd: HWND) {
            // The touchpad first: its manipulation ends through the registry.
            self.cancel_touchpad(hwnd);
            let time = EventTime::from_nanos(self.message_clock.message_time());
            let cancels = self.pointer_registry.borrow_mut().cancel_surface(
                events::SURFACE,
                time,
                NativeCancel::FocusLost,
            );
            for cancel in cancels {
                self.callbacks.dispatch_input(cancel);
            }
        }
    }

    /// Whether the cursor is over `hwnd`'s client area, the window on top there;
    /// `false` when the cursor cannot be read.
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
}
