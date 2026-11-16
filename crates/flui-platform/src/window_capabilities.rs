//! Which commands a window honours and which reports it makes, and why each
//! one it lacks is missing (ADR-0254 §1 and §6).
//!
//! A capability is one value, a [`WindowCapability`]: how the window has it,
//! or the lasting reason it does not. Nothing else records the reason, so a
//! capability cannot be missing for one reason in the snapshot and another
//! in a refusal.

use std::num::NonZeroU64;

use flui_foundation::{AtomicIdMint, IdExhausted};

use crate::{
    Availability, CapabilityAbsence, WindowCommand, WindowFrame, WindowMode, WindowRevision,
};

/// How a window has one capability, or why it does not.
///
/// What a window has it has [`Native`](Self::Native), when the window system
/// carries the command out (or makes the report) itself, or
/// [`Emulated`](Self::Emulated), when the backend builds it from what the
/// window system does provide (a fullscreen made of a borderless window sized
/// to its monitor); a boolean could not say "only approximately". What it
/// lacks is [`Absent`](Self::Absent) with the reason, which no later attempt
/// changes: a capability stays absent until the window publishes it again.
///
/// ```
/// use flui_platform::{CapabilityAbsence, WindowCapability};
///
/// let blur = WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST);
/// assert!(!blur.is_available());
/// assert!(WindowCapability::Emulated.is_available());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability is the window system's, made up by the host, or absent; there is no fourth way"
)]
pub enum WindowCapability {
    /// The window system does it itself.
    Native,
    /// The backend builds it from what the window system provides, and it is
    /// only as good as that.
    Emulated,
    /// The window does not have it, for this reason.
    Absent(CapabilityAbsence),
}

impl WindowCapability {
    /// Absent because no device of the platform has it: what a window that
    /// honours nothing answers for each capability.
    pub const NOT_ON_THIS_PLATFORM: Self = Self::Absent(CapabilityAbsence::NOT_ON_THIS_PLATFORM);

    /// Whether the window has the capability at all, natively or emulated.
    #[must_use]
    pub const fn is_available(self) -> bool {
        !matches!(self, Self::Absent(_))
    }
}

/// Which commands a window honours and which reports it makes, and how: the
/// [`CapabilityParts`] a window published, read through `Deref`
/// (`capabilities.material`), and which publication they are.
///
/// A backend reports exactly what it does today. A command is refused by
/// [`WindowCommand::admit`] as
/// [`AdmissionError::Unsupported`](crate::AdmissionError::Unsupported) exactly
/// when its capability is [`Absent`](WindowCapability::Absent), with the
/// reason the capability carries: the platform's where no device has it, the
/// device's where its operating system build or its hardware lacks it, the
/// host's where the operating system has it and the host does not implement
/// it yet. A command the capabilities admit can still fail in the operating
/// system, which [`ApplyError::Platform`](crate::ApplyError::Platform)
/// reports: that is a failure, not a missing capability. A backend names what
/// its window does as [`CapabilityParts`], starting from
/// [`NONE`](Self::NONE)'s, and turns them into capabilities with
/// [`CapabilityParts::into_capabilities`].
///
/// The reports a window makes,
/// [`reports_presentation`](CapabilityParts::reports_presentation),
/// [`reports_keyboard_inset`](CapabilityParts::reports_keyboard_inset) and
/// [`reports_system_gestures`](CapabilityParts::reports_system_gestures), carry
/// their reason the same way: no verb answers for them, so the field is where
/// a caller learns why frames come without
/// [`FrameRequest::presented`](crate::FrameRequest::presented) or why those
/// insets stay zero.
///
/// Four fields are a plain [`Availability`]: what the window observes or can
/// be placed at ([`observes_position`](CapabilityParts::observes_position),
/// [`observes_minimized`](CapabilityParts::observes_minimized),
/// [`places_at_open`](CapabilityParts::places_at_open)), which nothing
/// refuses, and the one verb a window takes outside the command queue,
/// [`frame_rate_hint`](CapabilityParts::frame_rate_hint), which answers
/// [`Unsupported`](crate::Unsupported) exactly when it is `Unavailable` and
/// whose reason is the verb's own answer: nothing admits a verb against the
/// capabilities, so a reason kept here would be a second copy of what the
/// host's verb already says. The same holds for the input regions, whose
/// capability lives in the input capabilities.
///
/// # Publication
///
/// Capabilities a window's snapshot publishes are one *publication*: an
/// identity this crate mints, when a host publishes a window's first snapshot
/// ([`publish`](Self::publish)) and whenever
/// [`WindowState::successor`](crate::WindowState::successor) publishes
/// capabilities that differ from the ones before. No caller can name it,
/// build it or move it onto other capabilities: the fields cannot be written
/// through a `WindowCapabilities`, and every way to change one
/// ([`unavailable`](Self::unavailable), [`into_parts`](Self::into_parts))
/// yields capabilities of no publication. A copy of a window's published
/// capabilities is the publication itself, so a value that carries one holds
/// exactly what that window published.
///
/// An [`AdmittedCommand`](crate::AdmittedCommand) is bound to the publication
/// it was admitted against, and opens only under it: a command checked against
/// capabilities the window no longer has, another window's, or capabilities
/// no window published is
/// [`ApplyError::Superseded`](crate::ApplyError::Superseded), never applied.
///
/// Equality and hashing compare what is honoured and why not, not the
/// publication or the [`revision`](Self::revision): those say which window
/// published the capabilities and when.
#[derive(Debug, Clone, Copy)]
pub struct WindowCapabilities {
    /// What the window honours and why not.
    parts: CapabilityParts,
    /// The publication these capabilities are; `None` for capabilities no
    /// window published.
    publication: Option<Publication>,
    /// The snapshot revision at which these capabilities were published;
    /// kept by [`WindowState::successor`](crate::WindowState::successor), zero
    /// for capabilities no snapshot has published.
    revision: WindowRevision,
}

/// One publication of a window's capabilities, unique in the process: minted
/// only by [`WindowCapabilities::publish`] and
/// [`WindowState::successor`](crate::WindowState::successor), never built from
/// a number and never reissued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Publication(NonZeroU64);

/// The publications issued so far in the process.
static PUBLICATIONS: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();

impl Publication {
    /// A publication no capabilities had before.
    ///
    /// # Errors
    /// [`IdExhausted`] once the process issued every publication it can; the
    /// mint never reissues one.
    pub(crate) fn mint() -> Result<Self, IdExhausted> {
        PUBLICATIONS.next().map(Self)
    }
}

impl std::ops::Deref for WindowCapabilities {
    type Target = CapabilityParts;

    /// What the window honours and why not, to read. Nothing writes through
    /// it: a field changes by taking the capabilities apart
    /// ([`into_parts`](Self::into_parts)), which leaves the publication behind.
    fn deref(&self) -> &CapabilityParts {
        &self.parts
    }
}

impl PartialEq for WindowCapabilities {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Eq for WindowCapabilities {}

impl std::hash::Hash for WindowCapabilities {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.parts.hash(state);
    }
}

/// Declares the capabilities a [`WindowCommand`] is admitted against, once:
/// the [`Slot`] of each and the field it reads.
/// [`WindowCapabilities::slot`] maps every command to one with an exhaustive
/// match, so a new command does not compile until it names a slot here.
macro_rules! command_capabilities {
    ($($slot:ident => $field:ident),+ $(,)?) => {
        /// A capability a [`WindowCommand`] is admitted against.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Slot {
            $($slot),+
        }

        impl CapabilityParts {
            /// The field `slot` reads.
            const fn field(&self, slot: Slot) -> WindowCapability {
                match slot {
                    $(Slot::$slot => self.$field),+
                }
            }

            /// The field `slot` reads, to set.
            const fn field_mut(&mut self, slot: Slot) -> &mut WindowCapability {
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
    SetFrame => sets_frame,
    SetAppTitleBar => app_title_bar,
    Minimize => minimizes,
    Maximize => maximizes,
    EnterFullscreen => enters_fullscreen,
    Hide => hides,
    Show => shows,
    Activate => activates,
    Close => closes,
    SetMaterial => material,
}

impl WindowCapabilities {
    /// A window that honours no command, reports nothing and observes
    /// nothing, on a platform where no device does.
    pub const NONE: Self = CapabilityParts {
        sets_title: WindowCapability::NOT_ON_THIS_PLATFORM,
        sets_position: WindowCapability::NOT_ON_THIS_PLATFORM,
        sets_size: WindowCapability::NOT_ON_THIS_PLATFORM,
        sets_constraints: WindowCapability::NOT_ON_THIS_PLATFORM,
        sets_resizable: WindowCapability::NOT_ON_THIS_PLATFORM,
        sets_frame: WindowCapability::NOT_ON_THIS_PLATFORM,
        app_title_bar: WindowCapability::NOT_ON_THIS_PLATFORM,
        minimizes: WindowCapability::NOT_ON_THIS_PLATFORM,
        maximizes: WindowCapability::NOT_ON_THIS_PLATFORM,
        enters_fullscreen: WindowCapability::NOT_ON_THIS_PLATFORM,
        hides: WindowCapability::NOT_ON_THIS_PLATFORM,
        shows: WindowCapability::NOT_ON_THIS_PLATFORM,
        activates: WindowCapability::NOT_ON_THIS_PLATFORM,
        closes: WindowCapability::NOT_ON_THIS_PLATFORM,
        material: WindowCapability::NOT_ON_THIS_PLATFORM,
        reports_presentation: WindowCapability::NOT_ON_THIS_PLATFORM,
        reports_keyboard_inset: WindowCapability::NOT_ON_THIS_PLATFORM,
        reports_system_gestures: WindowCapability::NOT_ON_THIS_PLATFORM,
        observes_position: Availability::Unavailable,
        observes_minimized: Availability::Unavailable,
        places_at_open: Availability::Unavailable,
        frame_rate_hint: Availability::Unavailable,
    }
    .into_capabilities();

    /// A window whose window system honours every command and reports
    /// everything itself. No host is such a window: a host starts from
    /// [`NONE`](Self::NONE), so a capability the contract gains is absent
    /// there until the host carries it out.
    pub const ALL: Self = CapabilityParts {
        sets_title: WindowCapability::Native,
        sets_position: WindowCapability::Native,
        sets_size: WindowCapability::Native,
        sets_constraints: WindowCapability::Native,
        sets_resizable: WindowCapability::Native,
        sets_frame: WindowCapability::Native,
        app_title_bar: WindowCapability::Native,
        minimizes: WindowCapability::Native,
        maximizes: WindowCapability::Native,
        enters_fullscreen: WindowCapability::Native,
        hides: WindowCapability::Native,
        shows: WindowCapability::Native,
        activates: WindowCapability::Native,
        closes: WindowCapability::Native,
        material: WindowCapability::Native,
        reports_presentation: WindowCapability::Native,
        reports_keyboard_inset: WindowCapability::Native,
        reports_system_gestures: WindowCapability::Native,
        observes_position: Availability::Native,
        observes_minimized: Availability::Native,
        places_at_open: Availability::Native,
        frame_rate_hint: Availability::Native,
    }
    .into_capabilities();

    /// The snapshot revision at which what these capabilities honour was last
    /// published: it moves when a field changes, not with every state change.
    /// [`ZERO`](WindowRevision::ZERO) for capabilities built by hand or
    /// unchanged since the window's first snapshot.
    #[must_use]
    pub const fn revision(&self) -> WindowRevision {
        self.revision
    }

    /// These capabilities as a new publication, one no capabilities in the
    /// process were before: what a host stamps on a window's first snapshot,
    /// which [`WindowState::successor`](crate::WindowState::successor) then
    /// carries while the capabilities stay the same. Publishing again gives
    /// another publication, which no token admitted before opens under.
    ///
    /// # Errors
    /// [`IdExhausted`] once the process issued every publication it can.
    pub fn publish(mut self) -> Result<Self, IdExhausted> {
        self.publication = Some(Publication::mint()?);
        Ok(self)
    }

    /// The publication these capabilities are, for the admission that binds
    /// a token to it.
    pub(crate) const fn publication(&self) -> Option<Publication> {
        self.publication
    }

    /// These capabilities as a sample of the window that published `previous`:
    /// the publication and revision `previous` carries, whatever the sample's.
    pub(crate) const fn carried_from(mut self, previous: &Self) -> Self {
        self.publication = previous.publication;
        self.revision = previous.revision;
        self
    }

    /// These capabilities as the publication a window's snapshot at `revision`
    /// makes of them, because they differ from the ones it published before.
    ///
    /// # Errors
    /// [`IdExhausted`] once the process issued every publication it can.
    pub(crate) fn republished(mut self, revision: WindowRevision) -> Result<Self, IdExhausted> {
        self.publication = Some(Publication::mint()?);
        self.revision = revision;
        Ok(self)
    }

    /// How this window honours `command`, or why it does not.
    ///
    /// [`WindowMode::Normal`] is always [`Native`](WindowCapability::Native):
    /// asking a window for the mode every window has is never a no-op that
    /// pretends.
    #[must_use]
    pub const fn capability(&self, command: &WindowCommand) -> WindowCapability {
        match Self::slot(command) {
            Some(slot) => self.parts.field(slot),
            None => WindowCapability::Native,
        }
    }

    /// Whether this window honours `command` at all, natively or emulated.
    #[must_use]
    pub const fn permits(&self, command: &WindowCommand) -> bool {
        self.capability(command).is_available()
    }

    /// These capabilities with `command`'s capability absent because of
    /// `absence`, which admission then answers for `command`: the field the
    /// command is admitted against, set to
    /// [`Absent`](WindowCapability::Absent).
    ///
    /// The absence says where the capability is missing:
    /// [`NotOnThisDevice`](CapabilityAbsence::NOT_ON_THIS_DEVICE) where the
    /// platform has it and this device — its operating system build or its
    /// hardware — lacks it,
    /// [`NotYetOnThisHost`](CapabilityAbsence::NOT_YET_ON_THIS_HOST) where the
    /// operating system has it and the host does not implement it yet, and
    /// [`NotOnThisPlatform`](CapabilityAbsence::NOT_ON_THIS_PLATFORM) where no
    /// device of the platform has it. The mode every window has
    /// ([`WindowMode::Normal`]) cannot be taken away. The result is an edit, so
    /// it is of no publication: a token admitted against it opens on no window.
    ///
    /// ```
    /// use flui_platform::{
    ///     CapabilityAbsence, UnsupportedReason, WindowCapabilities, WindowCapability,
    ///     WindowCommand, WindowMaterial,
    /// };
    ///
    /// let material = WindowCommand::SetMaterial(WindowMaterial::System);
    /// let capabilities =
    ///     WindowCapabilities::ALL.unavailable(&material, CapabilityAbsence::NOT_YET_ON_THIS_HOST);
    /// assert_eq!(
    ///     capabilities.material,
    ///     WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST)
    /// );
    /// let refused = material.admit(&capabilities).unwrap_err();
    /// assert_eq!(
    ///     refused.unsupported().map(|absent| absent.reason),
    ///     Some(UnsupportedReason::NotYetOnThisHost)
    /// );
    /// ```
    #[must_use]
    pub const fn unavailable(self, command: &WindowCommand, absence: CapabilityAbsence) -> Self {
        let mut parts = self.parts;
        if let Some(slot) = Self::slot(command) {
            *parts.field_mut(slot) = WindowCapability::Absent(absence);
        }
        parts.into_capabilities()
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
            WindowCommand::SetFrame(WindowFrame::AppTitleBar(_)) => Slot::SetAppTitleBar,
            WindowCommand::SetFrame(WindowFrame::System | WindowFrame::None) => Slot::SetFrame,
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

    /// Take the capabilities apart into what they honour and why not, leaving
    /// the publication and the revision behind: parts changed and turned back
    /// into capabilities are capabilities no window published.
    #[must_use]
    pub const fn into_parts(self) -> CapabilityParts {
        self.parts
    }
}

/// Every capability of a [`WindowCapabilities`], public: what a backend names
/// its window's capabilities as, what a reader reads through
/// [`WindowCapabilities`], and the boundary every serialization and mirror of
/// a window's capabilities goes through (a recording's schema), for the same
/// reason as [`WindowStateParts`](crate::WindowStateParts).
///
/// Deliberately not `#[non_exhaustive]`, unlike [`WindowCapabilities`]: a
/// mirror builds and destructures this struct exhaustively, so a capability
/// the contract gains breaks every mirror at compile time until it carries
/// the capability. Which publication capabilities are, and at which revision,
/// is the publishing window's
/// ([`WindowState::successor`](crate::WindowState::successor)), so it is not a
/// field here: parts are what any caller may write, and capabilities built from
/// them belong to no window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityParts {
    /// [`WindowCommand::SetTitle`].
    pub sets_title: WindowCapability,
    /// [`WindowCommand::SetPosition`].
    pub sets_position: WindowCapability,
    /// [`WindowCommand::SetSize`].
    pub sets_size: WindowCapability,
    /// [`WindowCommand::SetConstraints`].
    pub sets_constraints: WindowCapability,
    /// [`WindowCommand::SetResizable`].
    pub sets_resizable: WindowCapability,
    /// [`WindowCommand::SetFrame`] to [`WindowFrame::System`] or
    /// [`WindowFrame::None`]: the system's frame put on or taken off.
    pub sets_frame: WindowCapability,
    /// [`WindowFrame::AppTitleBar`], at open ([`WindowOptions::frame`](crate::WindowOptions::frame))
    /// and through [`WindowCommand::SetFrame`]: the window system keeps the
    /// resize borders, shadow and snapping of a frame whose title bar the
    /// application draws. A desktop window opened with it where this is absent
    /// opens with [`WindowFrame::System`]. Hosts without a system frame, such as
    /// mobile and web, retain their native frame policy.
    pub app_title_bar: WindowCapability,
    /// [`WindowMode::Minimized`] through [`WindowCommand::SetMode`].
    pub minimizes: WindowCapability,
    /// [`WindowMode::Maximized`] through [`WindowCommand::SetMode`].
    pub maximizes: WindowCapability,
    /// [`WindowMode::Fullscreen`] through [`WindowCommand::SetMode`].
    pub enters_fullscreen: WindowCapability,
    /// [`WindowCommand::Hide`].
    pub hides: WindowCapability,
    /// [`WindowCommand::Show`].
    pub shows: WindowCapability,
    /// [`WindowCommand::Activate`]: bring the window to the front and request
    /// focus, under the window system's focus rules. `Emulated` where the
    /// window can only ask and the window system decides from what the
    /// request carries (a Wayland activation token).
    pub activates: WindowCapability,
    /// [`WindowCommand::Close`]: destroy the window. A window whose platform
    /// owns its lifetime (an Android activity) cannot close itself.
    pub closes: WindowCapability,
    /// [`WindowCommand::SetMaterial`]: the window system draws every
    /// [`WindowMaterial`](crate::WindowMaterial) behind the window.
    pub material: WindowCapability,
    /// [`FrameRequest::presented`](crate::FrameRequest::presented): the window
    /// reports when the frames presented into its surface reached the screen,
    /// on the event timeline.
    pub reports_presentation: WindowCapability,
    /// [`WindowInsets::keyboard`](crate::WindowInsets::keyboard): the window
    /// reports where the soft keyboard covers it. A zero keyboard inset from
    /// a window that does not report it is unknown, not a hidden keyboard.
    pub reports_keyboard_inset: WindowCapability,
    /// [`WindowInsets::system_gestures`](crate::WindowInsets::system_gestures):
    /// the window reports the edges system gestures take.
    pub reports_system_gestures: WindowCapability,
    /// The snapshot's [`WindowGeometry::position`](crate::WindowGeometry::position)
    /// is reported.
    pub observes_position: Availability,
    /// [`WindowMode::Minimized`] is ever published.
    pub observes_minimized: Availability,
    /// A saved position can be applied again when the window opens.
    pub places_at_open: Availability,
    /// [`PlatformWindow::set_frame_rate_hint`](crate::PlatformWindow::set_frame_rate_hint):
    /// the window system takes a refresh-rate request for the window.
    pub frame_rate_hint: Availability,
}

impl CapabilityParts {
    /// The capabilities these fields describe, of no publication and at
    /// [`WindowRevision::ZERO`], as capabilities built by hand are: a token
    /// admitted against them opens on no window.
    #[must_use]
    pub const fn into_capabilities(self) -> WindowCapabilities {
        WindowCapabilities {
            parts: self,
            publication: None,
            revision: WindowRevision::ZERO,
        }
    }
}
