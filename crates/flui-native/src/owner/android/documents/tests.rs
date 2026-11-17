//! The documents an intent names, opened off the owner thread: the owner's
//! turn never waits for a provider, the intents arrive in order, an answer
//! reaches only the document whose ticket it names, a stalled provider holds
//! no later document, and every accepted document ends with exactly one
//! outcome on every exit.

use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use flui_platform::{Launch, OpenedFile};

use super::{
    Busy, CancelSignal, DocumentProvider, Documents, Job, MAX_WAITING_INTENTS, MAX_WORKERS,
    OPEN_DEADLINE, OpenRefusal, Opener, Openings, Outcome, Ready, work,
};
use crate::owner::android::intent::{Document, Opening};

/// A cancellation the test can read back through a clone.
#[derive(Clone, Default)]
struct Flag(Arc<AtomicBool>);

impl CancelSignal for Flag {
    fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Flag {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A provider whose opens of a URI ending in `slow` block, ignoring their
/// cancellation as `ContentProvider.openFile` does by default, until the test
/// opens `gate`; every other open answers at once. It counts the opens it
/// entered and hands the test each slow open's signal.
#[derive(Clone)]
struct Provider {
    gate: Arc<AtomicBool>,
    entered: Arc<AtomicUsize>,
    signals: mpsc::Sender<Flag>,
}

impl Provider {
    fn new() -> (Self, mpsc::Receiver<Flag>) {
        let (signals, signal_of) = mpsc::channel();
        let provider = Self {
            gate: Arc::default(),
            entered: Arc::default(),
            signals,
        };
        (provider, signal_of)
    }

    fn open_gate(&self) {
        self.gate.store(true, Ordering::SeqCst);
    }

    fn entered(&self) -> usize {
        self.entered.load(Ordering::SeqCst)
    }

    /// Documents whose workers run this provider on threads of their own.
    fn documents(&self) -> Documents<Flag> {
        let provider = self.clone();
        Documents::new(Box::new(move |opener| {
            let mut provider = provider.clone();
            std::thread::Builder::new()
                .name("flui-test-documents".to_owned())
                .spawn(move || work(&opener, &mut provider, || {}))
                .map(drop)
        }))
    }
}

impl DocumentProvider for Provider {
    type Signal = Flag;

    fn signal(&mut self) -> Result<Flag, OpenRefusal> {
        Ok(Flag::default())
    }

    fn open(&mut self, uri: &str, signal: &Flag) -> Outcome {
        let n = self.entered.fetch_add(1, Ordering::SeqCst);
        if uri.ends_with("slow") {
            let _ = self.signals.send(signal.clone());
            let until = Instant::now() + Duration::from_secs(10);
            while !self.gate.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(file(100 + n))
    }
}

fn content(uris: &[&str]) -> Opening {
    Opening {
        links: Vec::new(),
        documents: uris
            .iter()
            .map(|uri| Document::Content((*uri).to_owned()))
            .collect(),
        launch: Launch::Running,
    }
}

fn link(url: &str) -> Opening {
    Opening {
        links: vec![url.to_owned()],
        documents: Vec::new(),
        launch: Launch::Running,
    }
}

fn file(n: usize) -> OpenedFile {
    OpenedFile::new(format!("/proc/self/fd/{n}"))
}

/// One turn of the owner at `now`, cancelling what it says to: the intents it
/// hands over.
fn turn(documents: &mut Documents<Flag>, now: Instant) -> Vec<Ready> {
    let turn = documents.turn(now);
    for signal in turn.cancel {
        signal.cancel();
    }
    assert!(turn.not_started.is_none(), "every worker starts");
    turn.ready
}

/// Turns of the owner at `now` until they have handed over `count` intents or
/// `within` passes on the real clock.
fn turns_until(
    documents: &mut Documents<Flag>,
    now: Instant,
    count: usize,
    within: Duration,
) -> Vec<Ready> {
    let until = Instant::now() + within;
    let mut ready = Vec::new();
    while ready.len() < count && Instant::now() < until {
        ready.extend(turn(documents, now));
        std::thread::sleep(Duration::from_millis(2));
    }
    ready
}

/// Waits until `provider` entered `count` opens, on the real clock.
fn wait_entered(provider: &Provider, count: usize) {
    let until = Instant::now() + Duration::from_secs(5);
    while provider.entered() < count && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(provider.entered(), count, "the worker is inside the open");
}

/// A provider that answers only after a while (a cloud provider that
/// downloads first) holds no turn of the owner: accepting the intent and the
/// turns after it return while the open is still running, the link-only
/// intent that arrived after it waits behind it, and both arrive in order
/// once the document opened.
#[test]
fn a_slow_provider_holds_no_owner_turn() {
    let (provider, _signals) = Provider::new();
    let mut documents = provider.documents();
    let started = Instant::now();
    let not_started = documents
        .accept(content(&["content://notes/doc/slow"]), started)
        .expect("room");
    assert!(not_started.is_none());
    documents
        .accept(link("notes:///note/2"), started)
        .expect("room");
    assert!(
        turn(&mut documents, started).is_empty(),
        "the link waits behind the document"
    );
    wait_entered(&provider, 1);
    assert!(turn(&mut documents, started).is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "no owner call waited for the provider"
    );
    assert_eq!(
        documents.next_deadline(),
        Some(started + OPEN_DEADLINE),
        "the loop wakes for the deadline"
    );

    provider.open_gate();
    let ready = turns_until(&mut documents, started, 2, Duration::from_secs(10));
    assert_eq!(ready.len(), 2, "both intents, once");
    assert_eq!(ready[0].opened.files, [file(100)]);
    assert_eq!(ready[0].refused, Vec::new());
    assert_eq!(ready[1].opened.urls, ["notes:///note/2"]);
    assert_eq!(documents.next_deadline(), None);
}

/// A provider that ignores the cancellation keeps the worker inside the open
/// of a document past its deadline; the document of a later intent does not
/// wait for it: a fresh worker opens it, and its intent arrives with its
/// file while the stalled open is still inside the provider.
#[test]
fn a_stalled_provider_holds_no_later_document() {
    let (provider, signals) = Provider::new();
    let mut documents = provider.documents();
    let start = Instant::now();
    documents
        .accept(content(&["content://cloud/doc/slow"]), start)
        .expect("room");
    let stalled = signals
        .recv_timeout(Duration::from_secs(5))
        .expect("the slow open began");
    let later = start + Duration::from_secs(1);
    documents
        .accept(content(&["content://local/doc/fast"]), later)
        .expect("room");
    assert!(
        turn(&mut documents, later).is_empty(),
        "the fast one waits behind"
    );
    assert_eq!(provider.entered(), 1);

    // The first intent's deadline: its open is cancelled, which the provider
    // ignores, and the second is opened by a worker of its own.
    let due = start + OPEN_DEADLINE;
    let ready = turns_until(&mut documents, due, 2, Duration::from_secs(5));
    assert!(stalled.is_cancelled(), "the stalled open is cancelled");
    assert_eq!(ready.len(), 2, "both intents, in order");
    assert_eq!(ready[0].refused, [OpenRefusal::TimedOut]);
    assert_eq!(ready[1].opened.files, [file(101)]);
    assert!(
        !provider.gate.load(Ordering::SeqCst),
        "the stalled open is still inside"
    );
    provider.open_gate();
}

/// An answer reaches only the document its ticket names. A document passes
/// its deadline while its worker is inside the open; another intent's
/// document waits for its own; the first open's late file is not given to
/// it: it is returned to drop, which closes its descriptor, and the second
/// document still waits until its own answer arrives.
#[test]
fn a_late_answer_reaches_no_other_intents_document() {
    let now = Instant::now();
    let mut openings = Openings::default();
    let first = openings
        .accept(content(&["content://a/doc"]), now)
        .expect("room");
    assert_eq!(openings.expire(now + OPEN_DEADLINE), [first[0].ticket]);
    let later = now + OPEN_DEADLINE;
    let second = openings
        .accept(content(&["content://b/doc"]), later)
        .expect("room");

    let late = late_file();
    let held = late.as_ref().map(|late| late.path.clone());
    let stray = openings.finish(first[0].ticket, Ok(late.unwrap_or_else(|| file(1))));
    assert!(stray.is_some(), "the late file belongs to no document");
    drop(stray);
    if let Some(held) = held {
        assert!(!held.exists(), "the late file's descriptor is closed");
    }

    let ready = openings.take_ready();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].refused, [OpenRefusal::TimedOut]);
    assert_eq!(ready[0].opened.files, Vec::new());
    assert!(openings.take_ready().is_empty(), "the second still waits");
    assert_eq!(openings.next_deadline(), Some(later + OPEN_DEADLINE));
    assert!(openings.finish(second[0].ticket, Ok(file(2))).is_none());
    assert_eq!(openings.take_ready()[0].opened.files, [file(2)]);
}

/// A file held by an open descriptor, as a provider's document is, where the
/// host has `/proc/self/fd`.
fn late_file() -> Option<OpenedFile> {
    #[cfg(target_os = "linux")]
    {
        let descriptor = std::os::fd::OwnedFd::from(std::fs::File::open("/dev/null").ok()?);
        OpenedFile::try_from_descriptor(descriptor, None).ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Each way a document can end gives it one outcome, and its intent is
/// delivered once every document has one: opened, the provider's failure,
/// the read grant ended, the deadline passed, the platform retired; past the
/// intents that may wait, an intent is turned away whole.
#[test]
fn every_accepted_document_ends_with_one_outcome() {
    let now = Instant::now();
    let mut openings = Openings::default();
    let jobs = openings
        .accept(
            Opening {
                links: vec!["notes:///note/1".to_owned()],
                documents: vec![
                    Document::Content("content://notes/doc/1".to_owned()),
                    Document::Path(PathBuf::from("/sdcard/a.note")),
                    Document::Content("content://notes/doc/2".to_owned()),
                    Document::Content("content://notes/doc/3".to_owned()),
                ],
                launch: Launch::Cold,
            },
            now,
        )
        .expect("room");
    assert_eq!(jobs.len(), 3, "a path needs no provider");
    assert_eq!(openings.next_deadline(), Some(now + OPEN_DEADLINE));
    assert!(openings.finish(jobs[0].ticket, Ok(file(41))).is_none());
    assert!(
        openings
            .finish(jobs[1].ticket, Err(OpenRefusal::GrantEnded))
            .is_none()
    );
    assert!(openings.take_ready().is_empty(), "one still opens");
    let just_before = now + OPEN_DEADLINE.saturating_sub(Duration::from_millis(1));
    assert_eq!(openings.expire(just_before), Vec::new());
    assert_eq!(openings.expire(now + OPEN_DEADLINE), [jobs[2].ticket]);
    let ready = openings.take_ready();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].opened.urls, ["notes:///note/1"]);
    assert_eq!(
        ready[0].opened.files,
        [file(41), OpenedFile::new("/sdcard/a.note")]
    );
    assert_eq!(
        ready[0].refused,
        [OpenRefusal::GrantEnded, OpenRefusal::TimedOut]
    );
    assert_eq!(openings.next_deadline(), None);

    // A provider's failure is its document's outcome.
    let jobs = openings
        .accept(content(&["content://x/1"]), now)
        .expect("room");
    assert!(
        openings
            .finish(jobs[0].ticket, Err(OpenRefusal::Failed))
            .is_none()
    );
    assert_eq!(openings.take_ready()[0].refused, [OpenRefusal::Failed]);

    // The intents that may wait are bounded; the next is turned away.
    for n in 0..MAX_WAITING_INTENTS {
        openings
            .accept(content(&[&format!("content://x/{n}")]), now)
            .expect("room");
    }
    assert_eq!(openings.accept(link("notes:///late"), now), Err(Busy));

    // Retirement refuses every document still out, in order.
    let ready = openings.retire();
    assert_eq!(ready.len(), MAX_WAITING_INTENTS);
    assert!(
        ready
            .iter()
            .all(|ready| ready.refused == [OpenRefusal::Retired])
    );
    assert!(openings.take_ready().is_empty());
}

fn job(n: u64, uri: &str) -> Job {
    Job {
        ticket: NonZeroU64::new(n).expect("nonzero"),
        uri: uri.to_owned(),
    }
}

/// The workers' side of each exit: a worker that cannot start refuses its
/// jobs; retirement cancels the open in progress, opens nothing more, and
/// closes what finishes after it; a refused document's job is never opened;
/// workers are added only while every one is inside an abandoned open, and
/// never past `MAX_WORKERS`.
#[test]
fn a_refused_document_is_withdrawn_and_its_open_cancelled() {
    let now = Instant::now();

    // No thread: every job is refused, and the intent still arrives.
    let mut documents =
        Documents::<Flag>::new(Box::new(|_| Err(std::io::Error::other("no thread"))));
    let not_started = documents
        .accept(content(&["content://x/1"]), now)
        .expect("room");
    assert!(not_started.is_some());
    let ready = documents.turn(now).ready;
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].refused, [OpenRefusal::NoThread]);

    // The platform retires while a document opens: its open is cancelled
    // and its intent let go of with the document refused.
    let (provider, signals) = Provider::new();
    let mut documents = provider.documents();
    documents
        .accept(content(&["content://x/slow"]), now)
        .expect("room");
    let signal = signals
        .recv_timeout(Duration::from_secs(5))
        .expect("the open began");
    let retired = documents.retire();
    assert_eq!(retired.cancel.len(), 1);
    for cancel in retired.cancel {
        cancel.cancel();
    }
    assert!(signal.is_cancelled());
    assert_eq!(retired.ready.len(), 1);
    assert_eq!(retired.ready[0].refused, [OpenRefusal::Retired]);
    provider.open_gate();

    // After retirement nothing is opened, nothing is queued, and an outcome
    // a worker finishes is handed back to drop.
    let opener = Opener::<Flag>::default();
    assert!(
        opener.enqueue(vec![job(1, "content://x/1")]),
        "a worker starts"
    );
    let first = opener.next().expect("queued");
    assert!(opener.enter(first.ticket, Arc::new(Flag::default())));
    let (finished, cancel) = opener.retire();
    assert_eq!(finished.len(), 0);
    assert_eq!(cancel.len(), 1);
    assert!(
        opener.finish(first.ticket, Ok(file(43))).is_some(),
        "closed by the worker"
    );
    assert!(opener.next().is_none(), "nothing more after retirement");
    assert!(!opener.enqueue(vec![job(2, "content://x/2")]));
    assert_eq!(opener.take_finished()[0].1, Err(OpenRefusal::Retired));

    // A withdrawn job is never opened: queued, it leaves the queue; about to
    // open, its worker does not.
    let opener = Opener::<Flag>::default();
    assert!(opener.enqueue(vec![job(1, "content://x/1"), job(2, "content://x/2")]));
    let first = opener.next().expect("queued");
    let withdrawn = opener.withdraw(&[first.ticket, NonZeroU64::new(2).expect("nonzero")]);
    assert_eq!(withdrawn.cancel.len(), 0, "no signal made yet");
    assert!(!withdrawn.start, "nothing waits");
    assert!(
        !opener.enter(first.ticket, Arc::new(Flag::default())),
        "not opened"
    );
    assert!(
        opener
            .finish(first.ticket, Err(OpenRefusal::Withdrawn))
            .is_none()
    );
    assert!(opener.next().is_none(), "the queued one left");

    // Workers grow only while every one is inside an abandoned open, and
    // never past the bound.
    let opener = Opener::<Flag>::default();
    let mut ticket = 0;
    let mut next_job = |opener: &Opener<Flag>| {
        ticket += 1;
        opener.enqueue(vec![job(ticket, "content://x/slow")])
    };
    assert!(next_job(&opener), "the first worker");
    assert!(!next_job(&opener), "one keeps up with the queue");
    for workers in 1..=MAX_WORKERS {
        let held = opener.next().expect("a job");
        assert!(opener.enter(held.ticket, Arc::new(Flag::default())));
        let withdrawn = opener.withdraw(&[held.ticket]);
        assert_eq!(withdrawn.cancel.len(), 1);
        assert_eq!(
            withdrawn.start,
            workers < MAX_WORKERS,
            "a fresh worker while under the bound, with {workers} held"
        );
        if workers < MAX_WORKERS {
            assert!(!next_job(&opener), "the fresh one keeps up");
        }
    }
}

/// A provider that panics fails its own job alone: the worker opens the next
/// one, hands back both outcomes, and raises the panic once it has ended.
#[test]
fn a_provider_panic_fails_its_job_alone() {
    /// Panics on the URI that says so.
    struct Panics;

    impl DocumentProvider for Panics {
        type Signal = Flag;

        fn signal(&mut self) -> Result<Flag, OpenRefusal> {
            Ok(Flag::default())
        }

        fn open(&mut self, uri: &str, _signal: &Flag) -> Outcome {
            assert!(!uri.ends_with("panics"), "a provider panics");
            Ok(file(44))
        }
    }

    let opener = Arc::new(Opener::<Flag>::default());
    assert!(opener.enqueue(vec![
        job(1, "content://x/panics"),
        job(2, "content://x/opens"),
    ]));
    let worker = Arc::clone(&opener);
    let joined = std::thread::spawn(move || work(&worker, &mut Panics, || {})).join();
    assert!(joined.is_err(), "the panic is raised once the worker ended");
    let finished = opener.take_finished();
    assert_eq!(finished.len(), 2);
    assert_eq!(finished[0].1, Err(OpenRefusal::Failed));
    assert_eq!(finished[1].1, Ok(file(44)));
}
