//! Expiring clipboard entries (ADR-0361): when the host is asked to clear the
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
//! A session lock, or the machine going to sleep, serves every owed clear
//! now. The loop's end asks the host to clear whatever entry is still there
//! within the exit's budget ([`NativeClipboard::clear_expired_before_exit`]).
//! A clear the host could not do for now (the clipboard held by another
//! process, out of sight in the background) is owed again on
//! [`CLEAR_RETRY`], at most [`MAX_CLEAR_RETRIES`] times.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_foundation::MonotonicClock;
use flui_native::{ExpiredClear, ExpiredEntry, NativeClipboard};
use flui_platform::capability::CompletionSource;
use flui_platform::{Backoff, ClipboardEntryId, ClipboardWrite, ExpiryClearing};
use web_time::Instant;

/// When a clear the host could not do for now is tried again: one second
/// after the first miss, doubling, at most a minute apart.
pub(crate) const CLEAR_RETRY: Backoff =
    Backoff::new(Duration::from_secs(1), Duration::from_secs(60));

/// How many times a clear the host could not do is tried again before it is
/// given up with a warning: with [`CLEAR_RETRY`], about four minutes.
pub(crate) const MAX_CLEAR_RETRIES: u32 = 8;

/// The most clears owed at once. Each expiring write owes one until its time;
/// past this many the oldest is given up, whose entry the writes after it
/// replaced unless every one of them failed.
pub(crate) const MAX_OWED: usize = 16;

/// The expiring entries of one loop's clipboard: the clears owed, and the host
/// clipboard that serves them.
pub(crate) struct ClipboardExpiries {
    native: Rc<dyn NativeClipboard>,
    clock: Rc<dyn MonotonicClock>,
    owed: RefCell<Vec<Owed>>,
}

/// One clear owed: the entry, when it is due, how many attempts missed, and
/// where its answer settles (the writer's UI runtime).
struct Owed {
    entry: ClipboardEntryId,
    due: Instant,
    misses: u32,
    completions: CompletionSource,
}

impl ClipboardExpiries {
    /// The expiring entries of `native`, timed on `clock`.
    pub(crate) fn new(native: Rc<dyn NativeClipboard>, clock: Rc<dyn MonotonicClock>) -> Rc<Self> {
        Rc::new(Self {
            native,
            clock,
            owed: RefCell::new(Vec::new()),
        })
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
        // `ClipboardExpiry` is at most a day, which no clock overflows.
        let due = self.clock.now() + expiry.get();
        let given_up = {
            let mut owed = self.owed.borrow_mut();
            owed.push(Owed {
                entry,
                due,
                misses: 0,
                completions: completions.clone(),
            });
            (owed.len() > MAX_OWED).then(|| owed.remove(0))
        };
        if let Some(given_up) = given_up {
            tracing::debug!(
                entry = given_up.entry.get(),
                "more than {MAX_OWED} expiring clipboard writes owed a clear: the oldest is given up"
            );
        }
    }

    /// The earliest owed clear's time, part of the loop's turn deadline;
    /// `None` when nothing is owed, or while a turn holds the list (it asks
    /// again).
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.owed
            .try_borrow()
            .ok()?
            .iter()
            .map(|owed| owed.due)
            .min()
    }

    /// The owner's turn: ask the host for every clear due by now.
    pub(crate) fn serve(self: &Rc<Self>) {
        let now = self.clock.now();
        let due: Vec<Owed> = {
            let mut owed = self.owed.borrow_mut();
            let (due, later) = std::mem::take(&mut *owed)
                .into_iter()
                .partition(|owed| owed.due <= now);
            *owed = later;
            due
        };
        for owed in due {
            self.ask(owed);
        }
    }

    /// The user's session locked, or the machine is going to sleep: every
    /// owed clear is asked now, each of its own entry.
    pub(crate) fn session_locked(self: &Rc<Self>) {
        let owed = std::mem::take(&mut *self.owed.borrow_mut());
        for owed in owed {
            self.ask(owed);
        }
    }

    /// The loop ends: where a clear is still owed, ask the host to clear
    /// whatever entry it still holds, waiting at most `within`. `None` when
    /// nothing was owed.
    pub(crate) fn before_exit(&self, within: Duration) -> Option<ExpiredClear> {
        let owed = std::mem::take(&mut *self.owed.borrow_mut());
        if owed.is_empty() {
            return None;
        }
        drop(owed);
        let cleared = self.native.clear_expired_before_exit(within);
        tracing::debug!(
            ?cleared,
            "the loop's end cleared the expiring clipboard entry"
        );
        Some(cleared)
    }

    /// Ask the host to clear `owed`'s entry if it is still there, with nothing
    /// borrowed: the host's answer may come at once.
    fn ask(self: &Rc<Self>, owed: Owed) {
        let Owed {
            entry,
            misses,
            completions,
            ..
        } = owed;
        let (completion, answer) = completions.pending::<ExpiredClear>();
        let expiries: Weak<Self> = Rc::downgrade(self);
        let retry = completions.clone();
        completion.on_complete(move |answered| {
            // A UI runtime that retired took the answer with it: the host
            // still did the clear.
            let (Ok(cleared), Some(expiries)) = (answered, expiries.upgrade()) else {
                return;
            };
            expiries.answered(entry, misses, retry.clone(), cleared);
        });
        self.native
            .clear_expired(ExpiredEntry::Entry(entry), answer);
    }

    /// The host answered the clear of `entry` after `misses` attempts missed.
    fn answered(
        &self,
        entry: ClipboardEntryId,
        misses: u32,
        completions: CompletionSource,
        cleared: ExpiredClear,
    ) {
        if !cleared.is_worth_a_retry() {
            match cleared {
                ExpiredClear::Cleared | ExpiredClear::NotHeld => {
                    tracing::debug!(
                        entry = entry.get(),
                        ?cleared,
                        "an expiring clipboard entry's clear"
                    );
                }
                _ => tracing::warn!(
                    entry = entry.get(),
                    ?cleared,
                    "an expiring clipboard entry could not be cleared"
                ),
            }
            return;
        }
        if misses >= MAX_CLEAR_RETRIES {
            tracing::warn!(
                entry = entry.get(),
                ?cleared,
                "an expiring clipboard entry could not be cleared after {MAX_CLEAR_RETRIES} retries"
            );
            return;
        }
        let due = self.clock.now() + CLEAR_RETRY.delay(misses);
        self.owed.borrow_mut().push(Owed {
            entry,
            due,
            misses: misses + 1,
            completions,
        });
    }
}

#[cfg(test)]
#[path = "clipboard_expiry_tests.rs"]
mod tests;
