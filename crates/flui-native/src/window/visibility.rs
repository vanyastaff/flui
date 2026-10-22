//! Pure hidden-surface visibility rules for the native backends.
//!
//! The runtime's hidden-surface gating (`PlatformWindow::
//! on_visibility_status_change` → `PlatformToUi::WindowVisibility` → the
//! per-presentation `FrameClock` gate and the UI runtime-wide `AppLifecycleState`
//! derivation) only ever fires if a backend actually *dispatches* the
//! callback. The winit backend rides `WindowEvent::Occluded`; the native
//! Win32 and AppKit backends have no such ready-made event and must derive
//! the signal from their own message streams. The derivations are decidable
//! rules over plain values, so they live here, free of FFI, while the
//! backends keep only the plumbing that calls the operating system; each
//! half is compiled for the host that applies it, and both are under `test`
//! on every host.
//!
//! Contract note (mirrors `PlatformWindow::on_visibility_status_change`):
//! the callback parameter is `is_visible` — `true` when the window's surface
//! became visible again, `false` when it stopped being visible (minimized,
//! hidden, or fully occluded). Every rule here speaks that polarity.

#![forbid(unsafe_code)]

/// `WM_SIZE` `wParam` values (`winuser.h`). Defined here — not imported from
/// the `windows` crate — so the rules over them need no Win32 binding; the
/// Win32 window procedure reads `WM_SIZE` through these, so the backend keeps
/// the same single source of truth. `SIZE_MAXSHOW` (3) and `SIZE_MAXHIDE` (4)
/// report another window's change and imply nothing here, which the tests
/// pin.
#[cfg(any(windows, test))]
pub(crate) const SIZE_RESTORED: u32 = 0;
/// See [`SIZE_RESTORED`].
#[cfg(any(windows, test))]
pub(crate) const SIZE_MINIMIZED: u32 = 1;
/// See [`SIZE_RESTORED`].
#[cfg(any(windows, test))]
pub(crate) const SIZE_MAXIMIZED: u32 = 2;

/// `WS_VISIBLE` (`winuser.h`: `0x1000_0000`) — defined here, like the
/// `SIZE_*` values above, so the seed rule below needs no Win32 binding.
#[cfg(any(windows, test))]
pub(crate) const WS_VISIBLE_STYLE_BIT: u32 = 0x1000_0000;

/// The initial value for the Win32 backend's visibility edge filter,
/// derived from the window's ACTUAL style at context-install time — never
/// a constant default.
///
/// Why a default is wrong: creation style depends on decoration. A
/// decorated window is created without `WS_VISIBLE` and shown afterwards,
/// so the creation-time `ShowWindow(SW_SHOW)` flips the bit and delivers
/// the `WM_SHOWWINDOW` edge that brings the filter to `true`. An
/// UNdecorated window is created `WS_POPUP | WS_VISIBLE` — already visible
/// before the context installs — and that same `ShowWindow(SW_SHOW)` is a
/// no-op on the bit, so no edge ever fires; a seed of `false` would then
/// swallow the window's FIRST minimize (the edge filter would see
/// false→false and dispatch nothing, leaving the runtime pumping frames
/// for a hidden surface). Seeding from the style is correct for every
/// creation-style combination.
#[cfg(any(windows, test))]
#[must_use]
pub(crate) fn win32_initial_visibility(style: u32) -> bool {
    style & WS_VISIBLE_STYLE_BIT != 0
}

/// The visibility transition (if any) a Win32 `WM_SIZE` message implies.
///
/// Win32 delivers no occlusion events at all; what it does deliver is
/// minimize/restore, and a minimized window's surface is exactly a hidden
/// surface (the DWM stops composing it). So:
///
/// - `SIZE_MINIMIZED` while not already minimized → `Some(false)`;
/// - `SIZE_RESTORED`/`SIZE_MAXIMIZED` while minimized → `Some(true)`;
/// - everything else (plain resizes, repeated minimize messages,
///   `SIZE_MAXSHOW`/`SIZE_MAXHIDE`, unknown values) → `None` — no dispatch,
///   so a live-resize drag can never spam the runtime's lifecycle ladder.
///
/// `was_minimized` is the backend's own mode tracking *before* this message
/// (`WindowContext::mode` on the Win32 backend) — the edge filter that makes
/// the rule idempotent under Win32's repeated-message delivery.
#[cfg(any(windows, test))]
#[must_use]
pub(crate) fn win32_size_visibility(size_type: u32, was_minimized: bool) -> Option<bool> {
    match size_type {
        SIZE_MINIMIZED if !was_minimized => Some(false),
        SIZE_RESTORED | SIZE_MAXIMIZED if was_minimized => Some(true),
        _ => None,
    }
}

/// The visibility transition (if any) a Win32 `WM_SHOWWINDOW` message
/// implies.
///
/// `WM_SHOWWINDOW` fires when the `WS_VISIBLE` style bit actually changes
/// (`ShowWindow(SW_HIDE)`/`SW_SHOW` and friends), so it is already
/// edge-triggered at the OS level; the one wrinkle is a `SW_SHOW` delivered
/// while the window is *minimized* — the style bit is set but the surface is
/// still not composed, so the show must not be reported as visible (the
/// later restore's `WM_SIZE` will report it, via
/// [`win32_size_visibility`]).
///
/// - `shown == false` → `Some(false)` (hide always hides the surface);
/// - `shown == true` while not minimized → `Some(true)`;
/// - `shown == true` while minimized → `None`.
#[cfg(any(windows, test))]
#[must_use]
pub(crate) fn win32_show_window_visibility(shown: bool, is_minimized: bool) -> Option<bool> {
    match (shown, is_minimized) {
        (false, _) => Some(false),
        (true, false) => Some(true),
        (true, true) => None,
    }
}

/// `NSWindowOcclusionStateVisible` (`NSWindow.h`: `1 << 1`) — the only bit
/// AppKit defines in `NSWindowOcclusionState`. Kept here, with the decidable
/// half of the AppKit occlusion derivation (bit test + polarity), apart from
/// the delegate callback that reads it.
#[cfg(any(target_os = "macos", test))]
pub(crate) const NS_WINDOW_OCCLUSION_STATE_VISIBLE: u64 = 1 << 1;

/// Whether an AppKit `NSWindowOcclusionState` raw value means the window's
/// surface is visible. AppKit reports "visible" as a positive bit (set when
/// at least part of the window is on screen and composed), so the callback
/// polarity (`is_visible`) is the bit itself — no negation, unlike winit's
/// `Occluded` event.
#[cfg(any(target_os = "macos", test))]
#[must_use]
pub(crate) fn appkit_occlusion_state_is_visible(occlusion_state: u64) -> bool {
    occlusion_state & NS_WINDOW_OCCLUSION_STATE_VISIBLE != 0
}

/// Edge filter shared by dispatch sites that receive level-triggered
/// signals: `Some(current)` only when it differs from the last *dispatched*
/// value. Keeps a backend from re-announcing an unchanged visibility (AppKit
/// documents that `windowDidChangeOcclusionState` can fire for reasons other
/// than the visible bit flipping).
#[cfg(any(target_os = "macos", test))]
#[must_use]
pub(crate) fn visibility_edge(last_dispatched: bool, current: bool) -> Option<bool> {
    (current != last_dispatched).then_some(current)
}

#[cfg(test)]
mod tests {
    use super::{
        NS_WINDOW_OCCLUSION_STATE_VISIBLE, SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED,
        WS_VISIBLE_STYLE_BIT, appkit_occlusion_state_is_visible, visibility_edge,
        win32_initial_visibility, win32_show_window_visibility, win32_size_visibility,
    };

    /// `WM_SIZE` reports of another window's change (`winuser.h`).
    const SIZE_MAXSHOW: u32 = 3;
    const SIZE_MAXHIDE: u32 = 4;

    /// A minimize hides the surface and a restore or maximize out of it shows
    /// it again, once each; a plain resize, a repeat and another window's
    /// change say nothing.
    #[test]
    fn a_win32_size_message_reports_only_the_minimize_edges() {
        let cases = [
            (SIZE_MINIMIZED, false, Some(false)),
            (SIZE_MINIMIZED, true, None),
            (SIZE_RESTORED, true, Some(true)),
            (SIZE_MAXIMIZED, true, Some(true)),
            (SIZE_RESTORED, false, None),
            (SIZE_MAXIMIZED, false, None),
            (SIZE_MAXSHOW, false, None),
            (SIZE_MAXHIDE, true, None),
            (99, true, None),
        ];
        for (size_type, was_minimized, expected) in cases {
            assert_eq!(
                win32_size_visibility(size_type, was_minimized),
                expected,
                "WM_SIZE {size_type} with the window minimized: {was_minimized}"
            );
        }
    }

    /// A hide always hides; a show shows unless the window is minimized, whose
    /// restore reports it.
    #[test]
    fn a_win32_show_message_reports_unless_the_window_is_minimized() {
        assert_eq!(win32_show_window_visibility(false, false), Some(false));
        assert_eq!(win32_show_window_visibility(false, true), Some(false));
        assert_eq!(win32_show_window_visibility(true, false), Some(true));
        assert_eq!(win32_show_window_visibility(true, true), None);
    }

    /// The edge filter starts from the style the window was created with.
    #[test]
    fn a_win32_window_starts_as_visible_as_its_style() {
        assert!(win32_initial_visibility(WS_VISIBLE_STYLE_BIT));
        assert!(win32_initial_visibility(WS_VISIBLE_STYLE_BIT | 0x00CF_0000));
        assert!(!win32_initial_visibility(0x00CF_0000));
    }

    /// AppKit's visible bit is the visibility, and an unchanged value is not
    /// reported again.
    #[test]
    fn appkit_occlusion_reports_the_visible_bit_once() {
        assert!(appkit_occlusion_state_is_visible(
            NS_WINDOW_OCCLUSION_STATE_VISIBLE
        ));
        assert!(!appkit_occlusion_state_is_visible(0));
        assert!(!appkit_occlusion_state_is_visible(1 << 3));
        assert_eq!(visibility_edge(true, false), Some(false));
        assert_eq!(visibility_edge(false, true), Some(true));
        assert_eq!(visibility_edge(true, true), None);
    }
}
