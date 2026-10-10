//! Owner-local closed numeric answers. Native evaluation belongs outside measurement.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::{Rc, Weak};

use flui_foundation::{TextSize, TextSizeRequest};
use lru::LruCache;

/// Identity of the retained answer store for one native sizing capture.
/// Cloning this identity grants no admission authority.
/// Policies and answer leases stay on their measurement owner's thread.
#[derive(Clone)]
pub struct TextSizingSource(Rc<ExactAnswers>);

impl fmt::Debug for TextSizingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextSizingSource")
            .field(&Rc::as_ptr(&self.0))
            .finish()
    }
}

impl PartialEq for TextSizingSource {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for TextSizingSource {}

impl TextSizingSource {
    /// Observe this capture's numeric answers without admission authority.
    #[must_use]
    pub fn policy(&self) -> TextSizing {
        TextSizing(Policy::Exact(self.clone()), None)
    }
}

/// Numeric sizing authority explicitly supplied to a measurement.
#[derive(Clone, Debug)]
pub struct TextSizing(pub(super) Policy, Option<Weak<AttemptAnswers>>);

#[derive(Clone, Debug)]
pub(super) enum Policy {
    Fixed,
    Linear(f64),
    Exact(TextSizingSource),
}

#[derive(Debug)]
pub(super) struct Answer {
    request: TextSizeRequest,
    size: TextSize,
}

#[derive(Debug)]
struct AnswerState {
    indexed: HashMap<TextSizeRequest, Weak<Answer>>,
    warm: LruCache<TextSizeRequest, Rc<Answer>>,
    revision: u64,
}

impl Default for AnswerState {
    fn default() -> Self {
        Self {
            indexed: HashMap::new(),
            warm: LruCache::unbounded(),
            revision: 0,
        }
    }
}

type CohortAnswers = RefCell<HashMap<TextSizeRequest, Option<Rc<Answer>>>>;

#[derive(Debug, Default)]
struct AttemptAnswers {
    sources: RefCell<Vec<(TextSizingSource, Rc<CohortAnswers>)>>,
}

impl AttemptAnswers {
    fn pins(&self, source: &TextSizingSource) -> Rc<CohortAnswers> {
        let mut sources = self.sources.borrow_mut();
        if let Some((_, pins)) = sources.iter().find(|(candidate, _)| candidate == source) {
            return Rc::clone(pins);
        }
        let pins = Rc::new(RefCell::new(HashMap::new()));
        source.0.cohorts.borrow_mut().push(Rc::downgrade(&pins));
        sources.push((source.clone(), Rc::clone(&pins)));
        pins
    }
}

impl Drop for AttemptAnswers {
    fn drop(&mut self) {
        let sources = std::mem::take(self.sources.get_mut());
        for (source, pins) in sources {
            let identity = Rc::downgrade(&pins);
            source
                .0
                .cohorts
                .borrow_mut()
                .retain(|cohort| !Weak::ptr_eq(cohort, &identity));
            // Numeric pins retire after the registry guard has ended.
            drop(pins);
            source.0.trim();
        }
    }
}

#[derive(Debug)]
struct ExactAnswers {
    state: RefCell<AnswerState>,
    cohorts: RefCell<Vec<Weak<CohortAnswers>>>,
    warm_capacity: Cell<usize>,
}

impl ExactAnswers {
    fn new(warm_capacity: usize) -> Self {
        Self {
            state: RefCell::new(AnswerState::default()),
            cohorts: RefCell::new(Vec::new()),
            warm_capacity: Cell::new(warm_capacity),
        }
    }

    fn pin(&self, answers: &[(TextSizeRequest, Rc<Answer>)]) {
        let cohorts: Vec<_> = {
            let mut cohorts = self.cohorts.borrow_mut();
            let active: Vec<_> = cohorts.iter().filter_map(Weak::upgrade).collect();
            cohorts.retain(|cohort| cohort.strong_count() != 0);
            active
        };
        for cohort in cohorts {
            let mut pins = cohort.borrow_mut();
            for (request, answer) in answers {
                if let Some(pin) = pins.get_mut(request) {
                    *pin = Some(Rc::clone(answer));
                }
            }
        }
    }

    fn trim(&self) {
        let mut state = self.state.borrow_mut();
        let capacity = self.warm_capacity.get();
        while state.warm.len() > capacity {
            state.warm.pop_lru();
        }
        state.indexed.retain(|_, answer| answer.strong_count() != 0);
    }

    fn refresh_warm(&self, answers: &[(TextSizeRequest, Rc<Answer>)]) {
        let mut state = self.state.borrow_mut();
        let capacity = self.warm_capacity.get();
        for (request, answer) in answers {
            state.warm.put(*request, Rc::clone(answer));
        }
        while state.warm.len() > capacity {
            state.warm.pop_lru();
        }
    }
}

/// An admitted size retaining the answer used by live geometry.
///
/// An exact answer remains conflict-protected while this value is retained,
/// even after its warm-cache entry is evicted. Native captures must themselves
/// be deterministic when answering a request whose prior leases have retired.
#[derive(Clone, Debug)]
pub struct TextResolvedSize {
    size: TextSize,
    answer_lease: Option<Rc<Answer>>,
}

impl TextResolvedSize {
    /// The admitted logical value.
    #[must_use]
    pub const fn value(&self) -> f64 {
        self.size.value()
    }

    /// The validated numeric size, without retaining its answer authority.
    #[must_use]
    pub const fn size(&self) -> TextSize {
        self.size
    }

    fn fixed(size: TextSize) -> Self {
        Self {
            size,
            answer_lease: None,
        }
    }
}

/// Conflicting answers cannot describe the same retained native capture.
#[derive(Debug, thiserror::Error)]
#[error("conflicting exact text size answers for {request:?}")]
pub struct TextSizingConflict {
    /// The request whose repeated answer differs.
    pub request: TextSizeRequest,
}

/// A batch refused before changing any admitted answers.
#[derive(Debug, thiserror::Error)]
pub enum TextSizingAdmissionError {
    /// An answer contradicts another answer in the batch or a retained answer.
    #[error(transparent)]
    Conflict(#[from] TextSizingConflict),
    /// This store cannot publish another answer revision.
    #[error("text sizing answer revision exhausted")]
    RevisionExhausted,
}

/// The sole owner-local capability that can extend one captured answer store.
/// It cannot publish into another source and is neither cloneable nor Send.
#[derive(Debug)]
pub struct TextSizingAdmission {
    source: TextSizingSource,
}

/// Retains answers used during a finite measurement attempt across frontiers.
/// Drop after convergence or cancellation, including after a suspended attempt.
pub struct TextSizingCohort {
    policy: TextSizing,
    attempt: Rc<AttemptAnswers>,
}

impl fmt::Debug for TextSizingCohort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextSizingCohort")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl TextSizingCohort {
    /// An explicit attempt view. Its cache identity remains the capture's;
    /// requests measured through it belong only to this cohort.
    #[must_use]
    pub fn policy(&self) -> TextSizing {
        TextSizing(self.policy.0.clone(), Some(Rc::downgrade(&self.attempt)))
    }
}

impl TextSizingAdmission {
    /// The identity used to match requests to this admitted native capture.
    #[must_use]
    pub fn source(&self) -> TextSizingSource {
        self.source.clone()
    }

    /// Bound retained historical answers, independently of live geometry/pins.
    /// Zero disables warm retention; active finite working sets remain usable.
    pub fn set_warm_capacity(&mut self, capacity: usize) {
        self.source.0.warm_capacity.set(capacity);
        self.source.0.trim();
    }

    /// Atomically extend exact captured answers. Identical repeats coalesce;
    /// conflicts reject the complete batch without replacing live answers.
    /// Native evaluation and extensible iterators execute outside store guards.
    pub fn admit(
        &mut self,
        answers: impl IntoIterator<Item = (TextSizeRequest, TextSize)>,
    ) -> Result<(), TextSizingAdmissionError> {
        let unique = unique_answers(answers)?;
        let store = &self.source.0;
        let retained: Vec<_> = {
            let mut state = store.state.borrow_mut();
            let mut fresh = Vec::new();
            let mut retained = Vec::new();
            for (request, size) in &unique {
                if let Some(answer) = state.indexed.get(request).and_then(Weak::upgrade) {
                    if answer.size != *size {
                        return Err(TextSizingConflict { request: *request }.into());
                    }
                    retained.push((*request, answer));
                } else {
                    fresh.push((*request, *size));
                }
            }
            let revision = if fresh.is_empty() {
                state.revision
            } else {
                state
                    .revision
                    .checked_add(1)
                    .ok_or(TextSizingAdmissionError::RevisionExhausted)?
            };
            for (request, size) in fresh {
                let answer = Rc::new(Answer { request, size });
                state.indexed.insert(request, Rc::downgrade(&answer));
                retained.push((request, answer));
            }
            for (request, answer) in &retained {
                state.warm.put(*request, Rc::clone(answer));
            }
            state.revision = revision;
            retained
        };
        store.pin(&retained);
        drop(retained);
        store.trim();
        Ok(())
    }
}

fn unique_answers(
    answers: impl IntoIterator<Item = (TextSizeRequest, TextSize)>,
) -> Result<HashMap<TextSizeRequest, TextSize>, TextSizingConflict> {
    let mut unique = HashMap::new();
    for (request, answer) in answers {
        if let Some(previous) = unique.insert(request, answer)
            && previous != answer
        {
            return Err(TextSizingConflict { request });
        }
    }
    Ok(unique)
}

impl Default for TextSizing {
    fn default() -> Self {
        Self::fixed()
    }
}

impl TextSizing {
    /// Preserve authored logical sizes.
    #[must_use]
    pub const fn fixed() -> Self {
        Self(Policy::Fixed, None)
    }

    /// Resolve by a positive finite uniform factor.
    pub fn linear(factor: f64) -> Result<Self, crate::TextLayoutError> {
        if factor > 0.0 && factor.is_finite() {
            Ok(Self(Policy::Linear(factor), None))
        } else {
            Err(crate::TextLayoutError::InvalidScale { factor })
        }
    }

    pub(super) const fn linear_unchecked(factor: f64) -> Self {
        Self(Policy::Linear(factor), None)
    }

    /// Seal a separately identified exact answer set. Missing requests cannot
    /// be satisfied by manufacturing another answer set with its source label.
    pub fn exact(
        answers: impl IntoIterator<Item = (TextSizeRequest, TextSize)>,
    ) -> Result<Self, TextSizingConflict> {
        let unique = unique_answers(answers)?;
        let store = ExactAnswers::new(unique.len());
        {
            let mut state = store.state.borrow_mut();
            for (request, size) in unique {
                let answer = Rc::new(Answer { request, size });
                state.indexed.insert(request, Rc::downgrade(&answer));
                state.warm.put(request, answer);
            }
        }
        Ok(Self(Policy::Exact(TextSizingSource(Rc::new(store))), None))
    }

    /// Create a captured store and its sole owner-local admission capability.
    /// Cloned policies observe admitted extensions without changing identity.
    #[must_use]
    pub fn captured() -> (Self, TextSizingAdmission) {
        let source = TextSizingSource(Rc::new(ExactAnswers::new(256)));
        (
            Self(Policy::Exact(source.clone()), None),
            TextSizingAdmission { source },
        )
    }

    /// Retain the finite working set of this measurement attempt, including
    /// explicitly selected policies from other sources. This grants no writer.
    #[must_use]
    pub fn begin_cohort(&self) -> TextSizingCohort {
        TextSizingCohort {
            policy: TextSizing(self.0.clone(), None),
            attempt: Rc::new(AttemptAnswers::default()),
        }
    }

    /// Resolve using closed numeric data, retaining the live answer authority.
    pub fn resolve(
        &self,
        request: TextSizeRequest,
    ) -> Result<TextResolvedSize, TextMeasurementError> {
        Ok(self.resolve_requests(&[request])?.remove(0))
    }

    /// Resolve an explicit override within this measurement attempt. Numeric
    /// authority stays with the selected policy; retention follows the attempt.
    pub fn resolve_with(
        &self,
        request: TextSizeRequest,
        sizing_override: Option<&Self>,
    ) -> Result<TextResolvedSize, TextMeasurementError> {
        self.selected(sizing_override).resolve(request)
    }

    pub(super) fn selected(&self, sizing_override: Option<&Self>) -> Self {
        sizing_override
            .unwrap_or(self)
            .clone()
            .with_cohort_from(self)
    }

    pub(super) fn retain_resolved(&self, answers: &[TextResolvedSize]) {
        if let Policy::Exact(source) = &self.0
            && let Some(attempt) = self.1.as_ref().and_then(Weak::upgrade)
        {
            let cohort = attempt.pins(source);
            let mut pins = cohort.borrow_mut();
            for answer in answers
                .iter()
                .filter_map(|answer| answer.answer_lease.as_ref())
            {
                pins.insert(answer.request, Some(Rc::clone(answer)));
            }
        }
    }

    pub(super) fn resolve_requests(
        &self,
        requests: &[TextSizeRequest],
    ) -> Result<Vec<TextResolvedSize>, TextMeasurementError> {
        match &self.0 {
            Policy::Fixed => Ok(requests
                .iter()
                .map(|request| TextResolvedSize::fixed(request.size))
                .collect()),
            Policy::Linear(factor) => {
                if !factor.is_finite() || *factor <= 0.0 {
                    return Err(crate::TextLayoutError::InvalidScale { factor: *factor }.into());
                }
                requests
                    .iter()
                    .map(|request| {
                        let size = request.size.value() * factor;
                        TextSize::new(size)
                            .map(TextResolvedSize::fixed)
                            .map_err(|_| crate::TextLayoutError::InvalidFontSize { size }.into())
                    })
                    .collect()
            }
            Policy::Exact(source) => {
                let store = &source.0;
                let cohort = self
                    .1
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .map(|attempt| (attempt.pins(source), attempt));
                let (resolved, retained, missing) = {
                    let state = store.state.borrow();
                    let mut resolved = Vec::with_capacity(requests.len());
                    let mut retained = Vec::new();
                    let mut missing = Vec::new();
                    let mut missing_keys = HashSet::new();
                    for request in requests {
                        if let Some(answer) = state.indexed.get(request).and_then(Weak::upgrade) {
                            resolved.push(TextResolvedSize {
                                size: answer.size,
                                answer_lease: Some(Rc::clone(&answer)),
                            });
                            retained.push((*request, answer));
                        } else if missing_keys.insert(*request) {
                            missing.push(*request);
                        }
                    }
                    // Commit membership as part of this closed lookup before
                    // returning an owned frontier to native service. Only
                    // state -> numeric pins nests; admission releases state
                    // before borrowing pins. No extensible code runs here.
                    if let Some((cohort, _attempt)) = &cohort {
                        let mut pins = cohort.borrow_mut();
                        for request in requests {
                            pins.entry(*request).or_insert(None);
                        }
                        for (request, answer) in &retained {
                            pins.insert(*request, Some(Rc::clone(answer)));
                        }
                    }
                    (resolved, retained, missing)
                };
                store.refresh_warm(&retained);
                if missing.is_empty() {
                    Ok(resolved)
                } else {
                    Err(TextPreparationPending {
                        policy_source: source.clone(),
                        requests: missing,
                    }
                    .into())
                }
            }
        }
    }

    pub(super) fn matches(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Policy::Fixed, Policy::Fixed) => true,
            (Policy::Linear(a), Policy::Linear(b)) => a.to_bits() == b.to_bits(),
            (Policy::Exact(a), Policy::Exact(b)) => a == b,
            _ => false,
        }
    }

    pub(super) fn linear_factor(&self) -> Option<f64> {
        if let Policy::Linear(factor) = self.0 {
            Some(factor)
        } else {
            None
        }
    }

    pub(super) fn with_cohort_from(mut self, inherited: &Self) -> Self {
        if inherited.1.is_some() {
            self.1.clone_from(&inherited.1);
        }
        self
    }
}

impl PartialEq for TextSizing {
    fn eq(&self, other: &Self) -> bool {
        self.matches(other)
    }
}

/// An owned frontier whose answers are absent from the retained capture.
#[derive(Clone, Debug, thiserror::Error)]
#[error("text preparation awaits {} exact sizing answers", requests.len())]
pub struct TextPreparationPending {
    /// The capture to evaluate after all measurement loans have been released.
    pub policy_source: TextSizingSource,
    /// Every missing authored size/profile pair in this paragraph, once each.
    pub requests: Vec<TextSizeRequest>,
}

/// Preparation debt differs from an invalid authored layout.
#[derive(Debug, thiserror::Error)]
pub enum TextMeasurementError {
    /// Authored input or shaping failed.
    #[error(transparent)]
    Layout(#[from] crate::TextLayoutError),
    /// Native answers must be collected outside the borrowed text context.
    #[error(transparent)]
    Pending(#[from] TextPreparationPending),
}
