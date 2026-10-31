//! Presentation sampling owns provenance, admission and recovery before any build.

use super::{UiRuntime, text_preparation};
use crate::frame_failure::{
    FrameFailureKind, SamplingFailure, SamplingFailureOrigin, SamplingStage,
};
use crate::presentation::PresentationState;
use flui_foundation::panic::{FailureClass, PanicRecovery, RecoveryScope};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};
use flui_scheduler::{DemandKind, PollDecision};
use std::time::Duration;
use web_time::Instant;

pub(super) struct AdmittedSegment {
    pub(super) decision: PollDecision,
    pub(super) segment_start: Instant,
    pub(super) woken: bool,
    pub(super) pending: bool,
    pub(super) resuming: bool,
    pub(super) completion: bool,
}

enum Admission {
    None,
    Retained,
    Segment(AdmittedSegment),
}

enum DeadlineOperation {
    Observe,
    PollDue,
}

/// One presentation's first failure and its independent publication evidence.
pub(super) struct PresentationSampling<'runtime> {
    presentation: &'runtime PresentationState,
    recovery: PanicRecovery,
    first: Option<SamplingFailure>,
    blocking: Option<SamplingFailure>,
    admission: Admission,
    completion_entered: bool,
    retained_sample: Option<text_preparation::RetainedSample>,
}

impl<'runtime> PresentationSampling<'runtime> {
    fn new(presentation: &'runtime PresentationState) -> Self {
        let admission = if presentation.text_preparation.borrow().segment.is_some() {
            Admission::Retained
        } else {
            Admission::None
        };
        let completion_entered = matches!(admission, Admission::Retained);
        Self {
            presentation,
            recovery: PanicRecovery::new(),
            first: None,
            blocking: None,
            admission,
            completion_entered,
            retained_sample: None,
        }
    }

    fn run(&mut self, stage: SamplingStage, action: impl FnOnce(&mut RecoveryScope<'_>)) {
        self.recovery
            .scope()
            .run_classified(FailureClass::Unknown, action);
        if self.first.is_none()
            && let Some(class) = self.recovery.first_failure_class()
        {
            self.first = Some(SamplingFailure {
                stage,
                origin: SamplingFailureOrigin::from_class(class),
            });
        }
        if self.blocking.is_none()
            && let Some(class) = self.recovery.blocking_failure_class()
        {
            self.blocking = Some(SamplingFailure {
                stage,
                origin: SamplingFailureOrigin::from_class(class),
            });
        }
    }

    fn sample(
        &mut self,
        runtime: &UiRuntime,
        constraints: BoxConstraints,
        raw: Duration,
        now: Instant,
    ) {
        let presentation = self.presentation;
        // The keyboard's occlusion at this frame's time, before anything
        // builds against it (ADR-0276 §3).
        self.run(SamplingStage::Animation, |_| {
            runtime.sample_keyboard_inset(presentation, runtime.event_time_at(now));
        });
        let mut prepared = false;
        let mut retained_sample = None;
        self.run(SamplingStage::Preparation, |recovery| {
            text_preparation::reconcile_with_recovery(presentation, constraints, None, recovery);
            retained_sample = text_preparation::retained_sample(presentation);
            prepared = true;
        });
        let retained_tick = retained_sample
            .as_ref()
            .map(text_preparation::RetainedSample::tick);
        self.retained_sample = retained_sample;
        if prepared {
            self.run(SamplingStage::Animation, |recovery| {
                if let Some(tick) = retained_tick {
                    presentation.sampled_motion_tick.set(Some(tick));
                    return;
                }
                let vsync = presentation.vsync();
                let tick = presentation.motion_tick(raw);
                presentation.sampled_motion_tick.set(Some(tick));
                let enabled =
                    runtime.scheduler.frames_enabled() && !presentation.clock().is_hidden();
                let was_running =
                    presentation.animation_running(runtime.scheduler.frames_enabled());
                if was_running {
                    // Admission belongs to this selected tick, including a final sample.
                    presentation.clock().mark_demand(DemandKind::Animation);
                } else {
                    presentation.clock().clear_demand(DemandKind::Animation);
                }
                if enabled {
                    vsync.tick_all_with_recovery(&tick, recovery);
                }
            });
        }
        if !presentation.closing_requested.get() {
            self.run(SamplingStage::DeadlineDiscovery, |recovery| {
                flui_interaction::__runtime::refresh_deadlines_with_recovery(
                    presentation.gestures(),
                    now,
                    recovery,
                );
            });
            self.run(SamplingStage::GestureDeadline, |recovery| {
                flui_interaction::__runtime::tick_deadlines_with_recovery(
                    presentation.gestures(),
                    now,
                    recovery,
                );
            });
            self.run(SamplingStage::HapticDelivery, |recovery| {
                presentation.haptics().service(recovery);
            });
        }
    }

    fn admit(&mut self, runtime: &UiRuntime) {
        let presentation = self.presentation;
        presentation.release_redraw_poke();
        if presentation.closing_requested.get() {
            self.admission = Admission::None;
            return;
        }
        // A presentation waiting out a dropped frame's backoff is not
        // attempted, and keeps every demand it has for the attempt it makes
        // when the wait ends.
        if !presentation.admit_failure_retry(presentation.clock().now()) {
            tracing::trace!(target: "flui.pace", event = "segment_backoff", presentation = ?presentation.id(), "presentation waits out a dropped frame");
            return;
        }
        let mut selected = None;
        let mut completion_entered = self.completion_entered;
        self.run(SamplingStage::Preparation, |recovery| {
            let resuming = presentation.text_preparation.borrow().segment.is_some();
            if !text_preparation::can_run(presentation) {
                return;
            }
            presentation.reconcile_semantics_enablement();
            let woken = presentation.take_redraw_pending();
            let pending = presentation.prepare_pending_work(recovery);
            let blocked = presentation.pipeline().with(PipelineOwner::layout_waits_for_input);
            if (woken && !blocked) || pending || resuming {
                presentation.clock().mark_demand(DemandKind::Dirty);
            }
            let completion = presentation.has_eligible_completion();
            if completion {
                presentation.clock().mark_demand(DemandKind::Completion);
            }
            let now = presentation.clock().now();
            let segment_start = presentation.text_preparation.borrow().segment.as_ref().map_or(now, |saved| saved.host_time);
            let decision = presentation.clock().poll(now);
            if decision.should_run_segment() && !(blocked && !pending && !resuming) {
                selected = Some(AdmittedSegment { decision, segment_start, woken, pending, resuming, completion });
                if runtime.scheduler.phase() == flui_scheduler::SchedulerPhase::PersistentCallbacks {
                    presentation.begin_completion_segment();
                    completion_entered = true;
                }
            }
            tracing::trace!(target: "flui.pace", event = "segment_poll", presentation = ?presentation.id(), ?decision, "presentation segment poll");
        });
        self.completion_entered = completion_entered;
        if let Some(selected) = selected {
            self.admission = Admission::Segment(selected);
        }
    }

    fn admitted(&self) -> bool {
        !matches!(self.admission, Admission::None)
    }

    pub(super) fn rejected(&self) -> bool {
        self.admitted() && self.blocking.is_some()
    }

    fn establish_recovery(&mut self) {
        if !self.rejected() {
            return;
        }
        let presentation = self.presentation;
        let completion_entered = self.completion_entered;
        self.run(SamplingStage::Preparation, |recovery| {
            text_preparation::withdraw_for_retry(presentation, recovery);
            presentation.advance_tree_revision();
            presentation.clock().mark_demand(DemandKind::Dirty);
            if completion_entered {
                presentation.suspend_completion_segment();
            }
        });
    }

    fn report(&mut self, runtime: &UiRuntime) {
        if let Some(kind) = self.take_failure_kind(runtime) {
            runtime.report_frame_failure(self.presentation, kind);
        }
    }

    fn take_failure_kind(&mut self, runtime: &UiRuntime) -> Option<FrameFailureKind> {
        let recovery = std::mem::take(&mut self.recovery);
        let failure = recovery.into_failure()?;
        let (message, internal_invariant) = runtime
            .frame_failure_detail
            .get()
            .panic_text(failure.payload());
        Some(FrameFailureKind::SamplingPanic {
            first: self
                .first
                .expect("BUG: captured sampling failure has stage provenance"),
            blocking: self.blocking,
            admitted_attempt: self.admitted(),
            message,
            internal_invariant,
        })
    }

    pub(super) fn presentation(&self) -> &'runtime PresentationState {
        self.presentation
    }

    /// Diagnostics can admit new premises after the sampling report has sealed.
    /// A failure of this later validation is a separate preparation failure;
    /// it never re-delivers or substitutes the already reported sampling cause.
    pub(super) fn reconcile_for_segment(
        &mut self,
        runtime: &UiRuntime,
        constraints: BoxConstraints,
    ) -> bool {
        if self.rejected() || self.presentation.closing_requested.get() {
            return false;
        }
        let presentation = self.presentation;
        let admitted = self.admitted();
        let mut recovery = PanicRecovery::new();
        if self.first.is_some() {
            recovery.inherit_failure();
        }
        let mut changed = false;
        recovery.run_with(|scope| {
            changed = text_preparation::reconcile_with_recovery(
                presentation,
                constraints,
                self.retained_sample.as_ref(),
                scope,
            );
        });
        if recovery.blocking_failure_class().is_some() {
            recovery.run_with(|scope| {
                text_preparation::withdraw_for_retry(presentation, scope);
                if admitted {
                    presentation.advance_tree_revision();
                }
            });
        }
        if changed || recovery.blocking_failure_class().is_some() {
            recovery.run_with(|_| {
                if self.completion_entered {
                    presentation.suspend_completion_segment();
                }
            });
            presentation.clock().mark_demand(DemandKind::Dirty);
            self.admission = Admission::None;
            self.completion_entered = false;
            changed = true;
        }
        if let Some(failure) = recovery.into_failure() {
            let first = SamplingFailure {
                stage: SamplingStage::Preparation,
                origin: SamplingFailureOrigin::from_class(failure.class()),
            };
            let blocking = failure.blocking_class().map(|class| SamplingFailure {
                stage: SamplingStage::Preparation,
                origin: SamplingFailureOrigin::from_class(class),
            });
            let (message, internal_invariant) = runtime
                .frame_failure_detail
                .get()
                .panic_text(failure.payload());
            runtime.report_frame_failure(
                presentation,
                FrameFailureKind::SamplingPanic {
                    first,
                    blocking,
                    admitted_attempt: admitted,
                    message,
                    internal_invariant,
                },
            );
            if !changed
                && text_preparation::retained_needs_reconciliation(
                    presentation,
                    constraints,
                    self.retained_sample.as_ref(),
                )
            {
                // This observer admitted new retained inputs. Withdraw before
                // publication, without recursively preparing or reporting.
                let mut after_report = PresentationSampling::new(presentation);
                after_report.recovery.inherit_failure();
                after_report.run(SamplingStage::Preparation, |scope| {
                    text_preparation::withdraw_for_retry(presentation, scope);
                });
                after_report.run(SamplingStage::Preparation, |_| {
                    presentation.advance_tree_revision();
                    presentation.clock().mark_demand(DemandKind::Dirty);
                    if self.completion_entered {
                        presentation.suspend_completion_segment();
                    }
                });
                self.admission = Admission::None;
                self.completion_entered = false;
                changed = true;
                if let Some(kind) = after_report.take_failure_kind(runtime) {
                    runtime.queue_sampling_failure(presentation, kind);
                }
            }
        }
        if changed {
            self.admission = Admission::None;
        }
        changed
    }

    pub(super) fn take_segment(&mut self) -> Option<AdmittedSegment> {
        if self.rejected() || self.presentation.closing_requested.get() {
            return None;
        }
        match std::mem::replace(&mut self.admission, Admission::None) {
            Admission::Segment(segment) => Some(segment),
            Admission::None | Admission::Retained => None,
        }
    }
}

impl UiRuntime {
    /// Advance the runtime's drags to `now`: spring-loading that came due, and
    /// the OS told of each drop whose requests have all resolved (ADR-0211 §5, §8).
    fn service_drags(&self, now: web_time::Instant) {
        self.drag.tick(self.event_time_at(now));
    }

    /// `instant` on the event timeline the runtime's input is stamped on.
    fn event_time_at(&self, instant: web_time::Instant) -> flui_platform::EventTime {
        let elapsed =
            instant.saturating_duration_since(flui_foundation::MonotonicClock::epoch(&self.clock));
        flui_platform::EventTime::from_nanos(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
    }

    /// Refresh extension observations after an owner operation, never from a getter.
    pub(crate) fn refresh_deadline_observations(&self, state: crate::owner::RecoveryState) {
        self.enter(|runtime| runtime.sample_deadline_operations(state, DeadlineOperation::Observe));
    }

    /// Input correctness progresses even while native frame production is disabled.
    pub(super) fn sample_background_deadlines(&self, state: crate::owner::RecoveryState) {
        self.enter(|runtime| runtime.sample_deadline_operations(state, DeadlineOperation::PollDue));
    }

    fn sample_deadline_operations(
        &self,
        state: crate::owner::RecoveryState,
        deadlines: DeadlineOperation,
    ) {
        // A source can reenter an inbox drain while it is being observed. Share
        // the enclosing owner budget without holding a borrow across that call:
        // nested drains still apply commands, but defer their metadata refresh.
        let _turn = self.begin_geometry_turn();
        let now = flui_foundation::MonotonicClock::now(&self.clock);
        let mut sampling: Vec<_> = self
            .presentations
            .iter()
            .filter(|presentation| !presentation.closing_requested.get())
            .map(PresentationSampling::new)
            .collect();
        for operation in &mut sampling {
            if state == crate::owner::RecoveryState::PreservingFailure {
                operation.recovery.inherit_failure();
            }
            let presentation = operation.presentation;
            // Host observations only read admissions. Apply their queued work
            // here, after caller-held registry guards have been released.
            operation.run(SamplingStage::Preparation, |recovery| {
                presentation.pipeline().with_mut(|owner| {
                    owner.drain_pending_dirty_with_recovery(recovery);
                });
            });
            operation.run(SamplingStage::DeadlineDiscovery, |recovery| {
                flui_interaction::__runtime::refresh_deadlines_with_recovery(
                    presentation.gestures(),
                    now,
                    recovery,
                );
            });
            if matches!(deadlines, DeadlineOperation::PollDue) {
                operation.run(SamplingStage::GestureDeadline, |recovery| {
                    flui_interaction::__runtime::tick_deadlines_with_recovery(
                        presentation.gestures(),
                        now,
                        recovery,
                    );
                });
                operation.run(SamplingStage::HapticDelivery, |recovery| {
                    presentation.haptics().service(recovery);
                });
            }
            operation.establish_recovery();
        }
        if matches!(deadlines, DeadlineOperation::PollDue)
            && let Some(operation) = sampling.first_mut()
        {
            operation.run(SamplingStage::GestureDeadline, |_| self.service_drags(now));
        }
        for operation in &mut sampling {
            if let Some(kind) = operation.take_failure_kind(self) {
                self.queue_sampling_failure(operation.presentation, kind);
                operation.recovery.inherit_failure();
            }
        }
        for operation in &mut sampling {
            if operation.rejected() {
                // Both report admission and addressed recovery demand precede
                // foreign wake delivery. A competing wake failure keeps the
                // original report authoritative and its wake debt retryable.
                operation
                    .recovery
                    .scope()
                    .run_classified(FailureClass::Delivery, |_| {
                        self.request_redraw_for(operation.presentation);
                    });
            }
        }
    }

    pub(super) fn sample_presentations(
        &self,
        constraints: BoxConstraints,
    ) -> Vec<PresentationSampling<'_>> {
        let raw = self.raw_frame_time();
        let now = self
            .frame_time
            .get()
            .unwrap_or_else(|| flui_foundation::MonotonicClock::now(&self.clock));
        let mut sampling: Vec<_> = self
            .presentations
            .iter()
            .map(PresentationSampling::new)
            .collect();
        for operation in &mut sampling {
            let constraints = self.root_constraints(operation.presentation(), constraints);
            operation.sample(self, constraints, raw, now);
        }
        // The runtime's drags are not any one presentation's; their deadlines
        // are reported against the first presentation sampled.
        if let Some(operation) = sampling.first_mut() {
            operation.run(SamplingStage::GestureDeadline, |_| self.service_drags(now));
        }
        for operation in &mut sampling {
            operation.admit(self);
            operation.establish_recovery();
        }
        for operation in &mut sampling {
            operation.report(self);
        }
        sampling
    }
}
