//! The documents an intent names, opened off the owner thread (ADR-0330 §5).
//!
//! A `content:` document is served by another application's content
//! provider, and opening it (`ContentResolver.openFileDescriptor`, then the
//! name query) can block for as long as that application takes: a cloud
//! provider answers once it has downloaded the file. The owner thread drains
//! NativeActivity's input queue and runs frames, so it never opens one.
//!
//! [`Openings`] is the owner's side, plain state: each intent is accepted
//! with a [deadline](OPEN_DEADLINE), each of its documents becomes a job
//! under a ticket of its own, and the intents are delivered in the order they
//! arrived, each once every one of its documents has an outcome. An outcome
//! finds its document by its ticket alone, so one that arrives for a document
//! already refused belongs to no other and is dropped, which closes its
//! descriptor. [`Opener`] is the queue the owner shares with the workers that
//! open the jobs through the [`DocumentProvider`] (the Java calls on Android,
//! a stand-in in the tests), one job at a time each, handing each outcome
//! back under its ticket and waking the owner.
//!
//! Every accepted document ends with exactly one outcome, an [`OpenedFile`]
//! or an [`OpenRefusal`] that says why: the provider failed, the read grant
//! ended, the deadline passed (the open is cancelled through its
//! `CancellationSignal`), the platform retired, or no thread could start.
//!
//! One worker runs while it keeps up. A provider may ignore the cancellation
//! (`ContentProvider.openFile` does unless the provider overrides it), so a
//! worker whose open was abandoned may stay inside it: once every worker is
//! inside such an open, a job waiting starts another, up to
//! [`MAX_WORKERS`]. Past them the jobs wait for a worker to come back and are
//! refused at their own deadlines, so their intents are still delivered and
//! the owner thread never waits.

use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_foundation::IdMint;
use flui_foundation::panic::FirstPanic;
use flui_platform::{Launch, MAX_OPENED_ITEMS, OpenTarget, Opened, OpenedFile};
use parking_lot::Mutex;

use super::intent::{Document, Opening};

/// How long the documents of one intent may take from when the intent is
/// accepted, their wait for a worker included. Long enough for a provider to
/// download a document of some tens of megabytes; past it each document
/// still unopened is refused [`OpenRefusal::TimedOut`] and its intent is
/// delivered with what did open.
pub(crate) const OPEN_DEADLINE: Duration = Duration::from_secs(30);

/// The most intents waiting for their documents at once: the most `FluiHost`
/// queues ([`MAX_OPENED_ITEMS`]). An intent past it is refused whole.
pub(crate) const MAX_WAITING_INTENTS: usize = MAX_OPENED_ITEMS;

/// The most workers opening documents at once, those inside an abandoned
/// open included: the most reads of another application's content the host
/// runs at once ([`MAX_PROVIDER_READS`](crate::host::MAX_PROVIDER_READS)).
pub(crate) const MAX_WORKERS: usize = crate::host::MAX_PROVIDER_READS;

/// Why a document an intent named was not opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenRefusal {
    /// The provider refused or failed (no such document, no descriptor, an
    /// I/O failure), or the open failed in this process.
    Failed,
    /// The read grant the intent carried had ended (`SecurityException`): it
    /// lasts only while the Activity that received the intent does.
    GrantEnded,
    /// The open passed its intent's [`OPEN_DEADLINE`] and was cancelled.
    TimedOut,
    /// The platform ended first (the Activity was destroyed, or the
    /// application quit).
    Retired,
    /// No thread could be started to open it.
    NoThread,
    /// The owner refused the document while its worker was about to open
    /// it, so it was not opened. The owner holds the document's own outcome
    /// already and drops this one.
    Withdrawn,
}

/// What opening one document answered.
pub(crate) type Outcome = Result<OpenedFile, OpenRefusal>;

/// A document to open, under its ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Job {
    /// The job's own identity, which its outcome names.
    pub(crate) ticket: NonZeroU64,
    /// The `content:` URI.
    pub(crate) uri: String,
}

/// An intent waiting for its documents.
#[derive(Debug)]
struct Waiting {
    links: Vec<String>,
    /// Each document's outcome, in the intent's order, once it has one.
    outcomes: Vec<Option<Outcome>>,
    /// The tickets of the jobs still out, and the document each opens.
    pending: Vec<(NonZeroU64, usize)>,
    /// The documents past the room an `Opened` leaves, never opened.
    unread: usize,
    launch: Launch,
    deadline: Instant,
}

impl Waiting {
    fn is_complete(&self) -> bool {
        self.pending.is_empty()
    }

    /// Refuse every document still out with `refusal`, its job's ticket
    /// added to `jobs`.
    fn refuse_pending(&mut self, refusal: OpenRefusal, jobs: &mut Vec<NonZeroU64>) {
        for (ticket, document) in self.pending.drain(..) {
            jobs.push(ticket);
            self.outcomes[document] = Some(Err(refusal));
        }
    }
}

/// An intent whose documents all have an outcome: what the application
/// hears, and the refusals the owner logs.
#[derive(Debug)]
pub(crate) struct Ready {
    /// What the application hears, bounded.
    pub(crate) opened: Opened,
    /// Why each document left out was not opened, in the intent's order.
    pub(crate) refused: Vec<OpenRefusal>,
}

/// The intents waiting for their documents, in the order they arrived.
/// Owner thread alone.
#[derive(Debug, Default)]
pub(crate) struct Openings {
    tickets: IdMint<NonZeroU64>,
    waiting: VecDeque<Waiting>,
}

/// The owner turned an intent away: [`MAX_WAITING_INTENTS`] wait already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Busy;

impl Openings {
    /// Accept `opening` at `now`: its paths are files at once, each
    /// `content:` document within the room an `Opened` leaves after its links
    /// becomes a job, returned to start, and the intent waits behind the
    /// earlier ones until each has an outcome.
    ///
    /// # Errors
    ///
    /// [`Busy`] when [`MAX_WAITING_INTENTS`] wait already; the intent is not
    /// accepted.
    pub(crate) fn accept(&mut self, opening: Opening, now: Instant) -> Result<Vec<Job>, Busy> {
        if self.waiting.len() >= MAX_WAITING_INTENTS {
            return Err(Busy);
        }
        let room = crate::owner::opened::room_for_files(&opening.links);
        let unread = opening.documents.len().saturating_sub(room);
        let mut jobs = Vec::new();
        let mut pending = Vec::new();
        let mut outcomes = Vec::with_capacity(opening.documents.len().min(room));
        for (index, document) in opening.documents.into_iter().take(room).enumerate() {
            outcomes.push(match document {
                Document::Path(path) => Some(Ok(OpenedFile::new(path))),
                Document::Content(uri) => match self.tickets.next() {
                    Ok(ticket) => {
                        jobs.push(Job { ticket, uri });
                        pending.push((ticket, index));
                        None
                    }
                    Err(_) => Some(Err(OpenRefusal::Failed)),
                },
            });
        }
        self.waiting.push_back(Waiting {
            links: opening.links,
            outcomes,
            pending,
            unread,
            launch: opening.launch,
            deadline: now + OPEN_DEADLINE,
        });
        Ok(jobs)
    }

    /// The outcome of job `ticket`, given to the document that job opens.
    /// One whose ticket no document waits for any more (its deadline passed,
    /// the platform retired) is returned, for the caller to drop, which
    /// closes its descriptor: it never reaches another document.
    #[must_use = "an outcome no document takes is dropped by the caller"]
    pub(crate) fn finish(&mut self, ticket: NonZeroU64, outcome: Outcome) -> Option<Outcome> {
        for intent in &mut self.waiting {
            if let Some(at) = intent.pending.iter().position(|(held, _)| *held == ticket) {
                let (_, document) = intent.pending.swap_remove(at);
                intent.outcomes[document] = Some(outcome);
                return None;
            }
        }
        Some(outcome)
    }

    /// Refuse each document whose intent's deadline is behind `now`
    /// [`OpenRefusal::TimedOut`]; the tickets of their jobs, to withdraw and
    /// cancel.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<NonZeroU64> {
        let mut jobs = Vec::new();
        for intent in &mut self.waiting {
            if intent.deadline <= now {
                intent.refuse_pending(OpenRefusal::TimedOut, &mut jobs);
            }
        }
        jobs
    }

    /// The intents at the front whose documents all have an outcome, in the
    /// order they arrived; an intent still waiting holds back the later ones.
    pub(crate) fn take_ready(&mut self) -> Vec<Ready> {
        let mut ready = Vec::new();
        while self.waiting.front().is_some_and(Waiting::is_complete) {
            let Some(intent) = self.waiting.pop_front() else {
                break;
            };
            let mut files = Vec::new();
            let mut refused = Vec::new();
            // Complete: every document has its outcome.
            for outcome in intent.outcomes.into_iter().flatten() {
                match outcome {
                    Ok(file) => files.push(file),
                    Err(refusal) => refused.push(refusal),
                }
            }
            ready.push(Ready {
                opened: crate::owner::opened::bounded(
                    intent.links,
                    files,
                    intent.unread,
                    OpenTarget::Application,
                    intent.launch,
                ),
                refused,
            });
        }
        ready
    }

    /// When the earliest intent with a document still out is due.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .filter(|intent| !intent.is_complete())
            .map(|intent| intent.deadline)
            .min()
    }

    /// The platform ended: every document still out is refused
    /// [`OpenRefusal::Retired`], and every intent is let go of, in order.
    pub(crate) fn retire(&mut self) -> Vec<Ready> {
        let mut jobs = Vec::new();
        for intent in &mut self.waiting {
            intent.refuse_pending(OpenRefusal::Retired, &mut jobs);
        }
        self.take_ready()
    }
}

/// A cancellation the owner sends to an open in progress: a
/// `CancellationSignal` on Android, whose `cancel` is a one-way call to the
/// provider and so never waits for it.
pub(crate) trait CancelSignal: Send + Sync + 'static {
    /// Cancel the open that took this signal.
    fn cancel(&self);
}

/// The Java side of opening one document, on a worker thread.
pub(crate) trait DocumentProvider {
    /// The signal an open takes.
    type Signal: CancelSignal;

    /// A new signal for the next open, made before the open is published, so
    /// a cancellation always finds it.
    ///
    /// # Errors
    ///
    /// The refusal the job ends with when no signal can be made.
    fn signal(&mut self) -> Result<Self::Signal, OpenRefusal>;

    /// Open `uri` for reading under `signal`, with the name its provider
    /// shows.
    ///
    /// # Errors
    ///
    /// Why it was not opened.
    fn open(&mut self, uri: &str, signal: &Self::Signal) -> Outcome;
}

/// An open in progress on a worker.
struct Current<S> {
    ticket: NonZeroU64,
    /// Its signal, once the worker made it.
    signal: Option<Arc<S>>,
    /// The owner refused its document meanwhile: a worker that has not
    /// opened yet does not, and one inside the open no longer counts as
    /// keeping up with the queue.
    abandoned: bool,
}

/// What the owner and the workers share.
struct Shared<S> {
    queue: VecDeque<Job>,
    /// The open each worker holds, at most one per worker.
    running: Vec<Current<S>>,
    finished: Vec<(NonZeroU64, Outcome)>,
    /// How many workers run, at most [`MAX_WORKERS`].
    workers: usize,
    /// The platform ended: nothing more is opened.
    retired: bool,
}

impl<S> Shared<S> {
    /// How many workers are inside an abandoned open.
    fn abandoned(&self) -> usize {
        self.running.iter().filter(|open| open.abandoned).count()
    }

    /// Whether a worker must start for the queue: work waits, and every
    /// worker running is inside an abandoned open, with room for one more.
    /// Counts the worker it asks for.
    fn claim_worker(&mut self) -> bool {
        let start = !self.retired
            && !self.queue.is_empty()
            && self.workers == self.abandoned()
            && self.workers < MAX_WORKERS;
        self.workers += usize::from(start);
        start
    }

    /// Abandon the opens in progress whose tickets `abandons` names, their
    /// signals added to `cancel`.
    fn abandon(&mut self, mut abandons: impl FnMut(NonZeroU64) -> bool, cancel: &mut Vec<Arc<S>>) {
        for open in &mut self.running {
            if !open.abandoned && abandons(open.ticket) {
                open.abandoned = true;
                cancel.extend(open.signal.clone());
            }
        }
    }
}

/// What the owner's withdrawal leaves to do once the queue is let go of.
#[derive(Debug)]
#[must_use = "the opens are to cancel and the worker to start"]
pub(crate) struct Withdrawn<S> {
    /// The signals of the opens in progress whose documents were refused.
    pub(crate) cancel: Vec<Arc<S>>,
    /// Whether a worker must start for the jobs that wait.
    pub(crate) start: bool,
}

/// The queue of jobs between the owner and the workers that open them.
pub(crate) struct Opener<S> {
    shared: Mutex<Shared<S>>,
}

impl<S> std::fmt::Debug for Opener<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opener").finish_non_exhaustive()
    }
}

impl<S> Default for Opener<S> {
    fn default() -> Self {
        Self {
            shared: Mutex::new(Shared {
                queue: VecDeque::new(),
                running: Vec::new(),
                finished: Vec::new(),
                workers: 0,
                retired: false,
            }),
        }
    }
}

impl<S: CancelSignal> Opener<S> {
    /// Queue `jobs`: `true` when a worker must start, which the caller then
    /// starts, or reports with [`Self::not_started`]. After retirement
    /// nothing is queued and each job finishes [`OpenRefusal::Retired`].
    pub(crate) fn enqueue(&self, jobs: Vec<Job>) -> bool {
        let mut shared = self.shared.lock();
        if shared.retired {
            shared.finished.extend(
                jobs.into_iter()
                    .map(|job| (job.ticket, Err(OpenRefusal::Retired))),
            );
            return false;
        }
        shared.queue.extend(jobs);
        shared.claim_worker()
    }

    /// A worker the queue asked for could not start: when no other worker
    /// keeps up with the queue, every queued job finishes
    /// [`OpenRefusal::NoThread`].
    pub(crate) fn not_started(&self) {
        let mut shared = self.shared.lock();
        shared.workers -= 1;
        if shared.workers == shared.abandoned() {
            let queued = std::mem::take(&mut shared.queue);
            shared.finished.extend(
                queued
                    .into_iter()
                    .map(|job| (job.ticket, Err(OpenRefusal::NoThread))),
            );
        }
    }

    /// Stop jobs `tickets`, whose documents the owner refused: a queued one
    /// leaves the queue, one in progress is abandoned and its signal returned
    /// to cancel once the lock is let go, and a worker is asked for when
    /// every one left is inside an abandoned open while jobs wait.
    pub(crate) fn withdraw(&self, tickets: &[NonZeroU64]) -> Withdrawn<S> {
        let mut shared = self.shared.lock();
        shared.queue.retain(|job| !tickets.contains(&job.ticket));
        let mut cancel = Vec::new();
        shared.abandon(|ticket| tickets.contains(&ticket), &mut cancel);
        let start = shared.claim_worker();
        Withdrawn { cancel, start }
    }

    /// The platform ended: nothing more is opened, the outcomes not taken
    /// are returned to drop, and the signals of the opens in progress to
    /// cancel.
    pub(crate) fn retire(&self) -> (Vec<(NonZeroU64, Outcome)>, Vec<Arc<S>>) {
        let mut shared = self.shared.lock();
        shared.retired = true;
        shared.queue.clear();
        let mut cancel = Vec::new();
        shared.abandon(|_| true, &mut cancel);
        (std::mem::take(&mut shared.finished), cancel)
    }

    /// The outcomes the workers handed back since the last call.
    pub(crate) fn take_finished(&self) -> Vec<(NonZeroU64, Outcome)> {
        std::mem::take(&mut self.shared.lock().finished)
    }

    /// A worker's next job; `None` ends the worker.
    fn next(&self) -> Option<Job> {
        let mut shared = self.shared.lock();
        let job = (!shared.retired)
            .then(|| shared.queue.pop_front())
            .flatten();
        match &job {
            Some(job) => shared.running.push(Current {
                ticket: job.ticket,
                signal: None,
                abandoned: false,
            }),
            None => shared.workers -= 1,
        }
        job
    }

    /// The worker made `signal` for its job `ticket`: `false` when its
    /// document was refused meanwhile, and it is not opened.
    fn enter(&self, ticket: NonZeroU64, signal: Arc<S>) -> bool {
        let mut shared = self.shared.lock();
        let Some(open) = shared.running.iter_mut().find(|open| open.ticket == ticket) else {
            return false;
        };
        open.signal = Some(signal);
        !open.abandoned
    }

    /// Job `ticket` ended with `outcome`. After retirement nobody takes it:
    /// it is returned for the worker to drop, which closes its descriptor.
    #[must_use = "an outcome after retirement is dropped by the worker"]
    fn finish(&self, ticket: NonZeroU64, outcome: Outcome) -> Option<Outcome> {
        let mut shared = self.shared.lock();
        shared.running.retain(|open| open.ticket != ticket);
        if shared.retired {
            return Some(outcome);
        }
        shared.finished.push((ticket, outcome));
        None
    }
}

/// A worker: open each job in turn through `provider`, hand back its outcome
/// and `wake` the owner, until no job waits or the platform ends. A panic in
/// the provider fails its job ([`OpenRefusal::Failed`]) while the worker goes
/// on, and is raised once it has ended.
pub(crate) fn work<P: DocumentProvider>(
    opener: &Opener<P::Signal>,
    provider: &mut P,
    wake: impl Fn(),
) {
    let mut failure = FirstPanic::new();
    while let Some(job) = opener.next() {
        let outcome = failure
            .contain(|| {
                let signal = Arc::new(provider.signal()?);
                if opener.enter(job.ticket, Arc::clone(&signal)) {
                    provider.open(&job.uri, &signal)
                } else {
                    Err(OpenRefusal::Withdrawn)
                }
            })
            .unwrap_or(Err(OpenRefusal::Failed));
        // After retirement the outcome is dropped here, with no lock held.
        drop(opener.finish(job.ticket, outcome));
        failure.contain(&wake);
    }
    failure.resume();
}

/// How the owner starts a worker over the queue it is handed.
pub(crate) type Spawn<S> = Box<dyn Fn(Arc<Opener<S>>) -> std::io::Result<()>>;

/// The owner's documents: the intents waiting for them and the workers that
/// open them. Owner thread.
pub(crate) struct Documents<S> {
    openings: Openings,
    opener: Arc<Opener<S>>,
    spawn: Spawn<S>,
}

impl<S> std::fmt::Debug for Documents<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Documents").finish_non_exhaustive()
    }
}

/// What a turn of the owner's documents leaves to do once they are let go
/// of: the owner holds them in a cell, and neither a cancellation (a JNI
/// call) nor a log line (a subscriber is user code) runs inside it.
#[derive(Debug)]
#[must_use = "the intents are to deliver, the opens to cancel and the failure to report"]
pub(crate) struct Turn<S> {
    /// The intents now ready, in the order they arrived.
    pub(crate) ready: Vec<Ready>,
    /// The signals of the opens in progress whose documents were refused, to
    /// cancel.
    pub(crate) cancel: Vec<Arc<S>>,
    /// Why a worker the queue asked for could not start; its jobs are
    /// refused [`OpenRefusal::NoThread`] when no other worker keeps up.
    pub(crate) not_started: Option<std::io::Error>,
}

impl<S: CancelSignal> Documents<S> {
    /// The documents of a platform whose workers `spawn` starts.
    pub(crate) fn new(spawn: Spawn<S>) -> Self {
        Self {
            openings: Openings::default(),
            opener: Arc::default(),
            spawn,
        }
    }

    /// Start the worker the queue asked for; why it could not, after its
    /// jobs are refused when no other worker keeps up.
    fn start_worker(&self) -> Option<std::io::Error> {
        let not_started = (self.spawn)(Arc::clone(&self.opener)).err();
        if not_started.is_some() {
            self.opener.not_started();
        }
        not_started
    }

    /// Accept `opening` at `now`, starting a worker when the queue asks for
    /// one; why it could not start is returned to report.
    ///
    /// # Errors
    ///
    /// [`Busy`] when [`MAX_WAITING_INTENTS`] wait already.
    pub(crate) fn accept(
        &mut self,
        opening: Opening,
        now: Instant,
    ) -> Result<Option<std::io::Error>, Busy> {
        let jobs = self.openings.accept(opening, now)?;
        if jobs.is_empty() || !self.opener.enqueue(jobs) {
            return Ok(None);
        }
        Ok(self.start_worker())
    }

    /// One owner turn at `now`: the outcomes the workers handed back (one no
    /// document waits for is dropped, closing its descriptor), then the
    /// documents past their deadline refused, their jobs withdrawn, and a
    /// worker started when every one left is inside an abandoned open.
    pub(crate) fn turn(&mut self, now: Instant) -> Turn<S> {
        for (ticket, outcome) in self.opener.take_finished() {
            drop(self.openings.finish(ticket, outcome));
        }
        let expired = self.openings.expire(now);
        let mut turn = Turn {
            ready: Vec::new(),
            cancel: Vec::new(),
            not_started: None,
        };
        if !expired.is_empty() {
            let withdrawn = self.opener.withdraw(&expired);
            turn.cancel = withdrawn.cancel;
            if withdrawn.start {
                turn.not_started = self.start_worker();
            }
        }
        turn.ready = self.openings.take_ready();
        turn
    }

    /// When the owner must take its next turn for a deadline.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.openings.next_deadline()
    }

    /// The platform ended: nothing more is opened, the outcomes not taken
    /// are dropped (closing their descriptors), the opens in progress are to
    /// cancel, and each intent still waiting is let go of with its documents
    /// refused [`OpenRefusal::Retired`]. What a worker finishes later it
    /// drops itself.
    pub(crate) fn retire(&mut self) -> Turn<S> {
        let (finished, cancel) = self.opener.retire();
        for (ticket, outcome) in finished {
            drop(self.openings.finish(ticket, outcome));
        }
        Turn {
            ready: self.openings.retire(),
            cancel,
            not_started: None,
        }
    }
}

#[cfg(test)]
mod tests;
