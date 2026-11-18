//! A window's snapshot: its capabilities, geometry, state, insets and how a close went.

use super::Absence;

vocabulary! {
    /// How a window occupies its display.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum WindowMode {
        /// Its own bounds.
        Normal => "normal",
        /// Minimized.
        Minimized => "minimized",
        /// Filling the display's usable area.
        Maximized => "maximized",
        /// Borderless on the whole display.
        Fullscreen => "fullscreen",
    }
}

vocabulary! {
    /// The user's theme as applied to a window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Appearance {
        /// Light.
        Light => "light",
        /// Dark.
        Dark => "dark",
        /// Light, with vibrancy.
        VibrantLight => "vibrant_light",
        /// Dark, with vibrancy.
        VibrantDark => "vibrant_dark",
    }
}

vocabulary! {
    /// Whether the platform lets a window run its UI.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeExecution"))]
    pub enum Execution {
        /// Running.
        Running => "running",
        /// Suspended.
        Suspended => "suspended",
        /// Detached from its surface.
        Detached => "detached",
    }
}

vocabulary! {
    /// How a window honours a command or makes a report.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeAvailability"))]
    pub enum Availability {
        /// The window system does it.
        Native => "native",
        /// The backend builds it from what the window system provides.
        Emulated => "emulated",
        /// Not at all.
        Unavailable => "unavailable",
    }
}

/// How a window honours a command it is admitted against, or why it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Capability {
    /// The window system does it.
    Native,
    /// The backend builds it from what the window system provides.
    Emulated,
    /// Not at all, for this reason.
    Unavailable(Absence),
}

impl Capability {
    /// The capability a recording that predates a field reads as.
    #[must_use]
    pub const fn platform_absence() -> Self {
        Self::Unavailable(Absence::NotOnThisPlatform)
    }

    /// Whether this is [`platform_absence`](Self::platform_absence), which a
    /// writer leaves out.
    #[must_use]
    pub const fn is_platform_absence(&self) -> bool {
        matches!(self, Self::Unavailable(Absence::NotOnThisPlatform))
    }
}

/// Which commands a window honours, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeCapabilities")
)]
pub struct Capabilities {
    /// Setting the title.
    pub sets_title: Capability,
    /// Moving the window.
    pub sets_position: Capability,
    /// Resizing the window.
    pub sets_size: Capability,
    /// Setting its size limits.
    pub sets_constraints: Capability,
    /// Making it resizable or not.
    pub sets_resizable: Capability,
    /// Giving it the system's frame or none.
    pub sets_frame: Capability,
    /// Giving it a title bar the application draws.
    pub app_title_bar: Capability,
    /// Minimizing it.
    pub minimizes: Capability,
    /// Maximizing it.
    pub maximizes: Capability,
    /// Making it fullscreen.
    pub enters_fullscreen: Capability,
    /// Hiding it.
    pub hides: Capability,
    /// Showing it.
    pub shows: Capability,
    /// Activating it.
    pub activates: Capability,
    /// Closing it.
    pub closes: Capability,
    /// Setting its material.
    pub material: Capability,
    /// Holding the pointer inside it; a recording made before the field
    /// existed reads as the platform's absence, as the fields below do.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub confines_cursor: Capability,
    /// Holding the pointer in place.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub locks_cursor: Capability,
    /// Keeping it above the other windows.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub keeps_on_top: Capability,
    /// Keeping it below the other windows.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub keeps_on_bottom: Capability,
    /// Resizing it in steps.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub sets_resize_increments: Capability,
    /// Keeping its proportion while resized.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub sets_aspect_ratio: Capability,
    /// Reporting that its frames reached the screen.
    pub reports_presentation: Capability,
    /// Reporting where the soft keyboard covers it; a recording made before
    /// the field existed reads as the platform's absence.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub reports_keyboard_inset: Capability,
    /// Reporting the edges system gestures take; read as
    /// [`reports_keyboard_inset`](Self::reports_keyboard_inset) is.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub reports_system_gestures: Capability,
    /// Reporting a covering by other windows as occluded; read as
    /// [`reports_keyboard_inset`](Self::reports_keyboard_inset) is.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub reports_occlusion: Capability,
    /// Reporting its position.
    pub observes_position: Availability,
    /// Reporting that it is minimized.
    pub observes_minimized: Availability,
    /// Placing it where asked when it opens.
    pub places_at_open: Availability,
    /// Taking a frame rate hint.
    pub frame_rate_hint: Availability,
    /// Showing a cursor image of the application's; a recording made before
    /// the field existed reads as unavailable.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Availability::unavailable",
            skip_serializing_if = "Availability::is_unavailable"
        )
    )]
    pub cursor_image: Availability,
}

impl Availability {
    /// What a recording that predates an availability field reads as.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self::Unavailable
    }

    /// Whether this is [`Unavailable`](Self::Unavailable), which a writer
    /// leaves out of a field older recordings lack.
    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

/// A window rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowGeometry {
    /// The top-left corner in global coordinates, where the OS reports one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub position: Option<[f64; 2]>,
    /// Width and height.
    pub size: [f64; 2],
}

/// A window's inner-size limits in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SizeConstraints {
    /// The smallest inner size, if limited.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub min: Option<[f64; 2]>,
    /// The largest inner size, if limited.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max: Option<[f64; 2]>,
}

/// What frames a window.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[expect(
    clippy::exhaustive_enums,
    reason = "a pinned mirror of the host's window vocabulary: a new variant is a format version, and a `_` arm would drop a recorded line"
)]
pub enum WindowFrame {
    /// The system's title bar and borders.
    System,
    /// A title bar the application draws, inside a frame the system keeps.
    AppTitleBar {
        /// The system's own controls over it.
        system_controls: SystemControls,
    },
    /// No frame.
    None,
}

/// The controls a system draws over a title bar the application draws.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[expect(
    clippy::exhaustive_enums,
    reason = "a pinned mirror of the host's window vocabulary: a new variant is a format version, and a `_` arm would drop a recorded line"
)]
pub enum SystemControls {
    /// Where the system puts them.
    Shown,
    /// Moved from there, in logical pixels: along the bar toward its trailing edge, and down.
    Offset([f64; 2]),
    /// Not drawn.
    Hidden,
}

/// A window's drawable area and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowMetrics {
    /// The drawable area in device pixels: width and height.
    pub physical: [i32; 2],
    /// Device pixels per logical pixel.
    pub scale: f64,
}

/// What the operating system reported about one window, at one revision.
///
/// `display` is the display's label, as a `platform` recording of the same session writes it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowState {
    /// The revision the window published it at.
    pub revision: u64,
    /// The outer rectangle.
    pub outer: WindowGeometry,
    /// The content area's size: width and height.
    pub content_size: [f64; 2],
    /// How it occupies its display.
    pub mode: WindowMode,
    /// The rectangle it returns to from another mode, where known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub normal_geometry: Option<WindowGeometry>,
    /// Whether it is shown.
    pub visible: bool,
    /// Whether the window system says nothing of it reaches the screen.
    pub occluded: bool,
    /// Whether it has keyboard focus.
    pub focused: bool,
    /// Its title.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub title: Option<String>,
    /// Its inner-size limits.
    #[cfg_attr(feature = "serde", serde(default))]
    pub constraints: SizeConstraints,
    /// Whether the user can resize it.
    pub resizable: bool,
    /// What frames it.
    pub frame: WindowFrame,
    /// The display it is on, by label.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub display: Option<u64>,
    /// Its drawable area and scale, once reported.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub metrics: Option<WindowMetrics>,
    /// The theme applied to it.
    pub appearance: Appearance,
    /// The safe-area insets in logical pixels: top, right, bottom, left.
    pub safe_area: [f64; 4],
    /// The soft keyboard's occlusion and its motion; absent while it is hidden
    /// and at rest.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub keyboard: Option<KeyboardInset>,
    /// The edges system gestures take, in logical pixels: top, right, bottom,
    /// left; absent while zero.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "is_zero_insets")
    )]
    pub system_gestures: [f64; 4],
    /// Whether the platform lets it run its UI.
    pub execution: Execution,
    /// Whether the pointer is over it.
    pub hovered: bool,
    /// Whether it is the active window of its desktop.
    pub active: bool,
    /// Whether the user is placing it through the window system, where the
    /// host can tell; absent where it cannot, and in recordings older than
    /// the fact, which a reader takes as not the user.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub user_placing: Option<bool>,
    /// Whether the placing the user does is a resize by the frame; absent
    /// while it is not, and in recordings older than the fact.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub user_resizing: bool,
    /// Whether it holds the pointer; absent while it holds none, and in
    /// recordings older than the fact.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub cursor_grab: Option<CursorGrab>,
    /// Where it stands among the other windows; absent at the normal level,
    /// and in recordings older than the fact.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub level: Option<WindowLevel>,
    /// Which commands it honours.
    pub capabilities: Capabilities,
}

vocabulary! {
    /// How a window holds the pointer, when it holds it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeCursorGrab"))]
    pub enum CursorGrab {
        /// Inside its content area.
        Confined => "confined",
        /// In place, its motion reported raw.
        Locked => "locked",
    }
}

vocabulary! {
    /// Where a window stands among the others, when not at the normal level.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeWindowLevel"))]
    pub enum WindowLevel {
        /// Above every normal window.
        AlwaysOnTop => "always_on_top",
        /// Below every normal window.
        AlwaysOnBottom => "always_on_bottom",
    }
}

/// Whether all four insets are zero, which a writer leaves out.
#[cfg(feature = "serde")]
fn is_zero_insets(insets: &[f64; 4]) -> bool {
    insets.iter().all(|inset| *inset == 0.0)
}

/// Where the soft keyboard covers a window, and how it gets there.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeyboardInset {
    /// The occlusion once any motion ends, in logical pixels: top, right,
    /// bottom, left.
    pub target: [f64; 4],
    /// The motion toward `target`; absent at rest.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub motion: Option<InsetMotion>,
}

/// A motion of the keyboard on the event timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InsetMotion {
    /// The occlusion it starts from: top, right, bottom, left.
    pub from: [f64; 4],
    /// When it starts on the event timeline, in nanoseconds.
    pub start_ns: u64,
    /// How long it lasts, in nanoseconds.
    pub duration_ns: u64,
    /// Its timing curve.
    pub curve: InsetCurve,
}

/// A keyboard motion's timing curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "curve", rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum InsetCurve {
    /// Progress proportional to time.
    Linear,
    /// A cubic Bézier through (0,0), the control points and (1,1).
    CubicBezier {
        /// The first control point's time.
        x1: f64,
        /// The first control point's progress.
        y1: f64,
        /// The second control point's time.
        x2: f64,
        /// The second control point's progress.
        y2: f64,
    },
    /// UIKit's keyboard curve.
    SystemKeyboard,
}

vocabulary! {
    /// What a window's handler answered when the window system asked to close the window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CloseAnswer {
        /// The handler granted it: the window closes.
        Close => "close",
        /// The handler vetoed it (the application's close guard held it): the window stays
        /// open.
        KeepOpen => "keep_open",
        /// An earlier request was granted and the window is closing: the handler was not asked.
        Closing => "closing",
        /// The window had let go of its handler: the window closes.
        NoHandler => "no_handler",
        /// The handler panicked while it answered: the panic went on and nothing was decided.
        Panicked => "panicked",
    }
}

vocabulary! {
    /// Why a window closed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CloseCause {
        /// Its handler granted a close request.
        GrantedRequest => "granted_request",
        /// Nobody was asked: the application's close command, or the system taking the window
        /// away.
        NoRequest => "no_request",
    }
}
