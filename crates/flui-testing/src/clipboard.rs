//! A scripted clipboard: the [`ClipboardCapability`] a headless host
//! registers, which a test reads, fills and steers (ADR-0152 §4).
//!
//! By default a read answers at once, as a desktop clipboard does; the
//! completion still settles at the next owner turn.
//! [`hold_reads`](ScriptedClipboard::hold_reads) keeps reads unanswered until
//! [`answer_held_reads`](ScriptedClipboard::answer_held_reads), as the web's
//! asynchronous clipboard does, and
//! [`refuse`](ScriptedClipboard::refuse) makes the provider refuse, as a host
//! without a clipboard does.

use std::{cell::RefCell, fmt, rc::Rc};

use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::{
    Unsupported,
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
    text: Option<String>,
    hold: bool,
    held: Vec<Resolver<Option<String>>>,
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
        self.state.borrow().text.clone()
    }

    /// Put `text` on the clipboard, as another application copying would.
    pub fn set_text(&self, text: impl Into<String>) {
        self.state.borrow_mut().text = Some(text.into());
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
            (std::mem::take(&mut state.held), state.text.clone())
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
        Ok(ClipboardHandle::new(backend, cx.completions().clone()))
    }
}

impl fmt::Debug for ScriptedClipboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("ScriptedClipboard")
            .field("text", &state.text)
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
    fn read_text(&self, answer: Resolver<Option<String>>) {
        let mut state = self.state.borrow_mut();
        if state.hold {
            state.held.push(answer);
            return;
        }
        let text = state.text.clone();
        drop(state);
        // A retired owner has nobody left to tell.
        let _ = answer.resolve(text);
    }

    fn write_text(&self, text: String) {
        self.state.borrow_mut().text = Some(text);
    }
}
