//! Which [`WindowMode`] an `NSWindow` is in, and the AppKit requests that take
//! it to another.
//!
//! AppKit has no mode property: a window is full screen when its style mask
//! says so, miniaturized when `isMiniaturized`, and zoomed (maximized) when its
//! frame equals the standard frame. Zooming and full screen are toggles, so a
//! request is sent only from the state it undoes.

use flui_platform_api::WindowMode;

/// What AppKit says about a window, as plain flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowFlags {
    /// The style mask contains `NSWindowStyleMaskFullScreen`.
    pub(crate) fullscreen: bool,
    /// `isMiniaturized`.
    pub(crate) miniaturized: bool,
    /// `isZoomed`.
    pub(crate) zoomed: bool,
    /// The style mask contains `NSWindowStyleMaskResizable`.
    pub(crate) resizable: bool,
}

/// The mode a window with these flags is in.
///
/// Full screen outranks the rest and a miniaturized window outranks a zoomed
/// one. `isZoomed` is also true for a window that cannot be resized and for one
/// whose frame merely equals the standard frame, so a zoom counts only when the
/// window is resizable.
pub(crate) const fn mode_of(flags: WindowFlags) -> WindowMode {
    if flags.fullscreen {
        WindowMode::Fullscreen
    } else if flags.miniaturized {
        WindowMode::Minimized
    } else if flags.zoomed && flags.resizable {
        WindowMode::Maximized
    } else {
        WindowMode::Normal
    }
}

/// Whether a window with these flags is shown: on screen, or in the Dock. A
/// miniaturized window is not `isVisible`, but the user sees it there.
pub(crate) const fn is_shown(on_screen: bool, miniaturized: bool) -> bool {
    on_screen || miniaturized
}

/// One AppKit request on the way to a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeStep {
    /// `deminiaturize:`.
    Deminiaturize,
    /// `toggleFullScreen:` out of full screen.
    ExitFullscreen,
    /// `zoom:` out of the zoomed state.
    Unzoom,
    /// `zoom:` into the zoomed state.
    Zoom,
    /// `miniaturize:`.
    Miniaturize,
    /// `toggleFullScreen:` into full screen.
    EnterFullscreen,
}

/// The requests that take a window with these flags to `target`, in order.
///
/// A window already in `target` needs none. Leaving full screen and leaving
/// the Dock come first, since AppKit refuses to zoom or miniaturize a
/// full-screen window and to zoom a miniaturized one. A zoomed window that is
/// miniaturized or made full screen keeps its zoom, and zooming it again would
/// undo it, so a request that would is left out.
///
/// What a window looks like once it has left full screen is not known until it
/// has (it may come back zoomed): the list for a window that is full screen is
/// advisory past its first request, and [`next_batch`] cuts it there.
pub(crate) fn steps_to(flags: WindowFlags, target: WindowMode) -> Vec<ModeStep> {
    use ModeStep::{Deminiaturize, EnterFullscreen, ExitFullscreen, Miniaturize, Unzoom, Zoom};

    let current = mode_of(flags);
    if current == target {
        return Vec::new();
    }
    let zoomed = flags.zoomed && flags.resizable;
    let mut requests = Vec::new();
    if flags.fullscreen {
        requests.push(ExitFullscreen);
    } else if flags.miniaturized {
        requests.push(Deminiaturize);
    }
    match target {
        WindowMode::Minimized => requests.push(Miniaturize),
        WindowMode::Maximized if !zoomed || flags.fullscreen => requests.push(Zoom),
        WindowMode::Fullscreen => requests.push(EnterFullscreen),
        WindowMode::Normal if zoomed && !flags.fullscreen => requests.push(Unzoom),
        // Normal with nothing zoomed, a zoomed window asked to be maximized,
        // and a mode added later: nothing more to send.
        _ => {}
    }
    requests
}

impl ModeStep {
    /// Whether AppKit animates this step: the window is in between until its
    /// `windowDid…` notification arrives, and a toggle sent meanwhile is
    /// ignored. Zooming is not animated in that sense; its end is its frame.
    pub(crate) const fn animates(self) -> bool {
        matches!(
            self,
            Self::Deminiaturize | Self::Miniaturize | Self::ExitFullscreen | Self::EnterFullscreen
        )
    }

    /// Whether the steps after this one wait for it to finish: what the window
    /// looks like once it has left full screen or the Dock is only known then.
    pub(crate) const fn holds_back_the_rest(self) -> bool {
        matches!(self, Self::ExitFullscreen | Self::Deminiaturize)
    }
}

/// The requests to send now, and whether the mode is asked for again once
/// AppKit has finished the last of them (`windowDidExitFullScreen:`,
/// `windowDidDeminiaturize:`), from the window's new state.
pub(crate) fn next_batch(requests: &[ModeStep]) -> (&[ModeStep], bool) {
    match requests
        .iter()
        .position(|request| request.holds_back_the_rest())
    {
        Some(index) => (&requests[..=index], true),
        None => (requests, false),
    }
}

#[cfg(test)]
mod tests {
    use WindowMode::{Fullscreen, Maximized, Minimized, Normal};

    use super::*;

    /// The flags a row names: `F` full screen, `M` miniaturized, `Z` zoomed,
    /// `R` resizable; anything else is off.
    fn flags(letters: &str) -> WindowFlags {
        WindowFlags {
            fullscreen: letters.contains('F'),
            miniaturized: letters.contains('M'),
            zoomed: letters.contains('Z'),
            resizable: letters.contains('R'),
        }
    }

    #[test]
    fn flags_become_a_mode() {
        let rows = [
            ("FR", Fullscreen),
            ("FMZR", Fullscreen),
            ("MR", Minimized),
            ("MZR", Minimized),
            ("ZR", Maximized),
            // A fixed-size window reports itself zoomed; it is not maximized.
            ("Z", Normal),
            ("R", Normal),
            ("", Normal),
        ];
        for (letters, expected) in rows {
            assert_eq!(mode_of(flags(letters)), expected, "flags {letters:?}");
        }
    }

    #[test]
    fn a_miniaturized_window_is_shown() {
        assert!(is_shown(true, false));
        assert!(is_shown(false, true));
        assert!(!is_shown(false, false));
    }

    #[test]
    fn a_mode_is_reached_by_the_requests_that_undo_the_current_one() {
        use ModeStep::{Deminiaturize, EnterFullscreen, ExitFullscreen, Miniaturize, Unzoom, Zoom};

        // (flags of the window, mode asked for, requests)
        let rows: &[(&str, WindowMode, &[ModeStep])] = &[
            ("R", Normal, &[]),
            ("R", Minimized, &[Miniaturize]),
            ("R", Maximized, &[Zoom]),
            ("R", Fullscreen, &[EnterFullscreen]),
            ("MR", Normal, &[Deminiaturize]),
            ("MR", Minimized, &[]),
            ("MR", Maximized, &[Deminiaturize, Zoom]),
            ("MR", Fullscreen, &[Deminiaturize, EnterFullscreen]),
            // A window miniaturized from the zoomed state is still zoomed.
            ("MZR", Normal, &[Deminiaturize, Unzoom]),
            ("MZR", Maximized, &[Deminiaturize]),
            ("ZR", Normal, &[Unzoom]),
            ("ZR", Minimized, &[Miniaturize]),
            ("ZR", Maximized, &[]),
            ("ZR", Fullscreen, &[EnterFullscreen]),
            ("FR", Normal, &[ExitFullscreen]),
            ("FR", Minimized, &[ExitFullscreen, Miniaturize]),
            ("FR", Maximized, &[ExitFullscreen, Zoom]),
            ("FR", Fullscreen, &[]),
            // Leaving full screen from a zoomed window must not zoom it again.
            ("FZR", Normal, &[ExitFullscreen]),
        ];
        for (letters, target, expected) in rows {
            assert_eq!(
                steps_to(flags(letters), *target),
                *expected,
                "{letters:?} to {target:?}"
            );
        }
    }

    #[test]
    fn a_request_that_animates_away_holds_back_the_ones_after_it() {
        use ModeStep::{Deminiaturize, EnterFullscreen, ExitFullscreen, Miniaturize, Zoom};

        // (requests, sent now, whether the mode is asked for again afterwards)
        let rows: [(&[ModeStep], &[ModeStep], bool); 6] = [
            (&[], &[], false),
            (&[Zoom], &[Zoom], false),
            // What the window is like after leaving full screen is only known
            // after, so even a last request of this kind asks again.
            (&[ExitFullscreen], &[ExitFullscreen], true),
            (&[Deminiaturize], &[Deminiaturize], true),
            (&[ExitFullscreen, Miniaturize], &[ExitFullscreen], true),
            (&[Deminiaturize, EnterFullscreen], &[Deminiaturize], true),
        ];
        for (requests, now, asks_again) in rows {
            assert_eq!(next_batch(requests), (now, asks_again), "{requests:?}");
        }
    }

    #[test]
    fn full_screen_and_the_dock_animate_and_zooming_does_not() {
        use ModeStep::{Deminiaturize, EnterFullscreen, ExitFullscreen, Miniaturize, Unzoom, Zoom};

        // (step, animates, the steps after it wait)
        let rows = [
            (Deminiaturize, true, true),
            (Miniaturize, true, false),
            (ExitFullscreen, true, true),
            (EnterFullscreen, true, false),
            (Zoom, false, false),
            (Unzoom, false, false),
        ];
        for (step, animates, holds_back) in rows {
            assert_eq!(step.animates(), animates, "{step:?} animates");
            assert_eq!(
                step.holds_back_the_rest(),
                holds_back,
                "{step:?} holds back"
            );
        }
    }
}
