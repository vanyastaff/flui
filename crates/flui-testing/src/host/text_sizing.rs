//! A scripted text-sizing provider for a headless host: what a native host's
//! measurement service is to a captured text-sizing source, answered at once
//! or held until the test answers.
//!
//! A runner hands each frontier a runtime asks for to the platform's text
//! measurement and settles it when the answer arrives; until then the
//! preparation that asked waits and the frame it belongs to draws nothing.
//! [`HeadlessHost::script_text_sizing`](super::HeadlessHost::script_text_sizing)
//! installs a captured source on one window and takes every frontier the
//! runtime asks after each pump: it answers each request with its authored
//! size, or keeps it unanswered while the test holds the provider, until
//! [`HeadlessHost::answer_text_sizing`](super::HeadlessHost::answer_text_sizing).

use std::cell::RefCell;
use std::rc::Rc;

use flui_painting::TextSizingAdmission;
use flui_runtime::ui_runtime::{TextSizingWork, UiRuntime};

/// A test's hold on a headless host's scripted text-sizing provider.
#[derive(Clone)]
pub struct ScriptedTextSizing {
    state: Rc<RefCell<ScriptedTextSizingState>>,
}

impl std::fmt::Debug for ScriptedTextSizing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("ScriptedTextSizing")
            .field("holding", &state.holding)
            .field("held", &state.held.len())
            .finish_non_exhaustive()
    }
}

impl ScriptedTextSizing {
    /// From now on keep every frontier the runtime asks for unanswered, as a
    /// measurement service that has not answered yet: the preparation waits.
    pub fn hold(&self) {
        self.state.borrow_mut().holding = true;
    }

    /// From now on answer every frontier as soon as the runtime asks, as the
    /// provider does from its install. What is held stays held until
    /// [`HeadlessHost::answer_text_sizing`](super::HeadlessHost::answer_text_sizing).
    pub fn answer_at_once(&self) {
        self.state.borrow_mut().holding = false;
    }

    /// How many frontiers the provider holds unanswered.
    #[must_use]
    pub fn held(&self) -> usize {
        self.state.borrow().held.len()
    }
}

/// The provider's state, shared between the host and the test's handle.
pub(super) struct ScriptedTextSizingState {
    writer: TextSizingAdmission,
    holding: bool,
    held: Vec<TextSizingWork>,
}

impl ScriptedTextSizingState {
    /// A provider that answers through `writer`, answering at once.
    pub(super) fn handle(writer: TextSizingAdmission) -> (Rc<RefCell<Self>>, ScriptedTextSizing) {
        let state = Rc::new(RefCell::new(Self {
            writer,
            holding: false,
            held: Vec::new(),
        }));
        let handle = ScriptedTextSizing {
            state: Rc::clone(&state),
        };
        (state, handle)
    }
}

/// Take what `runtime` asks of `provider` after a turn: answer it at once, or
/// hold it while the test holds the provider.
pub(super) fn serve(provider: &RefCell<ScriptedTextSizingState>, runtime: &UiRuntime) {
    let source = provider.borrow().writer.source();
    // Only the frontiers of the provider's own captured source: another
    // source's work is no answer of this provider's to give, and dropped it
    // stalls as a receipt nobody serves, as on a host with no producer for it.
    let work: Vec<TextSizingWork> = runtime
        .take_text_sizing_frontiers()
        .into_iter()
        .filter(|frontier| *frontier.source() == source)
        .collect();
    if work.is_empty() {
        return;
    }
    let holding = provider.borrow().holding;
    if holding {
        provider.borrow_mut().held.extend(work);
        return;
    }
    for frontier in work {
        settle(provider, runtime, frontier);
    }
}

/// Answer every frontier `provider` holds. How many the runtime accepted.
pub(super) fn answer_held(
    provider: &RefCell<ScriptedTextSizingState>,
    runtime: &UiRuntime,
) -> usize {
    let held = std::mem::take(&mut provider.borrow_mut().held);
    held.into_iter()
        .map(|frontier| settle(provider, runtime, frontier))
        .filter(|accepted| *accepted)
        .count()
}

/// Admit `frontier`'s requests at their authored sizes and settle it ready.
fn settle(
    provider: &RefCell<ScriptedTextSizingState>,
    runtime: &UiRuntime,
    frontier: TextSizingWork,
) -> bool {
    provider
        .borrow_mut()
        .writer
        .admit(
            frontier
                .requests()
                .iter()
                .map(|request| (*request, request.size)),
        )
        .expect("BUG: a scripted provider answers each request at its own size, never two sizes");
    runtime.settle_text_sizing(
        frontier,
        flui_runtime::ui_runtime::TextSizingSettlement::Ready,
    )
}
