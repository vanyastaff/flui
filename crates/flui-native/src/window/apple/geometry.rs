//! What the AppKit and UIKit windows report and are asked, as pure rules:
//! where an `NSWindow` is (`frame`), which mode it is in and how to reach
//! another (`mode`), whether its surface is seen (`surface`), and which
//! screen edges a UIKit window defers its system gestures on
//! (`screen_edges`).
//!
//! Nothing here calls the OS, so `test` keeps the rules checked on every host.

#![forbid(unsafe_code)]

#[cfg(any(target_os = "macos", test))]
pub(crate) mod frame {
    //! Where an `NSWindow` is, in the terms of the window contract.
    //!
    //! AppKit measures from the bottom-left corner of the primary screen, in
    //! points; the contract measures an outer top-left position in global logical
    //! pixels, as every other backend does. A display's usable area is published in
    //! device pixels of that display (`origin / scale` is its global logical
    //! origin), so a position relative to a display converts through the display's
    //! own scale.

    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{DisplayInfo, SizeConstraints};

    /// Any content limit at or beyond this counts as AppKit's "no limit"
    /// (`contentMaxSize` defaults to `CGFLOAT_MAX`, or `FLT_MAX` on older systems).
    const UNLIMITED_FROM: f64 = 1.0e30;

    /// The outer top-left of a window whose frame AppKit reports as `frame`, in
    /// global logical pixels. `primary_height` is the height of the primary screen
    /// (`NSScreen.screens[0]`), whose bottom-left corner is AppKit's origin.
    ///
    /// `None` when a number is not finite: a position is never published from a
    /// value that could not be applied again.
    pub(crate) fn top_left_of_frame(frame: Bounds<f64>, primary_height: f64) -> Option<Point<f64>> {
        let top = primary_height - (frame.origin.y + frame.size.height);
        [frame.origin.x, top, frame.size.width, frame.size.height]
            .iter()
            .all(|number| number.is_finite())
            .then(|| Point::new(frame.origin.x, top))
    }

    /// The point `setFrameTopLeftPoint:` takes to put a window's outer top-left
    /// corner at `top_left`.
    pub(crate) fn cocoa_top_left_point(top_left: Point<f64>, primary_height: f64) -> Point<f64> {
        Point::new(top_left.x, primary_height - top_left.y)
    }

    /// The frame `setFrame:display:` takes to give a window the outer rectangle
    /// that starts at `top_left` and measures `size`.
    pub(crate) fn cocoa_frame(
        top_left: Point<f64>,
        size: Size<f64>,
        primary_height: f64,
    ) -> Bounds<f64> {
        Bounds {
            origin: Point::new(top_left.x, primary_height - top_left.y - size.height),
            size,
        }
    }

    /// The frame that gives a window the outer size `size` while its top edge stays
    /// where it is: AppKit keeps a frame's bottom-left corner, so a taller or
    /// shorter window has to move to keep its top.
    pub(crate) fn cocoa_frame_keeping_top(current: Bounds<f64>, size: Size<f64>) -> Bounds<f64> {
        Bounds {
            origin: Point::new(
                current.origin.x,
                current.origin.y + current.size.height - size.height,
            ),
            size,
        }
    }

    /// A position or size measured on `display` (from the origin of its usable
    /// area, in its logical pixels), as global logical pixels. With no display the
    /// point already is global.
    pub(crate) fn globalize(at: Point<f64>, display: Option<&DisplayInfo>) -> Point<f64> {
        let Some(display) = display else {
            return at;
        };
        let scale = display.scale_factor;
        if !scale.is_finite() || scale <= 0.0 {
            return at;
        }
        let origin = display.usable_bounds.origin;
        Point::new(
            f64::from(origin.x) / scale + at.x,
            f64::from(origin.y) / scale + at.y,
        )
    }

    /// The inner-size limits a window reports through `contentMinSize` and
    /// `contentMaxSize`.
    ///
    /// AppKit's "no limit" is a zero minimum and a huge maximum. A limit on one
    /// axis only keeps the other axis at `f64::MAX`, which is what writing it back
    /// asks AppKit for, so the pair round-trips.
    pub(crate) fn constraints_of_content_limits(min: Size<f64>, max: Size<f64>) -> SizeConstraints {
        let floor = |value: f64| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        let ceiling = |value: f64| {
            if value.is_finite() && value < UNLIMITED_FROM {
                value
            } else {
                f64::MAX
            }
        };
        let min = Size::new(floor(min.width), floor(min.height));
        let max = Size::new(ceiling(max.width), ceiling(max.height));
        SizeConstraints::new(
            (min.width > 0.0 || min.height > 0.0).then_some(min),
            (max.width < f64::MAX || max.height < f64::MAX).then_some(max),
        )
    }

    /// The `contentMinSize` and `contentMaxSize` that give a window `constraints`.
    pub(crate) fn content_limits_of_constraints(
        constraints: SizeConstraints,
    ) -> (Size<f64>, Size<f64>) {
        (
            constraints.min.unwrap_or(Size::new(0.0, 0.0)),
            constraints.max.unwrap_or(Size::new(f64::MAX, f64::MAX)),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const PRIMARY: f64 = 900.0;

        fn frame(x: f64, y: f64, width: f64, height: f64) -> Bounds<f64> {
            Bounds {
                origin: Point::new(x, y),
                size: Size::new(width, height),
            }
        }

        #[test]
        fn a_frame_becomes_an_outer_top_left() {
            // (what the frame is, its cocoa rectangle, the top-left it publishes)
            let rows = [
                (
                    "at rest on the primary screen",
                    frame(100.0, 300.0, 800.0, 500.0),
                    Some((100.0, 100.0)),
                ),
                (
                    "flush with the top of the primary screen",
                    frame(0.0, 400.0, 800.0, 500.0),
                    Some((0.0, 0.0)),
                ),
                (
                    "above the primary screen",
                    frame(0.0, 1000.0, 800.0, 500.0),
                    Some((0.0, -600.0)),
                ),
                (
                    "on a screen left of the primary",
                    frame(-1920.0, 200.0, 800.0, 500.0),
                    Some((-1920.0, 200.0)),
                ),
                (
                    "taller than the screen",
                    frame(0.0, -100.0, 800.0, 1200.0),
                    Some((0.0, -200.0)),
                ),
                (
                    "a position that is not a number",
                    frame(f64::NAN, 0.0, 800.0, 500.0),
                    None,
                ),
                (
                    "an infinite extent",
                    frame(0.0, 0.0, f64::INFINITY, 500.0),
                    None,
                ),
            ];
            for (name, input, expected) in rows {
                let published = top_left_of_frame(input, PRIMARY).map(|at| (at.x, at.y));
                assert_eq!(published, expected, "{name}");
            }
        }

        #[test]
        fn a_top_left_and_a_cocoa_frame_are_each_others_inverse() {
            let size = Size::new(800.0, 500.0);
            for (x, y) in [(0.0, 0.0), (100.0, 100.0), (-1920.0, 200.0), (50.0, -600.0)] {
                let top_left = Point::new(x, y);
                let frame = cocoa_frame(top_left, size, PRIMARY);
                assert_eq!(frame.size, size);
                assert_eq!(
                    top_left_of_frame(frame, PRIMARY),
                    Some(top_left),
                    "({x}, {y})"
                );
                // `setFrameTopLeftPoint:` names the corner of that same frame.
                let point = cocoa_top_left_point(top_left, PRIMARY);
                assert_eq!(point.x, frame.origin.x);
                assert_eq!(point.y, frame.origin.y + frame.size.height);
            }
        }

        #[test]
        fn a_resized_frame_keeps_its_top_edge() {
            let current = frame(100.0, 300.0, 800.0, 500.0);
            // (new height, expected bottom-left y)
            let rows = [(300.0, 500.0), (500.0, 300.0), (700.0, 100.0)];
            for (height, y) in rows {
                let resized = cocoa_frame_keeping_top(current, Size::new(640.0, height));
                assert_eq!(resized.origin, Point::new(100.0, y), "height {height}");
                assert_eq!(
                    resized.origin.y + resized.size.height,
                    800.0,
                    "the top edge"
                );
            }
        }

        fn display(scale: f64, usable_origin: (i32, i32)) -> DisplayInfo {
            use flui_platform::{DisplayId, DisplayKey, DisplayRegistry};

            let id: DisplayId = DisplayRegistry::new()
                .mint(&DisplayKey::MacosUuid([1; 16]))
                .expect("a fresh registry has ids");
            let mut info = DisplayInfo::new(
                id,
                "test".to_string(),
                Bounds {
                    origin: Point::new(usable_origin.0, usable_origin.1),
                    size: Size::new(2880, 1800),
                },
                scale,
            );
            info.usable_bounds = Bounds {
                origin: Point::new(usable_origin.0, usable_origin.1),
                size: Size::new(2880, 1750),
            };
            info
        }

        #[test]
        fn a_position_on_a_display_becomes_a_global_one() {
            let at = Point::new(40.0, 30.0);
            let secondary = display(2.0, (2880, 50));
            let unscaled = display(0.0, (2880, 50));
            // (name, display, expected global position)
            let rows = [
                ("no display: already global", None, (40.0, 30.0)),
                (
                    "a 2x display starting at device x 2880",
                    Some(&secondary),
                    (1480.0, 55.0),
                ),
                (
                    "a display whose scale is unusable is not trusted",
                    Some(&unscaled),
                    (40.0, 30.0),
                ),
            ];
            for (name, display, (x, y)) in rows {
                let global = globalize(at, display);
                assert_eq!((global.x, global.y), (x, y), "{name}");
            }
        }

        #[test]
        fn content_limits_become_constraints_and_back() {
            let none = (Size::new(0.0, 0.0), Size::new(f64::MAX, f64::MAX));
            let default_max = Size::new(f64::from(f32::MAX), f64::from(f32::MAX));
            // (name, appkit min, appkit max, constraints)
            let rows = [
                ("appkit's defaults", none.0, none.1, SizeConstraints::NONE),
                (
                    "the older FLT_MAX default",
                    none.0,
                    default_max,
                    SizeConstraints::NONE,
                ),
                (
                    "a minimum only",
                    Size::new(320.0, 240.0),
                    none.1,
                    SizeConstraints::new(Some(Size::new(320.0, 240.0)), None),
                ),
                (
                    "a maximum only",
                    none.0,
                    Size::new(1600.0, 1200.0),
                    SizeConstraints::new(None, Some(Size::new(1600.0, 1200.0))),
                ),
                (
                    "both",
                    Size::new(320.0, 240.0),
                    Size::new(1600.0, 1200.0),
                    SizeConstraints::new(
                        Some(Size::new(320.0, 240.0)),
                        Some(Size::new(1600.0, 1200.0)),
                    ),
                ),
                (
                    "a limit on the width alone",
                    Size::new(0.0, 0.0),
                    Size::new(1600.0, f64::MAX),
                    SizeConstraints::new(None, Some(Size::new(1600.0, f64::MAX))),
                ),
                (
                    "numbers that are not numbers mean no limit",
                    Size::new(f64::NAN, -5.0),
                    Size::new(f64::NAN, f64::INFINITY),
                    SizeConstraints::NONE,
                ),
            ];
            for (name, min, max, expected) in rows {
                let read = constraints_of_content_limits(min, max);
                assert_eq!(read, expected, "{name}");
                let (min, max) = content_limits_of_constraints(read);
                assert_eq!(
                    constraints_of_content_limits(min, max),
                    read,
                    "{name}: round trip"
                );
            }
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) mod mode {
    //! Which [`WindowMode`] an `NSWindow` is in, the AppKit requests that take
    //! it to another.
    //!
    //! AppKit has no mode property: a window is full screen when its style mask
    //! says so, miniaturized when `isMiniaturized`, and zoomed (maximized) when its
    //! frame equals the standard frame. Zooming and full screen are toggles, so a
    //! request is sent only from the state it undoes.

    use std::time::{Duration, Instant};

    use flui_platform::WindowMode;

    use crate::window::state::{ModeFlags, mode_of};

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

        /// What these flags say of the window, which `mode_of` decides the mode
        /// from.
        ///
        /// `isZoomed` is also true for a window that cannot be resized and for
        /// one whose frame merely equals the standard frame, so a zoom counts
        /// only when the window is resizable. AppKit does not miniaturize a
        /// full-screen window (it leaves full screen first, see [`steps_to`]),
        /// so `isMiniaturized` counts only outside full screen: a window whose
        /// style mask says full screen is in full screen.
        pub(crate) const fn mode_flags(self) -> ModeFlags {
            ModeFlags {
                minimized: self.miniaturized && !self.fullscreen,
                fullscreen: self.fullscreen,
                maximized: self.is_maximized(),
            }
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

    /// What a window with these flags honours of `every`, the commands an
    /// AppKit window takes in normal mode, while it is `heading` for a mode,
    /// if a step toward one animates.
    ///
    /// AppKit zooms and full-screens only a resizable window, restyles none and
    /// orders none out while it is full screen, and a window is made fixed-size
    /// only in normal mode (a fixed-size window reports a zoomed frame as
    /// normal, and the user could not un-zoom it). The snapshot says so, so
    /// admission refuses such a command before AppKit does. The mode that
    /// decides the last is the one the window is heading for: a window coming
    /// back from the Dock still reads as miniaturized while it animates, and a
    /// fixed size asked for in the same turn as the return is one for a normal
    /// window.
    pub(crate) fn capabilities_of(
        flags: WindowFlags,
        heading: Option<WindowMode>,
        every: flui_platform::WindowCapabilities,
    ) -> flui_platform::WindowCapabilities {
        use flui_platform::Availability::Unavailable;

        let mut honoured = every;
        if !flags.resizable {
            honoured.maximizes = Unavailable;
            honoured.enters_fullscreen = Unavailable;
        }
        if flags.fullscreen {
            honoured.sets_decorated = Unavailable;
            honoured.hides = Unavailable;
        }
        let mode = heading.unwrap_or_else(|| mode_of(flags.mode_flags()));
        if flags.fullscreen || mode != WindowMode::Normal {
            honoured.sets_resizable = Unavailable;
        }
        honoured
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

        let current = mode_of(flags.mode_flags());
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
                Self::Deminiaturize
                    | Self::Miniaturize
                    | Self::ExitFullscreen
                    | Self::EnterFullscreen
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

    /// A command held back while the window heads for the mode it may be
    /// carried out in, and carried out once the step ends.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum HeldCommand {
        /// Stop being resizable.
        FixedSize,
    }

    /// What a window asked to be fixed in size does now.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum FixedSize {
        /// Restyle it now.
        Apply,
        /// Restyle it once the step taking it to normal mode ends.
        Held,
        /// Refuse, saying why.
        Refused(&'static str),
    }

    /// What a window does once the step that animated ends: plan the held
    /// mode again from its new flags, then carry out the held commands, in
    /// the order they arrived.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub(crate) struct AfterStep {
        pub(crate) mode: Option<WindowMode>,
        pub(crate) commands: Vec<HeldCommand>,
    }

    /// Why a window is not made fixed-size in another mode.
    const FIXED_SIZE_IN_NORMAL_MODE: &str = "a window is made fixed-size only in normal mode";

    /// The order in which a window's mode commands reach AppKit.
    ///
    /// AppKit ignores a toggle sent while a full-screen or Dock step animates, and
    /// what a window looks like after one is only known after. So a command that
    /// arrives during one, and the rest of a mode whose first step is one, are held
    /// here and planned again from the window's flags when the step's notification
    /// arrives. The newest command wins, and it replaces the mode a hidden window
    /// was to enter when first shown. A style command for the mode the window is
    /// heading to waits with it, behind the held mode; a newer mode command drops
    /// it, and so does a step that outlived [`ANIMATION_LIMIT`], so no old
    /// obligation fires on a later step of the user's own.
    #[derive(Debug, Default)]
    pub(crate) struct ModeSequence {
        /// The mode held back until the animating step ends.
        held: Option<WindowMode>,
        /// The commands held back until the animating step ends, in arrival
        /// order, carried out after the held mode.
        held_commands: Vec<HeldCommand>,
        /// When the step that is animating started.
        animating_since: Option<Instant>,
        /// The mode the last command sent the window toward; while a step
        /// animates, the window's mode for what it may be told next.
        heading: Option<WindowMode>,
        /// The mode a window that opened hidden enters when it is first shown.
        when_shown: Option<WindowMode>,
    }

    impl ModeSequence {
        /// The steps to send now to take a window with `flags` to `target`, at
        /// `now`. A style command held for an older target is dropped.
        pub(crate) fn command(
            &mut self,
            now: Instant,
            flags: WindowFlags,
            target: WindowMode,
        ) -> Vec<ModeStep> {
            self.when_shown = None;
            self.held_commands.clear();
            if self.animates(now) {
                self.held = Some(target);
                self.heading = Some(target);
                return Vec::new();
            }
            let steps = steps_to(flags, target);
            let (batch, asks_again) = next_batch(&steps);
            self.held = asks_again.then_some(target);
            self.animating_since = batch.iter().any(|step| step.animates()).then_some(now);
            // Only a command that leaves a step animating says where the
            // window is heading; one AppKit carried out at once leaves its
            // flags to say it, and a step the user starts later is not one
            // toward this target.
            self.heading = self.animating_since.is_some().then_some(target);
            batch.to_vec()
        }

        /// Whether a step started at most [`ANIMATION_LIMIT`] before `now` is
        /// still animating.
        fn animates(&self, now: Instant) -> bool {
            self.animating_since
                .is_some_and(|since| now.saturating_duration_since(since) < ANIMATION_LIMIT)
        }

        /// The mode the window is heading for while a step toward it
        /// animates, at `now`; `None` when nothing animates, and the window's
        /// flags say its mode.
        pub(crate) fn heading(&self, now: Instant) -> Option<WindowMode> {
            if self.animates(now) {
                self.heading
            } else {
                None
            }
        }

        /// A step that outlived [`ANIMATION_LIMIT`] at `now` gives up: what
        /// it said of where the window was heading, and the commands held for
        /// it. Answers whether that changed anything, so the window's
        /// published capabilities are read again.
        pub(crate) fn expire(&mut self, now: Instant) -> bool {
            if self.animating_since.is_none() || self.animates(now) {
                return false;
            }
            self.animating_since = None;
            self.heading = None;
            self.held_commands.clear();
            true
        }

        /// A step that animates has started, from this backend or from the
        /// user. A step that outlived the limit gives up first, so its held
        /// commands do not land on this one.
        pub(crate) fn step_started(&mut self, now: Instant) {
            self.expire(now);
            self.animating_since = Some(now);
        }

        /// The step that was animating is over, or failed: the window's flags
        /// say its mode again.
        pub(crate) fn step_ended(&mut self) {
            self.animating_since = None;
            self.heading = None;
        }

        /// What the window does now that the step ended: the held mode, then
        /// the held commands, in arrival order.
        pub(crate) fn take_after_step(&mut self) -> AfterStep {
            AfterStep {
                mode: self.held.take(),
                commands: std::mem::take(&mut self.held_commands),
            }
        }

        /// A window with `flags` asked, at `now`, to be fixed in size. A
        /// window that is no longer resizable reports a zoomed frame as normal
        /// and cannot be un-zoomed by the user, so it is restyled only in
        /// normal mode; heading there, it waits for the step to end, since
        /// AppKit's flags still say the mode it is leaving and a restyle
        /// mid-transition leaves it in that mode.
        pub(crate) fn fix_size(&mut self, now: Instant, flags: WindowFlags) -> FixedSize {
            self.expire(now);
            match self.heading(now) {
                Some(WindowMode::Normal) => {
                    self.held_commands.push(HeldCommand::FixedSize);
                    FixedSize::Held
                }
                None if mode_of(flags.mode_flags()) == WindowMode::Normal => FixedSize::Apply,
                Some(_) | None => FixedSize::Refused(FIXED_SIZE_IN_NORMAL_MODE),
            }
        }

        /// The window was made resizable after all: a held fixed size is
        /// dropped, the newer command winning.
        pub(crate) fn make_resizable(&mut self) {
            self.held_commands
                .retain(|command| *command != HeldCommand::FixedSize);
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
                assert_eq!(
                    mode_of(flags(letters).mode_flags()),
                    expected,
                    "flags {letters:?}"
                );
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
            use ModeStep::{
                Deminiaturize, EnterFullscreen, ExitFullscreen, Miniaturize, Unzoom, Zoom,
            };

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
            use ModeStep::{
                Deminiaturize, EnterFullscreen, ExitFullscreen, Miniaturize, Unzoom, Zoom,
            };

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
            assert_eq!(sequence.take_after_step().mode, Some(Normal));
            assert_eq!(
                sequence.take_after_step().mode,
                None,
                "a held mode is taken once"
            );
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
            assert_eq!(sequence.take_after_step().mode, Some(Minimized));
            assert_eq!(
                sequence.command(at(start, 1), flags("R"), Minimized),
                [ModeStep::Miniaturize]
            );
            assert_eq!(
                sequence.take_after_step().mode,
                None,
                "nothing is left to ask again"
            );
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

        #[test]
        fn a_window_honours_the_mode_and_style_commands_its_flags_allow() {
            use flui_platform::Availability::{self, Native, Unavailable};
            use flui_platform::WindowCapabilities;

            let mut every = WindowCapabilities::NONE;
            every.maximizes = Native;
            every.enters_fullscreen = Native;
            every.sets_resizable = Native;
            every.sets_decorated = Native;
            every.hides = Native;
            /// What a window with `flags` honours.
            #[derive(Debug, PartialEq)]
            struct Row {
                flags: &'static str,
                maximizes: Availability,
                enters_fullscreen: Availability,
                sets_resizable: Availability,
                sets_decorated: Availability,
                hides: Availability,
            }
            let rows = [
                Row {
                    flags: "R",
                    maximizes: Native,
                    enters_fullscreen: Native,
                    sets_resizable: Native,
                    sets_decorated: Native,
                    hides: Native,
                },
                // A fixed-size window is neither zoomed nor made full screen.
                Row {
                    flags: "",
                    maximizes: Unavailable,
                    enters_fullscreen: Unavailable,
                    sets_resizable: Native,
                    sets_decorated: Native,
                    hides: Native,
                },
                Row {
                    flags: "Z",
                    maximizes: Unavailable,
                    enters_fullscreen: Unavailable,
                    sets_resizable: Native,
                    sets_decorated: Native,
                    hides: Native,
                },
                // Made fixed-size only from normal mode.
                Row {
                    flags: "ZR",
                    maximizes: Native,
                    enters_fullscreen: Native,
                    sets_resizable: Unavailable,
                    sets_decorated: Native,
                    hides: Native,
                },
                Row {
                    flags: "MR",
                    maximizes: Native,
                    enters_fullscreen: Native,
                    sets_resizable: Unavailable,
                    sets_decorated: Native,
                    hides: Native,
                },
                Row {
                    flags: "M",
                    maximizes: Unavailable,
                    enters_fullscreen: Unavailable,
                    sets_resizable: Unavailable,
                    sets_decorated: Native,
                    hides: Native,
                },
                // Not restyled, nor ordered out, while full screen.
                Row {
                    flags: "FR",
                    maximizes: Native,
                    enters_fullscreen: Native,
                    sets_resizable: Unavailable,
                    sets_decorated: Unavailable,
                    hides: Unavailable,
                },
            ];
            for expected in rows {
                let honoured = capabilities_of(flags(expected.flags), None, every);
                let actual = Row {
                    flags: expected.flags,
                    maximizes: honoured.maximizes,
                    enters_fullscreen: honoured.enters_fullscreen,
                    sets_resizable: honoured.sets_resizable,
                    sets_decorated: honoured.sets_decorated,
                    hides: honoured.hides,
                };
                assert_eq!(actual, expected);
            }
        }

        /// A window coming back from the Dock still reads as miniaturized
        /// while it animates; a fixed size asked for meanwhile is one for the
        /// normal window it is heading to. A command AppKit carried out at
        /// once, a step that ended and one that outlived the limit leave the
        /// flags to say the mode.
        #[test]
        fn a_window_heading_for_normal_may_be_made_fixed_size() {
            use flui_platform::Availability::{Native, Unavailable};
            use flui_platform::WindowCapabilities;

            let mut every = WindowCapabilities::NONE;
            every.sets_resizable = Native;
            let fixed =
                |flags: WindowFlags, heading| capabilities_of(flags, heading, every).sets_resizable;
            let now = Instant::now();

            let mut sequence = ModeSequence::default();
            let steps = sequence.command(now, flags("MR"), Normal);
            assert_eq!(steps, [ModeStep::Deminiaturize]);
            assert_eq!(sequence.heading(now), Some(Normal));
            assert_eq!(fixed(flags("MR"), sequence.heading(now)), Native);
            assert_eq!(
                fixed(flags("MR"), None),
                Unavailable,
                "the flags alone say miniaturized"
            );
            assert_eq!(
                sequence.heading(now + ANIMATION_LIMIT),
                None,
                "a step past the limit no longer says where the window goes"
            );
            sequence.step_ended();
            assert_eq!(sequence.heading(now), None);

            let mut sequence = ModeSequence::default();
            assert_eq!(
                sequence.command(now, flags("ZR"), Normal),
                [ModeStep::Unzoom]
            );
            assert_eq!(
                sequence.heading(now),
                None,
                "an un-zoom is carried out at once"
            );

            let mut sequence = ModeSequence::default();
            sequence.command(now, flags("R"), Minimized);
            assert_eq!(sequence.heading(now), Some(Minimized));
            assert_eq!(
                fixed(flags("R"), sequence.heading(now)),
                Unavailable,
                "heading for the Dock"
            );
        }

        /// A fixed size asked for on the way to normal mode waits for the
        /// step to end, and is carried out then, once; once there, it is
        /// carried out at once. A newer resizable drops it, and one asked for
        /// heading anywhere else is refused, not held.
        #[test]
        fn a_fixed_size_waits_for_the_step_to_normal_mode() {
            let now = Instant::now();
            let mut sequence = ModeSequence::default();
            sequence.command(now, flags("MR"), Normal);
            assert_eq!(sequence.fix_size(now, flags("MR")), FixedSize::Held);
            sequence.step_ended();
            assert_eq!(
                sequence.take_after_step().commands,
                [HeldCommand::FixedSize],
                "carried out once the step ends"
            );
            assert_eq!(sequence.take_after_step(), AfterStep::default(), "and once");
            assert_eq!(
                sequence.fix_size(now, flags("R")),
                FixedSize::Apply,
                "in normal mode"
            );

            let mut sequence = ModeSequence::default();
            sequence.command(now, flags("MR"), Normal);
            sequence.fix_size(now, flags("MR"));
            sequence.make_resizable();
            sequence.step_ended();
            assert!(
                sequence.take_after_step().commands.is_empty(),
                "the newer resizable wins"
            );

            let mut sequence = ModeSequence::default();
            sequence.command(now, flags("R"), Minimized);
            assert!(matches!(
                sequence.fix_size(now, flags("R")),
                FixedSize::Refused(_)
            ));
            sequence.step_ended();
            assert!(
                sequence.take_after_step().commands.is_empty(),
                "refused, not held"
            );
        }

        /// The step ends: the held mode is planned first, then the held
        /// commands, in the order they arrived, so a fixed size meant for the
        /// normal window the held mode leads to lands after it.
        #[test]
        fn the_held_mode_comes_before_the_held_commands() {
            let now = Instant::now();
            let mut sequence = ModeSequence::default();
            // A zoomed window in full screen: leaving it first, then un-zooming.
            assert_eq!(
                sequence.command(now, flags("FZR"), Normal),
                [ModeStep::ExitFullscreen]
            );
            assert_eq!(sequence.fix_size(now, flags("FZR")), FixedSize::Held);
            sequence.step_ended();
            assert_eq!(
                sequence.take_after_step(),
                AfterStep {
                    mode: Some(Normal),
                    commands: vec![HeldCommand::FixedSize],
                }
            );
        }

        /// No old obligation fires late: a newer mode command drops the held
        /// commands, and so does a step that outlived the limit, before a
        /// later step of the user's own starts.
        #[test]
        fn a_held_command_is_dropped_by_a_newer_mode_or_the_limit() {
            let now = Instant::now();

            let mut sequence = ModeSequence::default();
            sequence.command(now, flags("MR"), Normal);
            sequence.fix_size(now, flags("MR"));
            sequence.command(now, flags("MR"), Maximized);
            sequence.step_ended();
            let after = sequence.take_after_step();
            assert_eq!(after.mode, Some(Maximized), "the newer mode is held");
            assert!(
                after.commands.is_empty(),
                "the fixed size went with the older mode"
            );

            let mut sequence = ModeSequence::default();
            sequence.command(now, flags("MR"), Normal);
            sequence.fix_size(now, flags("MR"));
            let late = now + ANIMATION_LIMIT;
            assert!(sequence.expire(late), "the step gave up");
            assert!(!sequence.expire(late), "once");
            assert_eq!(sequence.heading(late), None);
            sequence.step_started(late);
            sequence.step_ended();
            assert!(
                sequence.take_after_step().commands.is_empty(),
                "the user's own step later carries out nothing old"
            );
        }

        /// A window still full screen is not made fixed-size, whatever mode
        /// it heads for: AppKit restyles no full-screen window.
        #[test]
        fn a_full_screen_window_heading_for_normal_stays_resizable() {
            use flui_platform::Availability::Unavailable;
            use flui_platform::WindowCapabilities;

            let mut every = WindowCapabilities::NONE;
            every.sets_resizable = flui_platform::Availability::Native;
            let honoured = capabilities_of(flags("FR"), Some(Normal), every);
            assert_eq!(honoured.sets_resizable, Unavailable);
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) mod surface {
    //! When an `NSWindow`'s surface is seen, from its occlusion state. The
    //! delegate reads `occlusionState` into the window's snapshot.

    /// `NSWindowOcclusionStateVisible` (`NSWindow.h`: `1 << 1`) — the only bit
    /// AppKit defines in `NSWindowOcclusionState`.
    pub(crate) const OCCLUSION_STATE_VISIBLE: u64 = 1 << 1;

    /// Whether an `NSWindowOcclusionState` raw value means the window's surface
    /// is seen. AppKit reports "visible" as a positive bit (set when at least
    /// part of the window is on screen and composed), so the snapshot's
    /// visibility is the bit itself — no
    /// negation, unlike winit's `Occluded` event.
    #[must_use]
    pub(crate) const fn occlusion_state_is_visible(occlusion_state: u64) -> bool {
        occlusion_state & OCCLUSION_STATE_VISIBLE != 0
    }

    #[cfg(test)]
    mod tests {
        use super::{OCCLUSION_STATE_VISIBLE, occlusion_state_is_visible};

        /// AppKit's visible bit is the visibility.
        #[test]
        fn appkit_occlusion_reports_the_visible_bit() {
            assert!(occlusion_state_is_visible(OCCLUSION_STATE_VISIBLE));
            assert!(occlusion_state_is_visible(OCCLUSION_STATE_VISIBLE | 1));
            assert!(!occlusion_state_is_visible(0));
            assert!(!occlusion_state_is_visible(1 << 3));
        }
    }
}

#[cfg(any(target_os = "ios", test))]
pub(crate) mod screen_edges {
    //! The screen edges a set of system-gesture exclusion regions claims, for an OS that
    //! defers its edge gestures per edge rather than per rectangle (UIKit's
    //! `preferredScreenEdgesDeferringSystemGestures`).

    use flui_foundation::geometry::Size;
    use flui_platform::input_regions::{DeviceInputRegion, InputRegionKind};

    /// The window edges whose system gestures the application asks the OS to defer.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub(crate) struct DeferredEdges {
        /// The top edge.
        pub(crate) top: bool,
        /// The left edge.
        pub(crate) left: bool,
        /// The bottom edge.
        pub(crate) bottom: bool,
        /// The right edge.
        pub(crate) right: bool,
    }

    impl DeferredEdges {
        /// The edges that `regions` (device pixels) reach in a window `window` device pixels
        /// in size: a system-gesture exclusion region touching or crossing an edge defers that
        /// edge's gestures. Regions of other kinds, and regions clear of every edge, defer
        /// nothing; an OS without per-rectangle exclusion cannot honour them.
        #[must_use]
        pub(crate) fn of(regions: &[DeviceInputRegion], window: Size<i32>) -> Self {
            regions
                .iter()
                .filter(|region| region.kind() == InputRegionKind::SystemGestureExclusion)
                .fold(Self::default(), |edges, region| {
                    let rect = region.rect();
                    Self {
                        top: edges.top || rect.top() <= 0,
                        left: edges.left || rect.left() <= 0,
                        bottom: edges.bottom || rect.bottom() >= window.height,
                        right: edges.right || rect.right() >= window.width,
                    }
                })
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_foundation::geometry::{DevicePoint, DeviceRect};

        use super::*;

        const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;

        fn device(left: i32, top: i32, right: i32, bottom: i32) -> DeviceInputRegion {
            DeviceInputRegion::new(
                EXCLUSION,
                DeviceRect::from_min_max(
                    DevicePoint::new(left, top),
                    DevicePoint::new(right, bottom),
                ),
            )
        }

        #[test]
        fn an_exclusion_region_defers_the_edges_it_reaches() {
            let window = Size::new(100, 200);
            let edges = |regions: &[DeviceInputRegion]| DeferredEdges::of(regions, window);
            assert_eq!(edges(&[]), DeferredEdges::default());
            assert_eq!(
                edges(&[device(10, 10, 90, 190)]),
                DeferredEdges::default(),
                "clear of every edge"
            );
            assert_eq!(
                edges(&[device(0, 50, 20, 150)]),
                DeferredEdges {
                    left: true,
                    ..DeferredEdges::default()
                }
            );
            assert_eq!(
                edges(&[device(80, 180, 100, 200), device(0, 0, 100, 10)]),
                DeferredEdges {
                    top: true,
                    left: true,
                    bottom: true,
                    right: true,
                }
            );
        }
    }
}
