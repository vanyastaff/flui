//! A scripted clipboard: the [`ClipboardCapability`] a headless host
//! registers, which a test reads, fills and steers (ADR-0152 §4).
//!
//! By default a read answers at once, as a desktop clipboard does; the
//! completion still settles at the next owner turn.
//! [`hold_reads`](ScriptedClipboard::hold_reads) keeps reads unanswered until
//! [`answer_held_reads`](ScriptedClipboard::answer_held_reads), as the web's
//! asynchronous clipboard does,
//! [`refuse`](ScriptedClipboard::refuse) makes the provider refuse, as a host
//! without a clipboard does, and
//! [`fail_reads_with`](ScriptedClipboard::fail_reads_with) makes reads fail as
//! a host that cannot reach its clipboard does.
//! [`answer_concealment`](ScriptedClipboard::answer_concealment) sets what
//! the host does with a concealed write, and
//! [`is_concealed`](ScriptedClipboard::is_concealed) tells whether the text
//! came from one.

use std::{cell::RefCell, fmt, rc::Rc, sync::Arc};

use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, Concealment, Unsupported,
    capability::{CapabilityProvider, CapabilityRegistrar, ProviderContext, Resolver},
};

/// The clipboard a headless test host hands its widgets. Owner-thread, cheap
/// to clone; every clone is the same clipboard.
#[derive(Clone, Default)]
pub struct ScriptedClipboard {
    state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct State {
    text: Option<Arc<str>>,
    /// Whether `text` came from a concealed write.
    concealed: bool,
    /// What the host does with a concealed write; `None` is
    /// [`Concealment::Honored`].
    concealment: Option<Concealment>,
    failure: Option<ClipboardError>,
    reads: usize,
    hold: bool,
    held: Vec<Resolver<ClipboardRead>>,
    refusal: Option<Unsupported>,
    handles: Rc<()>,
}

impl ScriptedClipboard {
    /// An empty clipboard whose reads answer at once.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The clipboard's text: what the application last wrote, or what the
    /// test put there.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        self.state.borrow().text.as_deref().map(str::to_owned)
    }

    /// Put `text` on the clipboard, as another application copying would.
    pub fn set_text(&self, text: impl Into<String>) {
        // `Into` is the caller's code: it runs before the state is borrowed.
        let text = Arc::from(text.into());
        let mut state = self.state.borrow_mut();
        state.text = Some(text);
        state.concealed = false;
    }

    /// Whether the clipboard's text came from a concealed write
    /// ([`ClipboardWrite::concealed`]): `false` after an ordinary write or
    /// [`set_text`](Self::set_text).
    #[must_use]
    pub fn is_concealed(&self) -> bool {
        self.state.borrow().concealed
    }

    /// Answer `concealment` to every handle that asks what this host does
    /// with a concealed write; [`Concealment::Honored`] until set.
    pub fn answer_concealment(&self, concealment: Concealment) {
        self.state.borrow_mut().concealment = Some(concealment);
    }

    /// Keep every later read unanswered until
    /// [`answer_held_reads`](Self::answer_held_reads).
    pub fn hold_reads(&self) {
        self.state.borrow_mut().hold = true;
    }

    /// Answer the reads held so far with the clipboard's text now, and answer
    /// later reads at once again. Returns how many reads it answered.
    pub fn answer_held_reads(&self) -> usize {
        let (held, text) = {
            let mut state = self.state.borrow_mut();
            state.hold = false;
            (std::mem::take(&mut state.held), state.answer())
        };
        let answered = held.len();
        for answer in held {
            // A retired owner has nobody left to tell.
            let _ = answer.resolve(text.clone());
        }
        answered
    }

    /// Drop the reads held so far unanswered, as a backend torn down
    /// mid-read does: their completions end as abandoned.
    pub fn drop_held_reads(&self) -> usize {
        let held = std::mem::take(&mut self.state.borrow_mut().held);
        let dropped = held.len();
        drop(held);
        dropped
    }

    /// Make every later answer fail with `error`, as a host whose clipboard
    /// is out of reach answers, until [`stop_failing_reads`](Self::stop_failing_reads).
    pub fn fail_reads_with(&self, error: ClipboardError) {
        self.state.borrow_mut().failure = Some(error);
    }

    /// Answer later reads with the clipboard's text again.
    pub fn stop_failing_reads(&self) {
        self.state.borrow_mut().failure = None;
    }

    /// How many reads reached this clipboard. A handle shares one read among
    /// the readers that ask while it is in flight, so a burst of pastes
    /// counts once.
    #[must_use]
    pub fn reads_asked(&self) -> usize {
        self.state.borrow().reads
    }

    /// Make the provider refuse every later acquisition with `refusal`.
    pub fn refuse(&self, refusal: Unsupported) {
        self.state.borrow_mut().refusal = Some(refusal);
    }

    /// How many handles over this clipboard are alive: a widget that drops
    /// its last handle lets go of the clipboard.
    #[must_use]
    pub fn live_handles(&self) -> usize {
        Rc::strong_count(&self.state.borrow().handles) - 1
    }

    /// Register this clipboard as the provider of [`ClipboardCapability`].
    pub fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<ClipboardCapability>(self.clone());
    }
}

impl CapabilityProvider<ClipboardCapability> for ScriptedClipboard {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        let state = self.state.borrow();
        if let Some(refusal) = state.refusal {
            return Err(refusal);
        }
        let backend = Backend {
            state: Rc::clone(&self.state),
            _handle: Rc::clone(&state.handles),
        };
        Ok(ClipboardHandle::new(backend, cx))
    }
}

impl fmt::Debug for ScriptedClipboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("ScriptedClipboard")
            .field("text", &state.text)
            .field("concealed", &state.concealed)
            .field("failure", &state.failure)
            .field("reads", &state.reads)
            .field("held_reads", &state.held.len())
            .field("refusal", &state.refusal)
            .finish()
    }
}

/// One handle's backend; it counts as a live handle while it exists.
struct Backend {
    state: Rc<RefCell<State>>,
    _handle: Rc<()>,
}

impl ClipboardBackend for Backend {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        let mut state = self.state.borrow_mut();
        state.reads += 1;
        if state.hold {
            state.held.push(answer);
            return;
        }
        let text = state.answer();
        drop(state);
        // A retired owner has nobody left to tell.
        let _ = answer.resolve(text);
    }

    fn write(&self, write: ClipboardWrite) {
        let mut state = self.state.borrow_mut();
        state.concealed = write.is_concealed();
        state.text = Some(Arc::from(write.into_text()));
    }

    fn concealment(&self) -> Concealment {
        self.state
            .borrow()
            .concealment
            .unwrap_or(Concealment::Honored)
    }
}

impl State {
    /// What a read answers now: the scripted failure, or the text.
    fn answer(&self) -> ClipboardRead {
        match self.failure {
            Some(error) => Err(error),
            None => Ok(self.text.clone()),
        }
    }
}
