//! The window state contract: an immutable snapshot a window publishes, the
//! commands that change it and the capabilities that say which commands the
//! window honours (ADR-0237).
//!
//! # Snapshot
//!
//! [`WindowState`] describes what the operating system reports about a window,
//! never what code asked for: a size the OS clamped is the clamped size, and a
//! maximize the OS refused leaves the mode where it was. A change publishes a
//! new snapshot with a higher [`WindowRevision`] and one payload-free
//! `on_state_changed` notification; a sample equal to the published one keeps
//! the revision ([`WindowState::successor`]). The revision never wraps: the
//! last one is reached once, then the snapshot refuses further change.
//!
//! # Commands
//!
//! Every change is a [`WindowCommand`] applied through
//! [`PlatformWindow::apply`](crate::PlatformWindow::apply). A window answers
//! [`WindowCommandError::Unsupported`] exactly when the matching
//! [`WindowCapabilities`] field is `false`, so a caller can ask up front and is
//! never told "done" for a no-op. An accepted command is a request: its
//! outcome is visible only in the next snapshot.

use flui_foundation::geometry::{Point, Size};

use crate::DisplayId;

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

    /// The last revision a window can publish.
    #[must_use]
    pub const fn last() -> Self {
        Self(u64::MAX)
    }
}

/// A window state cannot advance: the last revision was reached.
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

/// A window rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// The top-left corner in global screen coordinates; `None` where the OS
    /// reports no global position (Wayland, Android, iOS).
    pub position: Option<Point<f64>>,
    /// The extent.
    pub size: Size<f64>,
}

impl WindowGeometry {
    /// A rectangle with this position and size.
    #[must_use]
    pub const fn new(position: Option<Point<f64>>, size: Size<f64>) -> Self {
        Self { position, size }
    }
}

/// The inner-size limits of a window, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SizeConstraints {
    /// The smallest inner size, if limited.
    pub min: Option<Size<f64>>,
    /// The largest inner size, if limited.
    pub max: Option<Size<f64>>,
}

impl SizeConstraints {
    /// No limits.
    pub const NONE: Self = Self {
        min: None,
        max: None,
    };

    /// Limits with these bounds.
    #[must_use]
    pub const fn new(min: Option<Size<f64>>, max: Option<Size<f64>>) -> Self {
        Self { min, max }
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
    /// The window's outer rectangle (frame included).
    pub outer: WindowGeometry,
    /// The content (client) area extent.
    pub content_size: Size<f64>,
    /// How the window occupies its display.
    pub mode: WindowMode,
    /// The rectangle the window returns to from a maximized or fullscreen
    /// mode, when the OS reports it.
    pub normal_geometry: Option<WindowGeometry>,
    /// Whether the window is shown.
    pub visible: bool,
    /// Whether the window has input focus.
    pub focused: bool,
    /// The title; `None` where the OS has no window title.
    pub title: Option<String>,
    /// The inner-size limits the OS enforces.
    pub constraints: SizeConstraints,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// Whether the window has its frame and title bar.
    pub decorated: bool,
    /// The display the window is on, named by the display snapshot's identity.
    pub display: Option<DisplayId>,
}

impl WindowState {
    /// A visible, unfocused, resizable, decorated window in normal mode at
    /// revision zero, with no title, limits or display.
    #[must_use]
    pub fn new(outer: WindowGeometry, content_size: Size<f64>) -> Self {
        Self {
            revision: WindowRevision::ZERO,
            outer,
            content_size,
            mode: WindowMode::Normal,
            normal_geometry: None,
            visible: true,
            focused: false,
            title: None,
            constraints: SizeConstraints::NONE,
            resizable: true,
            decorated: true,
            display: None,
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

    /// Set whether the window has input focus.
    #[must_use]
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
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

    /// Set whether the window has its frame.
    #[must_use]
    pub fn with_decorated(mut self, decorated: bool) -> Self {
        self.decorated = decorated;
        self
    }

    /// Set the display.
    #[must_use]
    pub fn with_display(mut self, display: Option<DisplayId>) -> Self {
        self.display = display;
        self
    }

    /// The snapshot that follows this one with `sample`: this very value when
    /// `sample` differs from it in nothing but the revision, otherwise
    /// `sample` at the next revision. The revision `sample` carries is
    /// ignored.
    ///
    /// # Errors
    /// [`WindowRevisionExhausted`] when `sample` differs and this snapshot is
    /// at the last revision. The revision never wraps.
    pub fn successor(&self, mut sample: Self) -> Result<Self, WindowRevisionExhausted> {
        sample.revision = self.revision;
        if sample == *self {
            return Ok(sample);
        }
        sample.revision = self.revision.next().ok_or(WindowRevisionExhausted)?;
        Ok(sample)
    }
}

/// Which commands a window honours.
///
/// A backend reports exactly what it does today: a capability is `true` only
/// when [`apply`](crate::PlatformWindow::apply) carries the command out, and
/// `false` exactly when it answers [`WindowCommandError::Unsupported`]. Fields
/// are public so a backend can start from [`none`](Self::none) or
/// [`all`](Self::all) and set the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent yes/no per command and observation; a bit mask would hide the names"
)]
pub struct WindowCapabilities {
    /// [`WindowCommand::SetTitle`].
    pub set_title: bool,
    /// [`WindowCommand::SetPosition`].
    pub set_position: bool,
    /// [`WindowCommand::SetSize`].
    pub set_size: bool,
    /// [`WindowCommand::SetConstraints`].
    pub set_constraints: bool,
    /// [`WindowCommand::SetResizable`].
    pub set_resizable: bool,
    /// [`WindowCommand::SetDecorated`].
    pub set_decorated: bool,
    /// [`WindowMode::Minimized`] through [`WindowCommand::SetMode`].
    pub minimize: bool,
    /// [`WindowMode::Maximized`] through [`WindowCommand::SetMode`].
    pub maximize: bool,
    /// [`WindowMode::Fullscreen`] through [`WindowCommand::SetMode`].
    pub fullscreen: bool,
    /// [`WindowCommand::Hide`].
    pub hide: bool,
    /// [`WindowCommand::Activate`] and [`WindowCommand::Show`]: showing a window
    /// unminimizes it and requests focus, so the two stand or fall together.
    pub activate: bool,
    /// The snapshot's [`WindowGeometry::position`] is reported.
    pub observes_position: bool,
    /// [`WindowMode::Minimized`] is ever published.
    pub observes_minimized: bool,
    /// A saved position can be applied again when the window opens.
    pub restores_position: bool,
}

impl WindowCapabilities {
    /// A window that honours no command and observes nothing.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            set_title: false,
            set_position: false,
            set_size: false,
            set_constraints: false,
            set_resizable: false,
            set_decorated: false,
            minimize: false,
            maximize: false,
            fullscreen: false,
            hide: false,
            activate: false,
            observes_position: false,
            observes_minimized: false,
            restores_position: false,
        }
    }

    /// A window that honours every command and observes everything.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            set_title: true,
            set_position: true,
            set_size: true,
            set_constraints: true,
            set_resizable: true,
            set_decorated: true,
            minimize: true,
            maximize: true,
            fullscreen: true,
            hide: true,
            activate: true,
            observes_position: true,
            observes_minimized: true,
            restores_position: true,
        }
    }

    /// Whether `command` is one this window honours.
    ///
    /// [`WindowMode::Normal`] is always honoured: asking a window for the
    /// mode every window has is never a no-op that pretends.
    #[must_use]
    pub const fn permits(&self, command: &WindowCommand) -> bool {
        match command {
            WindowCommand::SetTitle(_) => self.set_title,
            WindowCommand::SetPosition(_) => self.set_position,
            WindowCommand::SetSize(_) => self.set_size,
            WindowCommand::SetConstraints(_) => self.set_constraints,
            WindowCommand::SetResizable(_) => self.set_resizable,
            WindowCommand::SetDecorated(_) => self.set_decorated,
            WindowCommand::SetMode(WindowMode::Minimized) => self.minimize,
            WindowCommand::SetMode(WindowMode::Maximized) => self.maximize,
            WindowCommand::SetMode(WindowMode::Fullscreen) => self.fullscreen,
            WindowCommand::SetMode(WindowMode::Normal) => true,
            WindowCommand::Hide => self.hide,
            WindowCommand::Show | WindowCommand::Activate => self.activate,
        }
    }
}

/// A change requested of a window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WindowCommand {
    /// Set the title.
    SetTitle(String),
    /// Move the window's outer top-left corner to a global position in
    /// logical pixels.
    SetPosition(Point<f64>),
    /// Resize the window's inner (content) area, in logical pixels.
    SetSize(Size<f64>),
    /// Set the inner-size limits.
    SetConstraints(SizeConstraints),
    /// Allow or forbid user resizing.
    SetResizable(bool),
    /// Show or remove the frame and title bar.
    SetDecorated(bool),
    /// Enter a mode. [`WindowMode::Normal`] restores from any other mode.
    SetMode(WindowMode),
    /// Show the window.
    Show,
    /// Hide the window.
    Hide,
    /// Bring the window to the front and request focus.
    Activate,
}

impl WindowCommand {
    /// Refuse a command whose numbers cannot describe a window, before any
    /// operating system call.
    ///
    /// # Errors
    /// [`WindowCommandError::InvalidGeometry`] for a position or size that is
    /// not finite, a size that is not positive, or constraints whose minimum
    /// exceeds their maximum in either dimension.
    pub fn validate(&self) -> Result<(), WindowCommandError> {
        let valid = match self {
            Self::SetPosition(at) => at.x.is_finite() && at.y.is_finite(),
            Self::SetSize(size) => positive_size(*size),
            Self::SetConstraints(constraints) => {
                constraints.min.is_none_or(positive_size)
                    && constraints.max.is_none_or(positive_size)
                    && match (constraints.min, constraints.max) {
                        (Some(min), Some(max)) => {
                            min.width <= max.width && min.height <= max.height
                        }
                        _ => true,
                    }
            }
            Self::SetTitle(_)
            | Self::SetResizable(_)
            | Self::SetDecorated(_)
            | Self::SetMode(_)
            | Self::Show
            | Self::Hide
            | Self::Activate => true,
        };
        if valid {
            Ok(())
        } else {
            Err(WindowCommandError::InvalidGeometry)
        }
    }

    /// Admit a command to a window with these capabilities: its numbers must
    /// be valid, then the window must honour it. The check every backend's
    /// `apply` and every queue in front of one runs first.
    ///
    /// # Errors
    /// What [`validate`](Self::validate) refuses, or
    /// [`WindowCommandError::Unsupported`] when `capabilities` do not permit
    /// the command.
    pub fn admit(&self, capabilities: &WindowCapabilities) -> Result<(), WindowCommandError> {
        self.validate()?;
        if capabilities.permits(self) {
            Ok(())
        } else {
            Err(WindowCommandError::Unsupported)
        }
    }
}

fn positive_size(size: Size<f64>) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
}

/// Why a window did not take a command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WindowCommandError {
    /// This window does not honour the command; its capability is `false`.
    #[error("the window does not support this command")]
    Unsupported,
    /// The command's numbers cannot describe a window.
    #[error(
        "the command's geometry is not finite and positive, or its minimum exceeds its maximum"
    )]
    InvalidGeometry,
    /// The window has closed.
    #[error("the window is closed")]
    Closed,
    /// The operating system refused or failed the request.
    #[error("the window system failed the command: {0}")]
    Backend(String),
}

/// The control surface the framework gives app code over one window
/// (ADR-0237 decision 4). The host implements it over its window; a handle in
/// the view layer calls it.
pub trait WindowControl {
    /// The window's last published snapshot.
    fn state(&self) -> WindowState;

    /// Which commands the window honours.
    fn capabilities(&self) -> WindowCapabilities;

    /// Validate `command`, check it against the capabilities and queue it for
    /// the owner's next turn.
    ///
    /// # Errors
    /// What [`WindowCommand::admit`] refuses, or
    /// [`WindowCommandError::Closed`] after the window closed.
    fn submit(&self, command: WindowCommand) -> Result<(), WindowCommandError>;

    /// Ask the window to close through the vetoed close path.
    fn request_close(&self);
}
