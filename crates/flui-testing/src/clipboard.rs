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
//!
//! Writes go through a second script on the same scenario,
//! [`writes`](ScriptedClipboard::writes), of [`WriteRequest`]s (ADR-0337):
//! a step, a delayed step, the standing answer or a release decides whether
//! a write is taken and when, and whenever a write is answered `Ok` or
//! [`ApplicationOnly`](ClipboardError::ApplicationOnly), the clipboard takes
//! its text, in one place. [`fail_writes_with`](ScriptedClipboard::fail_writes_with)
//! makes the standing answer refuse, and
//! [`hold_writes`](ScriptedClipboard::hold_writes) holds writes while any
//! hold lives. Reads and writes keep one order, as a host's clipboard serves
//! them (ADR-0337 §3): a read asked after a write waits until that write is
//! answered, writes land in the order they were asked, however their
//! answers were scheduled, and a write's completion settles only once its
//! text has landed. An observer a test adds to
//! [`writes`](ScriptedClipboard::writes) hears beside the clipboard's own,
//! which it cannot replace.
//! [`answer_concealment`](ScriptedClipboard::answer_concealment) sets what
//! the host does with a concealed write, and
//! [`is_concealed`](ScriptedClipboard::is_concealed) tells whether the text
//! came from one.

use std::collections::{BTreeMap, VecDeque};
use std::{cell::RefCell, fmt, rc::Rc, sync::Arc};

use flui_foundation::panic::FirstPanic;
use flui_interaction::text_input::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment, ExpiryClearing,
    Unsupported,
    capability::scripted::{Scenario, Script, ServiceState, Track},
    capability::{
        CapabilityProvider, CapabilityRegistrar, CompletionSource, ProviderContext, Resolver,
    },
};

/// A request a clipboard's handle makes of its host and is answered later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardRequest {
    /// Read the clipboard's text.
    ReadText,
}

/// A write a clipboard's handle asked of its host: its number in the order
/// the writes were asked, from 1, and the write.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WriteRequest {
    /// The write's place among the writes asked of this clipboard, from 1.
    pub number: u64,
    /// What it asks.
    pub write: ClipboardWrite,
}

/// The clipboard a headless test host hands its widgets. Owner-thread, cheap
/// to clone; every clone is the same clipboard.
#[derive(Clone)]
pub struct ScriptedClipboard {
    contents: Rc<RefCell<Contents>>,
    order: Rc<RefCell<Order>>,
    reads: Script<ClipboardRequest, ClipboardRead>,
    writes: Script<WriteRequest, ClipboardWritten>,
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
    /// What the host does with an expiring write; `None` is
    /// [`ExpiryClearing::AtTimeAndLock`].
    expiry_clearing: Option<ExpiryClearing>,
    failure: Option<ClipboardError>,
    /// What the standing answer refuses a write with; `None` takes it.
    write_failure: Option<ClipboardError>,
    /// How many [`ClipboardWriteHold`]s are alive: writes are held while
    /// any is.
    write_holds: usize,
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

    /// What the standing answer gives a write: the scripted failure, or
    /// `Ok`.
    fn decide(&self) -> ClipboardWritten {
        self.write_failure.map_or(Ok(()), Err)
    }

    /// Take `write`'s text onto the clipboard.
    fn take(&mut self, write: ClipboardWrite) {
        self.concealed = write.is_concealed();
        self.text = Some(Arc::<str>::from(write.into_text()));
    }
}

/// The one order reads and writes keep.
#[derive(Default)]
struct Order {
    /// How many writes were asked.
    asked: u64,
    /// How many writes, from the first, were answered and landed.
    landed: u64,
    /// Writes answered ahead of an earlier one, by number: the write and
    /// its answer, or `None` for one that ended unanswered.
    early: BTreeMap<u64, Option<(ClipboardWrite, ClipboardWritten)>>,
    /// The completions of the writes not landed yet, by number: each is
    /// answered once its write lands, after its text, whatever order the
    /// script answered the writes in.
    owed: BTreeMap<u64, Resolver<ClipboardWritten>>,
    /// Reads asked behind writes not landed yet: the writes they wait for
    /// (by count) and their answers.
    reads: VecDeque<(u64, Resolver<ClipboardRead>)>,
}

impl Default for ScriptedClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedClipboard {
    /// An empty clipboard on a scenario of its own, whose reads and writes
    /// answer at once.
    #[must_use]
    pub fn new() -> Self {
        Self::on(&Scenario::new())
    }

    /// An empty clipboard on `scenario`, whose reads and writes answer at
    /// once.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        let contents = Rc::new(RefCell::new(Contents::default()));
        let reads = scenario.script("clipboard");
        let answered = Rc::clone(&contents);
        reads.otherwise(move |_: &ClipboardRequest| answered.borrow().answer());
        let writes = scenario.script("clipboard writes");
        let decided = Rc::clone(&contents);
        writes.otherwise(move |_: &WriteRequest| decided.borrow().decide());
        let clipboard = Self {
            contents,
            order: Rc::default(),
            reads,
            writes,
            service: scenario.service("clipboard"),
        };
        let heard = clipboard.landing();
        clipboard
            .writes
            .on_answer(move |request, written| heard.answered(request, written));
        clipboard
    }

    /// What the write script's observer keeps: the clipboard's state and
    /// order, and its read script, which a landed write lets waiting reads
    /// through to.
    fn landing(&self) -> Landing {
        Landing {
            contents: Rc::downgrade(&self.contents),
            order: Rc::downgrade(&self.order),
            reads: self.reads.clone(),
        }
    }

    /// The script that answers reads: its standing answer is the
    /// clipboard's text, and its steps answer reads in order before it.
    #[must_use]
    pub fn reads(&self) -> &Script<ClipboardRequest, ClipboardRead> {
        &self.reads
    }

    /// The script that answers writes (ADR-0337): its standing answer takes
    /// the write, or refuses it with what
    /// [`fail_writes_with`](Self::fail_writes_with) set; its steps answer
    /// writes in order before it. Whatever answers a write `Ok` or
    /// [`ApplicationOnly`](ClipboardError::ApplicationOnly) lands its text
    /// on the clipboard; any other answer leaves the clipboard as it was.
    ///
    /// The script resolves a stand-in: the clipboard's own observer, which
    /// runs before any a test adds, lands the write and then answers the
    /// caller's resolver, in the order the writes were asked. A test's
    /// observer hears an answer after the clipboard did (its text has landed
    /// unless an earlier write is still unanswered), and the
    /// caller's completion settles at a later owner turn, after every
    /// observer heard it.
    #[must_use]
    pub fn writes(&self) -> &Script<WriteRequest, ClipboardWritten> {
        &self.writes
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

    /// Answer `clearing` to every handle that asks what this host does with
    /// an expiring write; [`ExpiryClearing::AtTimeAndLock`] until set. The
    /// scripted clipboard clears nothing itself: a test reads the expiry a
    /// write asked for from its [`WriteRequest`].
    pub fn answer_expiry_clearing(&self, clearing: ExpiryClearing) {
        self.contents.borrow_mut().expiry_clearing = Some(clearing);
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

    /// Answer every later write `error` from the standing answer, until
    /// [`stop_failing_writes`](Self::stop_failing_writes): a refusal leaves
    /// the clipboard as it was, as a host answers when another application
    /// holds the clipboard past its patience, while
    /// [`ApplicationOnly`](ClipboardError::ApplicationOnly) keeps the text
    /// for this clipboard's own reads.
    pub fn fail_writes_with(&self, error: ClipboardError) {
        self.contents.borrow_mut().write_failure = Some(error);
    }

    /// Take later writes again.
    pub fn stop_failing_writes(&self) {
        self.contents.borrow_mut().write_failure = None;
    }

    /// Hold every later write, unanswered and off the clipboard, while the
    /// returned hold lives, as a host whose clipboard another application
    /// holds keeps it waiting; reads asked after a held write wait for it.
    /// Writes stay held while any hold lives; dropping the last answers the
    /// held writes, in order, with the standing answer.
    #[must_use = "writes are held only while the hold is alive"]
    pub fn hold_writes(&self) -> ClipboardWriteHold {
        let first = {
            let mut contents = self.contents.borrow_mut();
            contents.write_holds = contents.write_holds.saturating_add(1);
            contents.write_holds == 1
        };
        if first {
            self.writes.hold();
        }
        ClipboardWriteHold {
            writes: self.writes.clone(),
            contents: Rc::clone(&self.contents),
        }
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

    /// A read asked now: through the read script at once, or after the
    /// writes asked before it have landed.
    fn read(&self, answer: Resolver<ClipboardRead>) {
        let waiting = {
            let mut order = self.order.borrow_mut();
            let behind = order.landed < order.asked;
            if behind {
                let asked = order.asked;
                order.reads.push_back((asked, answer));
                None
            } else {
                Some(answer)
            }
        };
        if let Some(answer) = waiting {
            self.reads.receive(ClipboardRequest::ReadText, answer);
        }
    }

    /// A write asked now, numbered in the order writes were asked. The
    /// script answers a stand-in of `answer`, made from `completions`;
    /// `answer` itself is answered once the write lands.
    fn write(
        &self,
        write: ClipboardWrite,
        answer: Resolver<ClipboardWritten>,
        completions: &CompletionSource,
    ) {
        let (unwatched, stand_in) = completions.pending::<ClipboardWritten>();
        drop(unwatched);
        let number = {
            let mut order = self.order.borrow_mut();
            order.asked = order.asked.saturating_add(1);
            let number = order.asked;
            order.owed.insert(number, answer);
            number
        };
        self.writes
            .receive(WriteRequest { number, write }, stand_in);
    }
}

/// What the write script's observer holds: the clipboard's state, weakly,
/// so the script it sits in does not keep the clipboard alive.
struct Landing {
    contents: std::rc::Weak<RefCell<Contents>>,
    order: std::rc::Weak<RefCell<Order>>,
    reads: Script<ClipboardRequest, ClipboardRead>,
}

impl Landing {
    /// The write `request` was answered `written` (`None`: it ended
    /// unanswered): land it and every write after it already answered, in
    /// order, then answer their completions in that order, then let through
    /// the reads that waited for them. A completion never settles before
    /// its write's text is on the clipboard.
    fn answered(&self, request: &WriteRequest, written: Option<&ClipboardWritten>) {
        let (Some(contents), Some(order)) = (self.contents.upgrade(), self.order.upgrade()) else {
            return;
        };
        let (settled, released) = {
            let mut order = order.borrow_mut();
            let answered = written.map(|written| (request.write.clone(), *written));
            order.early.insert(request.number, answered);
            let mut settled = Vec::new();
            loop {
                let next = order.landed.saturating_add(1);
                let Some(answered) = order.early.remove(&next) else {
                    break;
                };
                let written = answered.map(|(write, written)| {
                    if let Ok(()) | Err(ClipboardError::ApplicationOnly) = written {
                        contents.borrow_mut().take(write);
                    }
                    written
                });
                if let Some(answer) = order.owed.remove(&next) {
                    settled.push((answer, written));
                }
                order.landed = next;
            }
            let landed = order.landed;
            let mut released = Vec::new();
            while order
                .reads
                .front()
                .is_some_and(|(after, _)| *after <= landed)
            {
                if let Some((_, answer)) = order.reads.pop_front() {
                    released.push(answer);
                }
            }
            (settled, released)
        };
        // Answered with nothing borrowed: a resolver's owner queue is the
        // test's code. One that panics leaves the others answered, and the
        // first failure goes on.
        let mut failure = FirstPanic::new();
        for (answer, written) in settled {
            let _ = match written {
                // A retired owner has nobody left to answer.
                Some(written) => failure.contain(|| {
                    let _ = answer.resolve(written);
                }),
                // Ended unanswered: its completion ends abandoned.
                None => failure.contain(|| drop(answer)),
            };
        }
        for answer in released {
            let _ = failure.contain(|| self.reads.receive(ClipboardRequest::ReadText, answer));
        }
        failure.resume();
    }
}

/// Writes held on a [`ScriptedClipboard`] while this, or another hold of
/// the same clipboard, lives; when the last drops, the held writes are
/// answered, in order, with the standing answer.
#[must_use = "writes are held only while the hold is alive"]
pub struct ClipboardWriteHold {
    writes: Script<WriteRequest, ClipboardWritten>,
    contents: Rc<RefCell<Contents>>,
}

impl ClipboardWriteHold {
    /// How many writes are held now.
    #[must_use]
    pub fn held(&self) -> usize {
        self.writes.held()
    }
}

impl Drop for ClipboardWriteHold {
    fn drop(&mut self) {
        let last = {
            let mut contents = self.contents.borrow_mut();
            contents.write_holds = contents.write_holds.saturating_sub(1);
            contents.write_holds == 0
        };
        if !last {
            return;
        }
        self.writes.resume();
        let contents = Rc::clone(&self.contents);
        let _ = self.writes.release_held(|_| contents.borrow().decide());
    }
}

impl fmt::Debug for ClipboardWriteHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardWriteHold")
            .field("held", &self.writes.held())
            .finish_non_exhaustive()
    }
}

impl CapabilityProvider<ClipboardCapability> for ScriptedClipboard {
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        self.service.check()?;
        let backend = Backend {
            clipboard: self.clone(),
            completions: ctx.completions().clone(),
            _handle: Rc::clone(&self.contents.borrow().handles),
        };
        Ok(ClipboardHandle::new(backend, ctx))
    }
}

impl fmt::Debug for ScriptedClipboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Copied out and the borrow released before writing: the formatter
        // is the caller's code.
        let (text, concealed, failure, write_failure) = {
            let contents = self.contents.borrow();
            (
                contents.text.clone(),
                contents.concealed,
                contents.failure,
                contents.write_failure,
            )
        };
        f.debug_struct("ScriptedClipboard")
            .field("text", &text)
            .field("concealed", &concealed)
            .field("failure", &failure)
            .field("write_failure", &write_failure)
            .field("reads", &self.reads)
            .field("writes", &self.writes)
            .field("service", &self.service.get())
            .finish_non_exhaustive()
    }
}

/// One handle's backend; it counts as a live handle while it exists.
struct Backend {
    clipboard: ScriptedClipboard,
    /// The handle's owner's completions, which a write's stand-in comes
    /// from.
    completions: CompletionSource,
    _handle: Rc<()>,
}

impl ClipboardBackend for Backend {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        self.clipboard.read(answer);
    }

    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        self.clipboard.write(write, answer, &self.completions);
    }

    fn concealment(&self) -> Concealment {
        self.clipboard
            .contents
            .borrow()
            .concealment
            .unwrap_or(Concealment::Honored)
    }

    fn expiry_clearing(&self) -> ExpiryClearing {
        self.clipboard
            .contents
            .borrow()
            .expiry_clearing
            .unwrap_or(ExpiryClearing::AtTimeAndLock)
    }
}
