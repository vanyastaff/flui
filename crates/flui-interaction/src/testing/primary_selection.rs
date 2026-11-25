//! A scripted primary selection: the [`PrimarySelectionCapability`] a
//! headless host registers, which a test reads, fills and steers
//! (ADR-0152 §4), on the scenario primitive
//! ([`flui_platform::capability::scripted`]).
//!
//! The selection lives in the test: a widget's claim is asked for its text
//! when the test or a widget reads, as another application would ask it;
//! [`select`](ScriptedPrimarySelection::select) stands in for another
//! application selecting, which makes every claim stale. Reads go through a
//! [`Script`] whose standing answer is the selection's text, so they answer
//! at once unless [`hold_reads`](ScriptedPrimarySelection::hold_reads), and
//! [`refuse`](ScriptedPrimarySelection::refuse) turns the service off so the
//! provider refuses, as a host without a primary selection does.

use std::{cell::RefCell, fmt, rc::Rc, sync::Arc};

use flui_platform::{
    ClipboardRead, Unsupported,
    capability::scripted::{Scenario, Script, ScriptHold, ServiceState, Track},
    capability::{CapabilityProvider, CapabilityRegistrar, ProviderContext, Resolver},
};

use crate::text_input::{
    PrimaryClaimId, PrimaryClaims, PrimarySelectionBackend, PrimarySelectionCapability,
    PrimarySelectionHandle, SelectionText,
};

/// A request a primary selection's handle makes of its host and is answered
/// later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrimarySelectionRequest {
    /// Read the selection's text.
    ReadText,
}

/// The primary selection a headless test host hands its widgets.
/// Owner-thread, cheap to clone; every clone is the same selection.
#[derive(Clone)]
pub struct ScriptedPrimarySelection {
    selection: Rc<RefCell<Selection>>,
    reads: Script<PrimarySelectionRequest, ClipboardRead>,
    service: Track<ServiceState>,
}

#[derive(Default)]
struct Selection {
    claims: PrimaryClaims,
    /// Another application's selection, which holds while no claim does.
    foreign: Option<String>,
    fail_releases: bool,
    /// The read script's hold
    /// [`hold_reads`](ScriptedPrimarySelection::hold_reads) took, one of the
    /// script's counted holds.
    read_hold: Option<ScriptHold>,
    /// One clone per live handle.
    handles: Rc<()>,
}

impl Drop for Selection {
    /// Torn down with the selection, which may be before its read script:
    /// the reads the hold kept stay held, for the run to report, instead of
    /// being answered here by whatever standing answer the test left.
    fn drop(&mut self) {
        if let Some(hold) = self.read_hold.take() {
            hold.leave_held();
        }
    }
}

impl Default for ScriptedPrimarySelection {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedPrimarySelection {
    /// An empty selection on a scenario of its own, whose reads answer at
    /// once.
    #[must_use]
    pub fn new() -> Self {
        Self::on(&Scenario::new())
    }

    /// An empty selection on `scenario`, whose reads answer at once.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        let selection = Rc::new(RefCell::new(Selection::default()));
        let reads = scenario.script("primary selection");
        let read = Rc::clone(&selection);
        reads
            .otherwise(move |_: &PrimarySelectionRequest| Ok(text_of(&read).map(Arc::<str>::from)));
        Self {
            selection,
            reads,
            service: scenario.service("primary selection"),
        }
    }

    /// The script that answers reads: its standing answer is the
    /// selection's text, and its steps answer reads in order before it.
    #[must_use]
    pub fn reads(&self) -> &Script<PrimarySelectionRequest, ClipboardRead> {
        &self.reads
    }

    /// Whether the primary selection is there: turned off (now, or at an
    /// instant with [`Track::set_at`]), the provider refuses every
    /// acquisition with the refusal it was turned off with; turned on again,
    /// it provides.
    #[must_use]
    pub fn service(&self) -> &Track<ServiceState> {
        &self.service
    }

    /// The selection's text: the current claim's, asked now, or another
    /// application's.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        text_of(&self.selection)
    }

    /// Whether a widget's claim holds the selection.
    #[must_use]
    pub fn is_claimed(&self) -> bool {
        self.selection.borrow().claims.current().is_some()
    }

    /// Select `text` in another application: every claim goes stale.
    pub fn select(&self, text: impl Into<String>) {
        // `Into` is the caller's code: it runs before the state is borrowed.
        let text: String = text.into();
        let lost = {
            let mut selection = self.selection.borrow_mut();
            selection.foreign = Some(text);
            selection.claims.lose()
        };
        drop(lost);
    }

    /// Keep every later read unanswered until
    /// [`answer_held_reads`](Self::answer_held_reads): one hold of the
    /// [`reads`](Self::reads) script, under the script's
    /// [held-request rules](Script#held-requests).
    pub fn hold_reads(&self) {
        let fresh = self.selection.borrow().read_hold.is_none();
        if fresh {
            let hold = self.reads.hold();
            self.selection.borrow_mut().read_hold = Some(hold);
        }
    }

    /// Release the hold [`hold_reads`](Self::hold_reads) took
    /// ([`ScriptHold::release`]), whose drain answers the kept reads by the
    /// script's steps and its standing answer, the selection's text asked as
    /// each is answered. Returns how many reads it answered.
    pub fn answer_held_reads(&self) -> usize {
        // Taken out first: releasing asks a claim for its text and answers
        // reads, the test's code, with nothing borrowed.
        let hold = self.selection.borrow_mut().read_hold.take();
        hold.map_or(0, ScriptHold::release)
    }

    /// Turn the primary selection off: the provider refuses every later
    /// acquisition with `refusal`.
    pub fn refuse(&self, refusal: Unsupported) {
        self.service.set(ServiceState::Off(refusal));
    }

    /// Make every later release of a claim panic once the claim is
    /// released, as a failing host backend does.
    pub fn fail_releases(&self) {
        self.selection.borrow_mut().fail_releases = true;
    }

    /// How many handles over this selection are alive.
    #[must_use]
    pub fn live_handles(&self) -> usize {
        Rc::strong_count(&self.selection.borrow().handles) - 1
    }

    /// Register this selection as the provider of
    /// [`PrimarySelectionCapability`].
    pub fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<PrimarySelectionCapability>(self.clone());
    }
}

/// The selection's text: the current claim's, asked with no borrow held
/// (the claim may read widget state), or another application's.
fn text_of(selection: &RefCell<Selection>) -> Option<String> {
    let (claim, foreign) = {
        let selection = selection.borrow();
        (selection.claims.current(), selection.foreign.clone())
    };
    match claim {
        Some(claim) => claim.text(),
        None => foreign,
    }
}

impl CapabilityProvider<PrimarySelectionCapability> for ScriptedPrimarySelection {
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<PrimarySelectionHandle, Unsupported> {
        self.service.check()?;
        let backend = Backend {
            selection: self.clone(),
            _handle: Rc::clone(&self.selection.borrow().handles),
        };
        Ok(PrimarySelectionHandle::new(backend, ctx))
    }
}

impl fmt::Debug for ScriptedPrimarySelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Copied out and the borrow released before writing: the formatter
        // is the caller's code.
        let (claimed, foreign) = {
            let selection = self.selection.borrow();
            (
                selection.claims.current().is_some(),
                selection.foreign.clone(),
            )
        };
        f.debug_struct("ScriptedPrimarySelection")
            .field("claimed", &claimed)
            .field("foreign", &foreign)
            .field("reads", &self.reads)
            .field("service", &self.service.get())
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
        let mut selection = self.selection.selection.borrow_mut();
        let claimed = selection.claims.claim(text);
        if selection.claims.is_current(claimed.0) {
            selection.foreign = None;
        }
        claimed
    }

    fn release(&self, claim: PrimaryClaimId) -> Option<Rc<dyn SelectionText>> {
        let (released, fails) = {
            let mut selection = self.selection.selection.borrow_mut();
            (selection.claims.release(claim), selection.fail_releases)
        };
        assert!(!fails, "the primary selection backend failed to release");
        released
    }

    fn is_current(&self, claim: PrimaryClaimId) -> bool {
        self.selection.selection.borrow().claims.is_current(claim)
    }

    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        self.selection
            .reads
            .receive(PrimarySelectionRequest::ReadText, answer);
    }
}
