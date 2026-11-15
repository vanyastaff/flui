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

use flui_foundation::geometry::{Point, Size, WindowMetrics};

use crate::{
    CapabilitiesOwner, CapabilityParts, DisplayId, InvalidGeometry, Unsupported, WindowAppearance,
    WindowCapabilities, WindowCapability, WindowExecutionState, WindowExtent, WindowFrame,
    WindowGeometry, WindowInsets,
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

/// The inner-size limits of a window, in logical pixels.
///
/// Each bound is a [`WindowExtent`], so a limit is finite and not negative
/// wherever it is held. That a limit is positive and the minimum does not
/// exceed the maximum is asked of a command
/// ([`WindowCommand::validate`]), not of what a window system reports: a
/// snapshot shows the limits as the window system holds them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
    pub content_size: WindowExtent,
    /// How the window occupies its display.
    pub mode: WindowMode,
    /// The rectangle the window returns to from a maximized or fullscreen
    /// mode, when the OS reports it.
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
    /// not tell, and always `false` for a hidden or minimized window, which
    /// [`successor`](Self::successor) holds: those are their own facts.
    pub occluded: bool,
    /// Whether the window has input focus.
    pub focused: bool,
    /// The title; `None` where the OS has no window title.
    pub title: Option<String>,
    /// The inner-size limits the OS enforces.
    pub constraints: SizeConstraints,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// What frames the window: the system's title bar, the application's, or none.
    pub frame: WindowFrame,
    /// The display the window is on, named by the display snapshot's identity
    /// (ADR-0236); `None` when the platform cannot tell, which a consumer reads
    /// as an unknown display, never as the primary one.
    pub display: Option<DisplayId>,
    /// The drawable area in device pixels and its ratio to logical pixels, as
    /// the window last reported a drawable one; `None` until it has. A report
    /// that is not a drawable area (a minimized window's zero size, a scale
    /// that is not finite and positive) is never published:
    /// [`successor`](Self::successor) keeps the metrics before it.
    pub metrics: Option<WindowMetrics>,
    /// The light/dark appearance the system applies to the window: the user's
    /// theme as applied to it, never one the backend chose for its own chrome
    /// (ADR-0222). A backend that cannot observe the theme reports
    /// [`WindowAppearance::Light`].
    pub appearance: WindowAppearance,
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
    /// Whether the window is the active window of its desktop: the one the
    /// user works in, holding the keyboard itself or through a window it owns
    /// (a Win32 window whose owned popup has the focus is active and not
    /// focused). Where the window system has no notion apart from focus, it
    /// is the focus.
    pub active: bool,
    /// Which commands the window honours. A change in what it honours (a
    /// compositor revising what it allows) is a state change like any other.
    pub capabilities: WindowCapabilities,
}

impl WindowState {
    /// A visible, unoccluded, unfocused, inactive, unhovered, running, resizable,
    /// system-framed window in normal mode at revision zero, with no title, limits, display,
    /// metrics or insets, the light appearance, and capabilities that
    /// honour nothing.
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
            focused: false,
            title: None,
            constraints: SizeConstraints::NONE,
            resizable: true,
            frame: WindowFrame::System,
            display: None,
            metrics: None,
            appearance: WindowAppearance::Light,
            insets: WindowInsets::ZERO,
            execution: WindowExecutionState::Running,
            hovered: false,
            active: false,
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

    /// Set the appearance the system applies to the window.
    #[must_use]
    pub fn with_appearance(mut self, appearance: WindowAppearance) -> Self {
        self.appearance = appearance;
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

    /// Set whether the window is the active window of its desktop.
    #[must_use]
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
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
    /// covering, and a hidden window has nothing to cover.
    ///
    /// The capabilities' own [`revision`](WindowCapabilities::revision) and
    /// owner are this function's to keep, so a sample's are ignored too: the
    /// owner stays this snapshot's, and the revision stays while the window
    /// honours the same commands the same way and becomes the new snapshot's
    /// revision when that changes.
    ///
    /// # Errors
    /// [`WindowRevisionExhausted`] when `sample` differs and this snapshot is
    /// at the last revision. The revision never wraps.
    pub fn successor(&self, mut sample: Self) -> Result<Self, WindowRevisionExhausted> {
        sample.revision = self.revision;
        sample.capabilities.revision = self.capabilities.revision;
        sample.capabilities.owner = self.capabilities.owner;
        if sample.metrics.is_none() {
            sample.metrics = self.metrics;
        }
        sample.occluded &= sample.visible && sample.mode != WindowMode::Minimized;
        if sample == *self {
            return Ok(sample);
        }
        let next = self.revision.next().ok_or(WindowRevisionExhausted)?;
        sample.revision = next;
        if sample.capabilities != self.capabilities {
            sample.capabilities.revision = next;
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
    /// Move the window's outer top-left corner to a global position in
    /// logical pixels.
    SetPosition(Point<f64>),
    /// Resize the window's inner (content) area, in logical pixels.
    SetSize(Size<f64>),
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
}

impl WindowCommand {
    /// Refuse a command whose numbers cannot describe a window, before any
    /// operating system call.
    ///
    /// # Errors
    /// [`AdmissionError::InvalidGeometry`] for a position or size that is
    /// not finite, a size or a limit that is not positive, or constraints
    /// whose minimum exceeds their maximum in either dimension.
    pub fn validate(&self) -> Result<(), AdmissionError> {
        let valid = match self {
            Self::SetPosition(at) => at.x.is_finite() && at.y.is_finite(),
            Self::SetSize(size) => positive_size(*size),
            Self::SetConstraints(constraints) => {
                // Finite and not negative by its type; a command's limit is
                // also an area.
                let positive = |limit: WindowExtent| limit.width() > 0.0 && limit.height() > 0.0;
                constraints.min.is_none_or(positive)
                    && constraints.max.is_none_or(positive)
                    && match (constraints.min, constraints.max) {
                        (Some(min), Some(max)) => {
                            min.width() <= max.width() && min.height() <= max.height()
                        }
                        _ => true,
                    }
            }
            Self::SetTitle(_)
            | Self::SetResizable(_)
            | Self::SetFrame(_)
            | Self::SetMode(_)
            | Self::Show
            | Self::Hide
            | Self::Activate
            | Self::Close
            | Self::SetMaterial(_) => true,
        };
        if valid {
            Ok(())
        } else {
            Err(AdmissionError::InvalidGeometry)
        }
    }

    /// Admit this command to a window with these capabilities: its numbers
    /// must be valid, then the window must honour it. The only way to build
    /// the [`AdmittedCommand`] that
    /// [`PlatformWindow::apply`](crate::PlatformWindow::apply) takes; the
    /// token is bound to the capabilities' window and
    /// [`revision`](WindowCapabilities::revision).
    ///
    /// Pass the capabilities of the window's published snapshot
    /// ([`WindowState::capabilities`]): capabilities built by hand belong to
    /// no window, and a host's window answers a token admitted against them
    /// with [`ApplyError::Superseded`].
    ///
    /// # Errors
    /// What [`validate`](Self::validate) refuses, or
    /// [`AdmissionError::Unsupported`] when the command's capability is
    /// [`Absent`](WindowCapability::Absent) in `capabilities`: the window
    /// does not take it, for the reason the capability carries.
    pub fn admit(
        self,
        capabilities: &WindowCapabilities,
    ) -> Result<AdmittedCommand, AdmissionError> {
        self.validate()?;
        if let WindowCapability::Absent(absence) = capabilities.capability(&self) {
            return Err(Unsupported::from(absence).into());
        }
        Ok(AdmittedCommand {
            command: self,
            owner: capabilities.owner,
            revision: capabilities.revision,
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

fn positive_size(size: Size<f64>) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
}

/// A [`WindowCommand`] that [`WindowCommand::admit`] checked: its numbers
/// describe a window, and the capabilities it was admitted against honour it.
/// [`PlatformWindow::apply`](crate::PlatformWindow::apply) takes nothing else,
/// so a window never receives an unchecked command.
///
/// **What the token proves, and what it does not.** It proves the command was
/// validated against the capabilities read at admission, and it carries their
/// window and [`revision`](WindowCapabilities::revision): a window opens it
/// with [`against`](Self::against) its current capabilities, which answers
/// [`ApplyError::Superseded`] when they belong to another window or moved
/// since, so a command is never applied under capabilities it was not checked
/// against. The operating system still keeps the last word: it speaks through
/// [`ApplyError::Platform`] and the next snapshot.
#[derive(Debug, PartialEq)]
#[must_use = "an admitted command does nothing until a window applies it"]
pub struct AdmittedCommand {
    command: WindowCommand,
    owner: Option<CapabilitiesOwner>,
    revision: WindowRevision,
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
    /// The contract is identity: the same window and the same capabilities
    /// revision. Within one window, one revision is one set of capabilities
    /// ([`WindowState::successor`] moves it with every change), so the
    /// command they admitted is honoured without asking again.
    ///
    /// # Errors
    /// [`ApplyError::Superseded`], carrying the command back, when
    /// `capabilities` belong to another window or a later revision. The
    /// caller admits it again.
    pub fn against(self, capabilities: &WindowCapabilities) -> Result<WindowCommand, ApplyError> {
        if self.owner == capabilities.owner && self.revision == capabilities.revision {
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
    /// The command's numbers cannot describe a window.
    #[error(
        "the command's geometry is not finite and positive, or its minimum exceeds its maximum"
    )]
    InvalidGeometry,
}

impl AdmissionError {
    /// What is not here, when that is why the command was refused.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::InvalidGeometry => None,
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
    /// The window system refused or failed the request.
    #[error("the window system did not carry out the command: {0}")]
    Platform(PlatformFailure),
}

/// What the window system said of a command it did not carry out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlatformFailure {
    /// A number the command carries, finite and positive as admission holds
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
    /// [`WindowState::focused`].
    pub focused: bool,
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
    /// [`WindowState::appearance`].
    pub appearance: WindowAppearance,
    /// [`WindowState::insets`].
    pub insets: WindowInsets,
    /// [`WindowState::execution`].
    pub execution: WindowExecutionState,
    /// [`WindowState::hovered`].
    pub hovered: bool,
    /// [`WindowState::active`].
    pub active: bool,
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
            focused,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            appearance,
            insets,
            execution,
            hovered,
            active,
            capabilities,
        } = self;
        WindowStateParts {
            outer,
            content_size,
            mode,
            normal_geometry,
            visible,
            occluded,
            focused,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            appearance,
            insets,
            execution,
            hovered,
            active,
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
            focused,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            appearance,
            insets,
            execution,
            hovered,
            active,
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
            focused,
            title,
            constraints,
            resizable,
            frame,
            display,
            metrics,
            appearance,
            insets,
            execution,
            hovered,
            active,
            capabilities: capabilities.into_capabilities(),
        }
    }
}
