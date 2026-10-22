//! The input messages of FLUI's window class: the mouse and its buttons and
//! wheel, the pointer messages, the keyboard and its characters, and the
//! keyboard layout.
//!
//! [`InputMessages`] is the hook the Windows host registers on the class
//! beside the window's own ([`WindowProc`](crate::window::windows::proc::WindowProc))
//! and the precision touchpad's; each message reaches one of them. It lives
//! with the Win32 input code until the input feature takes it.

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

use crate::owner::windows::WindowMessages;
use crate::window::windows::proc::{WindowContext, with_message_context};

mod keystroke_queue;
mod pointer_messages;

use keystroke_queue::{
    drain_dead_char, drain_translated_chars, restore_translated_sys_chars,
    take_translated_sys_chars,
};

/// The input part of FLUI's window procedure: it answers the input messages
/// and passes every other message on.
pub(crate) struct InputMessages;

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

                        let events = super::events::mouse_move_input(
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
                    use super::events::{
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
                        let cancels = super::events::capture_changed_input(
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
                        use crate::shared::pointer_assembly::WheelAxis;
                        let axis = if msg == WM_MOUSEWHEEL {
                            WheelAxis::Vertical
                        } else {
                            WheelAxis::Horizontal
                        };
                        let time = ctx.message_clock.message_time();
                        let event = super::events::mouse_wheel_input(
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
                        ctx.modifiers.set(super::events::message_modifiers());

                        // Merge the fully-translated character(s) for this
                        // keystroke into the keydown. The message loop runs
                        // `TranslateMessage` before `DispatchMessageW` (see
                        // `run`), so the WM_CHAR burst for this keydown —
                        // shift/caps/AltGr applied, dead keys resolved — is
                        // already in the queue and is drained synchronously
                        // here. Pairing model and merge rules:
                        // `crate::shared::keys` module doc.
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
                                super::events::Keystroke::Dead
                            } else {
                                super::events::Keystroke::Text(translated)
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
                        use super::events::key_down_event;
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
                        ctx.modifiers.set(super::events::message_modifiers());

                        use super::events::key_up_event;
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
                        let (pending, text) = crate::shared::keys::assemble_stray_wm_char(
                            ctx.pending_high_surrogate.get(),
                            wparam.0 as u16,
                        );
                        ctx.pending_high_surrogate.set(pending);
                        if let Some(text) = text {
                            use super::events::stray_char_event;
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
