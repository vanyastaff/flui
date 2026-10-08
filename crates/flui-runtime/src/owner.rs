//! Owner-local lifetime and publication of UI runtimes.
//!
//! Native identities and resources remain in the application host. A publication
//! permit holds only logical membership; a host must prepare its native side before
//! committing both maps without invoking user code between them.

use std::cell::{RefCell, RefMut};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use flui_foundation::{PresentationAddress, PresentationId, UiRuntimeId};

use crate::lifecycle_state::preserve_first_lifecycle_panic;
use crate::presentation::{PresentationState, PresentationWindow};
use crate::ui_runtime::{PresentationFactory, UiRuntime};

mod callback;
mod close;
mod registry;
pub use registry::{
    InitializationOutcome, InstallInitialization, InstallToken, RefusedInstallToken,
};
mod delivery;
mod preferences;
mod presentation_dispatch;
mod runtime_dispatch;
mod status;
pub use callback::OwnerCallback;
pub use delivery::{Delivery, DispatchError, FrameDispatcher, OwnerEffects};
pub use preferences::SystemPreferencesSnapshot;
pub use presentation_dispatch::{InputOutcome, PresentationDispatcher, WindowObservation};
use runtime_dispatch::RuntimeWork;
pub use runtime_dispatch::{RuntimeDispatcher, RuntimeOperation};
pub use status::RuntimeStatus;

/// The owner-affine logical registry. Handles and unpublished proposals belong
/// to this exact host, independently of the native trampoline that reaches it.
#[derive(Clone)]
pub struct OwnerHost {
    core: Rc<OwnerCore>,
}

struct OwnerCore {
    state: RefCell<OwnerState>,
    retired: RefCell<Vec<RetiredRuntime>>,
}

impl Drop for OwnerCore {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.state.get_mut().runtimes);
        let queued = std::mem::take(&mut self.state.get_mut().queue);
        let mut first_failure = None;
        retire_queued(queued, &mut first_failure);
        for entry in entries {
            retire_entry(entry, &mut first_failure);
        }
        for retired in self.retired.get_mut().drain(..) {
            retire_value(retired, &mut first_failure);
        }
        finish_retirement(first_failure);
    }
}

#[derive(Default)]
struct OwnerState {
    preferences: preferences::PreferenceState,
    runtimes: Vec<RuntimeEntry>,
    closed: bool,
    active_runtime: Option<UiRuntimeId>,
    queue: VecDeque<OwnerWork>,
    claim: DeliveryClaim,
    continuation: Option<u64>,
    next_continuation: u64,
    callback_depth: usize,
    callback_remaining: usize,
}

#[derive(Default, PartialEq, Eq)]
enum DeliveryClaim {
    #[default]
    Idle,
    Executing,
    Completing,
}

enum Residence {
    Resident(Box<UiRuntime>),
    CheckedOut,
    RetiringCheckedOut,
}

struct RuntimeEntry {
    id: UiRuntimeId,
    presentations: Vec<PresentationId>,
    closing: Vec<PresentationId>,
    residence: Residence,
    factory: Rc<PresentationFactory>,
    scheduler: flui_scheduler::WeakUpdateScheduler,
}

enum RetiredRuntime {
    Entry(RuntimeEntry),
    Runtime(Box<UiRuntime>),
}

type Failure = Option<Box<dyn std::any::Any + Send>>;

/// Whether mandatory host cleanup must preserve an already active failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    /// No failure has occurred in this operation.
    Healthy,
    /// Retire framework resources while retaining opaque user captures as required
    /// by the exceptional-path policy; optional callbacks must not run.
    PreservingFailure,
}

impl RecoveryState {
    fn current(failure: &Failure) -> Self {
        if failure.is_some() || std::thread::panicking() {
            Self::PreservingFailure
        } else {
            Self::Healthy
        }
    }
}

enum OwnerWork {
    Runtime(RuntimeWork),
    CommitInstall(InstallToken),
    StopRuntimes,
}

fn retire_queued(queued: VecDeque<OwnerWork>, first_failure: &mut Failure) {
    for work in queued {
        retire_work(work, first_failure);
    }
}

fn retire_work(work: OwnerWork, first_failure: &mut Failure) {
    if first_failure.is_some() || std::thread::panicking() {
        std::mem::forget(work);
    } else {
        let failure = catch_unwind(AssertUnwindSafe(|| drop(work))).err();
        preserve_first_lifecycle_panic(first_failure, failure, "queued owner work retirement");
    }
}

fn retire_value(retired: RetiredRuntime, first_failure: &mut Failure) {
    match retired {
        RetiredRuntime::Entry(entry) => retire_entry(entry, first_failure),
        RetiredRuntime::Runtime(runtime) => retire_runtime(runtime, first_failure),
    }
}

fn retire_runtime(runtime: Box<UiRuntime>, first_failure: &mut Failure) {
    let failure = catch_unwind(AssertUnwindSafe(|| drop(runtime))).err();
    preserve_first_lifecycle_panic(first_failure, failure, "owner runtime retirement");
}

fn retire_entry(
    RuntimeEntry {
        residence, factory, ..
    }: RuntimeEntry,
    first_failure: &mut Failure,
) {
    if let Residence::Resident(runtime) = residence {
        retire_runtime(runtime, first_failure);
    }
    if (first_failure.is_some() || std::thread::panicking()) && Rc::strong_count(&factory) == 1 {
        // ADR-0127: the last factory owns opaque callback/capability captures.
        // Other Rc owners are owner-local, so dropping a non-last clone is inert.
        std::mem::forget(factory);
        return;
    }
    let failure = catch_unwind(AssertUnwindSafe(|| drop(factory))).err();
    preserve_first_lifecycle_panic(first_failure, failure, "assembly capability retirement");
}

fn finish_retirement(first_failure: Failure) {
    if let Some(failure) = first_failure {
        if std::thread::panicking() {
            std::mem::forget(failure);
        } else {
            resume_unwind(failure);
        }
    }
}

enum InstallPayload {
    Runtime(Box<RuntimeEntry>),
    Presentation {
        authorizer: PresentationAddress,
        presentation: Box<PresentationState>,
    },
}

/// An unpublished runtime or presentation and its exact authorizing host.
/// Dropping a refused proposal retires it outside the registry's borrow.
pub struct PreparedInstall {
    owner: Weak<OwnerCore>,
    payload: Option<InstallPayload>,
    address: PresentationAddress,
    delivery: Option<InstallToken>,
}

impl Drop for PreparedInstall {
    fn drop(&mut self) {
        let mut first_failure = None;
        match self.payload.take() {
            Some(InstallPayload::Runtime(entry)) => retire_entry(*entry, &mut first_failure),
            Some(InstallPayload::Presentation { presentation, .. }) => {
                first_failure = catch_unwind(AssertUnwindSafe(|| drop(presentation))).err();
            }
            None => {}
        }
        finish_retirement(first_failure);
    }
}

/// Why a prepared runtime cannot currently be published.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PublicationError {
    /// A terminal close for the authorizing presentation has already been accepted.
    #[error("the authorizing presentation is closing")]
    PresentationClosing,
    /// The proposal belongs to a different host incarnation.
    #[error("the prepared runtime belongs to a different owner host")]
    ForeignOwner,
    /// Another publication is currently holding the registry.
    #[error("the owner registry is busy")]
    Busy,
    /// The authorizing runtime is no longer installed.
    #[error("the authorizing runtime is not installed")]
    UnknownRuntime,
    /// The exact authorizing presentation is no longer installed.
    #[error("the authorizing presentation is not installed")]
    UnknownPresentation,
    /// This host has shut down and cannot publish more membership.
    #[error("the owner host is closed")]
    Closed,
}

/// Refusal preserves the complete unpublished proposal for its caller.
pub struct RefusedInstall {
    /// The condition that prevented publication.
    pub error: PublicationError,
    /// The proposal remains unpublished and owned by the caller.
    pub prepared: PreparedInstall,
}

/// Preparation refused before assembly; ownership of the window is returned.
#[derive(Debug)]
pub struct RefusedWindow {
    /// Why the authorizer cannot prepare another presentation.
    pub error: PublicationError,
    /// The window has not been assembled or published.
    pub window: PresentationWindow,
}

/// A reserved logical publication interval. Capacity and identity checks precede
/// construction; [`Self::commit`] performs only the membership insertion.
///
/// If abandoned, the state borrow is released before the proposal is destroyed.
pub struct PublicationPermit<'a> {
    // Field order is significant: the guard retires before owned user state.
    state: RefMut<'a, OwnerState>,
    prepared: PreparedInstall,
}

impl Default for OwnerHost {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnerHost {
    /// Create an independent owner-local registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            core: Rc::new(OwnerCore {
                state: RefCell::new(OwnerState::default()),
                retired: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Prepare runtime ownership without making it visible to dispatch.
    #[must_use]
    pub fn prepare_runtime(&self, runtime: UiRuntime) -> PreparedInstall {
        let address = PresentationAddress {
            ui_runtime_id: runtime.id(),
            presentation_id: runtime.presentation_id(),
        };
        let factory = Rc::new(runtime.presentation_factory());
        let scheduler = runtime.scheduler().downgrade();
        let presentations = runtime.presentation_ids().collect();
        PreparedInstall {
            owner: Rc::downgrade(&self.core),
            payload: Some(InstallPayload::Runtime(Box::new(RuntimeEntry {
                id: runtime.id(),
                presentations,
                closing: Vec::new(),
                residence: Residence::Resident(Box::new(runtime)),
                factory,
                scheduler,
            }))),
            address,
            delivery: Some(InstallToken::new(&self.core, address, None)),
        }
    }

    /// Assemble a sibling outside registry borrows. The proposal retains the
    /// authorizer and revalidates it when publication is requested.
    ///
    /// # Errors
    /// Returns the window when the authorizer is absent, the registry is busy,
    /// or the runtime's scheduler has already retired.
    pub fn prepare_presentation(
        &self,
        authorizer: PresentationAddress,
        window: PresentationWindow,
    ) -> Result<PreparedInstall, RefusedWindow> {
        let factory = self
            .core
            .state
            .try_borrow()
            .map_err(|_| PublicationError::Busy)
            .and_then(|state| {
                state
                    .authorizer(authorizer)
                    .map(|index| Rc::clone(&state.runtimes[index].factory))
            });
        let factory = match factory {
            Ok(factory) => factory,
            Err(error) => return Err(RefusedWindow { error, window }),
        };
        let presentation = factory.assemble(window).map_err(|window| RefusedWindow {
            error: PublicationError::UnknownRuntime,
            window,
        })?;
        let address = PresentationAddress {
            ui_runtime_id: authorizer.ui_runtime_id,
            presentation_id: presentation.id(),
        };
        Ok(PreparedInstall {
            owner: Rc::downgrade(&self.core),
            address,
            delivery: Some(InstallToken::new(&self.core, address, Some(authorizer))),
            payload: Some(InstallPayload::Presentation {
                authorizer,
                presentation: Box::new(presentation),
            }),
        })
    }

    /// Reserve a pure publication interval after native preparation.
    ///
    /// # Errors
    /// Returns the untouched proposal if its owner differs or another
    /// publication holds the registry borrow. Runtime identities are unique;
    /// the proposal consumes the runtime and cannot be published twice.
    pub fn publication(
        &self,
        prepared: PreparedInstall,
    ) -> Result<PublicationPermit<'_>, RefusedInstall> {
        if !prepared.owner.ptr_eq(&Rc::downgrade(&self.core)) {
            return Err(RefusedInstall {
                error: PublicationError::ForeignOwner,
                prepared,
            });
        }
        let Ok(mut state) = self.core.state.try_borrow_mut() else {
            return Err(RefusedInstall {
                error: PublicationError::Busy,
                prepared,
            });
        };
        let unavailable = if state.closed {
            Some(PublicationError::Closed)
        } else if state.active_runtime.is_some() {
            Some(PublicationError::Busy)
        } else {
            None
        };
        if let Some(error) = unavailable {
            drop(state);
            return Err(RefusedInstall { error, prepared });
        }
        match prepared
            .payload
            .as_ref()
            .expect("BUG: unpublished proposal retains its payload")
        {
            InstallPayload::Runtime(entry) => {
                if let Residence::Resident(runtime) = &entry.residence
                    && !runtime.preferences_belong_to(&state.preferences.origin)
                {
                    drop(state);
                    return Err(RefusedInstall {
                        error: PublicationError::ForeignOwner,
                        prepared,
                    });
                }
                state.runtimes.reserve(1);
            }
            InstallPayload::Presentation { authorizer, .. } => {
                let index = match state.authorizer(*authorizer) {
                    Ok(index) => index,
                    Err(error) => {
                        drop(state);
                        return Err(RefusedInstall { error, prepared });
                    }
                };
                let entry = &mut state.runtimes[index];
                entry.presentations.reserve(1);
                let Residence::Resident(runtime) = &mut entry.residence else {
                    drop(state);
                    return Err(RefusedInstall {
                        error: PublicationError::Busy,
                        prepared,
                    });
                };
                runtime.reserve_presentation();
            }
        }
        Ok(PublicationPermit { state, prepared })
    }

    /// The number of published runtime memberships.
    #[must_use]
    pub fn runtime_count(&self) -> usize {
        self.core
            .state
            .borrow()
            .runtimes
            .iter()
            .filter(|entry| !matches!(entry.residence, Residence::RetiringCheckedOut))
            .count()
    }
}

impl OwnerState {
    fn authorizer(&self, address: PresentationAddress) -> Result<usize, PublicationError> {
        let index = self.presentation_index(address)?;
        if self.runtimes[index]
            .closing
            .contains(&address.presentation_id)
        {
            return Err(PublicationError::PresentationClosing);
        }
        Ok(index)
    }

    fn presentation_index(&self, address: PresentationAddress) -> Result<usize, PublicationError> {
        let index = self.runtime_index(address.ui_runtime_id)?;
        if !self.runtimes[index]
            .presentations
            .contains(&address.presentation_id)
        {
            return Err(PublicationError::UnknownPresentation);
        }
        Ok(index)
    }

    fn runtime_index(&self, id: UiRuntimeId) -> Result<usize, PublicationError> {
        if self.closed {
            return Err(PublicationError::Closed);
        }
        self.runtimes
            .iter()
            .position(|entry| {
                entry.id == id && !matches!(entry.residence, Residence::RetiringCheckedOut)
            })
            .ok_or(PublicationError::UnknownRuntime)
    }
}

impl PreparedInstall {
    /// The exact unpublished primary address, used to prepare its native mapping.
    #[must_use]
    pub const fn address(&self) -> PresentationAddress {
        self.address
    }
}

impl PublicationPermit<'_> {
    /// Publish the prepared runtime. Invokes no callback or outgoing destructor.
    #[must_use]
    pub fn commit(mut self) -> PresentationAddress {
        let address = self.prepared.address;
        match self
            .prepared
            .payload
            .take()
            .expect("BUG: publication consumes its proposal once")
        {
            InstallPayload::Runtime(entry) => self.state.runtimes.push(*entry),
            InstallPayload::Presentation {
                authorizer,
                presentation,
            } => {
                let index = self
                    .state
                    .authorizer(authorizer)
                    .expect("BUG: publication permit retains the validated membership");
                let entry = &mut self.state.runtimes[index];
                let Residence::Resident(runtime) = &mut entry.residence else {
                    unreachable!("BUG: publication permit excludes runtime checkout");
                };
                entry.presentations.push(presentation.id());
                runtime.install_presentation(*presentation);
            }
        }
        address
    }
}

impl std::fmt::Debug for OwnerHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerHost").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for PreparedInstall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedInstall")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RefusedInstall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefusedInstall")
            .field("error", &self.error)
            .field("prepared", &self.prepared)
            .finish()
    }
}

impl std::fmt::Debug for PublicationPermit<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicationPermit")
            .field("prepared", &self.prepared)
            .finish_non_exhaustive()
    }
}
