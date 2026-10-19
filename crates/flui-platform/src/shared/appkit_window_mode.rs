//! Which [`WindowMode`] an `NSWindow` is in, and the AppKit requests that take
//! it to another.
//!
//! AppKit has no mode property: a window is full screen when its style mask
//! says so, miniaturized when `isMiniaturized`, and zoomed (maximized) when its
//! frame equals the standard frame. Zooming and full screen are toggles, so a
//! request is sent only from the state it undoes.

use std::time::{Duration, Instant};

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

impl WindowFlags {
    /// Whether the window is zoomed in the sense that matters: `isZoomed` is
    /// also true for a window that cannot be resized, which is not maximized.
    pub(crate) const fn is_maximized(self) -> bool {
        self.zoomed && self.resizable
    }
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
    } else if flags.is_maximized() {
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
    let zoomed = flags.is_maximized();
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

/// How long a window counts as still animating a full-screen or Dock step when
/// the notification that ends it has not come. Past it a mode command is sent
/// instead of held, so a step AppKit silently refused cannot hold the window
/// for good.
pub(crate) const ANIMATION_LIMIT: Duration = Duration::from_secs(3);

/// The order in which a window's mode commands reach AppKit.
///
/// AppKit ignores a toggle sent while a full-screen or Dock step animates, and
/// what a window looks like after one is only known after. So a command that
/// arrives during one, and the rest of a mode whose first step is one, are held
/// here and planned again from the window's flags when the step's notification
/// arrives. The newest command wins, and it replaces the mode a hidden window
/// was to enter when first shown.
#[derive(Debug, Default)]
pub(crate) struct ModeSequence {
    /// The mode held back until the animating step ends.
    held: Option<WindowMode>,
    /// When the step that is animating started.
    animating_since: Option<Instant>,
    /// The mode a window that opened hidden enters when it is first shown.
    when_shown: Option<WindowMode>,
}

impl ModeSequence {
    /// The steps to send now to take a window with `flags` to `target`, at
    /// `now`.
    pub(crate) fn command(
        &mut self,
        now: Instant,
        flags: WindowFlags,
        target: WindowMode,
    ) -> Vec<ModeStep> {
        self.when_shown = None;
        if self
            .animating_since
            .is_some_and(|since| now.saturating_duration_since(since) < ANIMATION_LIMIT)
        {
            self.held = Some(target);
            return Vec::new();
        }
        let steps = steps_to(flags, target);
        let (batch, asks_again) = next_batch(&steps);
        self.held = asks_again.then_some(target);
        self.animating_since = batch.iter().any(|step| step.animates()).then_some(now);
        batch.to_vec()
    }

    /// A step that animates has started, from this backend or from the user.
    pub(crate) fn step_started(&mut self, now: Instant) {
        self.animating_since = Some(now);
    }

    /// The step that was animating is over, or failed.
    pub(crate) fn step_ended(&mut self) {
        self.animating_since = None;
    }

    /// The mode held back for the step that ended, if there is one, to be
    /// commanded again from the window's new state.
    pub(crate) fn take_held(&mut self) -> Option<WindowMode> {
        self.held.take()
    }

    /// A window that opened hidden enters `mode` when it is first shown.
    pub(crate) fn enter_when_shown(&mut self, mode: WindowMode) {
        self.when_shown = Some(mode);
    }

    /// The mode a hidden window enters now that it is shown, if no mode command
    /// came first.
    pub(crate) fn take_when_shown(&mut self) -> Option<WindowMode> {
        self.when_shown.take()
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

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn a_command_during_a_step_that_animates_is_held_and_the_newest_wins() {
        let start = Instant::now();
        let mut sequence = ModeSequence::default();
        // Full screen starts: the step is sent and the window animates.
        assert_eq!(
            sequence.command(start, flags("R"), Fullscreen),
            [ModeStep::EnterFullscreen]
        );
        // Two commands arrive meanwhile: nothing is sent, the newest is held.
        assert_eq!(sequence.command(at(start, 1), flags("FR"), Minimized), []);
        assert_eq!(sequence.command(at(start, 1), flags("FR"), Normal), []);
        // The step ends; the held mode is commanded again from the real flags.
        sequence.step_ended();
        assert_eq!(sequence.take_held(), Some(Normal));
        assert_eq!(sequence.take_held(), None, "a held mode is taken once");
        assert_eq!(
            sequence.command(at(start, 2), flags("FR"), Normal),
            [ModeStep::ExitFullscreen]
        );
    }

    #[test]
    fn the_rest_of_a_mode_waits_for_the_step_it_depends_on() {
        let start = Instant::now();
        let mut sequence = ModeSequence::default();
        assert_eq!(
            sequence.command(start, flags("FR"), Minimized),
            [ModeStep::ExitFullscreen]
        );
        sequence.step_ended();
        assert_eq!(sequence.take_held(), Some(Minimized));
        assert_eq!(
            sequence.command(at(start, 1), flags("R"), Minimized),
            [ModeStep::Miniaturize]
        );
        assert_eq!(sequence.take_held(), None, "nothing is left to ask again");
    }

    #[test]
    fn a_step_that_never_ends_stops_holding_commands() {
        let start = Instant::now();
        let mut sequence = ModeSequence::default();
        sequence.step_started(start);
        assert_eq!(sequence.command(at(start, 1), flags("R"), Maximized), []);
        // AppKit refused the step and sent no notification.
        assert_eq!(
            sequence.command(start + ANIMATION_LIMIT, flags("R"), Maximized),
            [ModeStep::Zoom]
        );
    }

    #[test]
    fn a_mode_command_replaces_the_mode_a_hidden_window_was_to_enter() {
        let start = Instant::now();
        let mut sequence = ModeSequence::default();
        sequence.enter_when_shown(Fullscreen);
        assert_eq!(sequence.command(start, flags("R"), Normal), []);
        assert_eq!(sequence.take_when_shown(), None);

        sequence.enter_when_shown(Fullscreen);
        assert_eq!(sequence.take_when_shown(), Some(Fullscreen));
        assert_eq!(sequence.take_when_shown(), None, "it is entered once");
    }
}
