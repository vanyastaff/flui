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
/// (navigation keys, Ctrl chords — see `shared::keys::wm_char_text`).
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

    crate::shared::keys::wm_char_text(&units)
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
        while PeekMessageW(&raw mut msg, Some(hwnd), WM_SYSCHAR, WM_SYSCHAR, PM_REMOVE).as_bool() {
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
