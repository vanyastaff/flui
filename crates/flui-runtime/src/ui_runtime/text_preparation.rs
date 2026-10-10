//! Presentation-local retained numeric preparation, serviced outside tree loans.

use std::rc::{Rc, Weak};

use flui_animation::FrameTick;
use flui_foundation::geometry::DevicePixelRatio;
use flui_foundation::{PresentationId, TextSizeRequest};
use flui_painting::{TextPreparationPending, TextSizing, TextSizingCohort, TextSizingSource};
use flui_rendering::{LayoutPremise, constraints::BoxConstraints, pipeline::PipelineOwner};
use flui_view::BuildPremise;
use web_time::{Duration, Instant};

use super::UiRuntime;
use crate::presentation::PresentationState;

const RECEIPT_RETRY: Duration = Duration::from_millis(100);

/// Detached numeric work for one exact retained presentation segment.
/// Dropping a receipt does not erase its accepted frontier.
pub struct TextSizingWork {
    presentation: PresentationId,
    segment: Rc<()>,
    receipt: Rc<()>,
    source: TextSizingSource,
    requests: Vec<TextSizeRequest>,
}

impl std::fmt::Debug for TextSizingWork {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextSizingWork")
            .field("presentation", &self.presentation)
            .field("source", &self.source)
            .field("requests", &self.requests)
            .finish_non_exhaustive()
    }
}

impl TextSizingWork {
    /// The exact presentation that retains this segment.
    #[must_use]
    pub fn presentation(&self) -> PresentationId {
        self.presentation
    }
    /// The numeric source whose writer may satisfy these requests.
    #[must_use]
    pub fn source(&self) -> &TextSizingSource {
        &self.source
    }
    /// The owned missing frontier, in measurement discovery order.
    #[must_use]
    pub fn requests(&self) -> &[TextSizeRequest] {
        &self.requests
    }
}

/// Host disposition after servicing a detached frontier.
#[derive(Debug, Clone, Copy)]
pub enum TextSizingSettlement {
    /// Numeric answers are already admitted into the exact source.
    Ready,
    /// An accepted native ticket awaits an explicit readiness cue.
    Waiting,
    /// No producer can currently answer this source.
    Unavailable,
    /// Bounded native service stalled; edits can repair unavailable geometry.
    Parked,
}

enum Wait {
    Running,
    Exportable,
    InFlight {
        receipt: Weak<()>,
        retry_at: Instant,
    },
    Waiting,
    Parked,
    Stalled {
        receipt: Weak<()>,
    },
    Resume,
}

pub(crate) struct SavedTextSegment {
    seal: Rc<()>,
    pub(crate) tick: FrameTick,
    pub(crate) host_time: Instant,
    pub(crate) constraints: BoxConstraints,
    dpr: DevicePixelRatio,
    policy: TextSizing,
    _cohort: TextSizingCohort,
    layout: LayoutPremise,
    build: BuildPremise,
    frontier: Option<TextPreparationPending>,
    wait: Wait,
    serviced: bool,
    service_attempts: u8,
}

#[derive(Default)]
pub(crate) struct TextPreparation {
    installed: Option<TextSizingSource>,
    replacement_barrier: bool,
    pub(crate) segment: Option<SavedTextSegment>,
}

impl TextPreparation {
    pub(crate) fn holds_document_barrier(&self) -> bool {
        self.replacement_barrier
            || self
                .segment
                .as_ref()
                .is_some_and(|segment| !matches!(segment.wait, Wait::Parked | Wait::Stalled { .. }))
    }
    pub(crate) fn next_wake(&self) -> Option<Instant> {
        match &self.segment.as_ref()?.wait {
            Wait::InFlight { retry_at, .. } => Some(*retry_at),
            _ => None,
        }
    }
    pub(crate) fn installed_policy(&self) -> Option<TextSizing> {
        self.installed.as_ref().map(TextSizingSource::policy)
    }
}

fn accepts(presentation: &PresentationState, saved: &SavedTextSegment) -> bool {
    !presentation.closing_requested.get()
        && presentation.pipeline().with(|owner| {
            owner.accepts_layout_premise(&saved.layout)
                && owner.text_sizing() == &saved.policy
                && owner.device_pixel_ratio().to_bits() == saved.dpr.get().to_bits()
        })
        && presentation
            .widgets()
            .with_build_owner(|owner| owner.accepts_build_premise(&saved.build))
}

pub(crate) fn reconcile(presentation: &PresentationState, constraints: BoxConstraints) {
    presentation.pipeline().with_mut(|owner| {
        owner.drain_pending_dirty();
    });
    let compatible = presentation
        .text_preparation
        .borrow()
        .segment
        .as_ref()
        .is_none_or(|saved| saved.constraints == constraints && accepts(presentation, saved));
    if !compatible {
        let retired = presentation.text_preparation.borrow_mut().segment.take();
        presentation.pipeline().withdraw_layout();
        drop(retired);
    }
}

pub(crate) fn can_run(presentation: &PresentationState) -> bool {
    presentation
        .text_preparation
        .borrow()
        .segment
        .as_ref()
        .is_none_or(|saved| matches!(saved.wait, Wait::Resume))
}

pub(crate) fn admit(
    presentation: &PresentationState,
    constraints: BoxConstraints,
    now: Instant,
) -> bool {
    if let Some(saved) = presentation.text_preparation.borrow_mut().segment.as_mut() {
        saved.wait = Wait::Running;
        presentation
            .pipeline()
            .with_mut(PipelineOwner::resume_retained_layout);
        return false;
    }
    let policy = presentation
        .pipeline()
        .with(|owner| owner.text_sizing().clone());
    let cohort = policy.begin_cohort();
    let attempt_policy = cohort.policy();
    presentation.pipeline().with_mut(|owner| {
        let _ = owner.set_text_sizing(attempt_policy);
        owner.set_root_constraints(Some(constraints));
    });
    let saved = SavedTextSegment {
        seal: Rc::new(()),
        tick: presentation
            .sampled_motion_tick
            .get()
            .expect("BUG: admitted segment has a sampled tick"),
        host_time: now,
        constraints,
        dpr: presentation.pipeline().with(|owner| {
            DevicePixelRatio::new(owner.device_pixel_ratio()).expect("BUG: accepted render DPR")
        }),
        policy,
        _cohort: cohort,
        layout: presentation.pipeline().with(PipelineOwner::layout_premise),
        build: presentation
            .widgets()
            .with_build_owner(flui_view::BuildOwner::build_premise),
        frontier: None,
        wait: Wait::Running,
        serviced: false,
        service_attempts: 0,
    };
    let mut state = presentation.text_preparation.borrow_mut();
    state.segment = Some(saved);
    state.replacement_barrier = false;
    true
}

pub(crate) fn pending(presentation: &PresentationState, frontier: TextPreparationPending) -> bool {
    let registered =
        presentation.text_preparation.borrow().installed.as_ref() == Some(&frontier.policy_source);
    let layout = presentation.pipeline().with(PipelineOwner::layout_premise);
    let build = presentation
        .widgets()
        .with_build_owner(flui_view::BuildOwner::build_premise);
    if let Some(saved) = presentation.text_preparation.borrow_mut().segment.as_mut() {
        saved.layout = layout;
        saved.build = build;
        saved.frontier = Some(frontier);
        saved.wait = if registered {
            Wait::Exportable
        } else {
            Wait::Parked
        };
        saved.serviced = false;
    }
    presentation
        .pipeline()
        .with_mut(PipelineOwner::defer_layout_until_input);
    registered
}

pub(crate) fn finish(presentation: &PresentationState) {
    let retired = presentation.text_preparation.borrow_mut().segment.take();
    drop(retired);
}

impl UiRuntime {
    /// Install one captured authority in both the render and inherited paths.
    pub fn install_captured_text_sizing_for(
        &self,
        id: PresentationId,
        source: TextSizingSource,
    ) -> bool {
        let Some(presentation) = self
            .presentations
            .get(id)
            .filter(|p| !p.closing_requested.get())
        else {
            return false;
        };
        let retired = {
            let mut state = presentation.text_preparation.borrow_mut();
            if state.installed.as_ref() == Some(&source) {
                return true;
            }
            state.installed = Some(source.clone());
            state.replacement_barrier = true;
            state.segment.take()
        };
        presentation.pipeline().withdraw_layout();
        presentation.text_input().set_transaction_open(true);
        drop(retired);
        let policy = source.policy();
        let wake = presentation
            .pipeline()
            .with_mut(|owner| owner.set_text_sizing(policy.clone()));
        let rebuild = presentation
            .media_query
            .commit(|data| data.text_sizing = policy);
        if let Some(wake) = wake {
            wake.notify();
        }
        if let Some(rebuild) = rebuild {
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        }
        self.request_redraw_for(presentation);
        true
    }

    /// Remove an exact installed producer and restore the accepted scalar fallback.
    pub fn withdraw_captured_text_sizing_for(
        &self,
        id: PresentationId,
        source: &TextSizingSource,
    ) -> bool {
        let Some(presentation) = self.presentations.get(id) else {
            return false;
        };
        let retired = {
            let mut state = presentation.text_preparation.borrow_mut();
            if state.installed.as_ref() != Some(source) {
                return false;
            }
            state.installed = None;
            state.replacement_barrier = true;
            state.segment.take()
        };
        presentation.pipeline().withdraw_layout();
        presentation.text_input().set_transaction_open(true);
        drop(retired);
        let factor = self
            .preferences
            .borrow()
            .as_ref()
            .and_then(|snapshot| snapshot.values.text_scale())
            .unwrap_or(1.0);
        let policy = TextSizing::linear(factor).expect("BUG: accepted scalar fallback");
        let wake = presentation
            .pipeline()
            .with_mut(|owner| owner.set_text_sizing(policy.clone()));
        presentation
            .media_query
            .update(|data| data.text_sizing = policy);
        if let Some(wake) = wake {
            wake.notify();
        }
        self.request_redraw_for(presentation);
        true
    }

    /// Make a waiting source exportable after actual host readiness or deadline.
    pub fn service_text_sizing_source_for(
        &self,
        id: PresentationId,
        source: &TextSizingSource,
    ) -> bool {
        let Some(presentation) = self.presentations.get(id) else {
            return false;
        };
        let changed = {
            let mut state = presentation.text_preparation.borrow_mut();
            if state.installed.as_ref() != Some(source) {
                return false;
            }
            if let Some(saved) = state.segment.as_mut()
                && saved
                    .frontier
                    .as_ref()
                    .is_some_and(|pending| &pending.policy_source == source)
            {
                saved.wait = Wait::Exportable;
                true
            } else {
                false
            }
        };
        if changed {
            presentation.text_input().set_transaction_open(true);
            self.request_redraw_for(presentation);
        }
        changed
    }

    /// Detach owned frontiers after the guarded measurement fixpoint returned.
    pub fn take_text_sizing_frontiers(&self) -> Vec<TextSizingWork> {
        let mut work = Vec::new();
        for presentation in self.presentations.iter() {
            let now = presentation.clock().now();
            presentation.pipeline().with_mut(|owner| {
                owner.drain_pending_dirty();
            });
            let mut state = presentation.text_preparation.borrow_mut();
            let Some(saved) = state
                .segment
                .as_mut()
                .filter(|saved| accepts(presentation, saved))
            else {
                continue;
            };
            if let Wait::InFlight { receipt, retry_at } = &saved.wait
                && *retry_at <= now
                && (receipt.strong_count() != 0 || saved.service_attempts >= 8)
            {
                saved.wait = Wait::Stalled {
                    receipt: receipt.clone(),
                };
                drop(state);
                presentation.pipeline().withdraw_layout();
                presentation.text_input().set_transaction_open(false);
                let _ = presentation.text_input().run_deferred_grants();
                continue;
            }
            let export = matches!(saved.wait, Wait::Exportable)
                || matches!(&saved.wait, Wait::InFlight {receipt, retry_at} if receipt.strong_count() == 0 && *retry_at <= now);
            if !export {
                continue;
            }
            let Some(frontier) = &saved.frontier else {
                continue;
            };
            let receipt = Rc::new(());
            saved.service_attempts = saved.service_attempts.saturating_add(1);
            saved.wait = Wait::InFlight {
                receipt: Rc::downgrade(&receipt),
                retry_at: now + RECEIPT_RETRY,
            };
            work.push(TextSizingWork {
                presentation: presentation.id(),
                segment: Rc::clone(&saved.seal),
                receipt,
                source: frontier.policy_source.clone(),
                requests: frontier.requests.clone(),
            });
            let service = !saved.serviced;
            saved.serviced = true;
            drop(state);
            presentation.text_input().set_transaction_open(true);
            if service {
                presentation.service_completion_segment();
            }
        }
        work
    }

    /// Settle only the exact still-current segment, after host numeric admission.
    pub fn settle_text_sizing(&self, work: TextSizingWork, outcome: TextSizingSettlement) -> bool {
        let Some(presentation) = self.presentations.get(work.presentation) else {
            return false;
        };
        presentation.pipeline().with_mut(|owner| {
            owner.drain_pending_dirty();
        });
        let accepted = {
            let mut state = presentation.text_preparation.borrow_mut();
            let Some(saved) = state.segment.as_mut().filter(|saved| {
                Rc::ptr_eq(&saved.seal, &work.segment) && accepts(presentation, saved)
            }) else {
                return false;
            };
            if !matches!(&saved.wait, Wait::InFlight {receipt, ..} | Wait::Stalled {receipt} if Weak::ptr_eq(receipt, &Rc::downgrade(&work.receipt)))
            {
                return false;
            }
            saved.wait = match outcome {
                TextSizingSettlement::Ready => Wait::Resume,
                TextSizingSettlement::Waiting => Wait::Waiting,
                TextSizingSettlement::Unavailable | TextSizingSettlement::Parked => Wait::Parked,
            };
            true
        };
        if accepted {
            if matches!(
                outcome,
                TextSizingSettlement::Ready | TextSizingSettlement::Waiting
            ) {
                presentation.text_input().set_transaction_open(true);
            }
            if matches!(outcome, TextSizingSettlement::Ready) {
                self.request_redraw_for(presentation);
            } else if matches!(
                outcome,
                TextSizingSettlement::Unavailable | TextSizingSettlement::Parked
            ) {
                presentation.pipeline().withdraw_layout();
                presentation.text_input().set_transaction_open(false);
                let _ = presentation.text_input().run_deferred_grants();
            }
        }
        accepted
    }
}
