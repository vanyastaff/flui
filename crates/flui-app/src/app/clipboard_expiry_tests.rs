//! The expiring entries' rule over a scripted host clipboard: the operating
//! system's answers (which entry it still holds, a clear it could not do) are
//! the test's inputs, and a manual clock stands in for the loop's.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock};
use flui_native::{ExpiredClear, ExpiredEntry, NativeClipboard};
use flui_platform::capability::{
    CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions, Resolver,
};
use flui_platform::{
    ClipboardEntryId, ClipboardError, ClipboardExpiry, ClipboardRead, ClipboardWrite,
    ClipboardWritten, Concealment, ExpiryClearing,
};

use super::{CLEAR_RETRY, ClipboardExpiries, MAX_CLEAR_RETRIES};

/// A host clipboard whose operating system the test speaks for: the entry of
/// this application's it holds, and the answers it gives a clear.
#[derive(Default)]
struct ScriptedHost {
    /// The expiring entry the clipboard holds; `None` once anything else was
    /// copied over it or it was cleared.
    held: Cell<Option<ClipboardEntryId>>,
    /// Answers the next clears give, before the clipboard is looked at.
    refusals: RefCell<VecDeque<ExpiredClear>>,
    asked: RefCell<Vec<ExpiredEntry>>,
    exits: Cell<u32>,
    unavailable: Cell<bool>,
    /// The entry whose clear panics, as a host's code may.
    panics_at: Cell<Option<ClipboardEntryId>>,
}

impl ScriptedHost {
    /// Another application copies: no entry of ours is there any more.
    fn copy_from_elsewhere(&self) {
        self.held.set(None);
    }

    fn clear(&self, entry: ExpiredEntry) -> ExpiredClear {
        self.asked.borrow_mut().push(entry);
        if let (ExpiredEntry::Entry(entry), Some(panics)) = (entry, self.panics_at.get())
            && entry == panics
        {
            panic!("the host's clear panicked");
        }
        if let Some(refusal) = self.refusals.borrow_mut().pop_front() {
            return refusal;
        }
        match self.held.get() {
            Some(held) if entry.names(held) => {
                self.held.set(None);
                ExpiredClear::Cleared
            }
            _ => ExpiredClear::NotHeld,
        }
    }
}

impl NativeClipboard for ScriptedHost {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        let _ = answer.resolve(Ok(None));
    }

    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        self.held.set(write.entry());
        let _ = answer.resolve(Ok(()));
    }

    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }

    fn expiry_clearing(&self) -> ExpiryClearing {
        if self.unavailable.get() {
            ExpiryClearing::Unavailable
        } else {
            ExpiryClearing::AtTimeAndLock
        }
    }

    fn clear_expired(&self, entry: ExpiredEntry, answer: Resolver<ExpiredClear>) {
        let _ = answer.resolve(self.clear(entry));
    }

    fn clear_expired_before_exit(&self, _within: Duration) -> ExpiredClear {
        self.exits.set(self.exits.get() + 1);
        self.clear(ExpiredEntry::Any)
    }
}

/// The owner a writer's answers settle on: its turn runs what was posted.
struct Owner {
    jobs: mpsc::Receiver<OwnerJob>,
    pending: PendingCompletions,
    completions: CompletionSource,
}

impl Owner {
    fn new() -> Self {
        let (sink, jobs) = mpsc::channel::<OwnerJob>();
        let poster = OwnerPoster::new(move |job| sink.send(job).map_err(|_| OwnerRetired));
        let pending = PendingCompletions::new();
        let completions = CompletionSource::new(poster, pending.clone());
        Self {
            jobs,
            pending,
            completions,
        }
    }

    fn turn(&self) {
        let scope = OwnerScope::new(&self.pending);
        while let Ok(job) = self.jobs.try_recv() {
            job.run(&scope);
        }
    }
}

/// One loop's clipboard under test.
struct Run {
    host: Rc<ScriptedHost>,
    clock: ManualClock,
    owner: Owner,
    expiries: Rc<ClipboardExpiries>,
}

impl Run {
    fn new() -> Self {
        let host = Rc::new(ScriptedHost::default());
        let clock = ManualClock::new();
        let expiries = ClipboardExpiries::new(
            Rc::clone(&host) as Rc<dyn NativeClipboard>,
            Rc::new(clock.clone()) as Rc<dyn MonotonicClock>,
        );
        Self {
            host,
            clock,
            owner: Owner::new(),
            expiries,
        }
    }

    /// A handle writes `write`, as the built-in provider passes it on.
    fn write(&self, write: ClipboardWrite) -> Option<ClipboardEntryId> {
        let entry = write.entry();
        self.expiries.wrote(&write, &self.owner.completions);
        let (_completion, answer) = self.owner.completions.pending::<ClipboardWritten>();
        self.host.write(write, answer);
        entry
    }

    /// The loop's turn at `elapsed` from the start, as its turn deadline gives.
    fn turn_at(&self, elapsed: Duration) {
        let step = elapsed
            .checked_sub(self.clock.elapsed())
            .expect("the clock only goes forward");
        self.clock.advance(step);
        self.expiries.serve();
        self.owner.turn();
    }

    fn due_in(&self) -> Option<Duration> {
        self.expiries
            .next_due()
            .map(|due| due.saturating_duration_since(self.clock.now()))
    }

    fn asked(&self) -> Vec<ExpiredEntry> {
        self.host.asked.borrow().clone()
    }
}

fn expiring(secs: u64) -> ClipboardWrite {
    ClipboardWrite::text("hunter2")
        .expires_after(ClipboardExpiry::try_new(Duration::from_secs(secs)).expect("a valid expiry"))
}

const fn secs(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

/// An expiring write owes one clear at its time: the turn deadline names the
/// time, a turn before it asks nothing, the turn at it clears the entry, and a
/// served clear is not owed again.
#[test]
fn a_clear_is_owed_at_its_time_and_served_once() {
    let run = Run::new();
    let entry = run.write(expiring(30)).expect("an expiring write");
    assert_eq!(run.due_in(), Some(secs(30)));
    run.turn_at(secs(29));
    assert_eq!(
        run.asked(),
        Vec::<ExpiredEntry>::new(),
        "nothing is due before its time"
    );
    run.turn_at(secs(30));
    assert_eq!(run.asked(), [ExpiredEntry::Entry(entry)]);
    assert_eq!(run.host.held.get(), None, "the entry is cleared");
    assert_eq!(run.due_in(), None, "a served clear is owed no more");
    run.turn_at(secs(90));
    assert_eq!(run.asked().len(), 1);
}

/// A copy another application made after the write is not the entry: the
/// clear at the time names the write's entry, and the host leaves the copy.
#[test]
fn another_applications_copy_survives_the_time() {
    let run = Run::new();
    let entry = run.write(expiring(30)).expect("an expiring write");
    run.host.copy_from_elsewhere();
    run.turn_at(secs(30));
    assert_eq!(run.asked(), [ExpiredEntry::Entry(entry)]);
    assert_eq!(
        run.due_in(),
        None,
        "nothing more is owed for a replaced entry"
    );
}

/// A second expiring write replaces the first's time: the first's clear names
/// the first entry and does nothing to the second, which clears at its own.
#[test]
fn a_second_expiring_write_replaces_the_firsts_time() {
    let run = Run::new();
    let first = run.write(expiring(30)).expect("an expiring write");
    run.turn_at(secs(10));
    let second = run.write(expiring(30)).expect("an expiring write");
    run.turn_at(secs(30));
    assert_eq!(run.asked(), [ExpiredEntry::Entry(first)]);
    assert_eq!(run.host.held.get(), Some(second), "the newer entry stays");
    assert_eq!(run.due_in(), Some(secs(10)));
    run.turn_at(secs(40));
    assert_eq!(
        run.asked(),
        [ExpiredEntry::Entry(first), ExpiredEntry::Entry(second)]
    );
    assert_eq!(run.host.held.get(), None);
}

/// A write that does not expire owes nothing, and an expiring one owes
/// nothing where the host cannot clear only what is still there.
#[test]
fn nothing_is_owed_without_an_expiry_or_a_conditional_clear() {
    let run = Run::new();
    run.write(ClipboardWrite::text("plain"));
    run.write(ClipboardWrite::text("secret").concealed());
    assert_eq!(run.due_in(), None);
    run.host.unavailable.set(true);
    run.write(expiring(30));
    assert_eq!(run.due_in(), None, "the web owes no clear it cannot make");
    run.expiries.session_locked();
    assert_eq!(run.asked(), Vec::<ExpiredEntry>::new());
}

/// A session lock asks every owed clear now, each of its own entry, and
/// leaves nothing owed for the time.
#[test]
fn a_session_lock_clears_now() {
    let run = Run::new();
    let first = run.write(expiring(30)).expect("an expiring write");
    let second = run.write(expiring(60)).expect("an expiring write");
    run.turn_at(secs(5));
    run.expiries.session_locked();
    run.owner.turn();
    assert_eq!(
        run.asked(),
        [ExpiredEntry::Entry(first), ExpiredEntry::Entry(second)]
    );
    assert_eq!(run.host.held.get(), None, "the entry still held is cleared");
    assert_eq!(run.due_in(), None);
}

/// A host whose clear panics at one entry still gets every other clear owed
/// at that turn, and the panic reaches the turn once all were asked.
#[test]
fn a_panicking_clear_costs_no_other_clear() {
    let run = Run::new();
    let first = run.write(expiring(30)).expect("an expiring write");
    let second = run.write(expiring(30)).expect("an expiring write");
    run.host.panics_at.set(Some(first));
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test catches the host's panic to assert what was asked"
    )]
    let lock = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run.expiries.session_locked();
    }));
    assert!(lock.is_err(), "the host's panic reaches the turn");
    run.owner.turn();
    assert_eq!(
        run.asked(),
        [ExpiredEntry::Entry(first), ExpiredEntry::Entry(second)]
    );
    assert_eq!(run.host.held.get(), None, "the second entry is cleared");
}

/// A clear the host could not do for now is owed again on the retry
/// schedule, and given up after the last retry; one the host refused for
/// good is not tried again.
#[test]
fn a_clear_the_host_could_not_do_is_tried_again() {
    let run = Run::new();
    run.write(expiring(30)).expect("an expiring write");
    run.host.refusals.borrow_mut().extend([
        ExpiredClear::Failed(ClipboardError::Unavailable),
        ExpiredClear::Unseen,
    ]);
    run.turn_at(secs(30));
    assert_eq!(run.due_in(), Some(CLEAR_RETRY.delay(0)));
    run.turn_at(secs(30) + CLEAR_RETRY.delay(0));
    assert_eq!(run.due_in(), Some(CLEAR_RETRY.delay(1)));
    run.turn_at(secs(30) + CLEAR_RETRY.delay(0) + CLEAR_RETRY.delay(1));
    assert_eq!(run.asked().len(), 3);
    assert_eq!(run.host.held.get(), None, "the third attempt cleared it");
    assert_eq!(run.due_in(), None);

    let run = Run::new();
    run.write(expiring(30)).expect("an expiring write");
    let retries = usize::try_from(MAX_CLEAR_RETRIES).expect("a small count") + 1;
    run.host
        .refusals
        .borrow_mut()
        .extend(std::iter::repeat_n(ExpiredClear::Unseen, retries));
    run.turn_at(secs(30));
    let mut at = secs(30);
    for misses in 0..MAX_CLEAR_RETRIES {
        at += CLEAR_RETRY.delay(misses);
        run.turn_at(at);
    }
    assert_eq!(run.asked().len(), retries);
    assert_eq!(run.due_in(), None, "given up after the last retry");

    let run = Run::new();
    run.write(expiring(30)).expect("an expiring write");
    run.host
        .refusals
        .borrow_mut()
        .push_back(ExpiredClear::Failed(ClipboardError::Gone));
    run.turn_at(secs(30));
    assert_eq!(
        run.due_in(),
        None,
        "a clipboard that is gone is not retried"
    );
}

/// The loop's end asks the host for whatever entry is still there while a
/// clear is owed, and asks nothing when none is.
#[test]
fn the_loop_end_clears_an_owed_entry() {
    let run = Run::new();
    assert_eq!(run.expiries.before_exit(secs(1)), None);
    assert_eq!(run.host.exits.get(), 0);
    run.write(expiring(30)).expect("an expiring write");
    assert_eq!(
        run.expiries.before_exit(secs(1)),
        Some(ExpiredClear::Cleared)
    );
    assert_eq!(run.host.exits.get(), 1);
    assert_eq!(run.due_in(), None);
}
