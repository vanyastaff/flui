//! Closed frame delivery and strong runtime checkout.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_foundation::{PresentationAddress, UiRuntimeId};

use crate::lifecycle_state::preserve_first_lifecycle_panic;
use crate::ui_runtime::UiRuntime;

use super::{
    DeliveryClaim, Failure, InstallToken, OwnerCore, OwnerHost, OwnerWork, PublicationError,
    RecoveryState, Residence, RetiredRuntime, RuntimeWork, finish_retirement, retire_value,
    retire_work,
};

/// Native work borrowed for one owner delivery, never stored in the registry.
pub trait OwnerEffects {
    /// Record an admitted runtime lifecycle observation at its FIFO execution point.
    fn runtime_lifecycle(&self, runtime: UiRuntimeId, state: flui_scheduler::AppLifecycleState);
    /// Every runtime present at the stop operation has been notified or retired.
    /// Called after all checkouts return, including when a listener failed.
    fn runtimes_stopped(&self, recovery: RecoveryState);
    /// Withdraw all listed native bindings before retiring any of their resources.
    /// Active driver leases retain resources until return; bindings from another
    /// host incarnation must remain untouched. Called once for host shutdown.
    fn retire_host(&self, presentations: &[PresentationAddress], recovery: RecoveryState);
    /// Take the complete native bundle and attempt its pure joint publication.
    fn commit_install(
        &self,
        token: InstallToken,
        recovery: RecoveryState,
    ) -> Option<super::InstallInitialization>;
    /// Settle the native installation after initial observations and lease return.
    fn finish_install(
        &self,
        address: PresentationAddress,
        outcome: super::InitializationOutcome,
        recovery: RecoveryState,
    );
    /// Remove and retire an unpublished native bundle after shutdown.
    fn cancel_install(&self, token: InstallToken, recovery: RecoveryState);
    /// Apply native surface metrics before the addressed logical metrics change.
    fn resize_surface(
        &self,
        address: PresentationAddress,
        size: flui_foundation::geometry::Size<f64>,
        scale_factor: f64,
    );
    /// Invoke the concrete driver installed for this exact surface binding.
    fn frame(&self, address: PresentationAddress, runtime: &mut UiRuntime);
    /// Withdraw native routing and retire resources before terminal UI callbacks.
    fn retire_presentation(
        &self,
        address: PresentationAddress,
        surviving_primary: Option<PresentationAddress>,
    );
    /// Complete application work after runtime checkout has returned.
    fn after_turn(&self, recovery: RecoveryState);
    /// Resolve one owned numeric frontier after every runtime loan has ended.
    /// Return it when no native producer accepts it; the owner then parks it.
    fn text_sizing(
        &self,
        frontier: super::TextSizingFrontier,
        _recovery: RecoveryState,
    ) -> Option<super::TextSizingFrontier> {
        Some(frontier)
    }
    /// Request another owner opportunity without fabricating a frame delivery.
    fn request_continuation(&self) -> bool;
}

/// Whether accepted work ran in this call or joined an existing owner turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// This call acquired the owner turn and started delivery.
    Driven,
    /// The current owner turn will deliver this work later.
    Queued,
}

/// An owner-local dispatch target is no longer available.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DispatchError {
    /// This host used its final preference revision; old revisions are never reissued.
    #[error("the host preference revision space is exhausted")]
    PreferenceRevisionExhausted,
    /// A terminal close has already fenced this exact presentation.
    #[error("the presentation is closing")]
    PresentationClosing,
    /// The host that minted this handle has been destroyed.
    #[error("the owner host no longer exists")]
    OwnerGone,
    /// The host has withdrawn all admission.
    #[error("the owner host is closed")]
    Closed,
    /// The runtime incarnation is absent.
    #[error("the UI runtime is no longer installed")]
    UnknownRuntime,
    /// The presentation incarnation is absent.
    #[error("the presentation is no longer installed")]
    UnknownPresentation,
    /// A runtime is already checked out or a publication owns the registry.
    #[error("the UI runtime is busy")]
    Busy,
}

impl From<PublicationError> for DispatchError {
    fn from(error: PublicationError) -> Self {
        match error {
            PublicationError::PresentationClosing => Self::PresentationClosing,
            PublicationError::Closed => Self::Closed,
            PublicationError::UnknownRuntime => Self::UnknownRuntime,
            PublicationError::UnknownPresentation => Self::UnknownPresentation,
            PublicationError::Busy => Self::Busy,
            PublicationError::ForeignOwner => Self::OwnerGone,
        }
    }
}

/// Weak authority to deliver frames for one installed presentation incarnation.
#[derive(Clone)]
pub struct FrameDispatcher {
    owner: Weak<OwnerCore>,
    address: PresentationAddress,
}

impl std::fmt::Debug for FrameDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameDispatcher")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl FrameDispatcher {
    /// Invalidate this presentation after its native surface was recreated.
    ///
    /// # Errors
    /// Refuses an expired or closing target, closed host, or active publication.
    pub fn surface_restored(&self, effects: &dyn OwnerEffects) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(RuntimeWork::SurfaceRestored(self.address), effects)
    }

    /// Admit a real frame delivery. Nested calls join the same owner FIFO.
    ///
    /// # Errors
    /// Refuses an expired host or exact presentation, or an active publication.
    pub fn deliver(&self, effects: &dyn OwnerEffects) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(RuntimeWork::Frame(self.address), effects)
    }
}

impl OwnerCore {
    pub(super) fn deliver(
        self: &Rc<Self>,
        work: RuntimeWork,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let _callback = self.begin_callback(effects)?;
        let starts_turn = {
            let mut state = self
                .state
                .try_borrow_mut()
                .map_err(|_| DispatchError::Busy)?;
            let index = work.validate_admission(&state)?;
            if let RuntimeWork::Close(address) = &work {
                state.runtimes[index].closing.push(address.presentation_id);
            }
            let carried_work = !state.queue.is_empty();
            let merged = state.queue.back_mut().is_some_and(|pending| match pending {
                OwnerWork::Runtime(previous) => previous.merge_pending_state(&work),
                _ => false,
            });
            if !merged {
                state.queue.push_back(OwnerWork::Runtime(work));
            }
            if !carried_work && state.claim == DeliveryClaim::Idle && state.callback_remaining != 0
            {
                state.claim = DeliveryClaim::Executing;
                true
            } else {
                false
            }
        };
        if starts_turn {
            self.drive(effects);
            Ok(Delivery::Driven)
        } else {
            Ok(Delivery::Queued)
        }
    }
}

impl OwnerHost {
    /// Notify every runtime of terminal shutdown as one closed owner operation.
    /// All sibling notifications finish before reentrant work or publications run.
    ///
    /// # Errors
    /// Refuses a closed host or an active publication.
    pub fn stop_runtimes(&self, effects: &dyn OwnerEffects) -> Result<Delivery, DispatchError> {
        let _callback = self.core.begin_callback(effects)?;
        let starts = {
            let mut state = self.core.state.borrow_mut();
            if state.closed {
                return Err(DispatchError::Closed);
            }
            state.queue.push_back(OwnerWork::StopRuntimes);
            if state.claim == DeliveryClaim::Idle && state.callback_remaining != 0 {
                state.claim = DeliveryClaim::Executing;
                true
            } else {
                false
            }
        };
        if starts {
            self.core.drive(effects);
        }
        Ok(if starts {
            Delivery::Driven
        } else {
            Delivery::Queued
        })
    }

    /// Acquire weak frame authority without exposing a runtime borrow.
    ///
    /// # Errors
    /// Refuses an absent presentation, closed host, or active publication.
    pub fn frame_dispatcher(
        &self,
        address: PresentationAddress,
    ) -> Result<FrameDispatcher, DispatchError> {
        self.core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?
            .authorizer(address)?;
        Ok(FrameDispatcher {
            owner: Rc::downgrade(&self.core),
            address,
        })
    }

    /// Spend one finite owner opportunity on accepted work. No frame is added.
    pub fn continue_work(&self, effects: &dyn OwnerEffects) {
        let _callback = self
            .core
            .begin_callback(effects)
            .expect("BUG: continuation requested during pure publication");
        self.core.continue_work(effects);
    }

    /// Withdraw logical membership and native routing before retiring work.
    /// An active lease retires on return; it never restores a runtime into
    /// this closed host or a replacement host. Reentrant shutdown is inert.
    pub fn shutdown(&self, effects: &dyn OwnerEffects) {
        let (retired, queued, presentations, idle) = {
            let mut state = self.core.state.borrow_mut();
            if state.closed {
                return;
            }
            state.closed = true;
            let queued = std::mem::take(&mut state.queue);
            state.continuation = None;
            let entries = std::mem::take(&mut state.runtimes);
            let mut retired = Vec::new();
            let mut presentations = Vec::new();
            for mut entry in entries {
                presentations.extend(entry.presentations.iter().map(|&presentation_id| {
                    PresentationAddress {
                        ui_runtime_id: entry.id,
                        presentation_id,
                    }
                }));
                match entry.residence {
                    Residence::Resident(_) => retired.push(RetiredRuntime::Entry(entry)),
                    Residence::CheckedOut | Residence::RetiringCheckedOut => {
                        entry.residence = Residence::RetiringCheckedOut;
                        state.runtimes.push(entry);
                    }
                }
            }
            (
                retired,
                queued,
                presentations,
                state.claim == DeliveryClaim::Idle,
            )
        };
        self.core.retired.borrow_mut().extend(retired);
        let mut first_failure = catch_unwind(AssertUnwindSafe(|| {
            effects.retire_host(&presentations, RecoveryState::current(&None));
        }))
        .err();
        for work in queued {
            match work {
                OwnerWork::CommitInstall(token) => {
                    let failure = catch_unwind(AssertUnwindSafe(|| {
                        effects.cancel_install(token, RecoveryState::current(&first_failure));
                    }))
                    .err();
                    preserve_first_lifecycle_panic(
                        &mut first_failure,
                        failure,
                        "pending install cancellation",
                    );
                }
                work @ (OwnerWork::Runtime(_) | OwnerWork::StopRuntimes) => {
                    retire_work(work, &mut first_failure);
                }
            }
        }
        if idle {
            self.core.drain_retired(&mut first_failure);
        }
        finish_retirement(first_failure);
    }
}

struct RuntimeLease {
    core: Rc<OwnerCore>,
    id: UiRuntimeId,
    runtime: Option<Box<UiRuntime>>,
}

impl RuntimeLease {
    fn checkout(core: &Rc<OwnerCore>, work: &RuntimeWork) -> Result<Self, DispatchError> {
        let runtime = {
            let mut state = core.state.borrow_mut();
            let index = work.validate(&state)?;
            let entry = &mut state.runtimes[index];
            let previous = std::mem::replace(&mut entry.residence, Residence::CheckedOut);
            let Residence::Resident(runtime) = previous else {
                entry.residence = previous;
                return Err(DispatchError::Busy);
            };
            state.active_runtime = Some(runtime.id());
            runtime
        };
        Ok(Self {
            core: Rc::clone(core),
            id: runtime.id(),
            runtime: Some(runtime),
        })
    }
}

impl Drop for RuntimeLease {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let retired = {
            let mut state = self.core.state.borrow_mut();
            if state.active_runtime == Some(self.id) {
                state.active_runtime = None;
            }
            match state.runtimes.iter().position(|entry| entry.id == self.id) {
                Some(index) if matches!(state.runtimes[index].residence, Residence::CheckedOut) => {
                    state.runtimes[index].residence = Residence::Resident(runtime);
                    None
                }
                Some(index)
                    if matches!(
                        state.runtimes[index].residence,
                        Residence::RetiringCheckedOut
                    ) =>
                {
                    state.runtimes[index].residence = Residence::Resident(runtime);
                    Some(RetiredRuntime::Entry(state.runtimes.remove(index)))
                }
                _ => Some(RetiredRuntime::Runtime(runtime)),
            }
        };
        if let Some(retired) = retired {
            self.core.retired.borrow_mut().push(retired);
        }
    }
}

impl OwnerCore {
    pub(super) fn continue_work(self: &Rc<Self>, effects: &dyn OwnerEffects) {
        let claimed = {
            let mut state = self.state.borrow_mut();
            if state.claim != DeliveryClaim::Idle
                || state.queue.is_empty()
                || state.callback_remaining == 0
            {
                false
            } else {
                state.claim = DeliveryClaim::Executing;
                true
            }
        };
        if claimed {
            self.drive(effects);
        }
    }

    fn drain_retired(&self, first_failure: &mut Failure) {
        loop {
            let retired = std::mem::take(&mut *self.retired.borrow_mut());
            if retired.is_empty() {
                break;
            }
            for value in retired {
                retire_value(value, first_failure);
            }
        }
    }

    pub(super) fn drive(self: &Rc<Self>, effects: &dyn OwnerEffects) {
        let mut first_failure = None;
        let mut completing_stop = false;
        loop {
            let next = {
                let mut state = self.state.borrow_mut();
                if state.callback_remaining == 0 {
                    None
                } else {
                    let next = state.queue.pop_front();
                    if next.is_some() {
                        state.callback_remaining -= 1;
                    }
                    next
                }
            };
            let Some(work) = next else { break };
            let failure = match work {
                OwnerWork::StopRuntimes => {
                    let ids: Vec<_> = self
                        .state
                        .borrow()
                        .runtimes
                        .iter()
                        .map(|entry| entry.id)
                        .collect();
                    let mut first = None;
                    for id in ids {
                        let work = RuntimeWork::Runtime(id, super::RuntimeOperation::Stop);
                        let Ok(mut lease) = RuntimeLease::checkout(self, &work) else {
                            continue;
                        };
                        let failure = catch_unwind(AssertUnwindSafe(|| {
                            work.run(
                                self,
                                lease
                                    .runtime
                                    .as_deref_mut()
                                    .expect("BUG: stop owns runtime lease"),
                                effects,
                            );
                        }))
                        .err();
                        drop(lease);
                        preserve_first_lifecycle_panic(
                            &mut first,
                            failure,
                            "runtime stop notification",
                        );
                    }
                    let failure = catch_unwind(AssertUnwindSafe(|| {
                        effects.runtimes_stopped(RecoveryState::current(&first));
                    }))
                    .err();
                    preserve_first_lifecycle_panic(&mut first, failure, "runtime stop completion");
                    // Terminal notification must finish accepted reentry within
                    // this callback's budget before resuming its first failure.
                    completing_stop |= first.is_some();
                    first
                }
                OwnerWork::CommitInstall(token) => {
                    self.state.borrow_mut().claim = DeliveryClaim::Completing;
                    let committed = catch_unwind(AssertUnwindSafe(|| {
                        effects.commit_install(token, RecoveryState::current(&first_failure))
                    }));
                    match committed {
                        Err(failure) => Some(failure),
                        Ok(None) => None,
                        Ok(Some(initial)) => {
                            self.state.borrow_mut().claim = DeliveryClaim::Executing;
                            let address = initial.address;
                            let work = RuntimeWork::Initialize(initial);
                            let mut first = None;
                            let outcome = match RuntimeLease::checkout(self, &work) {
                                Ok(mut lease) => {
                                    let result = catch_unwind(AssertUnwindSafe(|| {
                                        work.run(
                                            self,
                                            lease
                                                .runtime
                                                .as_deref_mut()
                                                .expect("BUG: initialization owns runtime lease"),
                                            effects,
                                        );
                                    }));
                                    drop(lease);
                                    if let Err(failure) = result {
                                        first = Some(failure);
                                        super::InitializationOutcome::Failed
                                    } else if self.state.borrow().authorizer(address).is_ok() {
                                        super::InitializationOutcome::Ready
                                    } else {
                                        super::InitializationOutcome::Failed
                                    }
                                }
                                Err(_) => super::InitializationOutcome::Failed,
                            };
                            self.state.borrow_mut().claim = DeliveryClaim::Completing;
                            let recovery = if first.is_some() {
                                RecoveryState::PreservingFailure
                            } else {
                                RecoveryState::current(&first_failure)
                            };
                            let failure = catch_unwind(AssertUnwindSafe(|| {
                                effects.finish_install(address, outcome, recovery);
                            }))
                            .err();
                            preserve_first_lifecycle_panic(
                                &mut first,
                                failure,
                                "installation completion",
                            );
                            first
                        }
                    }
                }
                OwnerWork::Runtime(work) => {
                    self.state.borrow_mut().claim = DeliveryClaim::Executing;
                    let Ok(mut lease) = RuntimeLease::checkout(self, &work) else {
                        continue;
                    };
                    let mut failure = catch_unwind(AssertUnwindSafe(|| {
                        work.run(
                            self,
                            lease
                                .runtime
                                .as_deref_mut()
                                .expect("BUG: active lease owns its runtime"),
                            effects,
                        );
                    }))
                    .err();
                    let runtime = lease
                        .runtime
                        .as_deref()
                        .expect("BUG: active lease retains its runtime until return");
                    let id = runtime.id();
                    let frontiers = if failure.is_none() && first_failure.is_none() {
                        runtime.take_text_sizing_frontiers()
                    } else {
                        Vec::new()
                    };
                    drop(lease);
                    self.state.borrow_mut().claim = DeliveryClaim::Completing;
                    for work in frontiers {
                        let frontier = super::TextSizingFrontier::new(self, id, work);
                        if failure.is_some()
                            || self.state.borrow().authorizer(frontier.address()).is_err()
                        {
                            let retired = catch_unwind(AssertUnwindSafe(|| drop(frontier))).err();
                            preserve_first_lifecycle_panic(
                                &mut failure,
                                retired,
                                "numeric preparation receipt retirement",
                            );
                            continue;
                        }
                        let delivery = catch_unwind(AssertUnwindSafe(|| {
                            if let Some(frontier) = effects
                                .text_sizing(frontier, RecoveryState::current(&first_failure))
                            {
                                let _ = frontier.settle(
                                    crate::ui_runtime::TextSizingSettlement::Unavailable,
                                    effects,
                                );
                            }
                        }))
                        .err();
                        preserve_first_lifecycle_panic(
                            &mut failure,
                            delivery,
                            "native text preparation",
                        );
                    }
                    failure
                }
            };
            preserve_first_lifecycle_panic(&mut first_failure, failure, "owner operation");
            self.state.borrow_mut().claim = DeliveryClaim::Completing;
            self.drain_retired(&mut first_failure);
            let failure = catch_unwind(AssertUnwindSafe(|| {
                effects.after_turn(RecoveryState::current(&first_failure));
            }))
            .err();
            preserve_first_lifecycle_panic(&mut first_failure, failure, "owner completion");
            self.drain_retired(&mut first_failure);
            if first_failure.is_some() && !completing_stop {
                self.state.borrow_mut().callback_remaining = 0;
                break;
            }
        }
        self.state.borrow_mut().claim = DeliveryClaim::Idle;
        finish_retirement(first_failure);
    }

    pub(super) fn post_continuation(
        &self,
        effects: &dyn OwnerEffects,
        first_failure: &mut Failure,
    ) {
        let sequence = {
            let mut state = self.state.borrow_mut();
            if state.queue.is_empty() || state.continuation.is_some() {
                return;
            }
            // Exhaustion permanently refuses another sequence rather than issuing
            // an old acknowledgement identity. Existing work stays in the FIFO.
            let Some(sequence) = state.next_continuation.checked_add(1) else {
                return;
            };
            state.next_continuation = sequence;
            state.continuation = Some(sequence);
            sequence
        };
        let posted = catch_unwind(AssertUnwindSafe(|| effects.request_continuation()));
        if !matches!(posted, Ok(true)) {
            let mut state = self.state.borrow_mut();
            if state.continuation == Some(sequence) {
                state.continuation = None;
            }
        }
        preserve_first_lifecycle_panic(first_failure, posted.err(), "owner continuation");
    }
}
