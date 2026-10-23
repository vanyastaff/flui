//! The Win32 window, in its halves (ADR-0151 §10):
//!
//! - [`limits`], [`mode`] and [`surface`]: the pure half, compiled and tested
//!   on every host: the size limits the system asks for, the mode the window
//!   tracks with what its way back needs, and when its surface is seen, as
//!   rules over plain values;
//! - `window`: [`WindowsWindow`], the handle a host hands out, how it opens
//!   and what its commands ask of the OS;
//! - `set`: [`WindowSet`], the windows of a platform, which the loop holds as
//!   `owner::windows::ClassWindows`, the window position and where a window
//!   opens;
//! - `proc`: the window's context in its `GWLP_USERDATA` slot and the
//!   window's own messages, the hook `WindowProc` the host registers on FLUI's
//!   window class;
//! - `backdrop`: what the desktop window manager draws around and behind the
//!   window, and the extension trait that sets it.

// The halves under `cfg(windows)` are one of the workspace's sanctioned
// `unsafe` FFI islands — direct Win32 calls have no safe wrapper. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`); every `unsafe`
// block still carries its own `// SAFETY:` comment stating the invariant
// that makes it sound. The pure half forbids it.
#![cfg_attr(windows, expect(unsafe_code))]

#[cfg(windows)]
pub(crate) mod backdrop;
pub(crate) mod limits;
#[cfg(windows)]
pub(crate) mod proc;
#[cfg(windows)]
mod set;
#[cfg(windows)]
mod window;

#[cfg(windows)]
pub(crate) use set::WindowSet;
#[cfg(windows)]
pub(crate) use window::WindowsWindow;

pub(crate) mod mode {
    //! How a Win32 window occupies its monitor. Win32 reports a window's
    //! minimized and maximized states (`WM_SIZE`), but fullscreen is FLUI's own
    //! toggle: a borderless window over its monitor, which keeps the rectangle
    //! it returns to.

    #![forbid(unsafe_code)]
    // Elsewhere than on Windows this module is compiled for its tests alone,
    // which reach the mode's flags and not every transition the window
    // procedure makes.
    #![cfg_attr(
        not(windows),
        allow(
            dead_code,
            reason = "off Windows the mode is compiled for its tests, which do not reach every transition"
        )
    )]

    use flui_foundation::geometry::Bounds;

    use crate::window::state::ModeFlags;

    /// How the window occupies its monitor, with what the way back needs: the
    /// native tracking behind the public `WindowMode`, which
    /// `window::state::mode_of` decides from its [`flags`](Self::flags).
    #[derive(Debug, Clone, Copy, Default)]
    pub(crate) enum NativeMode {
        #[default]
        Normal,
        /// Iconified. A fullscreen window that was minimized keeps the
        /// rectangle it returns to, so restoring it brings fullscreen back.
        Minimized {
            from_fullscreen: Option<Bounds<i32>>,
        },
        /// Maximized.
        Maximized,
        /// Borderless over the whole monitor; `restore_bounds` is the outer
        /// rectangle in device pixels the window returns to.
        Fullscreen { restore_bounds: Bounds<i32> },
    }

    impl NativeMode {
        pub(crate) fn is_fullscreen(&self) -> bool {
            matches!(self, Self::Fullscreen { .. })
        }

        /// The rectangle a fullscreen window returns to, also while it is
        /// minimized (a minimized fullscreen window is still the fullscreen
        /// window it comes back as); `None` for every other mode.
        pub(crate) fn fullscreen_bounds(&self) -> Option<Bounds<i32>> {
            match *self {
                Self::Fullscreen { restore_bounds }
                | Self::Minimized {
                    from_fullscreen: Some(restore_bounds),
                } => Some(restore_bounds),
                _ => None,
            }
        }

        /// Whether this is fullscreen, or minimized out of fullscreen.
        pub(crate) fn holds_fullscreen(&self) -> bool {
            self.fullscreen_bounds().is_some()
        }

        /// The same mode with the rectangle a fullscreen window returns to
        /// replaced; any other mode is returned as it is.
        pub(crate) fn with_fullscreen_bounds(self, bounds: Bounds<i32>) -> Self {
            match self {
                Self::Fullscreen { .. } => Self::Fullscreen {
                    restore_bounds: bounds,
                },
                Self::Minimized {
                    from_fullscreen: Some(_),
                } => Self::Minimized {
                    from_fullscreen: Some(bounds),
                },
                other => other,
            }
        }

        pub(crate) fn is_minimized(&self) -> bool {
            matches!(self, Self::Minimized { .. })
        }

        pub(crate) fn is_maximized(&self) -> bool {
            matches!(self, Self::Maximized)
        }

        pub(crate) fn is_normal(&self) -> bool {
            matches!(self, Self::Normal)
        }

        /// Every transition is allowed except to the same mode.
        pub(crate) fn can_transition_to(&self, next: &Self) -> bool {
            std::mem::discriminant(self) != std::mem::discriminant(next)
        }

        /// What this mode says of the window. The mode is one enum, but a
        /// fullscreen window that was minimized holds two facts, minimized
        /// and fullscreen, and which one wins is the one precedence
        /// `window::state::mode_of` keeps for every window system; mapping the
        /// enum straight to a `WindowMode` would state that precedence a
        /// second time.
        pub(crate) fn flags(&self) -> ModeFlags {
            ModeFlags {
                minimized: self.is_minimized(),
                fullscreen: self.holds_fullscreen(),
                maximized: self.is_maximized(),
            }
        }
    }
}

pub(crate) mod surface {
    //! When a Win32 window's surface is seen, from its own messages.
    //!
    //! Win32 delivers no occlusion event; what it delivers is minimize and
    //! restore (`WM_SIZE`) and the `WS_VISIBLE` bit changing
    //! (`WM_SHOWWINDOW`), and a minimized or hidden window's surface is
    //! exactly a hidden surface (the DWM stops composing it). These rules
    //! say what each message implies, in the polarity of
    //! `PlatformWindow::on_visibility_status_change` (`true`: the surface
    //! became visible again); the window procedure keeps the plumbing and
    //! reports through `window::visibility::ReportedVisibility`.

    #![forbid(unsafe_code)]

    /// `WM_SIZE` `wParam` values (`winuser.h`). Defined here — not imported
    /// from the `windows` crate — so the rules over them need no Win32
    /// binding; the window procedure reads `WM_SIZE` through these, so it
    /// keeps the same single source of truth. `SIZE_MAXSHOW` (3) and
    /// `SIZE_MAXHIDE` (4) report another window's change and imply nothing
    /// here, which the tests pin.
    pub(crate) const SIZE_RESTORED: u32 = 0;
    /// See [`SIZE_RESTORED`].
    pub(crate) const SIZE_MINIMIZED: u32 = 1;
    /// See [`SIZE_RESTORED`].
    pub(crate) const SIZE_MAXIMIZED: u32 = 2;

    /// `WS_VISIBLE` (`winuser.h`: `0x1000_0000`) — defined here, like the
    /// `SIZE_*` values above, so the seed rule below needs no Win32 binding.
    pub(crate) const WS_VISIBLE_STYLE_BIT: u32 = 0x1000_0000;

    /// The initial value for the window's visibility edge filter, derived
    /// from the window's ACTUAL style at context-install time — never a
    /// constant default.
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
    pub(crate) fn initial_visibility(style: u32) -> bool {
        style & WS_VISIBLE_STYLE_BIT != 0
    }

    /// The visibility transition (if any) a `WM_SIZE` message implies.
    ///
    /// - `SIZE_MINIMIZED` while not already minimized → `Some(false)`;
    /// - `SIZE_RESTORED`/`SIZE_MAXIMIZED` while minimized → `Some(true)`;
    /// - everything else (plain resizes, repeated minimize messages,
    ///   `SIZE_MAXSHOW`/`SIZE_MAXHIDE`, unknown values) → `None` — no
    ///   dispatch, so a live-resize drag can never spam the runtime's
    ///   lifecycle ladder.
    ///
    /// `was_minimized` is the window's own mode tracking *before* this
    /// message (`WindowContext::mode`) — the edge filter that makes the rule
    /// idempotent under Win32's repeated-message delivery.
    #[must_use]
    pub(crate) fn size_visibility(size_type: u32, was_minimized: bool) -> Option<bool> {
        match size_type {
            SIZE_MINIMIZED if !was_minimized => Some(false),
            SIZE_RESTORED | SIZE_MAXIMIZED if was_minimized => Some(true),
            _ => None,
        }
    }

    /// The visibility transition (if any) a `WM_SHOWWINDOW` message implies.
    ///
    /// `WM_SHOWWINDOW` fires when the `WS_VISIBLE` style bit actually changes
    /// (`ShowWindow(SW_HIDE)`/`SW_SHOW` and friends), so it is already
    /// edge-triggered at the OS level; the one wrinkle is a `SW_SHOW`
    /// delivered while the window is *minimized* — the style bit is set but
    /// the surface is still not composed, so the show must not be reported
    /// as visible (the later restore's `WM_SIZE` will report it, via
    /// [`size_visibility`]).
    ///
    /// - `shown == false` → `Some(false)` (hide always hides the surface);
    /// - `shown == true` while not minimized → `Some(true)`;
    /// - `shown == true` while minimized → `None`.
    #[must_use]
    pub(crate) fn show_window_visibility(shown: bool, is_minimized: bool) -> Option<bool> {
        match (shown, is_minimized) {
            (false, _) => Some(false),
            (true, false) => Some(true),
            (true, true) => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED, WS_VISIBLE_STYLE_BIT,
            initial_visibility, show_window_visibility, size_visibility,
        };

        /// `WM_SIZE` reports of another window's change (`winuser.h`).
        const SIZE_MAXSHOW: u32 = 3;
        const SIZE_MAXHIDE: u32 = 4;

        /// A minimize hides the surface and a restore or maximize out of it
        /// shows it again, once each; a plain resize, a repeat and another
        /// window's change say nothing.
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
                    size_visibility(size_type, was_minimized),
                    expected,
                    "WM_SIZE {size_type} with the window minimized: {was_minimized}"
                );
            }
        }

        /// A hide always hides; a show shows unless the window is minimized,
        /// whose restore reports it.
        #[test]
        fn a_win32_show_message_reports_unless_the_window_is_minimized() {
            assert_eq!(show_window_visibility(false, false), Some(false));
            assert_eq!(show_window_visibility(false, true), Some(false));
            assert_eq!(show_window_visibility(true, false), Some(true));
            assert_eq!(show_window_visibility(true, true), None);
        }

        /// The edge filter starts from the style the window was created with.
        #[test]
        fn a_win32_window_starts_as_visible_as_its_style() {
            assert!(initial_visibility(WS_VISIBLE_STYLE_BIT));
            assert!(initial_visibility(WS_VISIBLE_STYLE_BIT | 0x00CF_0000));
            assert!(!initial_visibility(0x00CF_0000));
        }
    }
}
