//! What the X11 host decides about a window before and while it exists, kept
//! apart from the host so a table of inputs checks it.
//!
//! - [`OpenPlan`] turns the creation options into what the window system is
//!   asked for: a desktop position, a content size at the scale of the display
//!   the window opens on and an initial mode, as far as the window's
//!   capabilities allow.
//! - [`FrameInset`] is the frame around the content of a decorated window,
//!   reported once a window with a frame is on screen, so a remembered outer
//!   size can be fitted before the next window exists.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{DevicePixelRatio, Point, Size};
use flui_platform::{
    DisplaySnapshot, WindowCapabilities, WindowGeometry, WindowMode, WindowOptions, WindowPosition,
    WindowReveal,
};
use parking_lot::Mutex;

use crate::window::desktop::DisplayFrame;

/// What the window system is asked for when a window is created.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OpenPlan {
    /// The outer top-left corner in the desktop's device pixels; `None` lets
    /// the window manager place the window.
    pub position: Option<Point<i32>>,
    /// The content size in logical pixels.
    pub content_size: Size<f64>,
    /// The scale the content is sized at: that of the display the window
    /// opens on, else the primary display's.
    pub scale: DevicePixelRatio,
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
    /// Whether the application asked for the window hidden (`visible: false`),
    /// which leaves it unmapped. Its opening mode is written into
    /// its properties like any other unmapped window's, and it stays unmapped
    /// until the application shows it.
    pub hidden_by_application: bool,
    /// The rectangle the window returns to from `mode`, as the window state
    /// publishes it (its position on its own display), when it opens
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

    /// Plan the creation of a window with `capabilities` from `options`, on
    /// the displays of `snapshot`.
    ///
    /// The window opens on the display its position names (the position of
    /// the rectangle it returns to, for a window that opens maximized or
    /// fullscreen), converted through that display's origin and scale
    /// ([`DisplayFrame`]); a position on a display the snapshot does not hold,
    /// or past the desktop's coordinates, places nothing, and the window
    /// manager places the window. Its content is sized at that display's
    /// scale, else the primary display's. `inset` is the frame around the
    /// content of a decorated window at a scale, which turns a remembered
    /// outer size into a content size.
    ///
    /// A window that opens maximized or fullscreen is created with the
    /// rectangle it returns to, so un-maximizing it later goes there; the
    /// window manager keeps that rectangle as the window's normal one. A mode
    /// the window cannot enter opens it normal, and a window that cannot be
    /// placed at open is left to the window manager.
    pub(crate) fn new(
        options: &WindowOptions,
        opening: &flui_platform::OpeningGeometry,
        capabilities: &WindowCapabilities,
        snapshot: Option<&DisplaySnapshot>,
        inset: impl FnOnce(DevicePixelRatio) -> Size<f64>,
    ) -> Self {
        let mode = match options.mode {
            WindowMode::Maximized if capabilities.maximizes().is_available() => {
                WindowMode::Maximized
            }
            WindowMode::Fullscreen if capabilities.enters_fullscreen().is_available() => {
                WindowMode::Fullscreen
            }
            _ => WindowMode::Normal,
        };
        let returns_to = match mode {
            WindowMode::Maximized | WindowMode::Fullscreen => options.normal_geometry,
            _ => None,
        };
        let at = match returns_to {
            Some(geometry) => geometry.position(),
            None => opening.position,
        };
        let frame_of = |position: WindowPosition| {
            snapshot.and_then(|snapshot| DisplayFrame::find(snapshot, position.display()))
        };
        let frame = at.and_then(frame_of);
        let scale = frame.map_or_else(
            || {
                snapshot
                    .and_then(DisplaySnapshot::primary)
                    .map_or(DevicePixelRatio::ONE, |display| display.scale_factor)
            },
            DisplayFrame::scale,
        );
        // A window without the system's frame has none to take from a remembered
        // outer size, whatever the framed windows before it measured.
        let inset = if options.frame.at_open(capabilities).has_system_title_bar() {
            inset(scale)
        } else {
            Size::new(0.0, 0.0)
        };
        let content_size = match returns_to {
            Some(geometry) => Size::new(
                (geometry.size().width - inset.width).max(1.0),
                (geometry.size().height - inset.height).max(1.0),
            ),
            None => opening.content.get(),
        };
        let hidden_until_first_frame =
            options.visible && options.reveal == WindowReveal::AfterFirstFrame;
        let position = if capabilities.places_at_open().is_available() {
            at.zip(frame)
                .and_then(|(at, frame)| frame.to_desktop(at.at()))
        } else {
            None
        };
        Self {
            position,
            content_size,
            scale,
            mode,
            hidden_until_first_frame,
            map_after_setup: options.visible
                && !hidden_until_first_frame
                && mode != WindowMode::Normal,
            hidden_by_application: !options.visible,
            // Its position is already measured on its own display, as the
            // window state publishes it.
            normal_geometry: returns_to,
        }
    }
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
    pub(crate) fn logical(&self, scale_factor: DevicePixelRatio) -> Size<f64> {
        let measured = *self.measured.lock();
        measured.map_or(Size::new(0.0, 0.0), |frame| {
            Size::new(
                scale_factor.to_logical(f64::from(frame.width)),
                scale_factor.to_logical(f64::from(frame.height)),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use flui_foundation::geometry::Bounds;
    use flui_platform::{
        AppTitleBar, CapabilityAbsence, DisplayId, DisplayInfo, DisplayKey, DisplayRegistry,
        WindowCommand, WindowExtent, WindowFrame,
    };

    use super::*;

    fn ratio(scale: f64) -> DevicePixelRatio {
        DevicePixelRatio::try_new(scale).expect("a positive ratio")
    }

    const ALL: WindowCapabilities = WindowCapabilities::ALL;

    /// A desktop of two displays: the primary at scale 2 with a 56-pixel
    /// panel along its top, and one at scale 1 right of it.
    struct Desk {
        snapshot: DisplaySnapshot,
        primary: DisplayId,
        right: DisplayId,
    }

    static DESK: LazyLock<Desk> = LazyLock::new(|| {
        let registry = DisplayRegistry::new();
        let display = |key, x, scale, panel| {
            let id = registry.mint(&DisplayKey::Injected(key)).expect("an id");
            let bounds = Bounds::new(Point::new(x, 0), Size::new(1920, 1080));
            let usable = Bounds::new(Point::new(x, panel), Size::new(1920, 1080 - panel));
            DisplayInfo::from_device(id, "display", bounds, usable, ratio(scale))
                .expect("a display")
        };
        let primary = display(1, 0, 2.0, 56).with_primary(true);
        let right = display(2, 1920, 1.0, 0);
        let (primary_id, right_id) = (primary.id, right.id);
        Desk {
            snapshot: DisplaySnapshot::empty()
                .successor(vec![primary, right])
                .expect("a snapshot"),
            primary: primary_id,
            right: right_id,
        }
    });

    fn on(display: DisplayId, x: f64, y: f64) -> WindowPosition {
        WindowPosition::try_new(display, Point::new(x, y)).expect("a finite point")
    }

    fn options() -> WindowOptions {
        WindowOptions::default()
            .size(WindowExtent::try_new(Size::new(800.0, 600.0)).expect("an extent"))
            .position(Some(on(DESK.right, 100.0, 50.0)))
    }

    fn geometry(position: Option<WindowPosition>, size: Size<f64>) -> WindowGeometry {
        WindowGeometry::try_new(position, size).expect("a finite rectangle")
    }

    /// The plan of `options` on the desk, with `capabilities` and a frame of
    /// `frame` logical pixels at every scale.
    fn plan_with(
        options: &WindowOptions,
        capabilities: &WindowCapabilities,
        frame: Size<f64>,
    ) -> OpenPlan {
        let opening = options.geometry().expect("valid options");
        OpenPlan::new(
            options,
            &opening,
            capabilities,
            Some(&DESK.snapshot),
            |_| frame,
        )
    }

    fn plan(options: &WindowOptions) -> OpenPlan {
        plan_with(options, &ALL, Size::new(0.0, 0.0))
    }

    /// A position is a point of its own display: it converts through that
    /// display's origin and scale, the content is sized at that display's
    /// scale, and a position on a display the snapshot does not hold, or past
    /// the desktop's coordinates, places nothing.
    #[test]
    fn open_plan_places_by_display_origin_and_scale() {
        let gone = DisplayRegistry::new()
            .mint(&DisplayKey::Injected(9))
            .expect("an id");
        // (case, position, device position, scale)
        let table = [
            (
                "on the right display at scale one",
                Some(on(DESK.right, 100.0, 50.0)),
                Some(Point::new(2020, 50)),
                1.0,
            ),
            (
                "on the primary at scale two, from its corner, not its panel",
                Some(on(DESK.primary, 10.0, 22.0)),
                Some(Point::new(20, 44)),
                2.0,
            ),
            (
                "left of the right display, on the primary's pixels",
                Some(on(DESK.right, -300.0, 10.0)),
                Some(Point::new(1620, 10)),
                1.0,
            ),
            (
                "fractions snap",
                Some(on(DESK.right, 0.4, 0.6)),
                Some(Point::new(1920, 1)),
                1.0,
            ),
            ("no position: the primary's scale", None, None, 2.0),
            (
                "a display that is gone",
                Some(on(gone, 10.0, 10.0)),
                None,
                2.0,
            ),
            (
                "scaled past every number",
                Some(on(DESK.primary, f64::MAX, 0.0)),
                None,
                2.0,
            ),
            (
                "past the coordinate range",
                Some(on(DESK.right, 3.0e9, 0.0)),
                None,
                1.0,
            ),
        ];
        for (case, position, expected, scale) in table {
            let planned = plan(&options().position(position));
            assert_eq!(planned.position, expected, "{case}");
            assert_eq!(planned.scale, ratio(scale), "{case}: scale");
        }
    }

    #[test]
    fn open_plan_leaves_placement_to_a_window_that_cannot_be_placed() {
        let mut capabilities = ALL.into_parts();
        capabilities.places_at_open = flui_platform::Availability::Unavailable;
        let capabilities = capabilities.into_capabilities();
        let planned = plan_with(&options(), &capabilities, Size::new(0.0, 0.0));
        assert_eq!(planned.position, None);
        assert_eq!(planned.content_size, Size::new(800.0, 600.0));
    }

    #[test]
    fn open_plan_modes() {
        let device = CapabilityAbsence::NOT_ON_THIS_DEVICE;
        let no_modes = ALL
            .unavailable(&WindowCommand::SetMode(WindowMode::Maximized), device)
            .unavailable(&WindowCommand::SetMode(WindowMode::Fullscreen), device);
        // (case, mode, capabilities, mode planned)
        let table = [
            ("normal", WindowMode::Normal, ALL, WindowMode::Normal),
            (
                "maximized",
                WindowMode::Maximized,
                ALL,
                WindowMode::Maximized,
            ),
            (
                "fullscreen",
                WindowMode::Fullscreen,
                ALL,
                WindowMode::Fullscreen,
            ),
            (
                "minimized opens normal",
                WindowMode::Minimized,
                ALL,
                WindowMode::Normal,
            ),
            (
                "maximized where the window manager cannot",
                WindowMode::Maximized,
                no_modes,
                WindowMode::Normal,
            ),
            (
                "fullscreen where the window manager cannot",
                WindowMode::Fullscreen,
                no_modes,
                WindowMode::Normal,
            ),
        ];
        for (case, mode, capabilities, expected) in table {
            let planned = plan_with(&options().mode(mode), &capabilities, Size::new(0.0, 0.0));
            assert_eq!(planned.mode, expected, "{case}");
        }
    }

    #[test]
    fn open_plan_opens_a_maximized_window_at_the_rectangle_it_returns_to() {
        let returns_to = geometry(Some(on(DESK.right, 300.0, 200.0)), Size::new(1000.0, 700.0));
        let frame = Size::new(2.0, 30.0);
        let maximized = options()
            .mode(WindowMode::Maximized)
            .normal_geometry(Some(returns_to));

        let planned = plan_with(&maximized, &ALL, frame);
        assert_eq!(planned.position, Some(Point::new(2220, 200)));
        assert_eq!(
            planned.content_size,
            Size::new(998.0, 670.0),
            "outer less the frame"
        );
        // The state publishes the rectangle on its own display, as the
        // options gave it.
        assert_eq!(planned.normal_geometry, Some(returns_to));

        // On a scaled display the rectangle's position converts through that
        // display's scale, and its content is sized at it.
        let on_scaled = options()
            .mode(WindowMode::Maximized)
            .normal_geometry(Some(geometry(
                Some(on(DESK.primary, 100.0, 50.0)),
                Size::new(1000.0, 700.0),
            )));
        let planned = plan(&on_scaled);
        assert_eq!(planned.position, Some(Point::new(200, 100)));
        assert_eq!(planned.scale, ratio(2.0));

        // A normal window ignores the rectangle it has no use for.
        let normal = options().normal_geometry(Some(returns_to));
        let planned = plan_with(&normal, &ALL, frame);
        assert_eq!(planned.position, Some(Point::new(2020, 50)));
        assert_eq!(planned.content_size, Size::new(800.0, 600.0));
        assert_eq!(planned.normal_geometry, None);

        // A rectangle smaller than the frame still has a content area.
        let tiny = options()
            .mode(WindowMode::Fullscreen)
            .normal_geometry(Some(geometry(None, Size::new(1.0, 1.0))));
        let planned = plan_with(&tiny, &ALL, frame);
        assert_eq!(planned.content_size, Size::new(1.0, 1.0));
        assert_eq!(
            planned.position, None,
            "its rectangle has no position either"
        );
    }

    #[test]
    fn open_plan_takes_no_frame_from_a_window_without_the_systems() {
        let returns_to = geometry(Some(on(DESK.right, 300.0, 200.0)), Size::new(1000.0, 700.0));
        let frame = Size::new(2.0, 30.0);
        // (case, frame asked for, content size)
        let table = [
            ("system", WindowFrame::System, Size::new(998.0, 670.0)),
            (
                "app title bar, drawn without decorations",
                WindowFrame::AppTitleBar(AppTitleBar::new()),
                Size::new(1000.0, 700.0),
            ),
            ("none", WindowFrame::None, Size::new(1000.0, 700.0)),
        ];
        for (case, frame_asked, expected) in table {
            let options = options()
                .frame(frame_asked)
                .mode(WindowMode::Maximized)
                .normal_geometry(Some(returns_to));
            let planned = plan_with(&options, &ALL, frame);
            assert_eq!(planned.content_size, expected, "{case}");
        }
    }

    #[test]
    fn open_plan_maps_a_window_that_opens_in_a_mode_after_setting_it_up() {
        // (case, visible, reveal, mode, hidden until first frame, mapped after setup)
        let table = [
            (
                "normal at open",
                true,
                WindowReveal::AtOpen,
                WindowMode::Normal,
                false,
                false,
            ),
            (
                "maximized at open",
                true,
                WindowReveal::AtOpen,
                WindowMode::Maximized,
                false,
                true,
            ),
            (
                "fullscreen at open",
                true,
                WindowReveal::AtOpen,
                WindowMode::Fullscreen,
                false,
                true,
            ),
            (
                "maximized after the first frame",
                true,
                WindowReveal::AfterFirstFrame,
                WindowMode::Maximized,
                true,
                false,
            ),
            (
                "maximized, opened hidden",
                false,
                WindowReveal::AtOpen,
                WindowMode::Maximized,
                false,
                false,
            ),
        ];
        for (case, visible, reveal, mode, hidden, mapped) in table {
            let options = options().visible(visible).reveal(reveal).mode(mode);
            let planned = plan(&options);
            assert_eq!(planned.hidden_until_first_frame, hidden, "{case}");
            assert_eq!(planned.map_after_setup, mapped, "{case}");
            assert_eq!(planned.hidden_by_application, !visible, "{case}");
            assert_eq!(
                planned.unmapped_until_shown(),
                hidden || mapped || !visible,
                "{case}"
            );
        }
    }

    #[test]
    fn open_plan_defers_the_reveal_only_where_it_can() {
        // (case, visible, reveal, hidden until the first frame)
        let table = [
            (
                "after first frame",
                true,
                WindowReveal::AfterFirstFrame,
                true,
            ),
            ("at open", true, WindowReveal::AtOpen, false),
            ("opened hidden", false, WindowReveal::AfterFirstFrame, false),
        ];
        for (case, visible, reveal, expected) in table {
            let options = options().visible(visible).reveal(reveal);
            assert_eq!(plan(&options).hidden_until_first_frame, expected, "{case}");
        }
    }

    #[test]
    fn frame_inset_is_recorded_from_a_framed_window_on_screen() {
        let inset = FrameInset::default();
        assert_eq!(
            inset.logical(DevicePixelRatio::ONE),
            Size::new(0.0, 0.0),
            "nothing recorded yet"
        );
        assert!(!inset.is_known());
        for (case, frame) in [
            ("a frame of nothing", Size::new(0, 0)),
            ("a negative width", Size::new(-1, 10)),
            ("a negative height", Size::new(10, -1)),
        ] {
            inset.record(frame);
            assert!(!inset.is_known(), "{case}");
        }

        inset.record(Size::new(20, 60));
        assert_eq!(inset.logical(DevicePixelRatio::ONE), Size::new(20.0, 60.0));
        assert!(inset.is_known());
        assert_eq!(
            inset.logical(ratio(2.0)),
            Size::new(10.0, 30.0),
            "device pixels over scale"
        );

        // A frame of one dimension only (a bar and no borders) is a frame, and a
        // later report replaces an earlier one.
        inset.record(Size::new(0, 30));
        assert_eq!(inset.logical(DevicePixelRatio::ONE), Size::new(0.0, 30.0));
        inset.record(Size::new(8, 38));
        assert_eq!(inset.logical(DevicePixelRatio::ONE), Size::new(8.0, 38.0));
    }
}
