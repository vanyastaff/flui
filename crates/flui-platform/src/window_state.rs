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
//! appearance, display, safe area, execution state, focus, activation, visibility,
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
//! [`Availability::Unavailable`], so a caller can ask up front and is never
//! told "done" for a no-op. [`PlatformWindow::apply`](crate::PlatformWindow::apply)
//! takes the admitted command and answers only what the world can cause
//! ([`ApplyError`]). An applied command is a request: its outcome is visible
//! only in the next snapshot.

use std::num::NonZeroU64;

use flui_foundation::geometry::{EdgeInsets, Point, Size, WindowMetrics};

use crate::{
    Availability, CapabilityAbsence, DisplayId, Unsupported, WindowAppearance, WindowExecutionState,
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
    /// Whether the window has its frame and title bar.
    pub decorated: bool,
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
    /// Safe-area intrusions in logical pixels, relative to the content area;
    /// zero where the platform reports none. Not keyboard occlusion.
    pub safe_area: EdgeInsets,
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
    /// decorated window in normal mode at revision zero, with no title, limits, display,
    /// metrics or safe area, the light appearance, and capabilities that
    /// honour nothing.
    #[must_use]
    pub fn new(outer: WindowGeometry, content_size: Size<f64>) -> Self {
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
            decorated: true,
            display: None,
            metrics: None,
            appearance: WindowAppearance::Light,
            safe_area: EdgeInsets::ZERO,
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

    /// Set the safe-area insets.
    #[must_use]
    pub fn with_safe_area(mut self, safe_area: EdgeInsets) -> Self {
        self.safe_area = safe_area;
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

/// Which commands a window honours, and how.
///
/// A backend reports exactly what it does today. Each field is an
/// [`Availability`]: [`Native`](Availability::Native) when the window system
/// carries the command out (or makes the report) itself,
/// [`Emulated`](Availability::Emulated) when the backend builds it from what
/// the window system does provide (a fullscreen made of a borderless window
/// sized to its monitor), and [`Unavailable`](Availability::Unavailable)
/// exactly when [`WindowCommand::admit`] refuses the command as
/// [`AdmissionError::Unsupported`]; a boolean could not say "only
/// approximately". A command the capabilities admit can still fail in the
/// operating system, which [`ApplyError::Platform`] reports: that is a
/// failure, not a missing capability. Fields are public so a backend can
/// start from [`NONE`](Self::NONE) or [`ALL`](Self::ALL) and set the
/// difference.
///
/// A command's capability this device lacks where its platform has it (an
/// operating system build below the feature's), or one the operating system
/// has and the host does not implement yet, is set with
/// [`unavailable`](Self::unavailable) and its [`CapabilityAbsence`], and
/// admission then refuses the command with
/// [`NotOnThisDevice`](crate::UnsupportedReason::NotOnThisDevice) or
/// [`NotYetOnThisHost`](crate::UnsupportedReason::NotYetOnThisHost) instead of
/// [`NotOnThisPlatform`](crate::UnsupportedReason::NotOnThisPlatform).
///
/// The one verb a window takes outside the command queue,
/// [`frame_rate_hint`](Self::frame_rate_hint), answers [`Unsupported`]
/// exactly when it is `Unavailable`, and its reason is the verb's own answer,
/// not a field here: nothing admits a verb against the capabilities, so a
/// reason kept here would be a second copy of what the host's verb already
/// says. The same holds for the input regions, whose capability lives in the
/// input capabilities.
///
/// The one report a window makes outside its snapshot,
/// [`reports_presentation`](Self::reports_presentation), does keep its reason
/// here: no verb answers for it, so
/// [`presentation_feedback`](Self::presentation_feedback) is where a caller
/// learns why frames come without [`FrameRequest::presented`](crate::FrameRequest::presented).
///
/// The capabilities in a published snapshot also carry their
/// [`revision`](Self::revision): the snapshot revision at which what the
/// window honours last changed. An [`AdmittedCommand`] is bound to it, so a
/// command checked against capabilities the window no longer has is
/// [`ApplyError::Superseded`], never applied. Equality and hashing compare
/// what is honoured and how, not the revision: the revision says when the
/// capabilities were published, and it moves only when they differ.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct WindowCapabilities {
    /// [`WindowCommand::SetTitle`].
    pub sets_title: Availability,
    /// [`WindowCommand::SetPosition`].
    pub sets_position: Availability,
    /// [`WindowCommand::SetSize`].
    pub sets_size: Availability,
    /// [`WindowCommand::SetConstraints`].
    pub sets_constraints: Availability,
    /// [`WindowCommand::SetResizable`].
    pub sets_resizable: Availability,
    /// [`WindowCommand::SetDecorated`].
    pub sets_decorated: Availability,
    /// [`WindowMode::Minimized`] through [`WindowCommand::SetMode`].
    pub minimizes: Availability,
    /// [`WindowMode::Maximized`] through [`WindowCommand::SetMode`].
    pub maximizes: Availability,
    /// [`WindowMode::Fullscreen`] through [`WindowCommand::SetMode`].
    pub enters_fullscreen: Availability,
    /// [`WindowCommand::Hide`].
    pub hides: Availability,
    /// [`WindowCommand::Show`].
    pub shows: Availability,
    /// [`WindowCommand::Activate`]: bring the window to the front and request
    /// focus, under the window system's focus rules. `Emulated` where the
    /// window can only ask and the window system decides from what the
    /// request carries (a Wayland activation token).
    pub activates: Availability,
    /// [`WindowCommand::Close`]: destroy the window. A window whose platform
    /// owns its lifetime (an Android activity) cannot close itself.
    pub closes: Availability,
    /// The snapshot's [`WindowGeometry::position`] is reported.
    pub observes_position: Availability,
    /// [`WindowMode::Minimized`] is ever published.
    pub observes_minimized: Availability,
    /// A saved position can be applied again when the window opens.
    pub places_at_open: Availability,
    /// [`PlatformWindow::set_frame_rate_hint`](crate::PlatformWindow::set_frame_rate_hint):
    /// the window system takes a refresh-rate request for the window.
    pub frame_rate_hint: Availability,
    /// [`WindowCommand::SetMaterial`]: the window system draws every
    /// [`WindowMaterial`](crate::WindowMaterial) behind the window.
    pub material: Availability,
    /// [`FrameRequest::presented`](crate::FrameRequest::presented): the window
    /// reports when the frames presented into its surface reached the screen,
    /// on the event timeline. Set it `Unavailable` with
    /// [`without_presentation_feedback`](Self::without_presentation_feedback),
    /// which records why; set by hand, the reason is the platform's.
    pub reports_presentation: Availability,
    /// Why each command's capability is `Unavailable`, indexed by [`Slot`];
    /// read only while it is.
    absences: [CapabilityAbsence; Slot::COUNT],
    /// Why [`reports_presentation`](Self::reports_presentation) is
    /// `Unavailable`; read only while it is.
    presentation_absence: CapabilityAbsence,
    /// The window whose snapshot publishes these capabilities; `None` for
    /// capabilities built by hand. Kept by [`WindowState::successor`].
    owner: Option<CapabilitiesOwner>,
    /// The snapshot revision at which these capabilities were published;
    /// kept by [`WindowState::successor`], zero for capabilities no snapshot
    /// has published.
    revision: WindowRevision,
}

/// The window a snapshot's capabilities belong to, unique in the process: a
/// host mints one per window it opens and stamps the window's first snapshot
/// with it ([`WindowCapabilities::owned_by`]). An [`AdmittedCommand`] is bound
/// to it, so a command admitted for one window is never applied to another,
/// whatever their revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilitiesOwner(NonZeroU64);

impl CapabilitiesOwner {
    /// The owner with this number. A host draws the numbers from one
    /// process-wide source and never reuses one.
    #[must_use]
    pub const fn new(number: NonZeroU64) -> Self {
        Self(number)
    }
}

impl PartialEq for WindowCapabilities {
    fn eq(&self, other: &Self) -> bool {
        self.honoured() == other.honoured()
            && self.absences_in_force() == other.absences_in_force()
            && self.presentation_absence_in_force() == other.presentation_absence_in_force()
    }
}

impl Eq for WindowCapabilities {}

impl std::hash::Hash for WindowCapabilities {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.honoured().hash(state);
        self.absences_in_force().hash(state);
        self.presentation_absence_in_force().hash(state);
    }
}

/// Declares the capabilities a [`WindowCommand`] is admitted against, once:
/// the [`Slot`] of each, how many there are, and the field each one reads.
/// [`WindowCapabilities::slot`] maps every command to one with an exhaustive
/// match, so a new command does not compile until it names a slot here.
macro_rules! command_capabilities {
    ($($slot:ident => $field:ident),+ $(,)?) => {
        /// A capability a [`WindowCommand`] is admitted against: the index of
        /// its absence in [`WindowCapabilities`].
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Slot {
            $($slot),+
        }

        impl Slot {
            /// Every slot, in declaration order, which is their index order.
            const ALL: [Self; [$(Self::$slot),+].len()] = [$(Self::$slot),+];
            /// How many there are: the length of the absences.
            const COUNT: usize = Self::ALL.len();
        }

        impl WindowCapabilities {
            /// The field `slot` reads.
            const fn field(&self, slot: Slot) -> Availability {
                match slot {
                    $(Slot::$slot => self.$field),+
                }
            }

            /// The field `slot` reads, to set.
            const fn field_mut(&mut self, slot: Slot) -> &mut Availability {
                match slot {
                    $(Slot::$slot => &mut self.$field),+
                }
            }
        }
    };
}

command_capabilities! {
    SetTitle => sets_title,
    SetPosition => sets_position,
    SetSize => sets_size,
    SetConstraints => sets_constraints,
    SetResizable => sets_resizable,
    SetDecorated => sets_decorated,
    Minimize => minimizes,
    Maximize => maximizes,
    EnterFullscreen => enters_fullscreen,
    Hide => hides,
    Show => shows,
    Activate => activates,
    Close => closes,
    SetMaterial => material,
}

/// The absence of a capability nothing marked: the platform's.
const NOT_ON_THIS_PLATFORM: [CapabilityAbsence; Slot::COUNT] =
    [CapabilityAbsence::NotOnThisPlatform; Slot::COUNT];

impl WindowCapabilities {
    /// Every field but the revision, for equality and hashing. Destructured,
    /// so a field the type gains does not compile until it is listed here.
    const fn honoured(&self) -> [Availability; 19] {
        let Self {
            sets_title,
            sets_position,
            sets_size,
            sets_constraints,
            sets_resizable,
            sets_decorated,
            minimizes,
            maximizes,
            enters_fullscreen,
            hides,
            shows,
            activates,
            closes,
            observes_position,
            observes_minimized,
            places_at_open,
            frame_rate_hint,
            material,
            reports_presentation,
            absences: _,
            presentation_absence: _,
            owner: _,
            revision: _,
        } = *self;
        [
            sets_title,
            sets_position,
            sets_size,
            sets_constraints,
            sets_resizable,
            sets_decorated,
            minimizes,
            maximizes,
            enters_fullscreen,
            hides,
            shows,
            activates,
            closes,
            observes_position,
            observes_minimized,
            places_at_open,
            frame_rate_hint,
            material,
            reports_presentation,
        ]
    }
}

impl WindowCapabilities {
    /// A window that honours no command and observes nothing.
    pub const NONE: Self = Self {
        sets_title: Availability::Unavailable,
        sets_position: Availability::Unavailable,
        sets_size: Availability::Unavailable,
        sets_constraints: Availability::Unavailable,
        sets_resizable: Availability::Unavailable,
        sets_decorated: Availability::Unavailable,
        minimizes: Availability::Unavailable,
        maximizes: Availability::Unavailable,
        enters_fullscreen: Availability::Unavailable,
        hides: Availability::Unavailable,
        shows: Availability::Unavailable,
        activates: Availability::Unavailable,
        closes: Availability::Unavailable,
        observes_position: Availability::Unavailable,
        observes_minimized: Availability::Unavailable,
        places_at_open: Availability::Unavailable,
        frame_rate_hint: Availability::Unavailable,
        material: Availability::Unavailable,
        reports_presentation: Availability::Unavailable,
        absences: NOT_ON_THIS_PLATFORM,
        presentation_absence: CapabilityAbsence::NotOnThisPlatform,
        owner: None,
        revision: WindowRevision::ZERO,
    };

    /// A window whose window system honours every command and reports
    /// everything itself.
    pub const ALL: Self = Self {
        sets_title: Availability::Native,
        sets_position: Availability::Native,
        sets_size: Availability::Native,
        sets_constraints: Availability::Native,
        sets_resizable: Availability::Native,
        sets_decorated: Availability::Native,
        minimizes: Availability::Native,
        maximizes: Availability::Native,
        enters_fullscreen: Availability::Native,
        hides: Availability::Native,
        shows: Availability::Native,
        activates: Availability::Native,
        closes: Availability::Native,
        observes_position: Availability::Native,
        observes_minimized: Availability::Native,
        places_at_open: Availability::Native,
        frame_rate_hint: Availability::Native,
        material: Availability::Native,
        reports_presentation: Availability::Native,
        absences: NOT_ON_THIS_PLATFORM,
        presentation_absence: CapabilityAbsence::NotOnThisPlatform,
        owner: None,
        revision: WindowRevision::ZERO,
    };

    /// The snapshot revision at which what these capabilities honour was last
    /// published: it moves when a field changes, not with every state change.
    /// [`ZERO`](WindowRevision::ZERO) for capabilities built by hand or
    /// unchanged since the window's first snapshot.
    #[must_use]
    pub const fn revision(&self) -> WindowRevision {
        self.revision
    }

    /// These capabilities as the ones `owner`'s snapshots publish: what a host
    /// stamps on a window's first snapshot, which
    /// [`WindowState::successor`] then carries to every later one.
    #[must_use]
    pub const fn owned_by(mut self, owner: CapabilitiesOwner) -> Self {
        self.owner = Some(owner);
        self
    }

    /// How this window honours `command`.
    ///
    /// [`WindowMode::Normal`] is always [`Native`](Availability::Native):
    /// asking a window for the mode every window has is never a no-op that
    /// pretends.
    #[must_use]
    pub const fn availability(&self, command: &WindowCommand) -> Availability {
        match Self::slot(command) {
            Some(slot) => self.field(slot),
            None => Availability::Native,
        }
    }

    /// Whether this window honours `command` at all, natively or emulated.
    #[must_use]
    pub const fn permits(&self, command: &WindowCommand) -> bool {
        self.availability(command).is_available()
    }

    /// These capabilities with `command`'s capability `Unavailable` because of
    /// `absence`, which admission then answers for `command`.
    ///
    /// The absence says where the capability is missing:
    /// [`NotOnThisDevice`](CapabilityAbsence::NotOnThisDevice) where the
    /// platform has it and this device — its operating system build or its
    /// hardware — lacks it,
    /// [`NotYetOnThisHost`](CapabilityAbsence::NotYetOnThisHost) where the
    /// operating system has it and the host does not implement it yet, and
    /// [`NotOnThisPlatform`](CapabilityAbsence::NotOnThisPlatform), what a
    /// field set to `Unavailable` by hand answers, where no device of the
    /// platform has it. The mode every window has ([`WindowMode::Normal`])
    /// cannot be taken away.
    ///
    /// ```
    /// use flui_platform::{
    ///     CapabilityAbsence, UnsupportedReason, WindowCapabilities, WindowCommand, WindowMaterial,
    /// };
    ///
    /// let material = WindowCommand::SetMaterial(WindowMaterial::System);
    /// let capabilities =
    ///     WindowCapabilities::ALL.unavailable(&material, CapabilityAbsence::NotYetOnThisHost);
    /// let refused = material.admit(&capabilities).unwrap_err();
    /// assert_eq!(
    ///     refused.unsupported().map(|absent| absent.reason),
    ///     Some(UnsupportedReason::NotYetOnThisHost)
    /// );
    /// ```
    #[must_use]
    pub const fn unavailable(
        mut self,
        command: &WindowCommand,
        absence: CapabilityAbsence,
    ) -> Self {
        let Some(slot) = Self::slot(command) else {
            return self;
        };
        *self.field_mut(slot) = Availability::Unavailable;
        self.absences[slot as usize] = absence;
        self
    }

    /// Why admission refuses `command` when its capability is `Unavailable`:
    /// the absence [`unavailable`](Self::unavailable) gave it or, for a field
    /// set by hand, [`NotOnThisPlatform`](CapabilityAbsence::NotOnThisPlatform).
    /// While the capability is available there is no refusal, and this answers
    /// `NotOnThisPlatform` too.
    #[must_use]
    pub const fn refusal(&self, command: &WindowCommand) -> CapabilityAbsence {
        match Self::slot(command) {
            Some(slot) => match self.absence_in_force(slot) {
                Some(absence) => absence,
                None => CapabilityAbsence::NotOnThisPlatform,
            },
            None => CapabilityAbsence::NotOnThisPlatform,
        }
    }

    /// These capabilities with [`reports_presentation`](Self::reports_presentation)
    /// `Unavailable` because of `absence`, which
    /// [`presentation_feedback`](Self::presentation_feedback) then answers.
    ///
    /// ```
    /// use flui_platform::{CapabilityAbsence, UnsupportedReason, WindowCapabilities};
    ///
    /// let capabilities = WindowCapabilities::ALL
    ///     .without_presentation_feedback(CapabilityAbsence::NotYetOnThisHost);
    /// assert_eq!(
    ///     capabilities.presentation_feedback().map_err(|absent| absent.reason),
    ///     Err(UnsupportedReason::NotYetOnThisHost)
    /// );
    /// ```
    #[must_use]
    pub const fn without_presentation_feedback(mut self, absence: CapabilityAbsence) -> Self {
        self.reports_presentation = Availability::Unavailable;
        self.presentation_absence = absence;
        self
    }

    /// How the window reports that its frames reached the screen
    /// ([`FrameRequest::presented`](crate::FrameRequest::presented)), or why it
    /// does not.
    ///
    /// # Errors
    /// [`Unsupported`] while [`reports_presentation`](Self::reports_presentation)
    /// is `Unavailable`, with the absence
    /// [`without_presentation_feedback`](Self::without_presentation_feedback)
    /// recorded, or the platform's for a field set by hand.
    pub const fn presentation_feedback(&self) -> Result<Availability, Unsupported> {
        match self.presentation_absence_in_force() {
            Some(absence) => Err(Unsupported::new(absence.reason())),
            None => Ok(self.reports_presentation),
        }
    }

    /// The presentation report's absence, while it is `Unavailable`.
    const fn presentation_absence_in_force(&self) -> Option<CapabilityAbsence> {
        match self.reports_presentation {
            Availability::Unavailable => Some(self.presentation_absence),
            Availability::Native | Availability::Emulated => None,
        }
    }

    /// The absence recorded for `slot`, while its capability is `Unavailable`;
    /// `None` while it is available, where a recorded absence means nothing.
    const fn absence_in_force(&self, slot: Slot) -> Option<CapabilityAbsence> {
        match self.field(slot) {
            Availability::Unavailable => Some(self.absences[slot as usize]),
            Availability::Native | Availability::Emulated => None,
        }
    }

    /// Every slot's absence in force, for equality and hashing: an absence left
    /// behind in a slot that became available again is no difference.
    const fn absences_in_force(&self) -> [Option<CapabilityAbsence>; Slot::COUNT] {
        let mut in_force = [None; Slot::COUNT];
        let mut at = 0;
        while at < Slot::COUNT {
            in_force[at] = self.absence_in_force(Slot::ALL[at]);
            at += 1;
        }
        in_force
    }

    /// The capability `command` is admitted against; `None` for the mode every
    /// window has. Exhaustive, so a command added to [`WindowCommand`] does
    /// not compile until it names its slot.
    const fn slot(command: &WindowCommand) -> Option<Slot> {
        Some(match command {
            WindowCommand::SetTitle(_) => Slot::SetTitle,
            WindowCommand::SetPosition(_) => Slot::SetPosition,
            WindowCommand::SetSize(_) => Slot::SetSize,
            WindowCommand::SetConstraints(_) => Slot::SetConstraints,
            WindowCommand::SetResizable(_) => Slot::SetResizable,
            WindowCommand::SetDecorated(_) => Slot::SetDecorated,
            WindowCommand::SetMode(WindowMode::Minimized) => Slot::Minimize,
            WindowCommand::SetMode(WindowMode::Maximized) => Slot::Maximize,
            WindowCommand::SetMode(WindowMode::Fullscreen) => Slot::EnterFullscreen,
            WindowCommand::SetMode(WindowMode::Normal) => return None,
            WindowCommand::Hide => Slot::Hide,
            WindowCommand::Show => Slot::Show,
            WindowCommand::Activate => Slot::Activate,
            WindowCommand::Close => Slot::Close,
            WindowCommand::SetMaterial(_) => Slot::SetMaterial,
        })
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
    /// not finite, a size that is not positive, or constraints whose minimum
    /// exceeds their maximum in either dimension.
    pub fn validate(&self) -> Result<(), AdmissionError> {
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
    /// [`Availability::Unavailable`] in `capabilities`: the window's platform
    /// does not take it, with the reason [`WindowCapabilities::refusal`] gives.
    pub fn admit(
        self,
        capabilities: &WindowCapabilities,
    ) -> Result<AdmittedCommand, AdmissionError> {
        self.validate()?;
        if !capabilities.permits(&self) {
            return Err(Unsupported::from(capabilities.refusal(&self)).into());
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
    /// [`Availability::Unavailable`].
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
#[allow(
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
