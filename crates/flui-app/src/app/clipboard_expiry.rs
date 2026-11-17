//! Expiring clipboard entries (ADR-0363): when the host is asked to clear the
//! entry of an expiring write, if the clipboard still holds it.
//!
//! The rule lives here, at the seam, for every host; a host knows only how to
//! clear an entry it recorded while the operating system still shows it
//! ([`NativeClipboard::clear_expired`]). Every expiring write the
//! application's handles make is owed one clear, at the time it asked for,
//! counted from the write's request. The clears are served at the owner turn
//! the loop's turn deadline gives (`AppHandler::turn_deadline`, ADR-0343 §1),
//! so nothing polls: [`ClipboardExpiries::next_due`] is part of that deadline,
//! and a turn serves every clear due by its own time, so a served clear is not
//! owed again.
//!
//! Each clear names its own write's entry. A later write that lands, an
//! expiring one of this application, an ordinary one or another
//! application's, takes the clipboard from the entry, and the host answers
//! [`ExpiredClear::NotHeld`]: an older clear never takes a newer entry, so a
//! second expiring write replaces the first's time, and the first's clear does
//! nothing. Where the second write failed, the first entry is still there and
//! its own clear takes it at its time.
//!
//! A session lock, or the machine going to sleep, asks the host to clear
//! whatever expiring entry of the application's it still holds
//! ([`ExpiredEntry::Any`]), once any expiring write was made, whatever is
//! still owed; so does the loop's end, within its own budget
//! ([`NativeClipboard::clear_expired_before_exit`]). A clear the host could not
//! see the clipboard for ([`ExpiredClear::Unseen`]: Android, for an
//! application without input focus) stays owed until the application is next
//! in front ([`ClipboardExpiries::in_front`]); one the host could not reach
//! the clipboard for is owed again on [`CLEAR_RETRY`], at most
//! [`MAX_CLEAR_RETRIES`] times.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_foundation::MonotonicClock;
use flui_foundation::panic::FirstPanic;
use flui_native::{ExpiredClear, ExpiredEntry, NativeClipboard};
use flui_platform::capability::CompletionSource;
use flui_platform::{Backoff, ClipboardEntryId, ClipboardWrite, ExpiryClearing};
use web_time::Instant;

/// When a clear the host could not reach the clipboard for is tried again:
/// one second after the first miss, doubling, at most a minute apart.
pub(crate) const CLEAR_RETRY: Backoff =
    Backoff::new(Duration::from_secs(1), Duration::from_secs(60));

/// How many times a clear the host could not reach the clipboard for is tried
/// again before it is given up with a warning: with [`CLEAR_RETRY`], the last
/// attempt comes 183 seconds after the first.
pub(crate) const MAX_CLEAR_RETRIES: u32 = 8;

/// The most clears owed at once. Each expiring write owes one until its time;
/// past this many the oldest is given up, whose entry the writes after it
/// replaced unless every one of them failed; a lock and the loop's end still
/// clear it.
pub(crate) const MAX_OWED: usize = 16;

/// The expiring entries of one loop's clipboard: the clears owed, and the host
/// clipboard that serves them.
pub(crate) struct ClipboardExpiries {
    native: Rc<dyn NativeClipboard>,
    clock: RefCell<Rc<dyn MonotonicClock>>,
    owed: RefCell<Vec<Owed>>,
    /// How many expiring writes the handles made: a lock's clear is
    /// [`ExpiredEntry::Any`], valid only while no later one was made.
    expiring_writes: Cell<u64>,
    /// Where the latest expiring write's answers settle, which a lock's clear
    /// answers through; `None` until an expiring write is made.
    latest_writer: RefCell<Option<CompletionSource>>,
}

/// One clear owed: what it clears, when, how many attempts missed, and where
/// its answer settles.
struct Owed {
    which: Which,
    due: Due,
    misses: u32,
    completions: CompletionSource,
}

/// What an owed clear clears.
#[derive(Clone, Copy, Debug)]
enum Which {
    /// The entry this write made, at its time.
    Entry(ClipboardEntryId),
    /// Whatever expiring entry the host holds, asked at a lock while
    /// `expiring_writes` was `writes`: a later expiring write makes it stale,
    /// so an older wake never clears a newer entry.
    Any { writes: u64 },
}

impl Which {
    fn entry(self) -> ExpiredEntry {
        match self {
            Self::Entry(entry) => ExpiredEntry::Entry(entry),
            Self::Any { .. } => ExpiredEntry::Any,
        }
    }
}

/// When an owed clear is asked.
#[derive(Clone, Copy, Debug)]
enum Due {
    /// At this instant, through the turn deadline.
    At(Instant),
    /// When the application is next in front: the host could not see the
    /// clipboard before. No deadline wakes the loop for it.
    InFront,
}

impl ClipboardExpiries {
    /// The expiring entries of `native`, timed on `clock`.
    pub(crate) fn new(native: Rc<dyn NativeClipboard>, clock: Rc<dyn MonotonicClock>) -> Rc<Self> {
        Rc::new(Self {
            native,
            clock: RefCell::new(clock),
            owed: RefCell::new(Vec::new()),
            expiring_writes: Cell::new(0),
            latest_writer: RefCell::new(None),
        })
    }

    /// Time the entries on `clock` from now on.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn set_clock(&self, clock: Rc<dyn MonotonicClock>) {
        self.clock.replace(clock);
    }

    fn now(&self) -> Instant {
        let clock = Rc::clone(&self.clock.borrow());
        clock.now()
    }

    /// The host clipboard the entries are cleared on.
    pub(crate) fn native(&self) -> &Rc<dyn NativeClipboard> {
        &self.native
    }

    /// A handle is about to ask the host for `write`: an expiring write owes
    /// a clear at its time from now, answered through `completions`. Nothing
    /// is owed where the host has no conditional clear.
    pub(crate) fn wrote(&self, write: &ClipboardWrite, completions: &CompletionSource) {
        let (Some(entry), Some(expiry)) = (write.entry(), write.expiry()) else {
            return;
        };
        if self.native.expiry_clearing() == ExpiryClearing::Unavailable {
            return;
        }
        self.expiring_writes.set(
            self.expiring_writes
                .get()
                .checked_add(1)
                .expect("BUG: a loop makes fewer than 2^64 expiring clipboard writes"),
        );
        self.latest_writer.replace(Some(completions.clone()));
        // `ClipboardExpiry` is at most a day, which no clock overflows.
        let due = self.now() + expiry.get();
        let given_up = {
            let mut owed = self.owed.borrow_mut();
            owed.push(Owed {
                which: Which::Entry(entry),
                due: Due::At(due),
                misses: 0,
                completions: completions.clone(),
            });
            (owed.len() > MAX_OWED).then(|| owed.remove(0))
        };
        if let Some(given_up) = given_up {
            tracing::debug!(
                which = ?given_up.which,
                "more than {MAX_OWED} expiring clipboard writes owed a clear: the oldest is given up"
            );
        }
    }

    /// The earliest owed clear's time, part of the loop's turn deadline;
    /// `None` when nothing is owed at a time.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.owed
            .borrow()
            .iter()
            .filter_map(|owed| match owed.due {
                Due::At(at) => Some(at),
                Due::InFront => None,
            })
            .min()
    }

    /// The owner's turn: ask the host for every clear due by now.
    pub(crate) fn serve(self: &Rc<Self>) {
        let now = self.now();
        let due = self.take(|due| matches!(due, Due::At(at) if at <= now));
        self.ask_each(due);
    }

    /// The application is in front again: ask the host for every clear it
    /// could not see the clipboard for.
    pub(crate) fn in_front(self: &Rc<Self>) {
        let waiting = self.take(|due| matches!(due, Due::InFront));
        self.ask_each(waiting);
    }

    /// The user's session locked, or the machine is going to sleep: once any
    /// expiring write was made, the host is asked to clear whatever expiring
    /// entry it still holds, and nothing else is owed.
    pub(crate) fn session_locked(self: &Rc<Self>) {
        let owed = std::mem::take(&mut *self.owed.borrow_mut());
        drop(owed);
        let Some(completions) = self.latest_writer.borrow().clone() else {
            return;
        };
        let writes = self.expiring_writes.get();
        self.ask_each(vec![Owed {
            which: Which::Any { writes },
            due: Due::At(self.now()),
            misses: 0,
            completions,
        }]);
    }

    /// The owed clears whose time `due` says has come, out of the list.
    fn take(&self, due: impl Fn(Due) -> bool) -> Vec<Owed> {
        let mut owed = self.owed.borrow_mut();
        let (taken, later) = std::mem::take(&mut *owed)
            .into_iter()
            .partition(|owed| due(owed.due));
        *owed = later;
        taken
    }

    /// Ask the host for each of `owed`, every one asked though the host
    /// panics at one; the first panic resumes once all were asked.
    fn ask_each(self: &Rc<Self>, owed: Vec<Owed>) {
        let mut first = FirstPanic::new();
        for owed in owed {
            first.contain(|| self.ask(owed));
        }
        first.resume();
    }

    /// The loop ends: once any expiring write was made, ask the host to clear
    /// whatever expiring entry it still holds, waiting at most `within`.
    /// `None` when no expiring write was made. The web's page has no loop end
    /// to ask at.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn before_exit(&self, within: Duration) -> Option<ExpiredClear> {
        drop(std::mem::take(&mut *self.owed.borrow_mut()));
        self.latest_writer.borrow().as_ref()?;
        let cleared = self.native.clear_expired_before_exit(within);
        tracing::debug!(
            ?cleared,
            "the loop's end asked for the expiring clipboard entry's clear"
        );
        Some(cleared)
    }

    /// Ask the host to clear what `owed` names if it is still there, with
    /// nothing borrowed: the host's answer may come at once. A lock's clear
    /// made stale by a later expiring write is dropped.
    fn ask(self: &Rc<Self>, owed: Owed) {
        let Owed {
            which,
            misses,
            completions,
            ..
        } = owed;
        if self.is_stale(which) {
            return;
        }
        let (completion, answer) = completions.pending::<ExpiredClear>();
        let expiries: Weak<Self> = Rc::downgrade(self);
        completion.on_complete(move |answered| {
            // A UI runtime that retired took the answer with it: the host
            // still did the clear.
            let (Ok(cleared), Some(expiries)) = (answered, expiries.upgrade()) else {
                return;
            };
            expiries.answered(which, misses, completions.clone(), cleared);
        });
        self.native.clear_expired(which.entry(), answer);
    }

    /// Whether `which` is a lock's clear a later expiring write made stale.
    fn is_stale(&self, which: Which) -> bool {
        matches!(which, Which::Any { writes } if writes != self.expiring_writes.get())
    }

    /// The host answered the clear of `which` after `misses` attempts missed.
    fn answered(
        &self,
        which: Which,
        misses: u32,
        completions: CompletionSource,
        cleared: ExpiredClear,
    ) {
        if cleared == ExpiredClear::Unseen && !self.is_stale(which) {
            // Owed until the application can see the clipboard, which no
            // time decides.
            self.owed.borrow_mut().push(Owed {
                which,
                due: Due::InFront,
                misses,
                completions,
            });
            return;
        }
        if !cleared.is_worth_a_retry() {
            if matches!(cleared, ExpiredClear::Cleared | ExpiredClear::NotHeld) {
                tracing::debug!(?which, ?cleared, "an expiring clipboard entry's clear");
            } else {
                tracing::warn!(
                    ?which,
                    ?cleared,
                    "an expiring clipboard entry could not be cleared"
                );
            }
            return;
        }
        if misses >= MAX_CLEAR_RETRIES {
            tracing::warn!(
                ?which,
                ?cleared,
                "an expiring clipboard entry could not be cleared after {MAX_CLEAR_RETRIES} retries"
            );
            return;
        }
        let due = self.now() + CLEAR_RETRY.delay(misses);
        self.owed.borrow_mut().push(Owed {
            which,
            due: Due::At(due),
            misses: misses + 1,
            completions,
        });
    }
}

#[cfg(test)]
#[path = "clipboard_expiry_tests.rs"]
mod tests;
