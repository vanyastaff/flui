//! The outgoing half of drag-and-drop: what a window's OS drag session is asked
//! to do, and how a session names itself to targets (ADR-0211).
//!
//! A drag starts inside its window on pointer capture. When the pointer leaves
//! the window (or at the start, on the web and iOS) and the session may reach
//! beyond the window, the runtime hands the OS an [`OsDragRequest`] through the
//! window's [`PlatformDragSource`]. The OS then offers the drag to every target
//! it finds, FLUI windows included, as [`DragDropEvent`](crate::DragDropEvent)s
//! whose offer carries the session's private identity ([`DragSessionKey`]).
//!
//! The Rust value of a drag never crosses this boundary: an offer names the
//! session, and only the source's own runtime redeems the name for the value.

use std::fmt;
use std::num::NonZeroU64;

use flui_foundation::{DragSessionId, IdExhausted, UiRuntimeId};

use crate::data_transfer::{
    DataTransferOffer, Mime, RepresentationDescriptor, RepresentationIndex, TransferActions,
    TransferError, TransferFormat, TransferPayload,
};
use crate::{Unsupported, UnsupportedReason};

/// Why a drag could not start, or could not continue as asked (ADR-0211 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DragError {
    /// The runtime's session identities are used up. Permanent.
    #[error("the drag session identity space is exhausted")]
    IdExhausted,
    /// Both `within_app` and `outside_app` are empty, so no target could ever
    /// accept the drag and no OS session may advertise an empty action mask.
    #[error("the drag allows no actions inside or outside the app")]
    NoAllowedActions,
    /// The session reaches the system but `outside_app` is empty.
    #[error("the drag reaches the system but allows no actions outside the app")]
    NoOutsideActions,
    /// `outside_app` is non-empty but no item carries a serialized
    /// representation, so no target outside the runtime could redeem it.
    #[error("the drag allows actions outside the app but offers nothing serialized")]
    NoSerializedRepresentation,
    /// The session already concluded, or its owner is gone.
    #[error("the drag session is closed")]
    SessionClosed,
    /// This backend cannot start an OS drag session (X11, a window without a
    /// drag source), or not for this contact. The session stays in-window.
    #[error("no OS drag session can start: {0}")]
    Unsupported(#[from] Unsupported),
}

impl DragError {
    /// What is not here, when that is why the drag could not reach the OS.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::IdExhausted
            | Self::NoAllowedActions
            | Self::NoOutsideActions
            | Self::NoSerializedRepresentation
            | Self::SessionClosed => None,
        }
    }
}

impl From<IdExhausted> for DragError {
    fn from(IdExhausted: IdExhausted) -> Self {
        Self::IdExhausted
    }
}

/// A random identity of one platform instance, so that a session id from
/// another process can never be mistaken for one of this process's.
///
/// Minted once per platform instance by its host and shared by every UI runtime
/// the host runs; never a `static`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessNonce(NonZeroU64);

impl ProcessNonce {
    /// A nonce no other call to this function, in this process or another,
    /// returns with overwhelming probability.
    #[must_use]
    pub fn mint() -> Self {
        use std::hash::{BuildHasher, Hasher};
        // `RandomState` is seeded per instance from the OS where there is one;
        // the address of a local adds entropy where ASLR is the only source.
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        let local = 0_u8;
        hasher.write_usize(std::ptr::from_ref(&local) as usize);
        // A browser page has neither: its std seeds `RandomState` with fixed
        // keys and lays out the same stack, so two pages would mint one nonce
        // and take each other's drags for their own. The page's generator is
        // seeded per page.
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        for _ in 0..2 {
            hasher.write_u64(js_sys::Math::random().to_bits());
        }
        Self(NonZeroU64::new(hasher.finish()).unwrap_or(NonZeroU64::MIN))
    }

    /// A nonce with a chosen value, for a test that needs two hosts to differ
    /// or to agree.
    #[must_use]
    pub const fn from_raw(raw: NonZeroU64) -> Self {
        Self(raw)
    }

    /// The raw value, for the private identity representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for ProcessNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProcessNonce({:016x})", self.0)
    }
}

/// The name of one drag session across presentations and processes:
/// `(ProcessNonce, UiRuntimeId, DragSessionId)` (ADR-0211 §3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DragSessionKey {
    nonce: ProcessNonce,
    runtime: UiRuntimeId,
    session: DragSessionId,
}

/// The media type of the FLUI-private identity representation.
pub const IDENTITY_ESSENCE: &str = "application/x-flui-drag-session";

impl DragSessionKey {
    /// Name the session `session` of `runtime` in the process `nonce`.
    #[must_use]
    pub const fn new(nonce: ProcessNonce, runtime: UiRuntimeId, session: DragSessionId) -> Self {
        Self {
            nonce,
            runtime,
            session,
        }
    }

    /// The process this session lives in.
    #[must_use]
    pub const fn nonce(&self) -> ProcessNonce {
        self.nonce
    }

    /// The runtime whose session table holds the session's value.
    #[must_use]
    pub const fn runtime(&self) -> UiRuntimeId {
        self.runtime
    }

    /// The session within its runtime.
    #[must_use]
    pub const fn session(&self) -> DragSessionId {
        self.session
    }

    /// The private representation that carries this identity in an offer.
    ///
    /// The identity rides in the media type's parameters, so a window reads it
    /// from the offer's descriptors during hover, when the OS wants an answer
    /// at once and no payload could be fetched yet. A backend whose OS cannot
    /// register a type per session may carry the identity natively (Win32's
    /// own data object, UIKit's local context) and still announces this
    /// descriptor.
    #[must_use]
    pub fn representation(&self) -> RepresentationDescriptor {
        let mime = Mime::new(&format!(
            "{IDENTITY_ESSENCE};process={};runtime={}.{};session={}",
            self.nonce.get(),
            self.runtime.index(),
            self.runtime.generation(),
            self.session.get(),
        ));
        RepresentationDescriptor {
            format: TransferFormat::Custom { mime },
            declared_len: Some(0),
        }
    }

    /// The identity an offer carries, if it carries a well-formed one. A
    /// malformed or absent identity reads as a foreign drag.
    #[must_use]
    pub fn from_offer(offer: &DataTransferOffer) -> Option<Self> {
        offer
            .representations()
            .iter()
            .find_map(|descriptor| match &descriptor.format {
                TransferFormat::Custom { mime } => Self::from_mime(mime),
                _ => None,
            })
    }

    fn from_mime(mime: &Mime) -> Option<Self> {
        let mut parts = mime.as_str().split(';');
        if parts.next()? != IDENTITY_ESSENCE {
            return None;
        }
        let (mut process, mut runtime, mut session) = (None, None, None);
        for part in parts {
            let (name, value) = part.split_once('=')?;
            match name {
                "process" => process = value.parse::<u64>().ok().and_then(NonZeroU64::new),
                "runtime" => {
                    let (index, generation) = value.split_once('.')?;
                    runtime = Some((
                        index.parse::<u32>().ok()?,
                        generation.parse::<u32>().ok()?.try_into().ok()?,
                    ));
                }
                "session" => session = value.parse::<u64>().ok().and_then(NonZeroU64::new),
                _ => {}
            }
        }
        let (index, generation) = runtime?;
        Some(Self {
            nonce: ProcessNonce(process?),
            runtime: UiRuntimeId::new_gen(index, generation),
            session: DragSessionId::from_wire(session?),
        })
    }
}

/// When a backend can start an OS drag session (ADR-0211 §11).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a backend starts a session whenever the pointer leaves the window, or only as the gesture starts (ADR-0211 §11)"
)]
pub enum OsDragStart {
    /// Any time: the session starts in-window and escalates when the pointer
    /// leaves the window (Win32, AppKit, Android, Wayland).
    OnLeavingWindow,
    /// Only at the start of the drag, never mid-gesture: HTML drag on the web
    /// (`eager_data`: the browser allows no lazy data, so representations are
    /// materialized at `dragstart`) and `UIDragSession` on iOS.
    AtStart {
        /// Whether representations must be materialized before the session.
        eager_data: bool,
    },
}

/// How an OS drag session ended, as the source sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a session ends with a drop a target took, or with none"
)]
pub enum OsDragEnd {
    /// A target took the drop with this action.
    Dropped(TransferActions),
    /// No target took it, or the user cancelled.
    Cancelled,
}

/// Serves the data of a started OS drag session, on the owner thread, when the
/// OS (or a target in another process) asks for a representation.
pub type OsDragProvider =
    Box<dyn FnMut(RepresentationIndex) -> Result<TransferPayload, TransferError>>;

/// Told once, on the owner thread, how the OS session ended.
pub type OsDragEndHook = Box<dyn FnOnce(OsDragEnd)>;

/// What the runtime asks a window's OS drag session to do. Neither `Send` nor
/// `Sync`: the provider holds owner-thread state, and the backend keeps the
/// request on the owner thread.
#[must_use = "an OS drag request does nothing until a drag source starts it"]
pub struct OsDragRequest {
    /// The session this request starts.
    pub key: DragSessionKey,
    /// The contact whose native event began the drag, for backends whose start
    /// call needs it (AppKit's dragging event, Wayland's grab serial).
    pub pointer: Option<crate::PointerId>,
    /// What the session offers, in the source's preference order, ending with
    /// the identity representation.
    pub representations: Vec<RepresentationDescriptor>,
    /// The actions advertised to the OS. Never empty.
    pub allowed: TransferActions,
    /// Serves a representation by its index in [`Self::representations`].
    pub provider: OsDragProvider,
    /// Told once how the session ended.
    pub on_end: OsDragEndHook,
}

impl fmt::Debug for OsDragRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsDragRequest")
            .field("key", &self.key)
            .field("allowed", &self.allowed)
            .field("representations", &self.representations.len())
            .finish_non_exhaustive()
    }
}

/// A window's way to start an OS drag session.
///
/// Reached through [`PlatformWindow::drag_source`](crate::PlatformWindow::drag_source);
/// a window whose backend has none answers `None`, and
/// [`InputCapabilities::outgoing_drag`](crate::input_capabilities::InputCapabilities::outgoing_drag)
/// says `Unavailable` for it.
pub trait PlatformDragSource: Send + Sync {
    /// When this backend can start a session.
    fn start_timing(&self) -> OsDragStart;

    /// Start the session. Owner thread only, outside any dispatch, borrow or
    /// frame: the host calls it at its outermost quiescent point, because a
    /// backend may run a modal loop inside (`DoDragDrop`).
    ///
    /// # Errors
    ///
    /// [`DragError::Unsupported`] when the OS refuses or cannot (winit's
    /// `NotSupported` on X11); the session then stays in-window.
    fn start(&self, request: OsDragRequest) -> Result<(), DragError>;
}

/// Whether a session starts as an OS session now (ADR-0211 §2, §9, §11).
///
/// What an OS session offers and advertises, decided from the session's reach
/// and actions: a pure function, so every backend's conformance row and every
/// runtime decide alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OsSessionPlan {
    /// What the session offers the OS.
    pub offer: OsOffer,
    /// What the session advertises to the OS as its allowed operations.
    pub advertised: TransferActions,
}

/// What an OS session offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a session offers the OS its representations with the identity, or the identity alone (ADR-0211 §9)"
)]
pub enum OsOffer {
    /// The serialized representations and the identity.
    Full,
    /// Only the private identity: only the source runtime's targets accept it.
    IdentityOnly,
}

/// How far a session may reach, typed in its configuration (ADR-0211 §9).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum DragReach {
    /// Never leaves its window: reorder.
    Window,
    /// Other presentations of its runtime, never another runtime or process.
    Runtime,
    /// Anything the OS can reach.
    System,
}

/// The actions a drag allows inside the app and outside it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct AllowedActions {
    /// What targets in the source's own runtime may do with the drag.
    pub within_app: TransferActions,
    /// What targets elsewhere may do with it.
    pub outside_app: TransferActions,
}

impl AllowedActions {
    /// Both sets given.
    #[must_use]
    pub const fn new(within_app: TransferActions, outside_app: TransferActions) -> Self {
        Self {
            within_app,
            outside_app,
        }
    }

    /// `within_app` only: the drag never offers anything outside the runtime.
    #[must_use]
    pub const fn within(within_app: TransferActions) -> Self {
        Self::new(within_app, TransferActions::empty())
    }

    /// The reach a drag with these actions derives when its source does not
    /// say otherwise: `System` when `outside_app` is non-empty, else `Runtime`.
    #[must_use]
    pub const fn derived_reach(&self) -> DragReach {
        if self.outside_app.is_empty() {
            DragReach::Runtime
        } else {
            DragReach::System
        }
    }

    /// Whether the OS would see an empty mask, or no target could ever accept.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.within_app.is_empty() && self.outside_app.is_empty()
    }

    /// The actions advertised to the OS for a `System` session: what either
    /// scope allows. The scope a recipient is in narrows it at the drop.
    #[must_use]
    pub const fn union(&self) -> TransferActions {
        self.within_app.union(self.outside_app)
    }
}

/// Validate a session's configuration (ADR-0211 §2, §9).
///
/// # Errors
///
/// [`DragError::NoAllowedActions`] when both sets are empty;
/// [`DragError::NoOutsideActions`] for a `System` reach with no outside
/// actions; [`DragError::NoSerializedRepresentation`] when outside actions are
/// allowed, whatever the reach, but `has_serialized` is false.
pub const fn validate_session(
    allowed: AllowedActions,
    reach: DragReach,
    has_serialized: bool,
) -> Result<(), DragError> {
    if allowed.is_empty() {
        return Err(DragError::NoAllowedActions);
    }
    if matches!(reach, DragReach::System) && allowed.outside_app.is_empty() {
        return Err(DragError::NoOutsideActions);
    }
    if !allowed.outside_app.is_empty() && !has_serialized {
        return Err(DragError::NoSerializedRepresentation);
    }
    Ok(())
}

/// The OS session a drag becomes at `moment`, if any.
///
/// `start` is the backend's [`OsDragStart`]; `moment` is
/// [`DragMoment::Start`] when the drag begins and [`DragMoment::LeavesWindow`]
/// when its pointer leaves the window. `available` is whether the backend can
/// start OS drags at all; `sibling_presentation` whether the source runtime has
/// another open presentation.
///
/// # Errors
///
/// [`DragError::Unsupported`] when the drag needs the OS and the backend has
/// none; the session stays in-window.
pub const fn plan_os_session(
    start: OsDragStart,
    moment: DragMoment,
    reach: DragReach,
    allowed: AllowedActions,
    sibling_presentation: bool,
    available: bool,
) -> Result<Option<OsSessionPlan>, DragError> {
    let now = matches!(
        (start, moment),
        (OsDragStart::OnLeavingWindow, DragMoment::LeavesWindow)
            | (OsDragStart::AtStart { .. }, DragMoment::Start)
    );
    if !now {
        return Ok(None);
    }
    let plan = match reach {
        DragReach::Window => return Ok(None),
        DragReach::System => OsSessionPlan {
            offer: OsOffer::Full,
            advertised: allowed.union(),
        },
        DragReach::Runtime => {
            if !sibling_presentation || allowed.within_app.is_empty() {
                return Ok(None);
            }
            OsSessionPlan {
                offer: OsOffer::IdentityOnly,
                advertised: allowed.within_app,
            }
        }
    };
    if available {
        Ok(Some(plan))
    } else {
        Err(DragError::Unsupported(Unsupported::new(
            UnsupportedReason::NotOnThisPlatform,
        )))
    }
}

/// When in a drag's life a decision is taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum DragMoment {
    /// The drag begins.
    Start,
    /// The pointer leaves the window the drag began in.
    LeavesWindow,
}
