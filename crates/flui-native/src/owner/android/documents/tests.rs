//! The documents an intent names, opened off the owner thread: the owner's
//! turn never waits for a provider, the intents arrive in order, and every
//! accepted document ends with exactly one outcome on every exit.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use flui_platform::{Launch, OpenedFile};

use super::{
    Busy, CancelSignal, DocumentProvider, Documents, MAX_WAITING_INTENTS, OPEN_DEADLINE,
    OpenRefusal, Opener, Openings, Outcome, Ready, work,
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

/// A provider whose every open is `open`, counting the opens it entered.
struct Provider<F> {
    open: F,
    entered: Arc<AtomicUsize>,
}

impl<F: FnMut(&str) -> Outcome> DocumentProvider for Provider<F> {
    type Signal = Flag;

    fn signal(&mut self) -> Result<Flag, OpenRefusal> {
        Ok(Flag::default())
    }

    fn open(&mut self, uri: &str, _signal: &Flag) -> Outcome {
        self.entered.fetch_add(1, Ordering::SeqCst);
        (self.open)(uri)
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
    if let Some(signal) = turn.cancel {
        signal.cancel();
    }
    turn.ready
}

/// Turns of the owner until one hands intents over or `within` passes.
fn turns_until_ready(documents: &mut Documents<Flag>, within: Duration) -> Vec<Ready> {
    let until = Instant::now() + within;
    loop {
        let ready = turn(documents, Instant::now());
        if !ready.is_empty() || Instant::now() >= until {
            return ready;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A provider that answers only after a while (a cloud provider that
/// downloads first) holds no turn of the owner: accepting the intent and the
/// turns after it return while the open is still running, the link-only
/// intent that arrived after it waits behind it, and both arrive in order
/// once the document opened.
#[test]
fn a_slow_provider_holds_no_owner_turn() {
    let (release, released) = mpsc::channel::<()>();
    let entered = Arc::new(AtomicUsize::new(0));
    let provider_entered = Arc::clone(&entered);
    let mut documents = Documents::<Flag>::default();
    let spawn = move |opener: Arc<Opener<Flag>>| {
        std::thread::Builder::new()
            .name("flui-test-documents".to_owned())
            .spawn(move || {
                let mut provider = Provider {
                    open: |_: &str| {
                        released
                            .recv_timeout(Duration::from_secs(10))
                            .map(|()| file(40))
                            .map_err(|_| OpenRefusal::Failed)
                    },
                    entered: provider_entered,
                };
                work(&opener, &mut provider, || {});
            })
            .map(drop)
    };

    let started = Instant::now();
    documents
        .accept(content(&["content://notes/doc/1"]), started, spawn)
        .expect("room");
    documents
        .accept(link("notes:///note/2"), started, |_| {
            unreachable!("a link starts no worker")
        })
        .expect("room");
    let ready = turn(&mut documents, Instant::now());
    assert!(ready.is_empty(), "the link waits behind the document");
    let until = Instant::now() + Duration::from_secs(5);
    while entered.load(Ordering::SeqCst) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "the worker is inside the open"
    );
    assert!(turn(&mut documents, Instant::now()).is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "no owner call waited for the provider"
    );
    assert_eq!(
        documents.next_deadline(),
        Some(started + OPEN_DEADLINE),
        "the loop wakes for the deadline"
    );

    release.send(()).expect("the worker waits");
    let mut ready = turns_until_ready(&mut documents, Duration::from_secs(10));
    if ready.len() < 2 {
        ready.extend(turns_until_ready(&mut documents, Duration::from_secs(10)));
    }
    assert_eq!(ready.len(), 2, "both intents, once");
    assert_eq!(ready[0].opened.files, [file(40)]);
    assert_eq!(ready[0].refused, Vec::new());
    assert_eq!(ready[1].opened.urls, ["notes:///note/2"]);
    assert_eq!(documents.next_deadline(), None);
}

/// Each way a document can end gives it one outcome, and its intent is
/// delivered once every document has one: opened, the provider's failure,
/// the read grant ended, the deadline passed (its open cancelled, and what it
/// answers late dropped), the platform retired; past the intents that may
/// wait, an intent is turned away whole.
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
    openings.finish(jobs[0].ticket, Ok(file(41)));
    openings.finish(jobs[1].ticket, Err(OpenRefusal::GrantEnded));
    assert!(openings.take_ready().is_empty(), "one still opens");
    let just_before = now + OPEN_DEADLINE.saturating_sub(Duration::from_millis(1));
    assert_eq!(openings.expire(just_before), Vec::new());
    assert_eq!(openings.expire(now + OPEN_DEADLINE), [jobs[2].ticket]);
    // What the cancelled open answers late is dropped.
    openings.finish(jobs[2].ticket, Ok(file(42)));
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
    openings.finish(jobs[0].ticket, Err(OpenRefusal::Failed));
    assert_eq!(openings.take_ready()[0].refused, [OpenRefusal::Failed]);

    // The intents that may wait are bounded; the next is turned away.
    for n in 0..MAX_WAITING_INTENTS {
        openings
            .accept(content(&[&format!("content://x/{n}")]), now)
            .expect("room");
    }
    assert_eq!(openings.accept(link("notes:///late"), now), Err(Busy));

    // Retirement refuses every document still waiting, in order.
    let ready = openings.retire();
    assert_eq!(ready.len(), MAX_WAITING_INTENTS);
    assert!(
        ready
            .iter()
            .all(|ready| ready.refused == [OpenRefusal::Retired])
    );
    assert!(openings.take_ready().is_empty());
}

/// The worker's side of each exit: a refused document's queued job is never
/// opened, the open in progress is cancelled through the signal it was
/// opened with and its late answer dropped, retirement cancels the open in
/// progress and opens nothing more, and a worker that cannot start refuses
/// its jobs.
#[test]
fn a_refused_document_is_withdrawn_and_its_open_cancelled() {
    let now = Instant::now();

    // No thread: every job is refused, and the intent still arrives.
    let mut documents = Documents::<Flag>::default();
    documents
        .accept(content(&["content://x/1"]), now, |_| {
            Err(std::io::Error::other("no thread"))
        })
        .expect("room");
    let ready = turn(&mut documents, now);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].refused, [OpenRefusal::NoThread]);

    // The deadline passes while the first open blocks and the second waits.
    let mut documents = Documents::<Flag>::default();
    let (release, released) = mpsc::channel::<()>();
    let (signals, signal_of) = mpsc::channel::<Flag>();
    let entered = Arc::new(AtomicUsize::new(0));
    let worker_entered = Arc::clone(&entered);
    let worker = std::cell::Cell::new(None);
    documents
        .accept(
            content(&["content://x/1", "content://x/2"]),
            now,
            |opener| {
                let thread = std::thread::spawn(move || {
                    let mut provider = SignalledProvider {
                        released,
                        signals,
                        entered: worker_entered,
                    };
                    work(&opener, &mut provider, || {});
                });
                worker.set(Some(thread));
                Ok(())
            },
        )
        .expect("room");
    let signal = signal_of
        .recv_timeout(Duration::from_secs(5))
        .expect("the first open began");
    let ready = turn(&mut documents, now + OPEN_DEADLINE);
    assert!(signal.is_cancelled(), "the open in progress is cancelled");
    assert_eq!(ready.len(), 1);
    assert_eq!(
        ready[0].refused,
        [OpenRefusal::TimedOut, OpenRefusal::TimedOut]
    );
    release.send(()).expect("the worker waits");
    worker
        .take()
        .expect("started")
        .join()
        .expect("the worker ends");
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "the withdrawn job is never opened"
    );
    assert!(
        turn(&mut documents, now + OPEN_DEADLINE).is_empty(),
        "the late answer is dropped"
    );

    // The platform retires while a document opens: its open is cancelled
    // and its intent let go of with the document refused.
    let mut documents = Documents::<Flag>::default();
    let (release, released) = mpsc::channel::<()>();
    let (signals, signal_of) = mpsc::channel::<Flag>();
    let worker = std::cell::Cell::new(None);
    documents
        .accept(content(&["content://x/9"]), now, |opener| {
            worker.set(Some(std::thread::spawn(move || {
                let mut provider = SignalledProvider {
                    released,
                    signals,
                    entered: Arc::default(),
                };
                work(&opener, &mut provider, || {});
            })));
            Ok(())
        })
        .expect("room");
    let signal = signal_of
        .recv_timeout(Duration::from_secs(5))
        .expect("the open began");
    let retired = documents.retire();
    retired.cancel.expect("an open in progress").cancel();
    assert!(signal.is_cancelled());
    assert_eq!(retired.ready.len(), 1);
    assert_eq!(retired.ready[0].refused, [OpenRefusal::Retired]);
    release.send(()).expect("the worker waits");
    worker
        .take()
        .expect("started")
        .join()
        .expect("the worker ends");

    // Retirement cancels the open in progress and opens nothing more.
    let opener = Opener::<Flag>::default();
    assert!(opener.enqueue(vec![super::Job {
        ticket: std::num::NonZeroU64::MIN,
        uri: "content://x/1".to_owned(),
    }]));
    let job = opener.next().expect("queued");
    let signal = Flag::default();
    assert!(opener.enter(Arc::new(signal.clone())));
    let cancelled = opener.retire().expect("an open in progress");
    cancelled.cancel();
    assert!(signal.is_cancelled());
    opener.finish(job.ticket, Ok(file(43)));
    assert!(opener.next().is_none(), "nothing more after retirement");
    assert!(!opener.enqueue(vec![super::Job {
        ticket: std::num::NonZeroU64::MAX,
        uri: "content://x/2".to_owned(),
    }]));
    let finished = opener.take_finished();
    assert_eq!(finished.len(), 2);
    assert_eq!(finished[1].1, Err(OpenRefusal::Retired));
}

/// A provider that hands the test the signal of each open it entered and
/// answers only once the test releases it.
struct SignalledProvider {
    released: mpsc::Receiver<()>,
    signals: mpsc::Sender<Flag>,
    entered: Arc<AtomicUsize>,
}

impl DocumentProvider for SignalledProvider {
    type Signal = Flag;

    fn signal(&mut self) -> Result<Flag, OpenRefusal> {
        Ok(Flag::default())
    }

    fn open(&mut self, _uri: &str, signal: &Flag) -> Outcome {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let _ = self.signals.send(signal.clone());
        self.released
            .recv_timeout(Duration::from_secs(10))
            .map(|()| file(45))
            .map_err(|_| OpenRefusal::Failed)
    }
}

/// A provider that panics fails its own job alone: the worker opens the next
/// one, hands back both outcomes, and raises the panic once it has ended.
#[test]
fn a_provider_panic_fails_its_job_alone() {
    let opener = Arc::new(Opener::<Flag>::default());
    assert!(opener.enqueue(vec![
        super::Job {
            ticket: std::num::NonZeroU64::MIN,
            uri: "content://x/panics".to_owned(),
        },
        super::Job {
            ticket: std::num::NonZeroU64::MAX,
            uri: "content://x/opens".to_owned(),
        },
    ]));
    let worker = Arc::clone(&opener);
    let joined = std::thread::spawn(move || {
        let mut provider = Provider {
            open: |uri: &str| {
                assert!(!uri.ends_with("panics"), "a provider panics");
                Ok(file(44))
            },
            entered: Arc::default(),
        };
        work(&worker, &mut provider, || {});
    })
    .join();
    assert!(joined.is_err(), "the panic is raised once the worker ended");
    let finished = opener.take_finished();
    assert_eq!(finished.len(), 2);
    assert_eq!(finished[0].1, Err(OpenRefusal::Failed));
    assert_eq!(finished[1].1, Ok(file(44)));
}
