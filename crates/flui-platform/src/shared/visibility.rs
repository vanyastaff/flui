//! Pure hidden-surface visibility rules for the native backends.
//!
//! The runtime's hidden-surface gating (`PlatformWindow::
//! on_visibility_status_change` → `PlatformToUi::WindowVisibility` → the
//! per-presentation `FrameClock` gate and the realm-wide `AppLifecycleState`
//! derivation) only ever fires if a backend actually *dispatches* the
//! callback. The winit backend rides `WindowEvent::Occluded`; the native
//! Win32 and AppKit backends have no such ready-made event and must derive
//! the signal from their own message streams. The derivations are decidable
//! rules over plain values, so they live here — cfg-free and unit-tested on
//! every host — while the backends keep only the FFI plumbing that cannot
//! run off-target. (Same split, and same `pub` rationale, as
//! [`super::hwnd_affinity`] and [`super::keys`].)
//!
//! Contract note (mirrors `PlatformWindow::on_visibility_status_change`):
//! the callback parameter is `is_visible` — `true` when the window's surface
//! became visible again, `false` when it stopped being visible (minimized,
//! hidden, or fully occluded). Every rule here speaks that polarity.

/// `WM_SIZE` `wParam` values (`winuser.h`). Defined here — not imported from
/// the `windows` crate — so the rules over them stay compilable and testable
/// on every host; `platforms/windows/util.rs` re-exports these to keep the
/// backend on the same single source of truth.
pub const SIZE_RESTORED: u32 = 0;
/// See [`SIZE_RESTORED`].
pub const SIZE_MINIMIZED: u32 = 1;
/// See [`SIZE_RESTORED`].
pub const SIZE_MAXIMIZED: u32 = 2;
/// See [`SIZE_RESTORED`].
pub const SIZE_MAXSHOW: u32 = 3;
/// See [`SIZE_RESTORED`].
pub const SIZE_MAXHIDE: u32 = 4;

/// `WS_VISIBLE` (`winuser.h`: `0x1000_0000`) — defined here, like the
/// `SIZE_*` values above, so the seed rule below is host-testable.
pub const WS_VISIBLE_STYLE_BIT: u32 = 0x1000_0000;

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
#[must_use]
pub fn win32_initial_visibility(style: u32) -> bool {
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
#[must_use]
pub fn win32_size_visibility(size_type: u32, was_minimized: bool) -> Option<bool> {
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
#[must_use]
pub fn win32_show_window_visibility(shown: bool, is_minimized: bool) -> Option<bool> {
    match (shown, is_minimized) {
        (false, _) => Some(false),
        (true, false) => Some(true),
        (true, true) => None,
    }
}

/// `NSWindowOcclusionStateVisible` (`NSWindow.h`: `1 << 1`) — the only bit
/// AppKit defines in `NSWindowOcclusionState`. Kept here so the decidable
/// half of the AppKit occlusion derivation (bit test + polarity) is pinned
/// by a host-runnable test instead of living only in `cfg(target_os =
/// "macos")` code that CI merely lints.
pub const NS_WINDOW_OCCLUSION_STATE_VISIBLE: u64 = 1 << 1;

/// Whether an AppKit `NSWindowOcclusionState` raw value means the window's
/// surface is visible. AppKit reports "visible" as a positive bit (set when
/// at least part of the window is on screen and composed), so the callback
/// polarity (`is_visible`) is the bit itself — no negation, unlike winit's
/// `Occluded` event.
#[must_use]
pub fn appkit_occlusion_state_is_visible(occlusion_state: u64) -> bool {
    occlusion_state & NS_WINDOW_OCCLUSION_STATE_VISIBLE != 0
}

/// Edge filter shared by dispatch sites that receive level-triggered
/// signals: `Some(current)` only when it differs from the last *dispatched*
/// value. Keeps a backend from re-announcing an unchanged visibility (AppKit
/// documents that `windowDidChangeOcclusionState` can fire for reasons other
/// than the visible bit flipping).
#[must_use]
pub fn visibility_edge(last_dispatched: bool, current: bool) -> Option<bool> {
    (current != last_dispatched).then_some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimize_from_normal_hides() {
        assert_eq!(win32_size_visibility(SIZE_MINIMIZED, false), Some(false));
    }

    #[test]
    fn repeated_minimize_is_silent() {
        assert_eq!(win32_size_visibility(SIZE_MINIMIZED, true), None);
    }

    #[test]
    fn restore_from_minimized_shows() {
        assert_eq!(win32_size_visibility(SIZE_RESTORED, true), Some(true));
        assert_eq!(win32_size_visibility(SIZE_MAXIMIZED, true), Some(true));
    }

    #[test]
    fn plain_resize_never_dispatches() {
        // A live-resize drag delivers a stream of SIZE_RESTORED while the
        // window was never minimized — zero visibility dispatches.
        assert_eq!(win32_size_visibility(SIZE_RESTORED, false), None);
        assert_eq!(win32_size_visibility(SIZE_MAXIMIZED, false), None);
        assert_eq!(win32_size_visibility(SIZE_MAXSHOW, false), None);
        assert_eq!(win32_size_visibility(SIZE_MAXHIDE, false), None);
        assert_eq!(win32_size_visibility(SIZE_MAXSHOW, true), None);
        assert_eq!(win32_size_visibility(SIZE_MAXHIDE, true), None);
    }

    #[test]
    fn initial_visibility_follows_the_ws_visible_bit() {
        // The bit itself is pinned (winuser.h WS_VISIBLE).
        assert_eq!(WS_VISIBLE_STYLE_BIT, 0x1000_0000);
        // Undecorated creation style: WS_POPUP (0x8000_0000) | WS_VISIBLE —
        // visible from birth, must seed true.
        assert!(win32_initial_visibility(0x8000_0000 | WS_VISIBLE_STYLE_BIT));
        // Decorated creation style: WS_OVERLAPPEDWINDOW (0x00CF_0000) has
        // no WS_VISIBLE — created hidden, shown later, must seed false so
        // the creation-time WM_SHOWWINDOW edge is the one that flips it.
        assert!(!win32_initial_visibility(0x00CF_0000));
        assert!(!win32_initial_visibility(0));
    }

    #[test]
    fn hide_always_hides_show_respects_minimized() {
        assert_eq!(win32_show_window_visibility(false, false), Some(false));
        assert_eq!(win32_show_window_visibility(false, true), Some(false));
        assert_eq!(win32_show_window_visibility(true, false), Some(true));
        // A SW_SHOW on a minimized window sets WS_VISIBLE but composes no
        // surface — the restore's own WM_SIZE reports visibility instead.
        assert_eq!(win32_show_window_visibility(true, true), None);
    }

    #[test]
    fn appkit_visible_bit_is_bit_one() {
        assert_eq!(NS_WINDOW_OCCLUSION_STATE_VISIBLE, 2);
        assert!(appkit_occlusion_state_is_visible(
            NS_WINDOW_OCCLUSION_STATE_VISIBLE
        ));
        assert!(!appkit_occlusion_state_is_visible(0));
        // Unrelated bits alone never read as visible.
        assert!(!appkit_occlusion_state_is_visible(1));
        assert!(appkit_occlusion_state_is_visible(
            NS_WINDOW_OCCLUSION_STATE_VISIBLE | 1
        ));
    }

    #[test]
    fn edge_filter_passes_changes_only() {
        assert_eq!(visibility_edge(true, false), Some(false));
        assert_eq!(visibility_edge(false, true), Some(true));
        assert_eq!(visibility_edge(true, true), None);
        assert_eq!(visibility_edge(false, false), None);
    }
}
