//! The window state contract: an immutable snapshot a window publishes, the
//! commands that change it and the capabilities that say which commands the
//! window honours (ADR-0237, ADR-0254).
//!
//! # Snapshot
//!
//! [`WindowState`] describes what the operating system reports about a window,
//! never what code asked for: a size the OS clamped is the clamped size, and a
//! maximize the OS refused leaves the mode where it was. It holds every fact of
//! the window (ADR-0254 §1): geometry and mode, the drawable metrics and scale,
//! appearance, display, insets, execution state, focus, activation, visibility,
//! occlusion, hover and the [`WindowCapabilities`]. Whether the window is shown
//! ([`visible`](WindowState::visible)), minimized ([`mode`](WindowState::mode))
//! and covered ([`occluded`](WindowState::occluded)) are three facts, and
//! [`WindowState::is_seen`] is the one reading of all three. A change publishes a new snapshot with a
//! higher [`WindowRevision`] and one payload-free `WindowHandler::state_changed`
//! notification; a sample equal to the published one keeps the revision
//! ([`WindowState::successor`]). The revision never wraps: the last one is
//! reached once, then the snapshot refuses further change.
//!
//! # Commands
//!
//! Every change is a [`WindowCommand`]. It reaches a window only as an
//! [`AdmittedCommand`], which [`WindowCommand::admit`] alone builds: the
//! command's numbers describe a window, and the capabilities it was admitted
//! against honour it. Admission refuses with [`AdmissionError::Unsupported`]
//! exactly when the matching [`WindowCapabilities`] field is
//! [`Absent`](WindowCapability::Absent), so a caller can ask up front and is
//! never told "done" for a no-op. [`PlatformWindow::apply`](crate::PlatformWindow::apply)
//! takes the admitted command and answers only what the world can cause
//! ([`ApplyError`]). An applied command is a request: its outcome is visible
//! only in the next snapshot.

use flui_foundation::geometry::{Size, WindowMetrics};

use crate::window::capabilities::Publication;
use crate::{
    CapabilityParts, CursorGrab, DisplayId, InvalidGeometry, Unsupported, WindowCapabilities,
    WindowCapability, WindowExecutionState, WindowExtent, WindowFrame, WindowGeometry,
    WindowInsets, WindowPosition,
};

/// The monotone version of a [`WindowState`]. Never wraps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowRevision(u64);

impl WindowRevision {
    /// The revision of a window that has published nothing yet.
    pub const ZERO: Self = Self(0);

    /// The revision number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The revision after this one, or `None` at the last.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }

    /// The revision `raw`, behind the `testing` feature: a test that drives a
    /// window near its last revision starts there instead of publishing every
    /// revision before it.
    #[cfg(feature = "testing")]
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The last revision a window can publish.
    pub const LAST: Self = Self(u64::MAX);
}

/// A window state cannot advance: the last revision was reached. Running
/// out of capability publications is no such failure: the window keeps
/// publishing its other facts with its capabilities withdrawn
/// ([`WindowCapabilities::withdraw`], ADR-0341).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("window revisions exhausted")]
pub struct WindowRevisionExhausted;

/// How a window occupies its display.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowMode {
    /// A window with its own bounds (the default).
    #[default]
    Normal,
    /// Iconified or minimized to the task bar or dock.
    Minimized,
    /// Filling the display's usable area, with its frame.
    Maximized,
    /// Borderless on the whole display. Exclusive video-mode changes are not
    /// part of this contract.
    Fullscreen,
}

/// Where a window stands among the other windows of its desktop
/// ([`WindowCommand::SetLevel`], [`WindowState::level`]).
///
/// A level is a fact of the snapshot, not only a request: the user can change
/// it from the window manager's menu (KDE's and GNOME's "Always on Top"), and
/// another program can change a Win32 window's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a window is kept above the others, below them, or neither; a level one OS alone has is that OS's extension"
)]
pub enum WindowLevel {
    /// Stacked with the other windows as the user activates them (the default).
    #[default]
    Normal,
    /// Kept above every normal window, active or not: a picture-in-picture
    /// player, a palette that stays in view.
    AlwaysOnTop,
    /// Kept below every normal window: a desktop widget.
    AlwaysOnBottom,
}

/// The proportion a window's content keeps while the user resizes it
/// ([`WindowCommand::SetAspectRatio`]): width to height, held in lowest
/// terms, so 32:18 and 16:9 are one ratio.
///
/// ```
/// use std::num::NonZeroU32;
///
/// use flui_platform::AspectRatio;
///
/// let wide = |w, h| AspectRatio::new(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap());
/// assert_eq!(wide(32, 18), wide(16, 9));
/// assert_eq!((wide(32, 18).width().get(), wide(32, 18).height().get()), (16, 9));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    width: std::num::NonZeroU32,
    height: std::num::NonZeroU32,
}

impl AspectRatio {
    /// The ratio `width:height`, reduced to lowest terms.
    #[must_use]
    pub const fn new(width: std::num::NonZeroU32, height: std::num::NonZeroU32) -> Self {
        let (mut a, mut b) = (width.get(), height.get());
        while b != 0 {
            (a, b) = (b, a % b);
        }
        // `a` is the greatest common divisor of two non-zero numbers: at
        // least 1, and it divides both, so each quotient is at least 1.
        match (
            std::num::NonZeroU32::new(width.get() / a),
            std::num::NonZeroU32::new(height.get() / a),
        ) {
            (Some(width), Some(height)) => Self { width, height },
            _ => panic!("BUG: a quotient by a common divisor of non-zero numbers is non-zero"),
        }
    }

    /// The width term, in lowest terms.
    #[must_use]
    pub const fn width(self) -> std::num::NonZeroU32 {
        self.width
    }

    /// The height term, in lowest terms.
    #[must_use]
    pub const fn height(self) -> std::num::NonZeroU32 {
        self.height
    }

    /// Width over height, the proportion a window system that takes a
    /// floating-point ratio is given.
    #[must_use]
    pub fn get(self) -> f64 {
        f64::from(self.width.get()) / f64::from(self.height.get())
    }
}

/// The inner-size limits of a window, in logical pixels.
///
/// Each bound is a [`WindowExtent`], so a limit is finite and not negative
/// wherever it is held. That a limit is positive and the minimum does not
/// exceed the maximum is asked of a command
/// ([`WindowCommand::validate`]), not of what a window system reports: a
/// snapshot shows the limits as the window system holds them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeConstraints {
    /// The smallest inner size, if limited.
    pub min: Option<WindowExtent>,
    /// The largest inner size, if limited.
    pub max: Option<WindowExtent>,
}

impl SizeConstraints {
    /// No limits.
    pub const NONE: Self = Self {
        min: None,
        max: None,
    };

    /// Limits with these bounds.
    #[must_use]
    pub const fn new(min: Option<WindowExtent>, max: Option<WindowExtent>) -> Self {
        Self { min, max }
    }

    /// Limits with these bounds, each read from a size.
    ///
    /// # Errors
    /// [`InvalidGeometry::Extent`] when a bound has a side that is NaN,
    /// infinite or negative.
    pub fn try_new(
        min: Option<Size<f64>>,
        max: Option<Size<f64>>,
    ) -> Result<Self, InvalidGeometry> {
        Ok(Self {
            min: min.map(WindowExtent::try_new).transpose()?,
            max: max.map(WindowExtent::try_new).transpose()?,
        })
    }

    /// These limits, when a window may be asked to hold them: each encloses
    /// an area and the minimum does not exceed the maximum. The one check a
    /// command ([`WindowCommand::validate`]) and an opening
    /// ([`WindowOptions::geometry`](crate::WindowOptions::geometry))
    /// both make.
    ///
    /// # Errors
    /// [`InvalidGeometry::Empty`] for a limit with a zero side,
    /// [`InvalidGeometry::Limits`] when the minimum exceeds the maximum in
    /// either dimension.
    pub(crate) fn asked(self) -> Result<Self, InvalidGeometry> {
        let min = self.min.map(WindowExtent::area).transpose()?;
        let max = self.max.map(WindowExtent::area).transpose()?;
        if let (Some(min), Some(max)) = (min, max)
            && (min.width() > max.width() || min.height() > max.height())
        {
            return Err(InvalidGeometry::Limits { min, max });
        }
        Ok(self)
    }
}

/// Whether the user is placing a window through the window system: a fact the
/// host states, never one inferred from what changed (ADR-0343).
///
/// Code that keeps a window's placement for the user (`AppConfig::restore_placement`)
/// tells a change the user made from one the host or the application made by
/// this alone, read on the report that carries the change: only
/// [`Self::UserPlacing`] there counts as the user's, and a host that cannot
/// tell says [`Self::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum WindowInteraction {
    /// The host cannot tell whether the user is placing the window: the
    /// value of a host that reads no signal for it. A consumer treats it as
    /// not the user.
    #[default]
    Unknown,
    /// The host can tell, and no placement by the user is under way.
    None,
    /// The user is placing the window through the window system: moving it,
    /// or changing its mode from the frame. Win32 states it inside its
    /// size-move loop while the user drags the window (by the frame or the
    /// system menu's move) and while the default procedure carries out
    /// `SC_MAXIMIZE`, `SC_MINIMIZE` or `SC_RESTORE` the user worked; the
    /// headless host states what its test simulates.
    UserPlacing,
    /// The user is resizing the window by its frame: a live resize, from the
    /// first size the drag asks for to the release. A size change the user is
    /// making, like [`Self::UserPlacing`], and also what an application that
    /// throttles costly work during a resize waits out (AppKit's
    /// `windowWillStartLiveResize`, Win32's `WM_SIZING` inside the size-move
    /// loop, a Wayland toplevel's `resizing` state).
    UserResizing,
}

impl WindowInteraction {
    /// Whether the user is changing the window through the window system:
    /// placing it ([`Self::UserPlacing`]) or resizing it
    /// ([`Self::UserResizing`]). [`Self::Unknown`] counts as not the user.
    #[must_use]
    pub const fn is_user(self) -> bool {
        matches!(self, Self::UserPlacing | Self::UserResizing)
    }
}

/// Where a window stands in its desktop's input: whether it is the active
/// window and whether it holds the keyboard itself, as one fact.
///
/// Focus implies activation, so a window that holds the keyboard and is not
/// active is not a value of this type. A window system that reports the two
/// apart may report them out of step for a moment (Win32 deactivates a window
/// before it takes the focus away; an X11 window manager updates
/// `_NET_ACTIVE_WINDOW` after the focus moved): [`Self::reported`] is the one
/// rule every host reads such a pair through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a window is inactive, active, or active with the keyboard; there is no fourth standing"
)]
pub enum WindowFocus {
    /// Another window is the active one, or none is.
    #[default]
    Inactive,
    /// The active window of its desktop, the one the user works in, with the
    /// keyboard held by a window it owns (a Win32 window whose owned popup has
    /// the focus).
    Active,
    /// The active window, holding the keyboard itself.
    Focused,
}

impl WindowFocus {
    /// The standing a window system's two reports describe: the keyboard
    /// focus wins, since the window that receives the keys is the one the
    /// user works in, and a window system with no notion of activation apart
    /// from focus passes its focus as both.
    #[must_use]
    pub const fn reported(focused: bool, active: bool) -> Self {
        match (focused, active) {
            (true, _) => Self::Focused,
            (false, true) => Self::Active,
            (false, false) => Self::Inactive,
        }
    }

    /// Whether the window holds the keyboard itself.
    #[must_use]
    pub const fn is_focused(self) -> bool {
        matches!(self, Self::Focused)
    }

    /// Whether the window is the active window of its desktop, holding the
    /// keyboard itself or through a window it owns.
    #[must_use]
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::Inactive)
    }
}

/// What the operating system reports about one window, at one revision.
///
/// Built with [`new`](Self::new) and the `with_*` methods so a field can be
/// added without breaking a backend.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WindowState {
    /// This snapshot's version.
    pub revision: WindowRevision,
    /// The window's outer rectangle (frame included). Its position, where the
    /// window system reports one, is measured on the window's
    /// [`display`](Self::display), which [`successor`](Self::successor)
    /// holds: the display a sample's position names is the display the
    /// window is on.
    pub outer: WindowGeometry,
    /// The content (client) area extent.
    pub content_size: WindowExtent,
    /// How the window occupies its display: a mode the window settled in.
    /// A command that changes the mode ([`WindowCommand::SetMode`], a
    /// [`WindowCommand::SetSize`] of a maximized or fullscreen window) and
    /// that the window system carries out in several steps (a window manager
    /// or compositor answering in turn, a full-screen animation) changes it
    /// once, from the mode the window left to the mode asked; until the
    /// window system reaches that mode, or the host stops waiting for it, a
    /// snapshot keeps the mode the window left while its geometry and metrics
    /// follow the window. The one exception is a window on its way out of
    /// [`WindowMode::Minimized`]: no snapshot says minimized of a window on
    /// screen, so the modes it passes through are published.
    pub mode: WindowMode,
    /// The rectangle the window returns to from a maximized, fullscreen or
    /// minimized mode, when the OS reports it. Its position names the display
    /// it lies on, which need not be the one the window is on now.
    ///
    /// A snapshot in normal mode names none, and its
    /// [`outer`](Self::outer) rectangle is the window's place, except while
    /// the window system carries out a mode change in steps (see
    /// [`mode`](Self::mode)). Then a snapshot that keeps the mode the window
    /// left names the rectangle the window returned to as the change began,
    /// also when that mode is normal, and so does a snapshot in normal mode
    /// of a window on its way there from another mode: the outer rectangle
    /// of either may be a step's (a window still at its fullscreen size).
    /// [`WindowPlacement::from_state`](crate::WindowPlacement::from_state)
    /// reads the window's place by this rule.
    pub normal_geometry: Option<WindowGeometry>,
    /// Whether the window is shown: opened visible or shown since, and not
    /// hidden since ([`WindowCommand::Show`], [`WindowCommand::Hide`]). It is
    /// the window's own show state as its window system holds it (Win32's
    /// `WS_VISIBLE`, an X11 window not withdrawn, AppKit's `isVisible` or
    /// `isMiniaturized`), so minimizing a window or covering it leaves it
    /// `true`: those are its [`mode`](Self::mode) and
    /// [`occluded`](Self::occluded). Where the application cannot hide its
    /// window (a Wayland toplevel, a mobile window, a page's canvas) it is
    /// shown while it is open.
    pub visible: bool,
    /// Whether the window system says none of a shown, unminimized window
    /// reaches the screen: covered by other windows, on another workspace or
    /// space, cloaked, behind a locked session, a background page or app, a
    /// toplevel the compositor suspended. `false` where the window system does
    /// not tell: whether a covering by other windows is told is the
    /// capabilities' [`reports_occlusion`](WindowCapabilities::reports_occlusion).
    /// Always `false` for a hidden or minimized window, which
    /// [`successor`](Self::successor) holds: those are their own facts.
    pub occluded: bool,
    /// Whether the window is the active window of its desktop and whether it
    /// holds the keyboard itself. Always [`WindowFocus::Inactive`] for a
    /// hidden window, which [`successor`](Self::successor) holds.
    pub focus: WindowFocus,
    /// The title; `None` where the OS has no window title.
    pub title: Option<String>,
    /// The inner-size limits the OS enforces.
    pub constraints: SizeConstraints,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// What frames the window: the system's title bar, the application's, or none.
    pub frame: WindowFrame,
    /// The display the window is on, named by the display snapshot's identity
    /// (ADR-0236): the one that holds the largest part of it. `None` when the
    /// platform cannot tell, which a consumer reads as an unknown display,
    /// never as the primary one; a window with no display has no position
    /// either, since a position is measured on one.
    pub display: Option<DisplayId>,
    /// The drawable area in device pixels and its ratio to logical pixels, as
    /// the window last reported a drawable one; `None` until it has. A report
    /// that is not a drawable area (a minimized window's zero size, a scale
    /// that is not finite and positive) is never published:
    /// [`successor`](Self::successor) keeps the metrics before it. The ratio
    /// is the window's own, not its [`display`](Self::display)'s: a window
    /// across displays of different scales may be drawn for the densest
    /// while it names the display it is on.
    pub metrics: Option<WindowMetrics>,
    /// Whether the theme the system applies to the window is light or dark:
    /// the user's theme as applied to it, never one the backend chose for its
    /// own chrome (ADR-0222). A backend that cannot observe the theme reports
    /// [`Brightness::Light`](crate::Brightness::Light). What the window system
    /// draws behind the content is its material, not its theme
    /// ([`WindowCommand::SetMaterial`]).
    pub brightness: crate::Brightness,
    /// Where the content is covered or contested: the safe area, the soft
    /// keyboard and its motion, and the system-gesture edges (ADR-0277).
    /// Zero where the platform reports none; whether a window reports the
    /// keyboard and the gesture edges at all is in its
    /// [`capabilities`](Self::capabilities).
    pub insets: WindowInsets,
    /// Whether the platform lets the window run its UI, independent of focus,
    /// visibility and the GPU surface (ADR-0035).
    pub execution: WindowExecutionState,
    /// Whether the pointer is over the window's content.
    pub hovered: bool,
    /// Whether the user is placing or resizing the window through the window
    /// system right now ([`WindowInteraction`]); [`WindowInteraction::Unknown`]
    /// where the host cannot tell.
    pub interaction: WindowInteraction,
    /// Whether the window holds the pointer ([`WindowCommand::SetCursorGrab`]).
    /// Always [`CursorGrab::None`] for a window without the keyboard focus,
    /// which [`successor`](Self::successor) holds: a window lets its grab go
    /// when it loses the focus, and the snapshot shows that it did.
    pub cursor_grab: CursorGrab,
    /// Where the window stands among the other windows of its desktop
    /// ([`WindowCommand::SetLevel`]). [`WindowLevel::Normal`] where the window
    /// system has no levels.
    pub level: WindowLevel,
    /// Which commands the window honours. A change in what it honours (a
    /// compositor revising what it allows) is a state change like any other.
    pub capabilities: WindowCapabilities,
}

impl WindowState {
    /// A visible, unoccluded, inactive, unhovered, running, resizable,
    /// system-framed window in normal mode at revision zero, with no title, limits, display,
    /// metrics or insets, a light theme, an interaction the host cannot
    /// tell, no grab, the normal level, and capabilities that honour nothing.
    #[must_use]
    pub fn new(outer: WindowGeometry, content_size: WindowExtent) -> Self {
        Self {
            revision: WindowRevision::ZERO,
            outer,
            content_size,
            mode: WindowMode::Normal,
            normal_geometry: None,
            visible: true,
            occluded: false,
            focus: WindowFocus::Inactive,
            title: None,
            constraints: SizeConstraints::NONE,
            resizable: true,
            frame: WindowFrame::System,
            display: None,
            metrics: None,
            brightness: crate::Brightness::Light,
            insets: WindowInsets::ZERO,
            execution: WindowExecutionState::Running,
            hovered: false,
            interaction: WindowInteraction::Unknown,
            cursor_grab: CursorGrab::None,
            level: WindowLevel::Normal,
            capabilities: WindowCapabilities::NONE,
        }
    }

    /// Set the mode.
    #[must_use]
    pub fn with_mode(mut self, mode: WindowMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the rectangle the window returns to from maximized or fullscreen.
    #[must_use]
    pub fn with_normal_geometry(mut self, normal_geometry: Option<WindowGeometry>) -> Self {
        self.normal_geometry = normal_geometry;
        self
    }

    /// Set whether the window is shown.
    #[must_use]
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set whether the window system says none of the window reaches the
    /// screen.
    #[must_use]
    pub fn with_occluded(mut self, occluded: bool) -> Self {
        self.occluded = occluded;
        self
    }

    /// Set whether the window is active and whether it holds the keyboard.
    #[must_use]
    pub fn with_focus(mut self, focus: WindowFocus) -> Self {
        self.focus = focus;
        self
    }

    /// Set the title.
    #[must_use]
    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title;
        self
    }

    /// Set the size limits.
    #[must_use]
    pub fn with_constraints(mut self, constraints: SizeConstraints) -> Self {
        self.constraints = constraints;
        self
    }

    /// Set whether the user can resize the window.
    #[must_use]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set what frames the window.
    #[must_use]
    pub fn with_frame(mut self, frame: WindowFrame) -> Self {
        self.frame = frame;
        self
    }

    /// Set the display.
    #[must_use]
    pub fn with_display(mut self, display: Option<DisplayId>) -> Self {
        self.display = display;
        self
    }

    /// Set the drawable metrics; `None` for a report that is not a drawable
    /// area, which [`successor`](Self::successor) does not publish.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Option<WindowMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    /// Set whether the theme the system applies to the window is light or dark.
    #[must_use]
    pub fn with_brightness(mut self, brightness: crate::Brightness) -> Self {
        self.brightness = brightness;
        self
    }

    /// Set the insets.
    #[must_use]
    pub fn with_insets(mut self, insets: WindowInsets) -> Self {
        self.insets = insets;
        self
    }

    /// Set the execution state.
    #[must_use]
    pub fn with_execution(mut self, execution: WindowExecutionState) -> Self {
        self.execution = execution;
        self
    }

    /// Set whether the pointer is over the window.
    #[must_use]
    pub fn with_hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Set whether the user is placing the window through the window system.
    #[must_use]
    pub fn with_interaction(mut self, interaction: WindowInteraction) -> Self {
        self.interaction = interaction;
        self
    }

    /// Set whether the window holds the pointer.
    #[must_use]
    pub fn with_cursor_grab(mut self, cursor_grab: CursorGrab) -> Self {
        self.cursor_grab = cursor_grab;
        self
    }

    /// Set where the window stands among the other windows.
    #[must_use]
    pub fn with_level(mut self, level: WindowLevel) -> Self {
        self.level = level;
        self
    }

    /// Set which commands the window honours.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: WindowCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Whether the window's content can be seen: it is shown, not minimized
    /// and not occluded. A minimized window stays shown and a covered one
    /// stays shown and unminimized, but neither puts content on screen, so
    /// neither draws frames: this, not [`visible`](Self::visible), is what
    /// frame scheduling and the lifecycle read.
    #[must_use]
    pub fn is_seen(&self) -> bool {
        self.visible && self.mode != WindowMode::Minimized && !self.occluded
    }

    /// The snapshot that follows this one with `sample`: this very value when
    /// `sample` differs from it in nothing but the revision, otherwise
    /// `sample` at the next revision. The revision `sample` carries is
    /// ignored, and a sample without [`metrics`](Self::metrics) (the window
    /// reported no drawable area) keeps this snapshot's: a report that is not
    /// a drawable area is never published. A sample of a hidden or minimized
    /// window is not [`occluded`](Self::occluded), whatever the window system
    /// said: AppKit calls a miniaturized window occluded, which is no
    /// covering, and a hidden window has nothing to cover. Nor is a sample of
    /// a hidden window active or focused ([`WindowFocus::Inactive`]): the user
    /// cannot work in a window that is not shown. A sample of a window
    /// without the keyboard focus holds no pointer ([`CursorGrab::None`]): a
    /// grab ends with the focus. And a sample whose outer position names a
    /// display is on that display ([`display`](Self::display)): the position
    /// is measured on the display the window is on, so the two are one fact.
    ///
    /// The capabilities' publication and their own
    /// [`revision`](WindowCapabilities::revision) are this function's to
    /// issue, so a sample's are ignored too. While the window honours the same
    /// commands the same way, both stay this snapshot's; when that changes,
    /// the new snapshot's revision becomes theirs, capabilities this snapshot
    /// held as a publication become a new one (ADR-0341), and capabilities of
    /// no publication stay of none. When the process has no publication left,
    /// the changed capabilities are withdrawn
    /// ([`WindowCapabilities::withdraw`]) and stay so in every later
    /// snapshot, while the window's other facts go on publishing. A successor
    /// a caller computes from a window's snapshot is therefore a publication
    /// of its own, which no token admitted against the window's capabilities
    /// opens under, and the other way round.
    ///
    /// # Errors
    /// [`WindowRevisionExhausted`] when `sample` differs and this snapshot is
    /// at the last revision. The revision never wraps.
    pub fn successor(&self, sample: Self) -> Result<Self, WindowRevisionExhausted> {
        self.successor_from(sample, crate::PublicationMint::process())
    }

    /// [`successor`](Self::successor) with the mint a changed publication comes
    /// from named, for a host that takes its mint (production passes
    /// [`PublicationMint::process`](crate::PublicationMint::process)).
    ///
    /// # Errors
    /// [`WindowRevisionExhausted`] when `sample` differs and this snapshot is
    /// at the last revision.
    pub fn successor_from(
        &self,
        mut sample: Self,
        mint: crate::PublicationMint,
    ) -> Result<Self, WindowRevisionExhausted> {
        sample.revision = self.revision;
        sample.capabilities = sample.capabilities.carried_from(&self.capabilities);
        if sample.metrics.is_none() {
            sample.metrics = self.metrics;
        }
        sample.occluded &= sample.visible && sample.mode != WindowMode::Minimized;
        if !sample.visible {
            sample.focus = WindowFocus::Inactive;
        }
        if !sample.focus.is_focused() {
            sample.cursor_grab = CursorGrab::None;
        }
        if let Some(position) = sample.outer.position() {
            sample.display = Some(position.display());
        }
        if sample == *self {
            return Ok(sample);
        }
        let next = self.revision.next().ok_or(WindowRevisionExhausted)?;
        sample.revision = next;
        if sample.capabilities != self.capabilities {
            sample.capabilities = sample
                .capabilities
                .changed_from(&self.capabilities, next, mint);
        }
        Ok(sample)
    }
}

/// A change requested of a window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WindowCommand {
    /// Set the title.
    SetTitle(String),
    /// Move the window's outer top-left corner to a point of a display, in
    /// that display's logical pixels from its top-left corner
    /// ([`WindowPosition`]). The window lands there whichever display it is
    /// on when it is asked; a display the window's host no longer has is
    /// refused with [`ApplyError::DisplayGone`].
    SetPosition(WindowPosition),
    /// Resize the window's inner (content) area, in logical pixels. A size
    /// that encloses no area is refused at admission.
    SetSize(WindowExtent),
    /// Set the inner-size limits.
    SetConstraints(SizeConstraints),
    /// Allow or forbid user resizing.
    SetResizable(bool),
    /// Frame the window: the system's frame, a title bar the application
    /// draws inside a frame the system keeps, or no frame.
    SetFrame(WindowFrame),
    /// Enter a mode. [`WindowMode::Normal`] restores from any other mode.
    SetMode(WindowMode),
    /// Show the window.
    Show,
    /// Hide the window.
    Hide,
    /// Bring the window to the front with the focus, as the window system's
    /// focus rules allow: where they refuse (another application has the
    /// user's input and handed this one no right to the foreground), the
    /// window asks for the user's attention instead and never takes the
    /// focus.
    Activate,
    /// Close and destroy the window: a decision already made, which asks no
    /// should-close veto (ADR-0123). Asking whether the window may close is
    /// [`WindowControl::request_close`].
    Close,
    /// Draw a material behind the window's content, in place of the one it
    /// drew (ADR-0254 §3). It shows through the window's frame and wherever
    /// the content is transparent; a window opens with the material its host
    /// gives a new window, which the host's documentation names. The
    /// material is not a fact of the snapshot: the command is the request,
    /// and a refusal of the window system is [`ApplyError::Platform`].
    SetMaterial(crate::WindowMaterial),
    /// Hold the pointer inside the window, hold it in place, or let it go
    /// ([`CursorGrab`]). Only a window with the keyboard focus takes a grab: a
    /// window without it, or one that loses it, holds none, which the next
    /// snapshot's [`cursor_grab`](WindowState::cursor_grab) shows. Letting go
    /// ([`CursorGrab::None`]) is admitted on every window, as the normal mode
    /// is: a window that cannot grab holds nothing to let go of.
    SetCursorGrab(CursorGrab),
    /// Keep the window above or below the other windows, or neither
    /// ([`WindowLevel`]); the next snapshot's [`level`](WindowState::level)
    /// shows what the window system did. [`WindowLevel::Normal`] is admitted
    /// on every window.
    SetLevel(WindowLevel),
    /// Resize the content only in steps of this extent while the user drags
    /// the frame (a terminal's character cell), counted from the window's
    /// minimum size where it has one; `None` lifts the steps. The window
    /// system holds it during the user's resize; a [`SetSize`](Self::SetSize),
    /// a maximize or a fullscreen is not stepped. Not a fact of the snapshot:
    /// the command is the request, as with [`SetMaterial`](Self::SetMaterial).
    SetResizeIncrements(Option<WindowExtent>),
    /// Keep the content at this proportion while the user drags the frame
    /// (a video); `None` lifts it. Held like
    /// [`SetResizeIncrements`](Self::SetResizeIncrements), and not a fact of
    /// the snapshot either.
    SetAspectRatio(Option<AspectRatio>),
}

impl WindowCommand {
    /// Refuse a command whose numbers cannot describe a window, before any
    /// operating system call.
    ///
    /// The command carries the types a snapshot holds ([`WindowPosition`],
    /// [`WindowExtent`], [`SizeConstraints`]), so a number that is not finite
    /// cannot be asked for at all; what is checked here is what those types
    /// do not say, and the refusal carries the numbers.
    ///
    /// # Errors
    /// [`AdmissionError::InvalidGeometry`] for a size, a limit or a resize
    /// step that encloses no area, or constraints whose minimum exceeds their
    /// maximum in either dimension.
    pub fn validate(&self) -> Result<(), AdmissionError> {
        match self {
            Self::SetSize(size) => size.area().map(drop),
            Self::SetConstraints(constraints) => constraints.asked().map(drop),
            Self::SetResizeIncrements(step) => step.map_or(Ok(()), |step| step.area().map(drop)),
            Self::SetPosition(_)
            | Self::SetTitle(_)
            | Self::SetResizable(_)
            | Self::SetFrame(_)
            | Self::SetMode(_)
            | Self::Show
            | Self::Hide
            | Self::Activate
            | Self::Close
            | Self::SetMaterial(_)
            | Self::SetCursorGrab(_)
            | Self::SetLevel(_)
            | Self::SetAspectRatio(_) => Ok(()),
        }
        .map_err(AdmissionError::InvalidGeometry)
    }

    /// Admit this command to a window with these capabilities: its numbers
    /// must be valid, then the window must honour it. The only way to build
    /// the [`AdmittedCommand`] that
    /// [`PlatformWindow::apply`](crate::PlatformWindow::apply) takes; the
    /// token is bound to the capabilities' publication
    /// ([`WindowCapabilities`], "Publication").
    ///
    /// A command is admitted only against a publication, and a window opens it
    /// only under its own (ADR-0341): pass the capabilities of the window's
    /// published snapshot ([`WindowState::capabilities`]). Capabilities built
    /// by hand, edited or withdrawn are no publication, and admission
    /// refuses them; capabilities a caller published itself, or computed with
    /// [`WindowState::successor`], are admitted, and
    /// no window opens the token.
    ///
    /// # Errors
    /// What [`validate`](Self::validate) refuses;
    /// [`AdmissionError::Unpublished`] when `capabilities` are no
    /// publication, which admitting them again never changes; or
    /// [`AdmissionError::Unsupported`] when the command's capability is
    /// [`Absent`](WindowCapability::Absent) in `capabilities`: the window
    /// does not take it, for the reason the capability carries.
    pub fn admit(
        self,
        capabilities: &WindowCapabilities,
    ) -> Result<AdmittedCommand, AdmissionError> {
        self.validate()?;
        let publication = capabilities
            .publication()
            .ok_or(AdmissionError::Unpublished)?;
        if let WindowCapability::Absent(absence) = capabilities.capability(&self) {
            return Err(Unsupported::from(absence).into());
        }
        Ok(AdmittedCommand {
            command: self,
            publication,
        })
    }

    /// Admit this command against `window`'s snapshot now and apply it: the
    /// one path for a caller that applies at once, with no queue in front of
    /// the window. A command the window answers
    /// [`Superseded`](ApplyError::Superseded) — its capabilities changed
    /// between the read and the application — is admitted once more against
    /// the snapshot then, and applied or refused.
    ///
    /// # Errors
    /// [`CommandRefusal::Admission`] when the window's capabilities refuse
    /// the command, [`CommandRefusal::Apply`] when the window does.
    pub fn admit_and_apply<W: crate::PlatformWindow + ?Sized>(
        self,
        window: &W,
    ) -> Result<(), CommandRefusal> {
        let admitted = self.admit(&window.state().capabilities)?;
        match window.apply(admitted) {
            Err(ApplyError::Superseded { command }) => {
                let readmitted = command.admit(&window.state().capabilities)?;
                Ok(window.apply(readmitted)?)
            }
            outcome => Ok(outcome?),
        }
    }
}

/// A [`WindowCommand`] that [`WindowCommand::admit`] checked: its numbers
/// describe a window, and the capabilities it was admitted against honour it.
/// [`PlatformWindow::apply`](crate::PlatformWindow::apply) takes nothing else,
/// so a window never receives an unchecked command.
///
/// **What the token proves, and what it does not** (ADR-0341). It proves the
/// command's numbers describe a window and that one publication of a window's
/// capabilities ([`WindowCapabilities`], "Publication") honours it, and it
/// carries that publication. No caller can make capabilities of a window's
/// publication say anything but what the window published: their fields are
/// read by value, an edit leaves the publication behind, the publication is
/// minted inside this crate and never reissued, and admission refuses
/// capabilities of none. A window opens the token with
/// [`against`](Self::against) its own published capabilities, which answers
/// [`ApplyError::Superseded`] unless they are the very publication the token
/// carries, so a window carries out only what its own published capabilities
/// honour, and never a command checked against capabilities it no longer has
/// or another window's. The operating system still keeps the last word: it
/// speaks through [`ApplyError::Platform`] and the next snapshot.
#[derive(Debug, PartialEq)]
#[must_use = "an admitted command does nothing until a window applies it"]
pub struct AdmittedCommand {
    command: WindowCommand,
    /// The publication the command was admitted against.
    publication: Publication,
}

impl AdmittedCommand {
    /// The admitted command.
    #[must_use]
    pub const fn command(&self) -> &WindowCommand {
        &self.command
    }

    /// The command, to carry out under `capabilities`: the window's own, as
    /// its last published snapshot holds them. The one way a window takes the
    /// command out of the token.
    ///
    /// The contract is identity: the token opens only under the publication
    /// it was admitted against. One publication is one set of capabilities,
    /// which no reader can change and [`WindowState::successor`] replaces with
    /// a new publication at every change, so the set `admit` checked the
    /// command against is the set `capabilities` hold, and the command is
    /// honoured without asking again.
    ///
    /// # Errors
    /// [`ApplyError::Superseded`], carrying the command back, when
    /// `capabilities` are another publication than the token's: a later one of
    /// the same window, or another window's. The caller admits it again
    /// against the window's snapshot.
    pub fn against(self, capabilities: &WindowCapabilities) -> Result<WindowCommand, ApplyError> {
        if capabilities.publication() == Some(self.publication) {
            Ok(self.command)
        } else {
            Err(ApplyError::Superseded {
                command: self.command,
            })
        }
    }
}

/// Why a command was not admitted: the caller's to fix, before any window
/// sees the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AdmissionError {
    /// The window does not honour the command; its capability is
    /// [`Absent`](WindowCapability::Absent).
    #[error("the window does not take this command: {0}")]
    Unsupported(#[from] Unsupported),
    /// The command's numbers cannot describe a window; the error names them.
    #[error("the command's geometry describes no window: {0}")]
    InvalidGeometry(InvalidGeometry),
    /// The capabilities the command was admitted against are no publication:
    /// built by hand, taken apart and edited, or withdrawn by a window that can
    /// publish no more (ADR-0341). Admitting against them again is refused
    /// again; admit against the window's snapshot.
    #[error("the capabilities are no publication")]
    Unpublished,
}

impl AdmissionError {
    /// What is not here, when that is why the command was refused.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::InvalidGeometry(_) | Self::Unpublished => None,
        }
    }
}

/// Why a window did not carry out an admitted command: what the world caused
/// between admission and the operating system.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ApplyError {
    /// The window has closed.
    #[error("the window is closed")]
    Closed,
    /// The command was admitted against capabilities that are not this
    /// window's now: another window's, or ones its window has revised since.
    /// Admit `command` again against the current snapshot.
    #[error("the command was admitted against capabilities the window does not have now")]
    Superseded {
        /// The command, given back to be admitted again.
        command: WindowCommand,
    },
    /// The command names a display the window's host no longer has (a
    /// [`SetPosition`](WindowCommand::SetPosition) on a monitor unplugged
    /// since the position was read): there is no such place to put the
    /// window. Read the window's display snapshot again and ask for a
    /// position on a display it holds.
    #[error("the display {} the command names is gone", .0.get())]
    DisplayGone(DisplayId),
    /// The window system refused or failed the request.
    #[error("the window system did not carry out the command: {0}")]
    Platform(PlatformFailure),
}

/// What the window system said of a request of a window it did not carry
/// out: a command ([`ApplyError::Platform`]), a cursor
/// ([`CursorError::Platform`](crate::CursorError::Platform)) or a preference
/// query ([`PreferenceQueryError::Platform`](crate::PreferenceQueryError::Platform)).
/// It is the one shape of that refusal, so a caller matches one type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlatformFailure {
    /// A number the request carries, finite and positive as admission holds
    /// it, is beyond what the window system can represent (Win32's device
    /// pixels at the window's scale).
    #[error("a value is beyond what the window system can represent")]
    OutOfRange,
    /// The window system refused or failed the call.
    #[error("{message}")]
    Failed {
        /// What the window system reported.
        message: String,
    },
}

impl ApplyError {
    /// A window-system failure with this message.
    #[must_use]
    pub fn failed(message: String) -> Self {
        Self::Platform(PlatformFailure::Failed { message })
    }
}

/// Why a command applied at once ([`WindowCommand::admit_and_apply`]) did not
/// take effect: its admission refused it, or the window did.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a command is refused at admission or at application; there is no third place"
)]
pub enum CommandRefusal {
    /// The window's capabilities refuse the command, or its numbers describe
    /// no window.
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    /// The window closed, its capabilities moved again, or its window system
    /// did not carry the command out.
    #[error(transparent)]
    Apply(#[from] ApplyError),
}

impl CommandRefusal {
    /// What is not here, when that is why the command was refused.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Admission(refused) => refused.unsupported(),
            Self::Apply(_) => None,
        }
    }
}

/// Why [`WindowControl::submit`] did not queue a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SubmitError {
    /// The window's capabilities refuse the command, or its numbers cannot
    /// describe a window.
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    /// The window is gone: there is no queue to take the command.
    #[error("the window is closed")]
    Closed,
}

impl SubmitError {
    /// What is not here, when that is why the command was refused.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Admission(refused) => refused.unsupported(),
            Self::Closed => None,
        }
    }
}

/// The control surface the framework gives app code over one window
/// (ADR-0237 decision 4). The host implements it over its window; a handle in
/// the view layer calls it.
pub trait WindowControl {
    /// The window's last published snapshot, with the capabilities that say
    /// which commands it honours.
    fn state(&self) -> WindowState;

    /// Admit `command` against the snapshot's capabilities and queue it for
    /// the owner's next turn, where it applies in submission order.
    ///
    /// # Errors
    /// [`SubmitError::Admission`] with what [`WindowCommand::admit`] refuses,
    /// or [`SubmitError::Closed`] after the window closed.
    fn submit(&self, command: WindowCommand) -> Result<(), SubmitError>;

    /// Ask the window to close through the vetoed close path.
    fn request_close(&self);
}

/// The fields of a [`WindowState`] other than its revision, public: the
/// boundary every serialization and mirror of a window's snapshot goes through
/// (a recording's schema).
///
/// Deliberately not `#[non_exhaustive]`, unlike [`WindowState`]: a mirror
/// builds and destructures this struct exhaustively, so a fact the snapshot
/// gains breaks every mirror at compile time until it carries the fact,
/// instead of dropping it from a recording. The revision stays apart because
/// [`WindowState::successor`] issues it: [`Self::into_state`] is a sample, and
/// publishing it gives it the revision its window's history earns.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowStateParts {
    /// [`WindowState::outer`].
    pub outer: WindowGeometry,
    /// [`WindowState::content_size`].
    pub content_size: WindowExtent,
    /// [`WindowState::mode`].
    pub mode: WindowMode,
    /// [`WindowState::normal_geometry`].
    pub normal_geometry: Option<WindowGeometry>,
    /// [`WindowState::visible`].
    pub visible: bool,
    /// [`WindowState::occluded`].
    pub occluded: bool,
    /// [`WindowState::focus`].
    pub focus: WindowFocus,
    /// [`WindowState::title`].
    pub title: Option<String>,
    /// [`WindowState::constraints`].
    pub constraints: SizeConstraints,
    /// [`WindowState::resizable`].
    pub resizable: bool,
    /// [`WindowState::frame`].
    pub frame: WindowFrame,
    /// [`WindowState::display`].
    pub display: Option<DisplayId>,
    /// [`WindowState::metrics`].
    pub metrics: Option<WindowMetrics>,
    /// [`WindowState::brightness`].
    pub brightness: crate::Brightness,
    /// [`WindowState::insets`].
    pub insets: WindowInsets,
    /// [`WindowState::execution`].
    pub execution: WindowExecutionState,
    /// [`WindowState::hovered`].
    pub hovered: bool,
    /// [`WindowState::interaction`].
    pub interaction: WindowInteraction,
    /// [`WindowState::cursor_grab`].
    pub cursor_grab: CursorGrab,
    /// [`WindowState::level`].
    pub level: WindowLevel,
    /// [`WindowState::capabilities`], taken apart.
    pub capabilities: CapabilityParts,
}

impl WindowState {
    /// Take the snapshot apart into every fact but its revision.
    #[must_use]
    pub fn into_parts(self) -> WindowStateParts {
        let Self {
            revision: _,
            outer,
            content_size,
            mode,
            normal_geometry,
            visible,
            occluded,
            focus,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            brightness,
            insets,
            execution,
            hovered,
            interaction,
            cursor_grab,
            level,
            capabilities,
        } = self;
        WindowStateParts {
            outer,
            content_size,
            mode,
            normal_geometry,
            visible,
            occluded,
            focus,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            brightness,
            insets,
            execution,
            hovered,
            interaction,
            cursor_grab,
            level,
            capabilities: capabilities.into_parts(),
        }
    }
}

impl WindowStateParts {
    /// A sample with these facts, at [`WindowRevision::ZERO`]: what a window
    /// publishes through [`WindowState::successor`], which issues its
    /// revision. Every field is valid by its type.
    #[must_use]
    pub fn into_state(self) -> WindowState {
        let Self {
            outer,
            content_size,
            mode,
            normal_geometry,
            visible,
            occluded,
            focus,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            brightness,
            insets,
            execution,
            hovered,
            interaction,
            cursor_grab,
            level,
            capabilities,
        } = self;
        WindowState {
            revision: WindowRevision::ZERO,
            outer,
            content_size,
            mode,
            normal_geometry,
            visible,
            occluded,
            focus,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            brightness,
            insets,
            execution,
            hovered,
            interaction,
            cursor_grab,
            level,
            capabilities: capabilities.into_capabilities(),
        }
    }
}
