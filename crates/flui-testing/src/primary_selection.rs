//! A scripted primary selection: the [`PrimarySelectionCapability`] a
//! headless host registers, which a test reads, fills and steers
//! (ADR-0152 §4).
//!
//! The selection lives in the test: a widget's claim is asked for its text
//! when the test or a widget reads, as another application would ask it;
//! [`select`](ScriptedPrimarySelection::select) stands in for another
//! application selecting, which makes every claim stale. Reads answer at once
//! unless [`hold_reads`](ScriptedPrimarySelection::hold_reads), and
//! [`refuse`](ScriptedPrimarySelection::refuse) makes the provider refuse,
//! as a host without a primary selection does.

use std::{cell::RefCell, fmt, rc::Rc};

use flui_interaction::{
    PrimaryClaimId, PrimaryClaims, PrimarySelectionBackend, PrimarySelectionCapability,
    PrimarySelectionHandle, SelectionText,
};
use flui_platform::{
    Unsupported,
    capability::{CapabilityProvider, CapabilityRegistrar, ProviderContext, Resolver},
};

/// The primary selection a headless test host hands its widgets.
/// Owner-thread, cheap to clone; every clone is the same selection.
#[derive(Clone, Default)]
pub struct ScriptedPrimarySelection {
    state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct State {
    claims: PrimaryClaims,
    /// Another application's selection, which holds while no claim does.
    foreign: Option<String>,
    hold: bool,
    held: Vec<Resolver<Option<String>>>,
    refusal: Option<Unsupported>,
    fail_releases: bool,
    handles: Rc<()>,
}

impl ScriptedPrimarySelection {
    /// An empty selection whose reads answer at once.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The selection's text: the current claim's, asked now, or another
    /// application's.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        let (claim, foreign) = {
            let state = self.state.borrow();
            (state.claims.current(), state.foreign.clone())
        };
        // Asked with no borrow held: the claim may read widget state.
        match claim {
            Some(claim) => claim.text(),
            None => foreign,
        }
    }

    /// Whether a widget's claim holds the selection.
    #[must_use]
    pub fn is_claimed(&self) -> bool {
        self.state.borrow().claims.current().is_some()
    }

    /// Select `text` in another application: every claim goes stale.
    pub fn select(&self, text: impl Into<String>) {
        let lost = {
            let mut state = self.state.borrow_mut();
            state.foreign = Some(text.into());
            state.claims.lose()
        };
        drop(lost);
    }

    /// Keep every later read unanswered until
    /// [`answer_held_reads`](Self::answer_held_reads).
    pub fn hold_reads(&self) {
        self.state.borrow_mut().hold = true;
    }

    /// Answer the reads held so far with the selection's text now, and
    /// answer later reads at once again. Returns how many reads it answered.
    pub fn answer_held_reads(&self) -> usize {
        let held = {
            let mut state = self.state.borrow_mut();
            state.hold = false;
            std::mem::take(&mut state.held)
        };
        let text = self.text();
        let answered = held.len();
        for answer in held {
            // A retired owner has nobody left to tell.
            let _ = answer.resolve(text.clone());
        }
        answered
    }

    /// Make the provider refuse every later acquisition with `refusal`.
    pub fn refuse(&self, refusal: Unsupported) {
        self.state.borrow_mut().refusal = Some(refusal);
    }

    /// Make every later release of a claim panic once the claim is
    /// released, as a failing host backend does.
    pub fn fail_releases(&self) {
        self.state.borrow_mut().fail_releases = true;
    }

    /// How many handles over this selection are alive.
    #[must_use]
    pub fn live_handles(&self) -> usize {
        Rc::strong_count(&self.state.borrow().handles) - 1
    }

    /// Register this selection as the provider of
    /// [`PrimarySelectionCapability`].
    pub fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<PrimarySelectionCapability>(self.clone());
    }
}

impl CapabilityProvider<PrimarySelectionCapability> for ScriptedPrimarySelection {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<PrimarySelectionHandle, Unsupported> {
        let state = self.state.borrow();
        if let Some(refusal) = state.refusal {
            return Err(refusal);
        }
        let backend = Backend {
            selection: self.clone(),
            _handle: Rc::clone(&state.handles),
        };
        Ok(PrimarySelectionHandle::new(
            backend,
            cx.completions().clone(),
        ))
    }
}

impl fmt::Debug for ScriptedPrimarySelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("ScriptedPrimarySelection")
            .field("claims", &state.claims)
            .field("foreign", &state.foreign)
            .field("held_reads", &state.held.len())
            .field("refusal", &state.refusal)
            .finish()
    }
}

/// One handle's backend; it counts as a live handle while it exists.
struct Backend {
    selection: ScriptedPrimarySelection,
    _handle: Rc<()>,
}

impl PrimarySelectionBackend for Backend {
    fn claim(
        &self,
        text: Rc<dyn SelectionText>,
    ) -> (PrimaryClaimId, Option<Rc<dyn SelectionText>>) {
        let mut state = self.selection.state.borrow_mut();
        let claimed = state.claims.claim(text);
        if state.claims.is_current(claimed.0) {
            state.foreign = None;
        }
        claimed
    }

    fn release(&self, claim: PrimaryClaimId) -> Option<Rc<dyn SelectionText>> {
        let (released, fails) = {
            let mut state = self.selection.state.borrow_mut();
            (state.claims.release(claim), state.fail_releases)
        };
        assert!(!fails, "the primary selection backend failed to release");
        released
    }

    fn is_current(&self, claim: PrimaryClaimId) -> bool {
        self.selection.state.borrow().claims.is_current(claim)
    }

    fn read_text(&self, answer: Resolver<Option<String>>) {
        {
            let mut state = self.selection.state.borrow_mut();
            if state.hold {
                state.held.push(answer);
                return;
            }
        }
        // A retired owner has nobody left to tell.
        let _ = answer.resolve(self.selection.text());
    }
}
