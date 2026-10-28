//! What a desktop host decides about a window before and while it exists, the
//! winit and the X11 hosts alike, kept apart from both so a table of inputs
//! checks it.
//!
//! - [`WindowShell`] says what the window system lets a program do to a
//!   window, and so which [`WindowCapabilities`] a window has. X11, Win32 and
//!   AppKit let a program place, hide, show and focus a window and read where
//!   it is; a Wayland compositor does none of that.
//! - [`OpenPlan`] turns the creation options into what the window system is
//!   asked for: a physical position, a content size and an initial mode.
//! - [`FrameInset`] is the frame around the content of a decorated window,
//!   measured once a window with a frame is on screen, so a remembered outer
//!   size can be fitted before the next window exists.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    DisplayId, DisplaySnapshot, WindowCapabilities, WindowGeometry, WindowMode, WindowOptions,
    WindowReveal,
};
use parking_lot::Mutex;

/// What the window system lets a program do to a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowShell {
    /// X11, Win32 or AppKit: windows are placed, hidden, shown and focused by
    /// the program, and their position and minimization can be read.
    Desktop,
    /// A Wayland compositor: a program can neither place a window nor hide it,
    /// read where it is, tell whether it is minimized, nor take focus. A
    /// client can still minimize its window, but only the user can bring it
    /// back: asking for normal mode restores a maximized or fullscreen window
    /// and leaves a minimized one to the user.
    // Only the winit host opens windows on a Wayland shell so far.
    #[cfg(any(winit_host, test))]
    Wayland,
}

impl WindowShell {
    /// Which commands and observations a window on this shell has. winit
    /// reports no presentation on any shell, though the compositors under it
    /// do (`wp_presentation_time`, DXGI statistics): the native hosts read
    /// them, and this one does not.
    pub(super) const fn capabilities(self) -> WindowCapabilities {
        self.shell_capabilities()
            .without_presentation_feedback(flui_platform::CapabilityAbsence::NotYetOnThisHost)
    }

    const fn shell_capabilities(self) -> WindowCapabilities {
        match self {
            Self::Desktop => WindowCapabilities::ALL,
            #[cfg(any(winit_host, test))]
            Self::Wayland => {
                let mut capabilities = WindowCapabilities::ALL;
                capabilities.sets_position = flui_platform::Availability::Unavailable;
                capabilities.hides = flui_platform::Availability::Unavailable;
                capabilities.shows = flui_platform::Availability::Unavailable;
                capabilities.activates = flui_platform::Availability::Unavailable;
                capabilities.observes_position = flui_platform::Availability::Unavailable;
                capabilities.observes_minimized = flui_platform::Availability::Unavailable;
                capabilities.places_at_open = flui_platform::Availability::Unavailable;
                capabilities
            }
        }
    }

    /// Whether the shell lets a window be created hidden and shown after its
    /// first frame. On Wayland a window has no buffer, and so is not shown,
    /// until its first frame is presented; nothing is deferred.
    pub(super) const fn defers_reveal(self) -> bool {
        matches!(self, Self::Desktop)
    }
}

/// The display a window is placed on: where its origin is in the global
/// device-pixel space, and how many device pixels make a logical one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacementDisplay {
    pub origin: Point<i32>,
    pub scale_factor: f64,
}

impl PlacementDisplay {
    /// `geometry`, measured from this display's usable origin in its logical
    /// pixels, as the global logical rectangle a window state publishes.
    fn to_global(self, geometry: WindowGeometry) -> WindowGeometry {
        let scale = self.scale_factor;
        if !(scale.is_finite() && scale > 0.0) {
            return geometry;
        }
        let origin = Point::new(
            f64::from(self.origin.x) / scale,
            f64::from(self.origin.y) / scale,
        );
        WindowGeometry::new(
            geometry
                .position
                .map(|at| Point::new(at.x + origin.x, at.y + origin.y)),
            geometry.size,
        )
    }

    /// The display options measure their position on: the one `wanted` names
    /// if the snapshot holds it, else global coordinates (origin zero) at the
    /// primary display's scale.
    ///
    /// A named display's origin is that of its **usable area**, which is what
    /// `WindowPlacement` measures a remembered offset from: a window on a
    /// display with a panel along its top reopens below the panel, not under
    /// it.
    pub(crate) fn choose(snapshot: Option<&DisplaySnapshot>, wanted: Option<DisplayId>) -> Self {
        let named = snapshot
            .zip(wanted)
            .and_then(|(snapshot, id)| snapshot.get(id));
        match named {
            Some(display) => Self {
                origin: display.usable_bounds.origin,
                scale_factor: display.scale_factor,
            },
            None => Self {
                origin: Point::new(0, 0),
                scale_factor: snapshot
                    .and_then(DisplaySnapshot::primary)
                    .map_or(1.0, |display| display.scale_factor),
            },
        }
    }
}

/// What winit is asked for when a window is created.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OpenPlan {
    /// The outer top-left corner in global device pixels; `None` lets the
    /// window manager place the window.
    pub position: Option<Point<i32>>,
    /// The content size in logical pixels.
    pub content_size: Size<f64>,
    /// The mode the window opens in. Minimized is not one: a window cannot be
    /// created minimized, so it opens in [`WindowMode::Normal`].
    pub mode: WindowMode,
    /// Whether the window is created hidden, to be shown after its first
    /// frame.
    pub hidden_until_first_frame: bool,
    /// Whether the window is created hidden and mapped as soon as it is set
    /// up: a window that opens maximized or fullscreen is mapped only after
    /// its mode is written into its properties, which a window manager reads
    /// when it maps a window, so it never appears in normal mode first.
    pub map_after_setup: bool,
    /// Whether the application asked for the window hidden (`visible: false`)
    /// on a shell where that means unmapped. Its opening mode is written into
    /// its properties like any other unmapped window's, and it stays unmapped
    /// until the application shows it.
    pub hidden_by_application: bool,
    /// The rectangle the window returns to from `mode`, in global logical
    /// pixels, when it opens
    /// maximized or fullscreen with one remembered.
    pub normal_geometry: Option<WindowGeometry>,
}

impl OpenPlan {
    /// Whether the window is created unmapped, to be mapped by the host: after
    /// its first frame, or once its opening mode is set.
    pub(crate) const fn created_unmapped(&self) -> bool {
        self.hidden_until_first_frame || self.map_after_setup
    }

    /// Whether the window is unmapped at creation for any reason, the
    /// application's included.
    #[cfg(any(target_os = "linux", test))]
    pub(super) const fn unmapped_until_shown(&self) -> bool {
        self.created_unmapped() || self.hidden_by_application
    }

    /// Plan the creation of a window on `shell` from `options`.
    ///
    /// `display` is the display the options measure their position on, or the
    /// primary one when they name none or it is gone. `inset` is the frame
    /// around the content of a decorated window, which turns a remembered
    /// outer size into a content size.
    ///
    /// A window that opens maximized or fullscreen is created with the
    /// rectangle it returns to, so un-maximizing it later goes there; the
    /// window manager keeps that rectangle as the window's normal one.
    pub(crate) fn new(
        options: &WindowOptions,
        shell: WindowShell,
        display: PlacementDisplay,
        inset: Size<f64>,
    ) -> Self {
        let capabilities = shell.capabilities();
        let mode = match options.mode {
            WindowMode::Maximized if capabilities.maximizes.is_available() => WindowMode::Maximized,
            WindowMode::Fullscreen if capabilities.enters_fullscreen.is_available() => {
                WindowMode::Fullscreen
            }
            _ => WindowMode::Normal,
        };
        let returns_to = match mode {
            WindowMode::Maximized | WindowMode::Fullscreen => options.normal_geometry,
            _ => None,
        };
        // An undecorated window has no frame to take from a remembered outer
        // size, whatever the decorated windows before it measured.
        let inset = if options.decorated {
            inset
        } else {
            Size::new(0.0, 0.0)
        };
        let (at, content_size) = match returns_to {
            Some(geometry) => (
                geometry.position,
                Size::new(
                    (geometry.size.width - inset.width).max(1.0),
                    (geometry.size.height - inset.height).max(1.0),
                ),
            ),
            None => (options.position, options.size),
        };
        let hidden_until_first_frame = options.visible
            && options.reveal == WindowReveal::AfterFirstFrame
            && shell.defers_reveal();
        let position = if capabilities.places_at_open.is_available() {
            at.and_then(|at| device_position(at, display))
        } else {
            None
        };
        Self {
            position,
            content_size,
            mode,
            hidden_until_first_frame,
            map_after_setup: options.visible
                && shell.defers_reveal()
                && !hidden_until_first_frame
                && mode != WindowMode::Normal,
            hidden_by_application: !options.visible && shell.defers_reveal(),
            normal_geometry: returns_to.map(|geometry| display.to_global(geometry)),
        }
    }
}

/// `at`, in logical pixels from `display`'s origin, as global device pixels.
/// `None` for a position that is not a finite number of pixels an X11 or Win32
/// coordinate can hold.
fn device_position(at: Point<f64>, display: PlacementDisplay) -> Option<Point<i32>> {
    let device = |offset: f64, origin: i32| {
        let pixels = (offset * display.scale_factor).round() + f64::from(origin);
        // Not `as`: a value past the range saturates silently, and a placement
        // that far from every display is a bad one, not one to clamp.
        (pixels.is_finite() && pixels >= f64::from(i32::MIN) && pixels <= f64::from(i32::MAX))
            .then_some(pixels as i32)
    };
    Some(Point::new(
        device(at.x, display.origin.x)?,
        device(at.y, display.origin.y)?,
    ))
}

/// The frame a decorated window has around its content, in device pixels:
/// the title bar and borders, measured from a window that is on screen.
///
/// The memory is shared by the platform's windows. It holds the last
/// measurement, and none until one decorated window has been seen with its
/// frame.
#[derive(Debug, Default)]
pub(crate) struct FrameInset {
    measured: Mutex<Option<Size<i32>>>,
}

impl FrameInset {
    /// Record the sizes of a window the caller found in normal mode (a
    /// maximized or fullscreen window's frame is not the one a normal window
    /// gets) if they show a frame: the window is decorated, and its outer
    /// extent is at least its content's in both dimensions, with some
    /// difference.
    // The X11 host reads the frame the window manager reports instead.
    #[cfg(any(winit_host, test))]
    pub(super) fn observe(&self, decorated: bool, outer: Size<i32>, content: Size<i32>) {
        if !decorated || outer.width < content.width || outer.height < content.height {
            return;
        }
        self.record(Size::new(
            outer.width - content.width,
            outer.height - content.height,
        ));
    }

    /// Record a frame the window manager reported, in device pixels. A frame
    /// of nothing, or a negative one, is not a measurement.
    pub(super) fn record(&self, frame: Size<i32>) {
        if frame.width < 0 || frame.height < 0 || (frame.width == 0 && frame.height == 0) {
            return;
        }
        *self.measured.lock() = Some(frame);
    }

    /// Whether any frame was measured.
    #[cfg(any(target_os = "linux", test))]
    pub(super) fn is_known(&self) -> bool {
        self.measured.lock().is_some()
    }

    /// The frame in logical pixels on a display of `scale_factor`; zero before
    /// any was measured.
    pub(crate) fn logical(&self, scale_factor: f64) -> Size<f64> {
        let measured = *self.measured.lock();
        match measured {
            Some(frame) if scale_factor.is_finite() && scale_factor > 0.0 => Size::new(
                f64::from(frame.width) / scale_factor,
                f64::from(frame.height) / scale_factor,
            ),
            _ => Size::new(0.0, 0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: PlacementDisplay = PlacementDisplay {
        origin: Point { x: 1920, y: 0 },
        scale_factor: 1.0,
    };

    fn options() -> WindowOptions {
        WindowOptions::default()
            .with_size(Size::new(800.0, 600.0))
            .with_position(Some(Point::new(100.0, 50.0)))
    }

    fn plan(options: &WindowOptions, shell: WindowShell) -> OpenPlan {
        OpenPlan::new(options, shell, DISPLAY, Size::new(0.0, 0.0))
    }

    #[test]
    fn placement_display_is_measured_from_the_usable_area() {
        use flui_foundation::geometry::Bounds;
        use flui_platform::{DisplayInfo, DisplayKey, DisplayRegistry};

        let registry = DisplayRegistry::new();
        let id = |_| registry.mint(&DisplayKey::Unkeyed).expect("an id");
        // A 1920x1080 display at scale 2 with a 56-pixel panel along its top.
        let bounds = Bounds::new(Point::new(1920, 0), Size::new(1920, 1080));
        let usable = Bounds::new(Point::new(1920, 56), Size::new(1920, 1024));
        let main = DisplayInfo::new(id(0), "main", bounds, 2.0)
            .with_usable_bounds(usable)
            .with_primary(true);
        let other = DisplayInfo::new(id(1), "other", bounds, 1.0);
        let (main_id, other_id) = (main.id, other.id);
        let snapshot = DisplaySnapshot::empty()
            .successor(vec![main, other])
            .expect("a snapshot");

        // (case, wanted display, display placed on)
        let table = [
            (
                "a named display: its usable origin and scale",
                Some(main_id),
                PlacementDisplay {
                    origin: Point::new(1920, 56),
                    scale_factor: 2.0,
                },
            ),
            (
                "a named display with no panel",
                Some(other_id),
                PlacementDisplay {
                    origin: Point::new(1920, 0),
                    scale_factor: 1.0,
                },
            ),
            (
                "none named: global, at the primary scale",
                None,
                PlacementDisplay {
                    origin: Point::new(0, 0),
                    scale_factor: 2.0,
                },
            ),
        ];
        for (case, wanted, expected) in table {
            assert_eq!(
                PlacementDisplay::choose(Some(&snapshot), wanted),
                expected,
                "{case}"
            );
        }
        let gone = registry.mint(&DisplayKey::Unkeyed).expect("an id");
        assert_eq!(
            PlacementDisplay::choose(Some(&snapshot), Some(gone)).origin,
            Point::new(0, 0),
            "a display that is gone"
        );
        assert_eq!(
            PlacementDisplay::choose(None, Some(main_id)),
            PlacementDisplay {
                origin: Point::new(0, 0),
                scale_factor: 1.0
            },
            "no snapshot"
        );

        // A window 100 logical pixels below the top of the display is saved as
        // an offset from the usable area and reopens where it was.
        let main_display = PlacementDisplay::choose(Some(&snapshot), Some(main_id));
        let options = options().with_position(Some(Point::new(10.0, 22.0)));
        let planned = OpenPlan::new(
            &options,
            WindowShell::Desktop,
            main_display,
            Size::new(0.0, 0.0),
        );
        assert_eq!(planned.position, Some(Point::new(1940, 100)));
    }

    #[test]
    fn window_shell_capabilities() {
        let desktop = WindowShell::Desktop.capabilities();
        assert_eq!(
            desktop,
            WindowCapabilities::ALL
                .without_presentation_feedback(flui_platform::CapabilityAbsence::NotYetOnThisHost),
            "winit reports no presentation on any shell"
        );

        let wayland = WindowShell::Wayland.capabilities();
        // (field, value on Wayland)
        let table = [
            ("sets_title", wayland.sets_title, true),
            ("sets_position", wayland.sets_position, false),
            ("sets_size", wayland.sets_size, true),
            ("sets_constraints", wayland.sets_constraints, true),
            ("sets_resizable", wayland.sets_resizable, true),
            ("sets_decorated", wayland.sets_decorated, true),
            ("minimizes", wayland.minimizes, true),
            ("maximizes", wayland.maximizes, true),
            ("enters_fullscreen", wayland.enters_fullscreen, true),
            ("hides", wayland.hides, false),
            ("shows", wayland.shows, false),
            ("activates", wayland.activates, false),
            ("observes_position", wayland.observes_position, false),
            ("observes_minimized", wayland.observes_minimized, false),
            ("places_at_open", wayland.places_at_open, false),
            ("reports_presentation", wayland.reports_presentation, false),
        ];
        for (field, actual, expected) in table {
            assert_eq!(actual.is_available(), expected, "{field}");
        }
    }

    #[test]
    fn open_plan_places_by_display_origin_and_scale() {
        // (case, position, display, device position)
        let scaled = PlacementDisplay {
            origin: Point::new(-2560, 100),
            scale_factor: 1.5,
        };
        let table = [
            (
                "on the origin display",
                Some(Point::new(100.0, 50.0)),
                DISPLAY,
                Some(Point::new(2020, 50)),
            ),
            (
                "scaled, left of the origin",
                Some(Point::new(10.0, 10.0)),
                scaled,
                Some(Point::new(-2545, 115)),
            ),
            (
                "fractions round",
                Some(Point::new(0.4, 0.6)),
                DISPLAY,
                Some(Point::new(1920, 1)),
            ),
            ("no position", None, DISPLAY, None),
            (
                "infinite",
                Some(Point::new(f64::INFINITY, 0.0)),
                DISPLAY,
                None,
            ),
            (
                "not a number",
                Some(Point::new(0.0, f64::NAN)),
                DISPLAY,
                None,
            ),
            (
                "past the coordinate range",
                Some(Point::new(3.0e9, 0.0)),
                DISPLAY,
                None,
            ),
            (
                "before the coordinate range",
                Some(Point::new(-3.0e9, 0.0)),
                DISPLAY,
                None,
            ),
        ];
        for (case, position, display, expected) in table {
            let options = options().with_position(position);
            let planned =
                OpenPlan::new(&options, WindowShell::Desktop, display, Size::new(0.0, 0.0));
            assert_eq!(planned.position, expected, "{case}");
        }
    }

    #[test]
    fn open_plan_leaves_placement_to_a_shell_that_cannot_place() {
        let planned = plan(&options(), WindowShell::Wayland);
        assert_eq!(planned.position, None);
        assert_eq!(planned.content_size, Size::new(800.0, 600.0));
    }

    #[test]
    fn open_plan_modes() {
        // (case, mode, shell, mode planned)
        let table = [
            (
                "normal",
                WindowMode::Normal,
                WindowShell::Desktop,
                WindowMode::Normal,
            ),
            (
                "maximized",
                WindowMode::Maximized,
                WindowShell::Desktop,
                WindowMode::Maximized,
            ),
            (
                "fullscreen",
                WindowMode::Fullscreen,
                WindowShell::Desktop,
                WindowMode::Fullscreen,
            ),
            (
                "minimized opens normal",
                WindowMode::Minimized,
                WindowShell::Desktop,
                WindowMode::Normal,
            ),
            (
                "maximized on Wayland",
                WindowMode::Maximized,
                WindowShell::Wayland,
                WindowMode::Maximized,
            ),
            (
                "fullscreen on Wayland",
                WindowMode::Fullscreen,
                WindowShell::Wayland,
                WindowMode::Fullscreen,
            ),
            (
                "minimized on Wayland",
                WindowMode::Minimized,
                WindowShell::Wayland,
                WindowMode::Normal,
            ),
        ];
        for (case, mode, shell, expected) in table {
            let planned = plan(&options().with_mode(mode), shell);
            assert_eq!(planned.mode, expected, "{case}");
        }
    }

    #[test]
    fn open_plan_opens_a_maximized_window_at_the_rectangle_it_returns_to() {
        let returns_to =
            WindowGeometry::new(Some(Point::new(300.0, 200.0)), Size::new(1000.0, 700.0));
        let frame = Size::new(2.0, 30.0);
        let maximized = options()
            .with_mode(WindowMode::Maximized)
            .with_normal_geometry(Some(returns_to));

        let planned = OpenPlan::new(&maximized, WindowShell::Desktop, DISPLAY, frame);
        assert_eq!(planned.position, Some(Point::new(2220, 200)));
        assert_eq!(
            planned.content_size,
            Size::new(998.0, 670.0),
            "outer less the frame"
        );
        // The state publishes global positions, whatever display the options
        // measured theirs from.
        assert_eq!(
            planned.normal_geometry,
            Some(WindowGeometry::new(
                Some(Point::new(2220.0, 200.0)),
                Size::new(1000.0, 700.0)
            ))
        );

        // On a scaled display the usable origin is in device pixels.
        let scaled = PlacementDisplay {
            origin: Point::new(1920, 56),
            scale_factor: 2.0,
        };
        let relative = WindowGeometry::new(Some(Point::new(100.0, 50.0)), Size::new(1000.0, 700.0));
        let on_scaled = options()
            .with_mode(WindowMode::Maximized)
            .with_normal_geometry(Some(relative));
        let planned = OpenPlan::new(
            &on_scaled,
            WindowShell::Desktop,
            scaled,
            Size::new(0.0, 0.0),
        );
        assert_eq!(
            planned
                .normal_geometry
                .and_then(|geometry| geometry.position),
            Some(Point::new(1060.0, 78.0))
        );

        // A normal window ignores the rectangle it has no use for.
        let normal = options().with_normal_geometry(Some(returns_to));
        let planned = OpenPlan::new(&normal, WindowShell::Desktop, DISPLAY, frame);
        assert_eq!(planned.position, Some(Point::new(2020, 50)));
        assert_eq!(planned.content_size, Size::new(800.0, 600.0));
        assert_eq!(planned.normal_geometry, None);

        // A rectangle smaller than the frame still has a content area.
        let tiny = WindowGeometry::new(None, Size::new(1.0, 1.0));
        let tiny = options()
            .with_mode(WindowMode::Fullscreen)
            .with_normal_geometry(Some(tiny));
        let planned = OpenPlan::new(&tiny, WindowShell::Desktop, DISPLAY, frame);
        assert_eq!(planned.content_size, Size::new(1.0, 1.0));
        assert_eq!(
            planned.position, None,
            "its rectangle has no position either"
        );
    }

    #[test]
    fn open_plan_takes_no_frame_from_an_undecorated_window() {
        let returns_to =
            WindowGeometry::new(Some(Point::new(300.0, 200.0)), Size::new(1000.0, 700.0));
        let frame = Size::new(2.0, 30.0);
        // (case, decorated, content size)
        let table = [
            ("decorated", true, Size::new(998.0, 670.0)),
            ("undecorated", false, Size::new(1000.0, 700.0)),
        ];
        for (case, decorated, expected) in table {
            let options = options()
                .with_decorated(decorated)
                .with_mode(WindowMode::Maximized)
                .with_normal_geometry(Some(returns_to));
            let planned = OpenPlan::new(&options, WindowShell::Desktop, DISPLAY, frame);
            assert_eq!(planned.content_size, expected, "{case}");
        }
    }

    #[test]
    fn open_plan_maps_a_window_that_opens_in_a_mode_after_setting_it_up() {
        // (case, visible, reveal, mode, shell, hidden until first frame, mapped after setup)
        let table = [
            (
                "normal at open",
                true,
                WindowReveal::AtOpen,
                WindowMode::Normal,
                WindowShell::Desktop,
                false,
                false,
            ),
            (
                "maximized at open",
                true,
                WindowReveal::AtOpen,
                WindowMode::Maximized,
                WindowShell::Desktop,
                false,
                true,
            ),
            (
                "fullscreen at open",
                true,
                WindowReveal::AtOpen,
                WindowMode::Fullscreen,
                WindowShell::Desktop,
                false,
                true,
            ),
            (
                "maximized after the first frame",
                true,
                WindowReveal::AfterFirstFrame,
                WindowMode::Maximized,
                WindowShell::Desktop,
                true,
                false,
            ),
            (
                "maximized, opened hidden",
                false,
                WindowReveal::AtOpen,
                WindowMode::Maximized,
                WindowShell::Desktop,
                false,
                false,
            ),
            (
                "maximized on Wayland",
                true,
                WindowReveal::AtOpen,
                WindowMode::Maximized,
                WindowShell::Wayland,
                false,
                false,
            ),
        ];
        for (case, visible, reveal, mode, shell, hidden, mapped) in table {
            let options = options()
                .with_visible(visible)
                .with_reveal(reveal)
                .with_mode(mode);
            let planned = plan(&options, shell);
            assert_eq!(planned.hidden_until_first_frame, hidden, "{case}");
            assert_eq!(planned.map_after_setup, mapped, "{case}");
            assert_eq!(
                planned.hidden_by_application,
                !visible && shell == WindowShell::Desktop,
                "{case}"
            );
            assert_eq!(
                planned.unmapped_until_shown(),
                hidden || mapped || (!visible && shell == WindowShell::Desktop),
                "{case}"
            );
        }
    }
    #[test]
    fn open_plan_defers_the_reveal_only_where_it_can() {
        // (case, visible, reveal, shell, hidden until the first frame)
        let table = [
            (
                "desktop, after first frame",
                true,
                WindowReveal::AfterFirstFrame,
                WindowShell::Desktop,
                true,
            ),
            (
                "desktop, at open",
                true,
                WindowReveal::AtOpen,
                WindowShell::Desktop,
                false,
            ),
            (
                "desktop, opened hidden",
                false,
                WindowReveal::AfterFirstFrame,
                WindowShell::Desktop,
                false,
            ),
            (
                "wayland shows with its first buffer",
                true,
                WindowReveal::AfterFirstFrame,
                WindowShell::Wayland,
                false,
            ),
        ];
        for (case, visible, reveal, shell, expected) in table {
            let options = options().with_visible(visible).with_reveal(reveal);
            assert_eq!(
                plan(&options, shell).hidden_until_first_frame,
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn frame_inset_is_measured_from_a_framed_window_on_screen() {
        let inset = FrameInset::default();
        assert_eq!(
            inset.logical(1.0),
            Size::new(0.0, 0.0),
            "nothing measured yet"
        );
        assert!(!inset.is_known());

        // (case, decorated, outer, content)
        let ignored = [
            (
                "undecorated",
                false,
                Size::new(810, 640),
                Size::new(800, 600),
            ),
            (
                "no difference",
                true,
                Size::new(800, 600),
                Size::new(800, 600),
            ),
            (
                "outer narrower than content",
                true,
                Size::new(790, 640),
                Size::new(800, 600),
            ),
            (
                "outer shorter than content",
                true,
                Size::new(810, 590),
                Size::new(800, 600),
            ),
        ];
        for (case, decorated, outer, content) in ignored {
            inset.observe(decorated, outer, content);
            assert_eq!(inset.logical(1.0), Size::new(0.0, 0.0), "{case}");
        }
        for (case, frame) in [
            ("a frame of nothing", Size::new(0, 0)),
            ("a negative width", Size::new(-1, 10)),
            ("a negative height", Size::new(10, -1)),
        ] {
            inset.record(frame);
            assert!(!inset.is_known(), "{case}");
        }

        inset.observe(true, Size::new(1620, 1260), Size::new(1600, 1200));
        assert_eq!(inset.logical(1.0), Size::new(20.0, 60.0));
        assert!(inset.is_known());
        assert_eq!(
            inset.logical(2.0),
            Size::new(10.0, 30.0),
            "device pixels over scale"
        );
        assert_eq!(
            inset.logical(0.0),
            Size::new(0.0, 0.0),
            "a scale that divides by zero"
        );
        assert_eq!(inset.logical(f64::NAN), Size::new(0.0, 0.0));

        // A frame of one dimension only (a bar and no borders) is a frame, and
        // a frame the window manager reports replaces a measured one.
        inset.observe(true, Size::new(800, 630), Size::new(800, 600));
        assert_eq!(inset.logical(1.0), Size::new(0.0, 30.0));
        inset.record(Size::new(8, 38));
        assert_eq!(inset.logical(1.0), Size::new(8.0, 38.0));
    }
}
