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
/// [`CapabilityParts`] a window published, each read by its accessor
/// (`capabilities.material()`), and which publication they are (ADR-0341).
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
/// [`reports_presentation`](Self::reports_presentation),
/// [`reports_keyboard_inset`](Self::reports_keyboard_inset),
/// [`reports_system_gestures`](Self::reports_system_gestures) and
/// [`reports_occlusion`](Self::reports_occlusion), carry
/// their reason the same way: no verb answers for them, so the field is where
/// a caller learns why frames come without
/// [`FrameRequest::presented`](crate::FrameRequest::presented), why those
/// insets stay zero or why a covered window reads unoccluded.
///
/// Four fields are a plain [`Availability`]: what the window observes or can
/// be placed at ([`observes_position`](Self::observes_position),
/// [`observes_minimized`](Self::observes_minimized),
/// [`places_at_open`](Self::places_at_open)), which nothing
/// refuses, and the one verb a window takes outside the command queue,
/// [`frame_rate_hint`](Self::frame_rate_hint), which answers
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
/// capabilities that differ from the ones before a published snapshot held.
/// No caller can name it, build it or move it onto other capabilities: the
/// fields are read by value and never written through a `WindowCapabilities`,
/// and every way to change one ([`unavailable`](Self::unavailable),
/// [`into_parts`](Self::into_parts)) yields capabilities of no publication,
/// as the constants and [`CapabilityParts::into_capabilities`] are. A copy of
/// a window's published capabilities is the publication itself, so a value
/// that carries one holds exactly what that window published.
///
/// [`WindowCommand::admit`] refuses capabilities of no publication
/// ([`AdmissionError::Unpublished`](crate::AdmissionError::Unpublished)), and
/// an [`AdmittedCommand`](crate::AdmittedCommand) is bound to the publication
/// it was admitted against and opens only under it: a command checked against
/// capabilities the window no longer has, or another window's, is
/// [`ApplyError::Superseded`](crate::ApplyError::Superseded), never applied.
///
/// Equality and hashing compare what is honoured and why not, not the
/// publication or the [`revision`](Self::revision): those say which window
/// published the capabilities and when.
#[derive(Debug, Clone, Copy)]
pub struct WindowCapabilities {
    /// What the window honours and why not.
    parts: CapabilityParts,
    /// Whether these capabilities are a publication, and which, or were
    /// withdrawn.
    edition: Edition,
    /// The snapshot revision at which these capabilities were published;
    /// kept by [`WindowState::successor`](crate::WindowState::successor), zero
    /// for capabilities no snapshot has published.
    revision: WindowRevision,
}

/// Whether capabilities are a window's publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edition {
    /// Built by hand, edited or computed: no window published them.
    Unpublished,
    /// This publication.
    Published(Publication),
    /// A window held them as a publication and can publish no more
    /// (`WindowCapabilities::withdraw`): every command is absent as withdrawn,
    /// and every later snapshot of the window keeps it so.
    Withdrawn,
}

/// One publication of a window's capabilities, unique in the process: minted
/// only by [`WindowCapabilities::publish`] and
/// [`WindowState::successor`](crate::WindowState::successor), never built from
/// a number and never reissued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Publication(NonZeroU64);

/// Where capability publications come from (ADR-0341). There is one issuer,
/// the process's ([`PublicationMint::process`]), which every host publishes
/// from: two issuers handing out the same number would make two publications
/// one, so no other issuing mint exists. Behind the `testing` feature,
/// [`PublicationMint::exhausted`] is one that issues nothing, so a test can
/// drive what a window does once the process has no publication left.
#[derive(Debug, Clone, Copy)]
pub struct PublicationMint(Option<&'static AtomicIdMint<NonZeroU64>>);

/// The process's publications.
static PUBLICATIONS: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();

impl PublicationMint {
    /// The process's mint, which every host publishes from.
    #[must_use]
    pub const fn process() -> Self {
        Self(Some(&PUBLICATIONS))
    }

    /// A mint that has issued everything it can: every publication it is
    /// asked for is refused with [`IdExhausted`], and it never issues a number
    /// the process's mint issues.
    #[cfg(feature = "testing")]
    #[must_use]
    pub const fn exhausted() -> Self {
        Self(None)
    }

    /// A publication no capabilities had before.
    ///
    /// # Errors
    /// [`IdExhausted`] once the mint issued every publication it can; it never
    /// reissues one.
    fn mint(self) -> Result<Publication, IdExhausted> {
        self.0.ok_or(IdExhausted)?.next().map(Publication)
    }
}

/// Declares [`CapabilityParts`] and, from the same field list, one read-only
/// accessor of [`WindowCapabilities`] per field, by value, so a capability
/// cannot be added to the parts without its accessor. None of the accessors
/// yields capabilities, so nothing read from a window's capabilities can stand
/// in for them.
macro_rules! capability_parts {
    (
        $(#[$meta:meta])*
        pub struct CapabilityParts {
            $($(#[$field_meta:meta])* pub $field:ident: $ty:ty,)+
        }
    ) => {
        $(#[$meta])*
        pub struct CapabilityParts {
            $($(#[$field_meta])* pub $field: $ty,)+
        }

        impl WindowCapabilities {
            $(
                #[doc = concat!("[`CapabilityParts::", stringify!($field), "`] of these capabilities.")]
                #[must_use]
                pub const fn $field(&self) -> $ty {
                    self.parts.$field
                }
            )+
        }
    };
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

            /// Every field a command is admitted against, absent because the
            /// window withdrew its capabilities; the reports, observations and
            /// verbs no command reads keep their values.
            const fn withdraw_commands(&mut self) {
                $(self.$field = WindowCapability::Absent(CapabilityAbsence::WITHDRAWN);)+
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
        reports_occlusion: WindowCapability::NOT_ON_THIS_PLATFORM,
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
        reports_occlusion: WindowCapability::Native,
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
    /// another publication, which no token admitted before opens under; a
    /// caller can publish capabilities of its own, but a window opens a token
    /// only under its own publication (ADR-0341).
    ///
    /// # Errors
    /// [`IdExhausted`] once the process issued every publication it can.
    pub fn publish(self) -> Result<Self, IdExhausted> {
        self.publish_from(PublicationMint::process())
    }

    /// These capabilities as a new publication of `mint`: [`publish`](Self::publish)
    /// with the mint named, for a host that takes its mint (production passes
    /// [`PublicationMint::process`]).
    ///
    /// # Errors
    /// [`IdExhausted`] once `mint` issued every publication it can.
    pub fn publish_from(mut self, mint: PublicationMint) -> Result<Self, IdExhausted> {
        self.edition = Edition::Published(mint.mint()?);
        Ok(self)
    }

    /// What a window publishes once it can publish no more of what it
    /// honours (ADR-0341): these capabilities with every command absent as
    /// [`WITHDRAWN`](CapabilityAbsence::WITHDRAWN), at this revision, which
    /// [`WindowState::successor`](crate::WindowState::successor) keeps so for
    /// every later snapshot. The reports, observations and verbs no command
    /// reads (`reports_presentation`, `frame_rate_hint`, `places_at_open`,
    /// the `observes_*`) keep their values: a window still serves them. Every
    /// token admitted against the publication these were answers
    /// [`ApplyError::Superseded`](crate::ApplyError::Superseded), and admission
    /// refuses them; no published capabilities carry the withdrawn reason, so
    /// a reader of the snapshot always sees the change.
    #[must_use]
    pub const fn withdraw(self) -> Self {
        let mut parts = self.parts;
        parts.withdraw_commands();
        Self {
            parts,
            edition: Edition::Withdrawn,
            revision: self.revision,
        }
    }

    /// Whether a window withdrew these capabilities because it can publish no
    /// more ([`withdraw`](Self::withdraw)).
    #[must_use]
    pub const fn is_withdrawn(&self) -> bool {
        matches!(self.edition, Edition::Withdrawn)
    }

    /// The publication these capabilities are, for the admission that binds
    /// a token to it; `None` for capabilities no window published and for
    /// withdrawn ones.
    pub(crate) const fn publication(&self) -> Option<Publication> {
        match self.edition {
            Edition::Published(publication) => Some(publication),
            Edition::Unpublished | Edition::Withdrawn => None,
        }
    }

    /// These capabilities as a sample of the window that published `previous`:
    /// the edition and revision `previous` carries, whatever the sample's, and
    /// every command withdrawn where `previous` was withdrawn (the sample's
    /// reports and observations still publish).
    pub(crate) const fn carried_from(mut self, previous: &Self) -> Self {
        if let Edition::Withdrawn = previous.edition {
            self = self.withdraw();
        }
        self.edition = previous.edition;
        self.revision = previous.revision;
        self
    }

    /// These capabilities as the snapshot at `revision` holds them after they
    /// changed from `previous`: a new publication where `previous` was one, or
    /// withdrawn ones where the process has none left; still none where
    /// `previous` was none, so a sample of hand-built capabilities never
    /// becomes admissible by being succeeded.
    pub(crate) fn changed_from(
        mut self,
        previous: &Self,
        revision: WindowRevision,
        mint: PublicationMint,
    ) -> Self {
        self.revision = revision;
        match previous.edition {
            Edition::Published(_) => match mint.mint() {
                Ok(publication) => self.edition = Edition::Published(publication),
                Err(IdExhausted) => return self.withdraw(),
            },
            Edition::Unpublished => self.edition = Edition::Unpublished,
            Edition::Withdrawn => return self.withdraw(),
        }
        self
    }

    /// How this window honours `command`, or why it does not.
    ///
    /// [`WindowMode::Normal`] is [`Native`](WindowCapability::Native) on every
    /// window but one that withdrew its capabilities: asking a window for the
    /// mode every window has is never a no-op that pretends. Withdrawn ones
    /// ([`withdraw`](Self::withdraw)) honour no command, the normal mode
    /// included.
    #[must_use]
    pub const fn capability(&self, command: &WindowCommand) -> WindowCapability {
        match Self::slot(command) {
            Some(slot) => self.parts.field(slot),
            None if self.is_withdrawn() => WindowCapability::Absent(CapabilityAbsence::WITHDRAWN),
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
    /// it is of no publication: a host publishes it before admitting against it.
    ///
    /// ```
    /// use flui_platform::{
    ///     CapabilityAbsence, UnsupportedReason, WindowCapabilities, WindowCapability,
    ///     WindowCommand, WindowMaterial,
    /// };
    ///
    /// let material = WindowCommand::SetMaterial(WindowMaterial::System);
    /// let capabilities = WindowCapabilities::ALL
    ///     .unavailable(&material, CapabilityAbsence::NOT_YET_ON_THIS_HOST)
    ///     .publish()
    ///     .expect("the process has publications left");
    /// assert_eq!(
    ///     capabilities.material(),
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

capability_parts! {
/// Every capability of a [`WindowCapabilities`], public: what a backend names
/// its window's capabilities as, what [`WindowCapabilities::into_parts`] hands
/// a reader, and the boundary every serialization and mirror of
/// a window's capabilities goes through (a recording's schema), for the same
/// reason as [`WindowStateParts`](crate::WindowStateParts).
///
/// Deliberately not `#[non_exhaustive]`: a
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
    /// [`WindowState::occluded`](crate::WindowState::occluded) for a covering
    /// by other windows: the window reads occluded once the windows above it
    /// leave none of it on screen, and unoccluded once they uncover some of
    /// it. Where it is absent, a covered window reads unoccluded, so an
    /// unoccluded snapshot says only that the window system reported nothing
    /// else that hides it (a lock, another workspace, a suspension, a
    /// background page or app), not that the window is seen.
    pub reports_occlusion: WindowCapability,
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
}

impl CapabilityParts {
    /// The capabilities these fields describe, of no publication and at
    /// [`WindowRevision::ZERO`], as capabilities built by hand are: a token
    /// admitted against them opens on no window.
    #[must_use]
    pub const fn into_capabilities(self) -> WindowCapabilities {
        WindowCapabilities {
            parts: self,
            edition: Edition::Unpublished,
            revision: WindowRevision::ZERO,
        }
    }
}
