//! The Win32 window, in its halves (ADR-0151 §10):
//!
//! - [`limits`] and [`mode`]: the pure half, compiled and tested on every
//!   host: the size limits the system asks for and the mode the window
//!   tracks with what its way back needs, as rules over plain values;
//! - [`material`]: the pure half too: from which build the window draws a
//!   material behind it;
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
pub(crate) mod frame_timing;
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

pub(crate) mod material {
    //! Which Windows builds draw a material behind a window: the one gate the
    //! window's [`WindowCapabilities::material`](flui_platform::WindowCapabilities::material),
    //! the `SetMaterial` command and `Win32Window::set_backdrop` all take, so
    //! none of them can disagree with another.

    #![forbid(unsafe_code)]

    use flui_platform::{
        Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand, WindowMaterial,
    };

    use crate::version::gate::windows::WindowsBuild;
    use crate::version::gate::{Gate, Since};

    /// `DWMWA_SYSTEMBACKDROP_TYPE`, the desktop window manager attribute that
    /// draws a system material behind a window, from Windows 11 22H2 (build
    /// 22621). Without it a window draws no material: its capability is
    /// `Unavailable` on this device, and admission refuses `SetMaterial` with
    /// `NotOnThisDevice`.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct SystemBackdrop;

    impl Gate for SystemBackdrop {
        type Version = WindowsBuild;
        const SINCE: u64 = 22621;
    }

    /// What windows draw materials with on `build`: the proof it has the
    /// attribute, or why not.
    pub(crate) fn gate(build: WindowsBuild) -> Result<Since<SystemBackdrop>, Unsupported> {
        build
            .since::<SystemBackdrop>()
            .ok_or(Unsupported::new(UnsupportedReason::NotOnThisDevice))
    }

    /// `capabilities` as a window on a build that passed or failed the
    /// [`gate`] has them: below it the material is `Unavailable` on this
    /// device.
    pub(crate) const fn below_the_gate(
        capabilities: WindowCapabilities,
        gate: &Result<Since<SystemBackdrop>, Unsupported>,
    ) -> WindowCapabilities {
        match gate {
            Ok(_) => capabilities,
            Err(_) => capabilities
                .unavailable_on_this_device(&WindowCommand::SetMaterial(WindowMaterial::Opaque)),
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{
            AdmissionError, Availability, UnsupportedReason, WindowCapabilities, WindowCommand,
            WindowMaterial,
        };

        use super::{SystemBackdrop, below_the_gate, gate};
        use crate::version::gate::testing::row;
        use crate::version::gate::windows::WindowsBuild;

        /// One row per build around the gate: below it a window has no
        /// material and admission refuses one with `NotOnThisDevice`; from it
        /// on, the material is admitted.
        #[test]
        fn system_backdrop_gate_table() {
            let mut failures = row::<SystemBackdrop>(
                "SystemBackdrop",
                WindowsBuild::from_number(22620),
                WindowsBuild::from_number(22621),
                WindowsBuild::UNKNOWN,
            );
            let cases = [
                (WindowsBuild::from_number(19045), Availability::Unavailable),
                (WindowsBuild::from_number(22000), Availability::Unavailable),
                (WindowsBuild::from_number(22620), Availability::Unavailable),
                (WindowsBuild::UNKNOWN, Availability::Unavailable),
                (WindowsBuild::from_number(22621), Availability::Native),
                (WindowsBuild::from_number(26100), Availability::Native),
            ];
            for (build, wanted) in cases {
                let capabilities = below_the_gate(WindowCapabilities::ALL, &gate(build));
                let admission =
                    WindowCommand::SetMaterial(WindowMaterial::System).admit(&capabilities);
                let answer_holds = match (&admission, wanted) {
                    (Ok(_), Availability::Native) => true,
                    (Err(AdmissionError::Unsupported(refused)), Availability::Unavailable) => {
                        refused.reason == UnsupportedReason::NotOnThisDevice
                    }
                    _ => false,
                };
                if capabilities.material != wanted || !answer_holds {
                    failures.push(format!(
                        "{build}: material {:?}, admission {admission:?}, wanted {wanted:?}",
                        capabilities.material
                    ));
                }
            }
            assert!(failures.is_empty(), "{failures:#?}");
        }
    }
}
