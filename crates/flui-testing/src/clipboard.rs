//! A scripted clipboard: the [`ClipboardCapability`] a headless host
//! registers, which a test reads, fills and steers (ADR-0152 §4), on the
//! scenario primitive ([`flui_platform::capability::scripted`]).
//!
//! The clipboard's text lives here; its reads go through a [`Script`] of
//! [`ClipboardRequest`]s whose standing answer is that text, as a desktop
//! clipboard answers, so the completion settles at the next owner turn.
//! [`reads`](ScriptedClipboard::reads) is the script, for ordered answers
//! and delays. [`hold_reads`](ScriptedClipboard::hold_reads) keeps reads
//! unanswered until
//! [`answer_held_reads`](ScriptedClipboard::answer_held_reads), as the web's
//! asynchronous clipboard does, [`refuse`](ScriptedClipboard::refuse) turns
//! the service off so the provider refuses, as a host without a clipboard
//! does, and [`fail_reads_with`](ScriptedClipboard::fail_reads_with) makes
//! reads fail as a host that cannot reach its clipboard does.
//! [`answer_concealment`](ScriptedClipboard::answer_concealment) sets what
//! the host does with a concealed write, and
//! [`is_concealed`](ScriptedClipboard::is_concealed) tells whether the text
//! came from one.

use std::{cell::RefCell, fmt, rc::Rc, sync::Arc};

use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, Concealment, Unsupported,
    capability::scripted::{Scenario, Script, ServiceState, Track},
    capability::{CapabilityProvider, CapabilityRegistrar, ProviderContext, Resolver},
};

/// A request a clipboard's handle makes of its host and is answered later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardRequest {
    /// Read the clipboard's text.
    ReadText,
}

/// The clipboard a headless test host hands its widgets. Owner-thread, cheap
/// to clone; every clone is the same clipboard.
#[derive(Clone)]
pub struct ScriptedClipboard {
    contents: Rc<RefCell<Contents>>,
    reads: Script<ClipboardRequest, ClipboardRead>,
    service: Track<ServiceState>,
}

#[derive(Default)]
struct Contents {
    text: Option<Arc<str>>,
    /// Whether `text` came from a concealed write.
    concealed: bool,
    /// What the host does with a concealed write; `None` is
    /// [`Concealment::Honored`].
    concealment: Option<Concealment>,
    failure: Option<ClipboardError>,
    /// One clone per live handle.
    handles: Rc<()>,
}

impl Contents {
    /// What a read answers now: the scripted failure, or the text.
    fn answer(&self) -> ClipboardRead {
        match self.failure {
            Some(error) => Err(error),
            None => Ok(self.text.clone()),
        }
    }
}

impl Default for ScriptedClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedClipboard {
    /// An empty clipboard on a scenario of its own, whose reads answer at
    /// once.
    #[must_use]
    pub fn new() -> Self {
        Self::on(&Scenario::new())
    }

    /// An empty clipboard on `scenario`, whose reads answer at once.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        let contents = Rc::new(RefCell::new(Contents::default()));
        let reads = scenario.script("clipboard");
        let answered = Rc::clone(&contents);
        reads.otherwise(move |_: &ClipboardRequest| answered.borrow().answer());
        Self {
            contents,
            reads,
            service: scenario.service("clipboard"),
        }
    }

    /// The script that answers reads: its standing answer is the
    /// clipboard's text, and its steps answer reads in order before it.
    #[must_use]
    pub fn reads(&self) -> &Script<ClipboardRequest, ClipboardRead> {
        &self.reads
    }

    /// Whether the clipboard is there: turned off (now, or at an instant
    /// with [`Track::set_at`]), the provider refuses every acquisition with
    /// the refusal it was turned off with; turned on again, it provides.
    #[must_use]
    pub fn service(&self) -> &Track<ServiceState> {
        &self.service
    }

    /// The clipboard's text: what the application last wrote, or what the
    /// test put there.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        self.contents.borrow().text.as_deref().map(str::to_owned)
    }

    /// Put `text` on the clipboard, as another application copying would.
    pub fn set_text(&self, text: impl Into<String>) {
        // `Into` is the caller's code: it runs before the state is borrowed.
        let text: String = text.into();
        let text: Arc<str> = Arc::from(text);
        let mut contents = self.contents.borrow_mut();
        contents.text = Some(text);
        contents.concealed = false;
    }

    /// Whether the clipboard's text came from a concealed write
    /// ([`ClipboardWrite::concealed`]): `false` after an ordinary write or
    /// [`set_text`](Self::set_text).
    #[must_use]
    pub fn is_concealed(&self) -> bool {
        self.contents.borrow().concealed
    }

    /// Answer `concealment` to every handle that asks what this host does
    /// with a concealed write; [`Concealment::Honored`] until set.
    pub fn answer_concealment(&self, concealment: Concealment) {
        self.contents.borrow_mut().concealment = Some(concealment);
    }

    /// Keep every later read unanswered until
    /// [`answer_held_reads`](Self::answer_held_reads).
    pub fn hold_reads(&self) {
        self.reads.hold();
    }

    /// Answer the reads held so far with the clipboard's text now, and answer
    /// later reads at once again. Returns how many reads it answered.
    pub fn answer_held_reads(&self) -> usize {
        self.reads.resume();
        let answer = self.contents.borrow().answer();
        self.reads.release_held(|_| answer.clone())
    }

    /// Drop the reads held so far unanswered, as a backend torn down
    /// mid-read does: their completions end as abandoned.
    pub fn drop_held_reads(&self) -> usize {
        self.reads.abandon_held()
    }

    /// Make every later answer fail with `error`, as a host whose clipboard
    /// is out of reach answers, until [`stop_failing_reads`](Self::stop_failing_reads).
    pub fn fail_reads_with(&self, error: ClipboardError) {
        self.contents.borrow_mut().failure = Some(error);
    }

    /// Answer later reads with the clipboard's text again.
    pub fn stop_failing_reads(&self) {
        self.contents.borrow_mut().failure = None;
    }

    /// How many reads reached this clipboard. A handle shares one read among
    /// the readers that ask while it is in flight, so a burst of pastes
    /// counts once.
    #[must_use]
    pub fn reads_asked(&self) -> usize {
        self.reads.received()
    }

    /// Turn the clipboard off: the provider refuses every later acquisition
    /// with `refusal`.
    pub fn refuse(&self, refusal: Unsupported) {
        self.service.set(ServiceState::Off(refusal));
    }

    /// How many handles over this clipboard are alive: a widget that drops
    /// its last handle lets go of the clipboard.
    #[must_use]
    pub fn live_handles(&self) -> usize {
        Rc::strong_count(&self.contents.borrow().handles) - 1
    }

    /// Register this clipboard as the provider of [`ClipboardCapability`].
    pub fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<ClipboardCapability>(self.clone());
    }
}

impl CapabilityProvider<ClipboardCapability> for ScriptedClipboard {
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        self.service.check()?;
        let backend = Backend {
            clipboard: self.clone(),
            _handle: Rc::clone(&self.contents.borrow().handles),
        };
        Ok(ClipboardHandle::new(backend, ctx))
    }
}

impl fmt::Debug for ScriptedClipboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Copied out and the borrow released before writing: the formatter
        // is the caller's code.
        let (text, concealed, failure) = {
            let contents = self.contents.borrow();
            (contents.text.clone(), contents.concealed, contents.failure)
        };
        f.debug_struct("ScriptedClipboard")
            .field("text", &text)
            .field("concealed", &concealed)
            .field("failure", &failure)
            .field("reads", &self.reads)
            .field("service", &self.service.get())
            .finish()
    }
}

/// One handle's backend; it counts as a live handle while it exists.
struct Backend {
    clipboard: ScriptedClipboard,
    _handle: Rc<()>,
}

impl ClipboardBackend for Backend {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        self.clipboard
            .reads
            .receive(ClipboardRequest::ReadText, answer);
    }

    fn write(&self, write: ClipboardWrite) {
        let concealed = write.is_concealed();
        let text = Arc::<str>::from(write.into_text());
        let mut contents = self.clipboard.contents.borrow_mut();
        contents.concealed = concealed;
        contents.text = Some(text);
    }

    fn concealment(&self) -> Concealment {
        self.clipboard
            .contents
            .borrow()
            .concealment
            .unwrap_or(Concealment::Honored)
    }
}
