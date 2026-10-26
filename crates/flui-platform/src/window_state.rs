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

use crate::{DisplayId, Unsupported, UnsupportedReason};

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
    pub const LAST: Self = Self(u64::MAX);
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
/// are public so a backend can start from [`NONE`](Self::NONE) or
/// [`ALL`](Self::ALL) and set the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent yes/no per command and observation; a bit mask would hide the names"
)]
pub struct WindowCapabilities {
    /// [`WindowCommand::SetTitle`].
    pub sets_title: bool,
    /// [`WindowCommand::SetPosition`].
    pub sets_position: bool,
    /// [`WindowCommand::SetSize`].
    pub sets_size: bool,
    /// [`WindowCommand::SetConstraints`].
    pub sets_constraints: bool,
    /// [`WindowCommand::SetResizable`].
    pub sets_resizable: bool,
    /// [`WindowCommand::SetDecorated`].
    pub sets_decorated: bool,
    /// [`WindowMode::Minimized`] through [`WindowCommand::SetMode`].
    pub minimizes: bool,
    /// [`WindowMode::Maximized`] through [`WindowCommand::SetMode`].
    pub maximizes: bool,
    /// [`WindowMode::Fullscreen`] through [`WindowCommand::SetMode`].
    pub enters_fullscreen: bool,
    /// [`WindowCommand::Hide`].
    pub hides: bool,
    /// [`WindowCommand::Show`].
    pub shows: bool,
    /// [`WindowCommand::Activate`]: bring the window to the front and request
    /// focus.
    pub activates: bool,
    /// The snapshot's [`WindowGeometry::position`] is reported.
    pub observes_position: bool,
    /// [`WindowMode::Minimized`] is ever published.
    pub observes_minimized: bool,
    /// A saved position can be applied again when the window opens.
    pub places_at_open: bool,
}

impl WindowCapabilities {
    /// A window that honours no command and observes nothing.
    pub const NONE: Self = Self {
        sets_title: false,
        sets_position: false,
        sets_size: false,
        sets_constraints: false,
        sets_resizable: false,
        sets_decorated: false,
        minimizes: false,
        maximizes: false,
        enters_fullscreen: false,
        hides: false,
        shows: false,
        activates: false,
        observes_position: false,
        observes_minimized: false,
        places_at_open: false,
    };

    /// A window that honours every command and observes everything.
    pub const ALL: Self = Self {
        sets_title: true,
        sets_position: true,
        sets_size: true,
        sets_constraints: true,
        sets_resizable: true,
        sets_decorated: true,
        minimizes: true,
        maximizes: true,
        enters_fullscreen: true,
        hides: true,
        shows: true,
        activates: true,
        observes_position: true,
        observes_minimized: true,
        places_at_open: true,
    };

    /// Whether `command` is one this window honours.
    ///
    /// [`WindowMode::Normal`] is always honoured: asking a window for the
    /// mode every window has is never a no-op that pretends.
    #[must_use]
    pub const fn permits(&self, command: &WindowCommand) -> bool {
        match command {
            WindowCommand::SetTitle(_) => self.sets_title,
            WindowCommand::SetPosition(_) => self.sets_position,
            WindowCommand::SetSize(_) => self.sets_size,
            WindowCommand::SetConstraints(_) => self.sets_constraints,
            WindowCommand::SetResizable(_) => self.sets_resizable,
            WindowCommand::SetDecorated(_) => self.sets_decorated,
            WindowCommand::SetMode(WindowMode::Minimized) => self.minimizes,
            WindowCommand::SetMode(WindowMode::Maximized) => self.maximizes,
            WindowCommand::SetMode(WindowMode::Fullscreen) => self.enters_fullscreen,
            WindowCommand::SetMode(WindowMode::Normal) => true,
            WindowCommand::Hide => self.hides,
            WindowCommand::Show => self.shows,
            WindowCommand::Activate => self.activates,
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
    /// the command: the window's platform does not take it
    /// ([`UnsupportedReason::NotOnThisPlatform`]).
    pub fn admit(&self, capabilities: &WindowCapabilities) -> Result<(), WindowCommandError> {
        self.validate()?;
        if capabilities.permits(self) {
            Ok(())
        } else {
            Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into())
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
    #[error("the window does not take this command: {0}")]
    Unsupported(#[from] Unsupported),
    /// The command's numbers cannot describe a window.
    #[error(
        "the command's geometry is not finite and positive, or its minimum exceeds its maximum"
    )]
    InvalidGeometry,
    /// The window has closed.
    #[error("the window is closed")]
    Closed,
    /// The operating system refused or failed the request.
    #[error("the window system failed the command: {message}")]
    Backend {
        /// What the window system reported.
        message: String,
    },
}

impl WindowCommandError {
    /// What is not here, when that is why the command was refused.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::InvalidGeometry | Self::Closed | Self::Backend { .. } => None,
        }
    }
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
