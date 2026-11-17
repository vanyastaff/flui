//! The documents an intent names, opened off the owner thread (ADR-0330 §5).
//!
//! A `content:` document is served by another application's content
//! provider, and opening it (`ContentResolver.openFileDescriptor`, then the
//! name query) can block for as long as that application takes: a cloud
//! provider answers once it has downloaded the file. The owner thread drains
//! NativeActivity's input queue and runs frames, so it never opens one.
//!
//! [`Openings`] is the owner's side, plain state: each intent is accepted
//! under a ticket with a [deadline](OPEN_DEADLINE), its documents become
//! jobs, and the intents are delivered in the order they arrived, each once
//! every one of its documents has an outcome. [`Opener`] is the queue the
//! owner and one worker thread share: the worker opens one job at a time
//! through the [`DocumentProvider`] (the Java calls on Android, a stand-in in
//! the tests) and hands the outcome back under the job's ticket, then wakes
//! the owner. Every accepted document ends with exactly one outcome, an
//! [`OpenedFile`] or an [`OpenRefusal`] that says why: the provider failed,
//! the read grant ended, the deadline passed (the open is cancelled through
//! its `CancellationSignal`), the platform retired, or no thread could start.
//! An outcome that arrives for a document already refused is dropped, which
//! closes its descriptor.
//!
//! A provider that ignores the cancellation keeps the worker; the documents
//! queued behind it are refused at their own deadlines, so their intents are
//! still delivered and the owner thread never waits.

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
/// accepted, their wait for the worker included. Long enough for a provider
/// to download a document of some tens of megabytes; past it each document
/// still unopened is refused [`OpenRefusal::TimedOut`] and its intent is
/// delivered with what did open.
pub(crate) const OPEN_DEADLINE: Duration = Duration::from_secs(30);

/// The most intents waiting for their documents at once: the most `FluiHost`
/// queues ([`MAX_OPENED_ITEMS`]). An intent past it is refused whole.
pub(crate) const MAX_WAITING_INTENTS: usize = MAX_OPENED_ITEMS;

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

/// One document of a waiting intent.
#[derive(Debug)]
enum Slot {
    /// Opened, or refused.
    Done(Outcome),
    /// Waiting for the outcome of its job.
    Waiting(NonZeroU64),
}

/// An intent waiting for its documents.
#[derive(Debug)]
struct Waiting {
    links: Vec<String>,
    documents: Vec<Slot>,
    /// The documents past the room an `Opened` leaves, never opened.
    unread: usize,
    launch: Launch,
    deadline: Instant,
}

impl Waiting {
    fn is_complete(&self) -> bool {
        self.documents
            .iter()
            .all(|slot| matches!(slot, Slot::Done(_)))
    }

    /// Refuse every document still waiting with `refusal`, its job's ticket
    /// added to `jobs`.
    fn refuse_waiting(&mut self, refusal: OpenRefusal, jobs: &mut Vec<NonZeroU64>) {
        for slot in &mut self.documents {
            if let Slot::Waiting(ticket) = *slot {
                jobs.push(ticket);
                *slot = Slot::Done(Err(refusal));
            }
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
        let mut documents = Vec::with_capacity(opening.documents.len().min(room));
        for document in opening.documents.into_iter().take(room) {
            documents.push(match document {
                Document::Path(path) => Slot::Done(Ok(OpenedFile::new(path))),
                Document::Content(uri) => match self.tickets.next() {
                    Ok(ticket) => {
                        jobs.push(Job { ticket, uri });
                        Slot::Waiting(ticket)
                    }
                    Err(_) => Slot::Done(Err(OpenRefusal::Failed)),
                },
            });
        }
        self.waiting.push_back(Waiting {
            links: opening.links,
            documents,
            unread,
            launch: opening.launch,
            deadline: now + OPEN_DEADLINE,
        });
        Ok(jobs)
    }

    /// The outcome of job `ticket`. One for a document already refused (its
    /// deadline passed, the platform retired) is dropped, which closes its
    /// descriptor.
    pub(crate) fn finish(&mut self, ticket: NonZeroU64, outcome: Outcome) {
        for waiting in &mut self.waiting {
            for slot in &mut waiting.documents {
                if matches!(slot, Slot::Waiting(waiting) if *waiting == ticket) {
                    *slot = Slot::Done(outcome);
                    return;
                }
            }
        }
    }

    /// Refuse each document whose intent's deadline is behind `now`
    /// [`OpenRefusal::TimedOut`]; the tickets of their jobs, to withdraw and
    /// cancel.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<NonZeroU64> {
        let mut jobs = Vec::new();
        for waiting in &mut self.waiting {
            if waiting.deadline <= now {
                waiting.refuse_waiting(OpenRefusal::TimedOut, &mut jobs);
            }
        }
        jobs
    }

    /// The intents at the front whose documents all have an outcome, in the
    /// order they arrived; an intent still waiting holds back the later ones.
    pub(crate) fn take_ready(&mut self) -> Vec<Ready> {
        let mut ready = Vec::new();
        while self.waiting.front().is_some_and(Waiting::is_complete) {
            let Some(waiting) = self.waiting.pop_front() else {
                break;
            };
            let mut files = Vec::new();
            let mut refused = Vec::new();
            for slot in waiting.documents {
                match slot {
                    Slot::Done(Ok(file)) => files.push(file),
                    Slot::Done(Err(refusal)) => refused.push(refusal),
                    Slot::Waiting(_) => refused.push(OpenRefusal::Failed),
                }
            }
            ready.push(Ready {
                opened: crate::owner::opened::bounded(
                    waiting.links,
                    files,
                    waiting.unread,
                    OpenTarget::Application,
                    waiting.launch,
                ),
                refused,
            });
        }
        ready
    }

    /// When the earliest intent with a document still waiting is due.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .filter(|waiting| !waiting.is_complete())
            .map(|waiting| waiting.deadline)
            .min()
    }

    /// The platform ended: every document still waiting is refused
    /// [`OpenRefusal::Retired`], and every intent is let go of, in order.
    pub(crate) fn retire(&mut self) -> Vec<Ready> {
        let mut jobs = Vec::new();
        for waiting in &mut self.waiting {
            waiting.refuse_waiting(OpenRefusal::Retired, &mut jobs);
        }
        self.take_ready()
    }
}

/// A cancellation the owner sends to the open in progress: a
/// `CancellationSignal` on Android, whose `cancel` is a one-way call to the
/// provider and so never waits for it.
pub(crate) trait CancelSignal: Send + Sync + 'static {
    /// Cancel the open that took this signal.
    fn cancel(&self);
}

/// The Java side of opening one document, on the worker thread.
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

/// The open in progress on the worker.
struct Current<S> {
    ticket: NonZeroU64,
    /// Its signal, once the worker made it.
    signal: Option<Arc<S>>,
    /// The owner refused its document meanwhile: a worker that has not
    /// opened yet does not.
    abandoned: bool,
}

/// What the owner and the worker share.
struct Shared<S> {
    queue: VecDeque<Job>,
    current: Option<Current<S>>,
    finished: Vec<(NonZeroU64, Outcome)>,
    /// Whether a worker runs.
    running: bool,
    /// The platform ended: the worker opens nothing more.
    retired: bool,
}

/// The queue of jobs between the owner and the one worker that opens them.
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
                current: None,
                finished: Vec::new(),
                running: false,
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
        let start = !shared.running && !shared.queue.is_empty();
        shared.running |= start;
        start
    }

    /// The worker [`Self::enqueue`] asked for could not start: every queued
    /// job finishes [`OpenRefusal::NoThread`], and the next enqueue asks
    /// again.
    pub(crate) fn not_started(&self) {
        let mut shared = self.shared.lock();
        shared.running = false;
        let queued = std::mem::take(&mut shared.queue);
        shared.finished.extend(
            queued
                .into_iter()
                .map(|job| (job.ticket, Err(OpenRefusal::NoThread))),
        );
    }

    /// Stop jobs `tickets`, whose documents the owner refused: a queued one
    /// leaves the queue, the one in progress is abandoned, and its signal is
    /// returned to cancel once the lock is let go.
    #[must_use]
    pub(crate) fn withdraw(&self, tickets: &[NonZeroU64]) -> Option<Arc<S>> {
        let mut shared = self.shared.lock();
        shared.queue.retain(|job| !tickets.contains(&job.ticket));
        let current = shared
            .current
            .as_mut()
            .filter(|current| tickets.contains(&current.ticket))?;
        current.abandoned = true;
        current.signal.clone()
    }

    /// The platform ended: nothing more is opened, and the signal of the open
    /// in progress is returned to cancel.
    #[must_use]
    pub(crate) fn retire(&self) -> Option<Arc<S>> {
        let mut shared = self.shared.lock();
        shared.retired = true;
        shared.queue.clear();
        let current = shared.current.as_mut()?;
        current.abandoned = true;
        current.signal.clone()
    }

    /// The outcomes the worker handed back since the last call.
    pub(crate) fn take_finished(&self) -> Vec<(NonZeroU64, Outcome)> {
        std::mem::take(&mut self.shared.lock().finished)
    }

    /// The worker's next job, `None` ending it.
    fn next(&self) -> Option<Job> {
        let mut shared = self.shared.lock();
        let job = (!shared.retired)
            .then(|| shared.queue.pop_front())
            .flatten();
        match &job {
            Some(job) => {
                shared.current = Some(Current {
                    ticket: job.ticket,
                    signal: None,
                    abandoned: false,
                });
            }
            None => shared.running = false,
        }
        job
    }

    /// The worker made `signal` for the current job: `false` when its
    /// document was refused meanwhile, and it is not opened.
    fn enter(&self, signal: Arc<S>) -> bool {
        let mut shared = self.shared.lock();
        let Some(current) = shared.current.as_mut() else {
            return false;
        };
        current.signal = Some(signal);
        !current.abandoned
    }

    /// The current job ended with `outcome`.
    fn finish(&self, ticket: NonZeroU64, outcome: Outcome) {
        let mut shared = self.shared.lock();
        shared.current = None;
        shared.finished.push((ticket, outcome));
    }
}

/// The worker: open each job in turn through `provider`, hand back its
/// outcome and `wake` the owner, until the queue is empty or the platform
/// ends. A panic in the provider fails its job ([`OpenRefusal::Failed`])
/// while the worker goes on, and is raised once it has ended.
pub(crate) fn work<P: DocumentProvider>(
    opener: &Opener<P::Signal>,
    provider: &mut P,
    wake: impl Fn(),
) {
    let mut failure = FirstPanic::new();
    while let Some(job) = opener.next() {
        let outcome = failure
            .contain(|| match provider.signal() {
                Ok(signal) => {
                    let signal = Arc::new(signal);
                    if opener.enter(Arc::clone(&signal)) {
                        provider.open(&job.uri, &signal)
                    } else {
                        // The owner refused its document meanwhile and
                        // drops whatever this job answers.
                        Err(OpenRefusal::Retired)
                    }
                }
                Err(refusal) => Err(refusal),
            })
            .unwrap_or(Err(OpenRefusal::Failed));
        opener.finish(job.ticket, outcome);
        failure.contain(&wake);
    }
    failure.resume();
}

/// The owner's documents: the intents waiting for them and the worker that
/// opens them. Owner thread.
pub(crate) struct Documents<S> {
    openings: Openings,
    opener: Arc<Opener<S>>,
}

impl<S> std::fmt::Debug for Documents<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Documents").finish_non_exhaustive()
    }
}

impl<S: CancelSignal> Default for Documents<S> {
    fn default() -> Self {
        Self {
            openings: Openings::default(),
            opener: Arc::default(),
        }
    }
}

/// What a turn of the owner's documents leaves to do once they are let go
/// of: the owner holds them in a cell, and neither a cancellation (a JNI
/// call) nor a log line (a subscriber is user code) runs inside it.
#[derive(Debug)]
#[must_use = "the intents are to deliver and the open to cancel"]
pub(crate) struct Turn<S> {
    /// The intents now ready, in the order they arrived.
    pub(crate) ready: Vec<Ready>,
    /// The signal of the open in progress whose document was refused, to
    /// cancel.
    pub(crate) cancel: Option<Arc<S>>,
}

impl<S: CancelSignal> Documents<S> {
    /// Accept `opening` at `now`, starting a worker through `spawn` when
    /// none runs. A worker that cannot start refuses its jobs
    /// [`OpenRefusal::NoThread`], answered by the next turn; its error is
    /// returned to report.
    ///
    /// # Errors
    ///
    /// [`Busy`] when [`MAX_WAITING_INTENTS`] wait already.
    pub(crate) fn accept(
        &mut self,
        opening: Opening,
        now: Instant,
        spawn: impl FnOnce(Arc<Opener<S>>) -> std::io::Result<()>,
    ) -> Result<Option<std::io::Error>, Busy> {
        let jobs = self.openings.accept(opening, now)?;
        if jobs.is_empty() || !self.opener.enqueue(jobs) {
            return Ok(None);
        }
        let not_started = spawn(Arc::clone(&self.opener)).err();
        if not_started.is_some() {
            self.opener.not_started();
        }
        Ok(not_started)
    }

    /// One owner turn at `now`: the outcomes the worker handed back, then
    /// the documents past their deadline refused and their jobs withdrawn.
    pub(crate) fn turn(&mut self, now: Instant) -> Turn<S> {
        for (ticket, outcome) in self.opener.take_finished() {
            self.openings.finish(ticket, outcome);
        }
        let expired = self.openings.expire(now);
        let cancel = if expired.is_empty() {
            None
        } else {
            self.opener.withdraw(&expired)
        };
        Turn {
            ready: self.openings.take_ready(),
            cancel,
        }
    }

    /// When the owner must take its next turn for a deadline.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.openings.next_deadline()
    }

    /// The platform ended: nothing more is opened, the open in progress is
    /// to cancel, and each intent still waiting is let go of with its
    /// documents refused [`OpenRefusal::Retired`].
    pub(crate) fn retire(&mut self) -> Turn<S> {
        let cancel = self.opener.retire();
        for (ticket, outcome) in self.opener.take_finished() {
            self.openings.finish(ticket, outcome);
        }
        Turn {
            ready: self.openings.retire(),
            cancel,
        }
    }
}

#[cfg(test)]
mod tests;
