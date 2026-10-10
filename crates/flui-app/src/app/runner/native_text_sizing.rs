//! Native numeric producers owned by exact installed presentations.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::PresentationAddress;
#[cfg(any(
    target_os = "android",
    target_os = "ios",
    all(test, not(target_arch = "wasm32"))
))]
use flui_painting::TextSizingSource;
use flui_painting::{TextSizing, TextSizingAdmission};
use flui_platform::traits::HostWindow;
use flui_platform::{CapturedTextSizing, OwnerPlatform, TextSizingCaptureState};
use flui_runtime::owner::{OwnerEffects, PresentationDispatcher, TextSizingFrontier};
use flui_runtime::ui_runtime::TextSizingSettlement;

struct State {
    capsule: Option<CapturedTextSizing>,
    admission: TextSizingAdmission,
    deadline: Option<web_time::Instant>,
    parked: bool,
}

#[cfg(test)]
type CaptureSampler =
    Box<dyn FnMut() -> Result<TextSizingCaptureState, flui_platform::TextSizingCaptureError>>;

pub(super) struct NativeTextSizing {
    address: PresentationAddress,
    window: Arc<dyn HostWindow>,
    owner: Rc<OwnerPlatform>,
    live: Cell<bool>,
    state: RefCell<State>,
    #[cfg(test)]
    capture_sampler: RefCell<Option<CaptureSampler>>,
}

impl NativeTextSizing {
    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        all(test, not(target_arch = "wasm32"))
    ))]
    pub(super) fn prepare(
        address: PresentationAddress,
        window: Arc<dyn HostWindow>,
        owner: Rc<OwnerPlatform>,
    ) -> Rc<Self> {
        let (_, admission) = TextSizing::captured();
        Rc::new(Self {
            address,
            window,
            owner,
            live: Cell::new(true),
            state: RefCell::new(State {
                capsule: None,
                admission,
                deadline: None,
                parked: false,
            }),
            #[cfg(test)]
            capture_sampler: RefCell::new(None),
        })
    }

    pub(super) fn address(&self) -> PresentationAddress {
        self.address
    }

    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        all(test, not(target_arch = "wasm32"))
    ))]
    pub(super) fn source(&self) -> TextSizingSource {
        self.state.borrow().admission.source()
    }

    pub(super) fn fence(&self) {
        self.live.set(false);
    }

    pub(super) fn deadline(&self) -> Option<web_time::Instant> {
        self.live
            .get()
            .then(|| self.state.borrow().deadline)
            .flatten()
    }

    pub(super) fn refresh(
        &self,
        dispatcher: &PresentationDispatcher,
        effects: &dyn OwnerEffects,
        completion_wake: bool,
        current: impl Fn() -> bool,
    ) {
        if !self.live.get() || !current() {
            return;
        }
        let now = web_time::Instant::now();
        let (deadline, capsule) = {
            let state = self.state.borrow();
            (state.deadline, state.capsule.clone())
        };
        if deadline.is_some_and(|deadline| deadline > now)
            && capsule.as_ref().is_none_or(CapturedTextSizing::is_current)
            && (!completion_wake || capsule.is_some())
        {
            return;
        }
        if deadline.is_some_and(|deadline| deadline <= now) {
            self.state.borrow_mut().deadline = None;
        }
        if capsule
            .as_ref()
            .is_some_and(|capsule| !capsule.is_current())
        {
            let (source, retired) = {
                let mut state = self.state.borrow_mut();
                let (_, admission) = TextSizing::captured();
                let retired = (
                    state.capsule.take(),
                    std::mem::replace(&mut state.admission, admission),
                );
                (state.admission.source(), retired)
            };
            let _ = dispatcher.install_captured_text_sizing(source, effects);
            drop(retired);
            if !self.live.get() || !current() {
                return;
            }
        }
        // No App/Runtime/native registry borrow crosses the OS capture call.
        let captured = self.capture();
        if !self.live.get() || !current() {
            return;
        }
        match captured {
            Ok(TextSizingCaptureState::Ready(capsule)) if capsule.is_current() => {
                let (source, changed, install, retired) = {
                    let mut state = self.state.borrow_mut();
                    let changed = state
                        .capsule
                        .as_ref()
                        .is_some_and(|old| !old.same_capture(&capsule));
                    let retired = if changed {
                        let (_, admission) = TextSizing::captured();
                        Some(std::mem::replace(&mut state.admission, admission))
                    } else {
                        None
                    };
                    let old = state.capsule.replace(capsule);
                    let had_deadline = state.deadline.take().is_some();
                    let was_parked = std::mem::replace(&mut state.parked, false);
                    (
                        state.admission.source(),
                        changed
                            || old.is_none()
                            || had_deadline
                            || deadline.is_some()
                            || was_parked,
                        changed,
                        (old, retired),
                    )
                };
                drop(retired);
                if changed && self.live.get() && current() {
                    if install {
                        let _ = dispatcher.install_captured_text_sizing(source.clone(), effects);
                    }
                    if self.live.get() && current() {
                        let _ = dispatcher.service_text_sizing_source(&source, effects);
                    }
                }
            }
            Ok(TextSizingCaptureState::Pending { check_after }) => {
                let was_parked = self.state.borrow().parked;
                self.schedule_check(check_after, false);
                if was_parked && self.live.get() && current() {
                    let source = self.state.borrow().admission.source();
                    let _ = dispatcher.service_text_sizing_source(&source, effects);
                }
            }
            Ok(TextSizingCaptureState::RetryAfter(check_after)) => {
                self.park_for_retry(dispatcher, effects, check_after, &current);
            }
            Ok(TextSizingCaptureState::Stalled) => {
                // Observe the same outstanding receipt at a bounded cadence.
                // This is neither cancellation nor admission of another job.
                self.park_for_retry(dispatcher, effects, Duration::from_millis(500), &current);
            }
            Ok(TextSizingCaptureState::Unsupported) => {
                self.fence();
                if current() {
                    let source = self.state.borrow().admission.source();
                    let _ = dispatcher.service_text_sizing_source(&source, effects);
                }
            }
            Ok(TextSizingCaptureState::Ready(_)) => {}
            Err(error) => {
                self.park_for_retry(dispatcher, effects, Duration::from_millis(500), &current);
                tracing::warn!(%error, "native text sizing acquisition failed");
            }
        }
    }

    fn park_for_retry(
        &self,
        dispatcher: &PresentationDispatcher,
        effects: &dyn OwnerEffects,
        delay: Duration,
        current: &impl Fn() -> bool,
    ) {
        let newly_parked = !self.state.borrow().parked;
        self.schedule_check(delay, true);
        if newly_parked && self.live.get() && current() {
            let source = self.state.borrow().admission.source();
            let _ = dispatcher.service_text_sizing_source(&source, effects);
        }
    }

    fn schedule_check(&self, delay: Duration, parked: bool) {
        let deadline = web_time::Instant::now() + delay;
        let changed = {
            let mut state = self.state.borrow_mut();
            let next = Some(state.deadline.map_or(deadline, |old| old.min(deadline)));
            let changed = state.deadline != next;
            state.deadline = next;
            state.parked = parked;
            changed
        };
        // Polling an unchanged receipt must not create a wake feedback loop.
        if changed {
            let _ = self.owner.proxy().wake();
        }
    }

    fn capture(&self) -> Result<TextSizingCaptureState, flui_platform::TextSizingCaptureError> {
        #[cfg(test)]
        {
            let sampler = self.capture_sampler.borrow_mut().take();
            if let Some(mut sampler) = sampler {
                let result = sampler();
                *self.capture_sampler.borrow_mut() = Some(sampler);
                return result;
            }
        }
        self.owner.capture_text_sizing(&self.window)
    }

    pub(super) fn resolve(
        &self,
        frontier: TextSizingFrontier,
        effects: &dyn OwnerEffects,
        current: impl Fn() -> bool,
    ) -> Option<TextSizingFrontier> {
        if !self.live.get() || !current() {
            return Some(frontier);
        }
        let (capsule, parked, registered, deadline) = {
            let state = self.state.borrow();
            (
                state.capsule.clone(),
                state.parked,
                state.admission.source() == *frontier.source(),
                state.deadline,
            )
        };
        if !registered {
            return Some(frontier);
        }
        let Some(capsule) = capsule.filter(CapturedTextSizing::is_current) else {
            let outcome = if parked {
                TextSizingSettlement::Parked
            } else {
                TextSizingSettlement::Waiting
            };
            let _ = frontier.settle(outcome, effects);
            return None;
        };
        if deadline.is_some_and(|deadline| deadline > web_time::Instant::now()) {
            let outcome = if parked {
                TextSizingSettlement::Parked
            } else {
                TextSizingSettlement::Waiting
            };
            let _ = frontier.settle(outcome, effects);
            return None;
        }
        let requests = frontier.requests().to_vec();
        let mut answers = Vec::with_capacity(requests.len());
        let mut failure = None;
        for request in requests {
            if !self.live.get() || !current() || !capsule.is_current() {
                return Some(frontier);
            }
            match capsule.query(request) {
                Ok(size) => answers.push((request, size)),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if !self.live.get() || !current() || !capsule.is_current() {
            return Some(frontier);
        }
        let admitted = {
            let mut state = self.state.borrow_mut();
            if !state
                .capsule
                .as_ref()
                .is_some_and(|accepted| accepted.same_capture(&capsule))
            {
                return Some(frontier);
            }
            if state.admission.source() != *frontier.source() {
                return Some(frontier);
            }
            state.admission.admit(answers)
        };
        let mut outcome = TextSizingSettlement::Ready;
        if let Some(error) = &failure {
            outcome = match error {
                flui_platform::TextSizingCaptureError::InvalidSize(_)
                | flui_platform::TextSizingCaptureError::UnsupportedProfile => {
                    TextSizingSettlement::Unavailable
                }
                _ => {
                    self.schedule_check(Duration::from_millis(500), true);
                    TextSizingSettlement::Parked
                }
            };
        } else if admitted.is_err() {
            outcome = TextSizingSettlement::Unavailable;
        }
        let _ = frontier.settle(outcome, effects);
        // Debt is committed before extensible diagnostics.
        if let Some(error) = failure {
            tracing::warn!(%error, "native text sizing query failed");
        }
        if let Err(error) = admitted {
            tracing::warn!(%error, "native text sizing admission failed");
        }
        None
    }
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[path = "native_text_sizing_tests.rs"]
mod tests;
