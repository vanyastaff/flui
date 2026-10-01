//! Instance-owned admission and submission retirement for prepared IR resources.
//! This quota excludes existing pools/caches and is not a physical VRAM cap.

use crate::external_texture_registry::ExternalAllocationLease;

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PreparedCost {
    pub(crate) gpu_bytes: usize,
    pub(crate) cpu_bytes: usize,
    pub(crate) objects: usize,
}

impl PreparedCost {
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            gpu_bytes: self.gpu_bytes.checked_add(other.gpu_bytes)?,
            cpu_bytes: self.cpu_bytes.checked_add(other.cpu_bytes)?,
            objects: self.objects.checked_add(other.objects)?,
        })
    }

    fn fits(self, cap: Self) -> bool {
        self.gpu_bytes <= cap.gpu_bytes
            && self.cpu_bytes <= cap.cpu_bytes
            && self.objects <= cap.objects
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedIrLimits {
    pub(crate) cost: PreparedCost,
    /// Prior queued-work backlog window, not the active frame work allowance.
    pub(crate) submissions: usize,
    pub(crate) frame_submissions: usize,
}

impl Default for PreparedIrLimits {
    fn default() -> Self {
        let cost = PreparedCost {
            gpu_bytes: 256 * 1024 * 1024,
            cpu_bytes: 128 * 1024 * 1024,
            objects: 65_536,
        };
        Self {
            cost,
            submissions: 64,
            // One callback metadata object per submission; cumulative work
            // cannot exceed the prepared object/CPU metadata profile even if
            // callbacks retire while recording continues.
            frame_submissions: cost
                .objects
                .min(cost.cpu_bytes / submission_metadata_base()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DomainError {
    #[error("prepared resource arithmetic overflow")]
    Overflow,
    #[error("prepared IR quota exhausted: requested {requested:?}, used {used:?}")]
    Budget {
        requested: PreparedCost,
        used: PreparedCost,
        limit: PreparedCost,
    },
    #[error("prepared resources are still held by submitted GPU work")]
    Backpressure,
    #[error("frame submission work limit exhausted ({limit})")]
    FrameSubmissionBudget { limit: usize },
    #[error("a managed frame scope is already active on this device domain")]
    FrameAlreadyActive,
    #[error("prepared submission belongs to another device domain")]
    ForeignOwner,
    #[error("device domain is closing or lost")]
    Unavailable,
    #[error("prepared submission quota exhausted")]
    SubmissionBudget,
    #[error("nonblocking device progress failed: {0}")]
    Poll(#[from] wgpu::PollError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lifecycle {
    Open,
    Closing,
    Lost,
}

#[derive(Debug)]
struct LedgerState {
    used: PreparedCost,
    submissions: usize,
    retirements: usize,
    lifecycle: Lifecycle,
    frame: Option<(u64, usize, usize)>,
    next_frame: u64,
}

#[derive(Debug)]
struct Ledger {
    limits: PreparedIrLimits,
    state: Mutex<LedgerState>,
}

impl Ledger {
    fn new(limits: PreparedIrLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            state: Mutex::new(LedgerState {
                used: PreparedCost::default(),
                submissions: 0,
                retirements: 0,
                lifecycle: Lifecycle::Open,
                frame: None,
                next_frame: 0,
            }),
        })
    }

    // All operations under this lock are primitive arithmetic, with no user code.
    // Poison recovery keeps reservation Drop infallible during containment.
    fn state(&self) -> MutexGuard<'_, LedgerState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn reserve(self: &Arc<Self>, cost: PreparedCost) -> Result<PreparedPermit, DomainError> {
        let mut state = self.state();
        if state.lifecycle != Lifecycle::Open {
            return Err(DomainError::Unavailable);
        }
        let next = state.used.checked_add(cost).ok_or(DomainError::Overflow)?;
        if !next.fits(self.limits.cost) {
            let current = state.frame.map_or(0, |(_, _, live)| live);
            if cost.fits(self.limits.cost) && (state.submissions > current || state.retirements > 0)
            {
                return Err(DomainError::Backpressure);
            }
            return Err(DomainError::Budget {
                requested: cost,
                used: state.used,
                limit: self.limits.cost,
            });
        }
        state.used = next;
        Ok(PreparedPermit {
            ledger: Arc::clone(self),
            cost,
        })
    }

    fn submission(self: &Arc<Self>) -> Result<SubmissionPermit, DomainError> {
        let mut state = self.state();
        if state.lifecycle == Lifecycle::Lost {
            return Err(DomainError::Unavailable);
        }
        if state
            .frame
            .is_some_and(|(_, count, _)| count >= self.limits.frame_submissions)
        {
            return Err(DomainError::FrameSubmissionBudget {
                limit: self.limits.frame_submissions,
            });
        }
        if state.frame.is_none() && state.submissions >= self.limits.submissions {
            return Err(DomainError::SubmissionBudget);
        }
        state.submissions += 1;
        let frame = state.frame.map(|(id, _, _)| id);
        if let Some((_, count, live)) = state.frame.as_mut() {
            *count += 1;
            *live += 1;
        }
        Ok(SubmissionPermit(Arc::clone(self), frame))
    }

    fn retirement(self: &Arc<Self>) -> Result<RetirementPermit, DomainError> {
        let mut state = self.state();
        state.retirements = state
            .retirements
            .checked_add(1)
            .ok_or(DomainError::Overflow)?;
        Ok(RetirementPermit(Arc::clone(self)))
    }

    fn begin_frame(self: &Arc<Self>) -> Result<FrameSubmissionScope, DomainError> {
        let mut state = self.state();
        if state.lifecycle != Lifecycle::Open {
            return Err(DomainError::Unavailable);
        }
        if state.frame.is_some() {
            return Err(DomainError::FrameAlreadyActive);
        }
        if state.submissions >= self.limits.submissions {
            return Err(DomainError::SubmissionBudget);
        }
        let id = state
            .next_frame
            .checked_add(1)
            .ok_or(DomainError::Overflow)?;
        state.next_frame = id;
        state.frame = Some((id, 0, 0));
        Ok(FrameSubmissionScope {
            ledger: Arc::clone(self),
            id,
        })
    }
}

/// Outer managed frame work bound; child painter resets never reset this count.
#[derive(Debug)]
pub(crate) struct FrameSubmissionScope {
    ledger: Arc<Ledger>,
    id: u64,
}

impl Drop for FrameSubmissionScope {
    fn drop(&mut self) {
        let mut state = self.ledger.state();
        if state.frame.is_some_and(|(id, _, _)| id == self.id) {
            state.frame = None;
        }
    }
}

/// Non-cloneable charge, retained until discard or submitted GPU completion.
#[derive(Debug)]
pub(crate) struct PreparedPermit {
    ledger: Arc<Ledger>,
    cost: PreparedCost,
}

impl Drop for PreparedPermit {
    fn drop(&mut self) {
        let mut state = self.ledger.state();
        state.used.gpu_bytes = state.used.gpu_bytes.saturating_sub(self.cost.gpu_bytes);
        state.used.cpu_bytes = state.used.cpu_bytes.saturating_sub(self.cost.cpu_bytes);
        state.used.objects = state.used.objects.saturating_sub(self.cost.objects);
    }
}

#[derive(Debug)]
struct SubmissionPermit(Arc<Ledger>, Option<u64>);

impl Drop for SubmissionPermit {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.submissions = state.submissions.saturating_sub(1);
        if let Some((id, _, live)) = state.frame.as_mut()
            && Some(*id) == self.1
        {
            *live = live.saturating_sub(1);
        }
    }
}

#[derive(Debug)]
struct RetirementPermit(Arc<Ledger>);

impl Drop for RetirementPermit {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.retirements = state.retirements.saturating_sub(1);
    }
}

#[derive(Debug)]
struct CompletionPayload {
    _permits: Vec<Arc<PreparedPermit>>,
    _external_leases: Vec<ExternalAllocationLease>,
    _submission: Option<SubmissionPermit>,
    _metadata: Option<Arc<PreparedPermit>>,
    _retirement: Option<RetirementPermit>,
}

#[derive(Debug)]
struct CompletionHold {
    state: Mutex<(bool, bool, Option<CompletionPayload>)>,
}

impl CompletionHold {
    fn new(
        permits: Vec<Arc<PreparedPermit>>,
        submission: Option<SubmissionPermit>,
        metadata: Option<Arc<PreparedPermit>>,
        retirement: Option<RetirementPermit>,
        external_leases: Vec<ExternalAllocationLease>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((
                true,
                false,
                Some(CompletionPayload {
                    _permits: permits,
                    _external_leases: external_leases,
                    _submission: submission,
                    _metadata: metadata,
                    _retirement: retirement,
                }),
            )),
        })
    }

    fn arm(&self) {
        let payload = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.0 = false;
            if state.1 { state.2.take() } else { None }
        };
        drop(payload);
    }

    fn complete(&self) {
        let payload = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.1 = true;
            if state.0 { None } else { state.2.take() }
        };
        drop(payload);
    }
}

fn quarantine(ledger: &Ledger, holds: &Mutex<Vec<Arc<CompletionHold>>>, hold: Arc<CompletionHold>) {
    ledger.state().lifecycle = Lifecycle::Lost;
    holds
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(hold);
}

// Conservative requested bookkeeping model, not a physical allocator charge.
fn submission_metadata_base() -> usize {
    std::mem::size_of::<CompletionHold>()
        + 2 * std::mem::size_of::<usize>()
        + std::mem::size_of::<Arc<CompletionHold>>()
}

/// Shared production/private-failure seam. Submit panic may follow partial GPU
/// acceptance, so no callback is taken as proof for that failed invocation.
fn submit_and_track<T>(
    ledger: &Arc<Ledger>,
    holds: &Mutex<Vec<Arc<CompletionHold>>>,
    permits: Vec<Arc<PreparedPermit>>,
    metadata: Option<Arc<PreparedPermit>>,
    external_leases: Vec<ExternalAllocationLease>,
    submit: impl FnOnce() -> T,
    register: impl FnOnce(Arc<CompletionHold>),
) -> Result<T, DomainError> {
    let submitted = ledger.submission()?;
    let hold = CompletionHold::new(permits, Some(submitted), metadata, None, external_leases);
    let result = catch_unwind(AssertUnwindSafe(submit));
    let registration = catch_unwind(AssertUnwindSafe(|| register(Arc::clone(&hold))));
    if result.is_err() || registration.is_err() {
        quarantine(ledger, holds, hold);
    } else {
        hold.arm();
    }
    match (result, registration) {
        (Err(primary), Err(secondary)) => {
            // A foreign panic payload's Drop can itself panic. Preserve the first
            // fault; only this competing-fault emergency intentionally forgets it.
            std::mem::forget(secondary);
            resume_unwind(primary)
        }
        (Err(primary), Ok(())) | (Ok(_), Err(primary)) => resume_unwind(primary),
        (Ok(value), Ok(())) => Ok(value),
    }
}

/// Consuming submission bundle: fields cannot be detached or submitted twice.
#[derive(Debug)]
pub(crate) struct PreparedSubmission {
    ledger: Arc<Ledger>,
    buffers: Vec<wgpu::CommandBuffer>,
    permits: Vec<Arc<PreparedPermit>>,
    external_leases: Vec<ExternalAllocationLease>,
}

/// Shared by managed painters for one device generation, never process-global.
#[derive(Debug)]
pub(crate) struct DeviceDomain {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    ledger: Arc<Ledger>,
    // Serialize submit + callback registration, whose API targets previous submit.
    submit_gate: Mutex<()>,
    // Owner teardown drops bookkeeping, not a claim of GPU/driver completion.
    // Imported allocations carry only a weak Domain owner; completion holds
    // must not form a Domain -> queue -> callback -> Domain ownership cycle.
    quarantine: Mutex<Vec<Arc<CompletionHold>>>,
}

impl DeviceDomain {
    pub(crate) fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Arc<Self> {
        Self::with_limits(device, queue, PreparedIrLimits::default())
    }

    pub(crate) fn with_limits(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        limits: PreparedIrLimits,
    ) -> Arc<Self> {
        Arc::new(Self {
            device,
            queue,
            ledger: Ledger::new(limits),
            submit_gate: Mutex::new(()),
            quarantine: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    pub(crate) fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    pub(crate) fn begin_frame_scope(&self) -> Result<FrameSubmissionScope, DomainError> {
        self.poll()?;
        let _gate = self
            .submit_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ledger.begin_frame()
    }

    pub(crate) fn reserve(&self, cost: PreparedCost) -> Result<Arc<PreparedPermit>, DomainError> {
        self.ledger.reserve(cost).map(Arc::new)
    }

    /// Validates a known simultaneous live footprint independently of transient
    /// submitted occupancy, so impossible double buffering is not retried forever.
    pub(crate) fn check_footprint(&self, cost: PreparedCost) -> Result<(), DomainError> {
        if !cost.fits(self.ledger.limits.cost) {
            return Err(DomainError::Budget {
                requested: cost,
                used: PreparedCost::default(),
                limit: self.ledger.limits.cost,
            });
        }
        Ok(())
    }

    pub(crate) fn prepare(
        &self,
        buffers: Vec<wgpu::CommandBuffer>,
        permits: Vec<Arc<PreparedPermit>>,
    ) -> Result<PreparedSubmission, DomainError> {
        self.prepare_with_external_leases(buffers, permits, Vec::new())
    }

    pub(crate) fn prepare_with_external_leases(
        &self,
        buffers: Vec<wgpu::CommandBuffer>,
        permits: Vec<Arc<PreparedPermit>>,
        external_leases: Vec<ExternalAllocationLease>,
    ) -> Result<PreparedSubmission, DomainError> {
        if external_leases
            .iter()
            .any(|lease| !lease.is_for_domain(self))
        {
            return Err(DomainError::ForeignOwner);
        }
        if permits
            .iter()
            .any(|permit| !Arc::ptr_eq(&permit.ledger, &self.ledger))
        {
            return Err(DomainError::ForeignOwner);
        }
        Ok(PreparedSubmission {
            ledger: Arc::clone(&self.ledger),
            buffers,
            permits,
            external_leases,
        })
    }

    pub(crate) fn submit(
        &self,
        prepared: PreparedSubmission,
    ) -> Result<wgpu::SubmissionIndex, DomainError> {
        if !Arc::ptr_eq(&prepared.ledger, &self.ledger) {
            return Err(DomainError::ForeignOwner);
        }
        // Progress must precede admission and stay outside submit_gate: a full
        // prior backlog must be able to retire on scheduled native retries.
        self.poll()?;
        let cpu_bytes = prepared
            .permits
            .capacity()
            .checked_mul(std::mem::size_of::<Arc<PreparedPermit>>())
            .and_then(|bytes| {
                prepared
                    .buffers
                    .capacity()
                    .checked_mul(std::mem::size_of::<wgpu::CommandBuffer>())
                    .and_then(|buffers| bytes.checked_add(buffers))
            })
            .and_then(|bytes| {
                prepared
                    .external_leases
                    .capacity()
                    .checked_mul(std::mem::size_of::<ExternalAllocationLease>())
                    .and_then(|leases| bytes.checked_add(leases))
            })
            .and_then(|bytes| bytes.checked_add(submission_metadata_base()))
            .ok_or(DomainError::Overflow)?;
        let metadata = self.reserve(PreparedCost {
            gpu_bytes: 0,
            cpu_bytes,
            objects: 1,
        })?;
        let _gate = self
            .submit_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        submit_and_track(
            &self.ledger,
            &self.quarantine,
            prepared.permits,
            Some(metadata),
            prepared.external_leases,
            || self.queue.submit(prepared.buffers),
            |hold| self.queue.on_submitted_work_done(move || hold.complete()),
        )
    }

    /// Native nonblocking progress; on WebGPU the browser supplies progress.
    pub(crate) fn poll(&self) -> Result<(), DomainError> {
        match self.device.poll(wgpu::PollType::Poll) {
            Ok(_) => Ok(()),
            Err(source) => {
                // GpuProgress is classified as device-fatal; expose the same
                // loss to the native recovery predicate even before a callback.
                self.mark_lost();
                Err(DomainError::Poll(source))
            }
        }
    }

    /// Stops new preparation; already admitted work may still be submitted/drained.
    pub(crate) fn close(&self) {
        let mut state = self.ledger.state();
        if state.lifecycle == Lifecycle::Open {
            state.lifecycle = Lifecycle::Closing;
        }
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.ledger.state().lifecycle == Lifecycle::Lost
    }

    /// Rejects submission without releasing outstanding completion-owned charges.
    pub(crate) fn mark_lost(&self) {
        let _gate = self
            .submit_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ledger.state().lifecycle = Lifecycle::Lost;
    }

    /// Conservatively retains live allocation charges through all earlier work.
    /// Used by persistent target Drop, including discarded candidates. This is
    /// accounting retirement, not a promise that driver memory is released.
    pub(crate) fn retire_after_previous_submissions(&self, permits: Vec<Arc<PreparedPermit>>) {
        self.retire_resources_after_previous_submissions(permits, Vec::new());
    }

    /// Trusted raw submissions retain captured allocations as well as charges.
    pub(crate) fn retire_resources_after_previous_submissions(
        &self,
        permits: Vec<Arc<PreparedPermit>>,
        external_leases: Vec<ExternalAllocationLease>,
    ) {
        if permits.is_empty() && external_leases.is_empty() {
            return;
        }
        let _gate = self
            .submit_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retirement = if let Ok(retirement) = self.ledger.retirement() {
            Some(retirement)
        } else {
            self.ledger.state().lifecycle = Lifecycle::Lost;
            None
        };
        let hold = CompletionHold::new(permits, None, None, retirement, external_leases);
        let callback = Arc::clone(&hold);
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            self.queue
                .on_submitted_work_done(move || callback.complete());
        })) {
            quarantine(&self.ledger, &self.quarantine, hold);
            // This method runs from allocation Drop: never start a second unwind.
            std::mem::forget(payload);
        } else {
            hold.arm();
        }
    }
}

// A final poll can invoke trusted embedder callbacks. Drop must not start a
// second unwind; foreign payload destruction is unsafe at this boundary too.
fn contain_final_progress(progress: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(progress)) {
        std::mem::forget(payload);
    }
}

impl Drop for DeviceDomain {
    fn drop(&mut self) {
        self.close();
        // A final nonblocking progress opportunity. Dropping the owner does not
        // claim GPU completion; raw device clones remain the embedder's obligation.
        contain_final_progress(|| {
            let _ = self.poll();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_quota_failure_and_retirement() {
        let limits = PreparedIrLimits {
            cost: PreparedCost {
                gpu_bytes: 8,
                cpu_bytes: 8,
                objects: 2,
            },
            submissions: 1,
            frame_submissions: 1,
        };
        let cost = PreparedCost {
            gpu_bytes: 8,
            cpu_bytes: 4,
            objects: 1,
        };
        let ledger = Ledger::new(limits);
        let first = ledger.reserve(cost).expect("initial admission");
        assert!(matches!(
            ledger.reserve(PreparedCost {
                gpu_bytes: usize::MAX,
                ..cost
            }),
            Err(DomainError::Overflow)
        ));
        assert!(matches!(
            ledger.reserve(cost),
            Err(DomainError::Budget { .. })
        ));
        let submitted = ledger.submission().expect("first submission");
        assert!(matches!(
            ledger.reserve(cost),
            Err(DomainError::Backpressure)
        ));
        assert!(matches!(
            ledger.submission(),
            Err(DomainError::SubmissionBudget)
        ));
        // A delayed callback owns these values; CPU frame finish releases nothing.
        let completion = move || {
            drop(first);
            drop(submitted);
        };
        assert_eq!(ledger.state().used, cost);
        completion();
        let next = ledger.reserve(cost).expect("capacity after completion");
        drop(next);
        assert_eq!(ledger.state().used, PreparedCost::default());
        let overflow = ledger.reserve(PreparedCost {
            gpu_bytes: usize::MAX,
            ..cost
        });
        assert!(matches!(overflow, Err(DomainError::Budget { .. })));
        let kept = ledger.reserve(cost).expect("next valid operation");
        ledger.state().lifecycle = Lifecycle::Lost;
        assert!(matches!(ledger.submission(), Err(DomainError::Unavailable)));
        assert_eq!(ledger.state().used, cost);
        drop(kept);
        assert_eq!(ledger.state().used, PreparedCost::default());
        #[cfg(all(feature = "testing", not(target_arch = "wasm32")))]
        real_gpu_completion_releases_charge();
        competing_submit_faults_keep_charges();
        bounded_frame_work_is_not_transient_backpressure();
        active_frame_work_is_separate_from_prior_backlog();
        delayed_allocation_retirement_keeps_retry_durable();
        current_frame_resource_shortage_is_hard();
        registration_completion_races();
        contain_final_progress(|| panic!("final progress fault"));
    }

    fn delayed_allocation_retirement_keeps_retry_durable() {
        let cost = PreparedCost {
            gpu_bytes: 8,
            cpu_bytes: 0,
            objects: 1,
        };
        let ledger = Ledger::new(PreparedIrLimits {
            cost,
            ..Default::default()
        });
        let allocation = Arc::new(ledger.reserve(cost).expect("old spare allocation"));
        let retirement = ledger.retirement().expect("nonempty allocation retirement");
        let hold = CompletionHold::new(vec![allocation], None, None, Some(retirement), Vec::new());
        hold.arm();
        let frame = ledger
            .begin_frame()
            .expect("resize frame has no GPU backlog");
        assert_eq!(ledger.state().submissions, 0);
        assert!(matches!(
            ledger.reserve(cost),
            Err(DomainError::Backpressure)
        ));
        // An intrinsically too-large allocation still fails hard, irrespective
        // of pending retirement. Known simultaneous footprints are preflighted.
        assert!(matches!(
            ledger.reserve(PreparedCost {
                gpu_bytes: 9,
                ..cost
            }),
            Err(DomainError::Budget { .. })
        ));
        hold.complete();
        hold.complete();
        assert_eq!(ledger.state().retirements, 0);
        drop(
            ledger
                .reserve(cost)
                .expect("next allocation after browser callback"),
        );
        drop(frame);
    }

    fn active_frame_work_is_separate_from_prior_backlog() {
        let ledger = Ledger::new(PreparedIrLimits {
            frame_submissions: 128,
            ..Default::default()
        });
        let frame = ledger.begin_frame().expect("first scene frame");
        let mut pending = Vec::new();
        // No callbacks run: models synchronous browser execution, not native
        // polling which can conceal the old fixed in-flight window failure.
        for _ in 0..100 {
            pending.push(
                ledger
                    .submission()
                    .expect("active frame exceeds prior backlog window"),
            );
        }
        drop(frame);
        assert!(matches!(
            ledger.begin_frame(),
            Err(DomainError::SubmissionBudget)
        ));
        drop(pending);
        let next = ledger
            .begin_frame()
            .expect("retired backlog permits next scene");
        drop(ledger.submission().expect("next scene work"));
        drop(next);
    }

    fn current_frame_resource_shortage_is_hard() {
        let cost = PreparedCost {
            gpu_bytes: 8,
            cpu_bytes: 0,
            objects: 1,
        };
        let ledger = Ledger::new(PreparedIrLimits {
            cost,
            submissions: 4,
            frame_submissions: 4,
        });
        let permit = ledger.reserve(cost).expect("initial allocation");
        let previous = ledger.submission().expect("previous frame work");
        let frame = ledger.begin_frame().expect("next frame");
        let current = ledger.submission().expect("current frame work");
        assert!(matches!(
            ledger.reserve(cost),
            Err(DomainError::Backpressure)
        ));
        drop(previous);
        assert!(matches!(
            ledger.reserve(cost),
            Err(DomainError::Budget { .. })
        ));
        drop(current);
        drop(permit);
        let next = ledger.reserve(cost).expect("next admitted operation");
        drop(next);
        drop(frame);
    }

    fn registration_completion_races() {
        for fail in [false, true] {
            let cost = PreparedCost {
                gpu_bytes: 8,
                cpu_bytes: 0,
                objects: 1,
            };
            let ledger = Ledger::new(PreparedIrLimits {
                cost,
                submissions: 1,
                frame_submissions: 1,
            });
            let holds = Mutex::new(Vec::new());
            let permit = Arc::new(ledger.reserve(cost).expect("initial charge"));
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                submit_and_track(
                    &ledger,
                    &holds,
                    vec![permit],
                    None,
                    Vec::new(),
                    || (),
                    |hold| {
                        hold.complete();
                        assert_eq!(
                            ledger.state().used,
                            cost,
                            "not armed before registration returns"
                        );
                        assert!(!fail, "registration fault");
                    },
                )
                .expect("submission admitted");
            }));
            assert_eq!(outcome.is_err(), fail);
            assert_eq!(
                ledger.state().used,
                if fail { cost } else { PreparedCost::default() }
            );
            if fail {
                assert!(matches!(
                    ledger.reserve(cost),
                    Err(DomainError::Unavailable)
                ));
            }
        }
    }

    fn bounded_frame_work_is_not_transient_backpressure() {
        let ledger = Ledger::new(PreparedIrLimits {
            cost: PreparedCost::default(),
            submissions: 1,
            frame_submissions: 1,
        });
        let frame = ledger.begin_frame().expect("outer scope");
        assert!(matches!(
            ledger.begin_frame(),
            Err(DomainError::FrameAlreadyActive)
        ));
        drop(ledger.submission().expect("first frame submission"));
        // Even after GPU work retired, repeating this oversized frame is hard.
        assert!(matches!(
            ledger.submission(),
            Err(DomainError::FrameSubmissionBudget { limit: 1 })
        ));
        drop(frame);
        let next = ledger.begin_frame().expect("next bounded frame");
        drop(ledger.submission().expect("next frame progresses"));
        drop(next);
    }

    fn competing_submit_faults_keep_charges() {
        for (submit_fault, registration_fault) in [(true, false), (false, true), (true, true)] {
            let cost = PreparedCost {
                gpu_bytes: 8,
                cpu_bytes: 0,
                objects: 1,
            };
            let ledger = Ledger::new(PreparedIrLimits {
                cost,
                submissions: 1,
                frame_submissions: 1,
            });
            let holds = Mutex::new(Vec::new());
            let permit = Arc::new(ledger.reserve(cost).expect("initial charge"));
            let mut callback = None;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                submit_and_track(
                    &ledger,
                    &holds,
                    vec![permit],
                    None,
                    Vec::new(),
                    || {
                        assert!(!submit_fault, "submit fault");
                    },
                    |hold| {
                        callback = Some(hold);
                        assert!(!registration_fault, "registration fault");
                    },
                )
                .expect("submission admitted");
            }));
            let payload = outcome.expect_err("injected GPU boundary failure");
            let expected = if submit_fault {
                "submit fault"
            } else {
                "registration fault"
            };
            assert_eq!(payload.downcast_ref::<&str>().copied(), Some(expected));
            assert!(matches!(
                ledger.reserve(cost),
                Err(DomainError::Unavailable)
            ));
            let callback = callback.expect("registration attempted even after submit panic");
            callback.complete();
            callback.complete();
            assert_eq!(
                ledger.state().used,
                cost,
                "uncertain GPU acceptance stays quarantined"
            );
            drop(callback);
            drop(holds);
            assert_eq!(ledger.state().used, PreparedCost::default());
            // Recovery is a fresh generation, not reopening the faulted owner.
            let recovered = Ledger::new(PreparedIrLimits {
                cost,
                submissions: 1,
                frame_submissions: 1,
            });
            let next = recovered
                .reserve(cost)
                .expect("recovered generation progresses");
            drop(next);
        }
    }

    #[cfg(all(feature = "testing", not(target_arch = "wasm32")))]
    fn real_gpu_completion_releases_charge() {
        let (device, queue) =
            crate::test_support::test_device_and_queue("Prepared quota retirement");
        let cost = PreparedCost {
            gpu_bytes: 8,
            cpu_bytes: 0,
            objects: 1,
        };
        let domain = DeviceDomain::with_limits(
            Arc::clone(&device),
            Arc::clone(&queue),
            PreparedIrLimits {
                cost: PreparedCost {
                    cpu_bytes: 4096,
                    objects: 2,
                    ..cost
                },
                submissions: 1,
                frame_submissions: 1,
            },
        );
        let lease = test_external_lease(&domain);
        let probe = lease.lifetime_probe();
        let charge = domain.reserve(cost).expect("GPU allocation admitted");
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Retirement regression buffer"),
            size: 8,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.clear_buffer(&buffer, 0, None);
        let index = domain
            .submit(
                domain
                    .prepare_with_external_leases(vec![encoder.finish()], vec![charge], vec![lease])
                    .expect("prepared submission"),
            )
            .expect("GPU submission");
        assert!(
            probe.is_alive(),
            "CPU handoff must not release the captured allocation"
        );
        // Callback has not been pumped: only actual completion may release it.
        assert!(matches!(
            domain.reserve(PreparedCost {
                gpu_bytes: 0,
                cpu_bytes: 0,
                objects: 1,
            }),
            Err(DomainError::Backpressure)
        ));
        assert!(matches!(
            domain.reserve(cost),
            Err(DomainError::Backpressure)
        ));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .expect("bounded GPU completion progress");
        assert!(
            !probe.is_alive(),
            "completion releases the engine allocation lease"
        );
        assert_eq!(domain.ledger.state().submissions, 0);
        let next = domain
            .reserve(cost)
            .expect("next allocation after actual GPU callback");
        drop(next);
        assert_eq!(domain.ledger.state().used, PreparedCost::default());
        external_leases_survive_competing_faults(&device, &queue);
    }

    #[cfg(all(feature = "testing", not(target_arch = "wasm32")))]
    fn test_external_lease(domain: &Arc<DeviceDomain>) -> ExternalAllocationLease {
        use crate::{
            ExternalAlpha, ExternalColorEncoding, ExternalSampling, ExternalTextureDescriptor,
            ExternalTextureRegistry,
        };
        let texture = domain.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("Completion-owned external allocation"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mut registry = ExternalTextureRegistry::new(domain);
        let id = flui_painting::paint::TextureId::new(1);
        registry
            .register(
                id,
                texture,
                ExternalTextureDescriptor {
                    sampling: ExternalSampling::Nearest,
                    alpha: ExternalAlpha::Straight,
                    color: ExternalColorEncoding::EncodedSrgb,
                },
            )
            .expect("validated allocation");
        registry
            .get(id)
            .expect("registered allocation")
            .lease()
            .clone()
    }

    #[cfg(all(feature = "testing", not(target_arch = "wasm32")))]
    fn external_leases_survive_competing_faults(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
    ) {
        for (submit_fault, registration_fault) in [(true, false), (false, true), (true, true)] {
            let domain = DeviceDomain::new(Arc::clone(device), Arc::clone(queue));
            let lease = test_external_lease(&domain);
            let probe = lease.lifetime_probe();
            let mut callback = None;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                submit_and_track(
                    &domain.ledger,
                    &domain.quarantine,
                    Vec::new(),
                    None,
                    vec![lease],
                    || assert!(!submit_fault, "submit fault"),
                    |hold| {
                        callback = Some(hold);
                        assert!(!registration_fault, "registration fault");
                    },
                )
                .expect("submission admission");
            }));
            let payload = outcome.expect_err("injected submit boundary failure");
            let expected = if submit_fault {
                "submit fault"
            } else {
                "registration fault"
            };
            assert_eq!(payload.downcast_ref::<&str>().copied(), Some(expected));
            drop(payload);
            let callback = callback.expect("callback retained");
            callback.complete();
            drop(callback);
            assert!(
                probe.is_alive(),
                "uncertain acceptance must retain allocation despite callback"
            );
            assert!(matches!(
                domain.reserve(PreparedCost::default()),
                Err(DomainError::Unavailable)
            ));
            drop(domain);
            assert!(
                !probe.is_alive(),
                "quarantined allocation releases at owner teardown"
            );
            let recovered = DeviceDomain::new(Arc::clone(device), Arc::clone(queue));
            drop(
                recovered
                    .reserve(PreparedCost::default())
                    .expect("new owner progresses"),
            );
        }
    }
}
