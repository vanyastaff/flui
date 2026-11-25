//! [`Unanswered`]: the one owner of a script's unanswered requests and of
//! its holds, with the one transition function, [`Unanswered::apply`],
//! every event goes through. The rules it carries out are stated once, on
//! [`Script`](super::Script#held-requests).
//!
//! It runs no code of the test's: it moves requests between states and hands
//! back an [`Effect`] for the script to carry out, outside any borrow, one
//! request at a time. A request stays in it, [`Waits::Answering`], while the
//! test's code makes its answer, so a ticket dropped meanwhile finds it and a
//! request kept for the test keeps its place.

use std::collections::VecDeque;

/// What a request waits for.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Waits<D> {
    /// The last hold: then it goes on to the steps and the standing answer.
    /// A request waits so only while a hold lives or a drain runs.
    Hold,
    /// The test: `release_oldest`, `release_held` or `abandon_held`.
    Test,
    /// The scenario's clock, to be answered with `D` then.
    Clock(D),
    /// Its answer, which the test's code (a matcher, a computed or standing
    /// answer) is making now.
    Answering,
}

impl<D> Waits<D> {
    /// Whether the request waits for a hold or the test.
    fn is_held(&self) -> bool {
        matches!(self, Self::Hold | Self::Test)
    }
}

/// Something that happened to a script's requests or holds.
#[derive(Debug)]
pub(super) enum Event<K, T, D> {
    /// A request reached the script.
    Arrived(K, T),
    /// The request being answered waits for the test: a step held it, or
    /// the code making its answer panicked.
    KeepForTest(K),
    /// The request being answered has an answer that waits for the clock.
    Schedule(K, D),
    /// The answer of the request being answered is made.
    Answered(K),
    /// A hold was taken.
    HoldTaken,
    /// A hold was dropped or released.
    HoldDropped,
    /// A hold was let go with `leave_held`.
    HoldLeft,
    /// The running drain asks for its next request.
    DrainNext,
    /// `verify` reported every held request unanswered.
    Reported,
    /// The request's ticket was dropped.
    Withdraw(K),
    /// `release_oldest` takes the oldest held request.
    ReleaseOldest,
    /// `release_held` or `abandon_held` takes the held request `K`.
    Release(K),
    /// The answer `release_held` was making for the held request `K`
    /// panicked.
    AnswerPanicked(K),
    /// The scenario's clock reached the request's scheduled answer.
    ClockReached(K),
}

/// What the script carries out after [`Unanswered::apply`].
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Effect<K, T, D> {
    /// Nothing.
    None,
    /// Make this request's answer by the steps and the standing answer; it
    /// stays unanswered, `Answering`, meanwhile.
    SendOn(K),
    /// The last hold went: send the kept requests on, one at a time, asking
    /// [`Event::DrainNext`] until it answers anything but `SendOn`.
    Drain,
    /// Answer this request with the scheduled answer `D`; it stays
    /// unanswered, `Answering`, meanwhile.
    Due(K, D),
    /// A ticket withdrew this request while its answer was made: take the
    /// answer `D` scheduled for it off the timeline and drop it.
    Orphaned(K, D),
    /// This request left the unanswered requests as the event asked
    /// (answered, released, abandoned, withdrawn), with what it was waiting
    /// for.
    Taken(K, T, Waits<D>),
}

/// One request.
#[derive(Debug)]
struct Entry<K, T, D> {
    key: K,
    waits: Waits<D>,
    payload: T,
}

/// A script's unanswered requests, oldest first, its live holds and whether
/// a drain is sending the kept requests on.
#[derive(Debug)]
pub(super) struct Unanswered<K, T, D> {
    entries: VecDeque<Entry<K, T, D>>,
    holds: usize,
    draining: bool,
}

impl<K, T, D> Default for Unanswered<K, T, D> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            holds: 0,
            draining: false,
        }
    }
}

impl<K: Copy + Eq, T, D> Unanswered<K, T, D> {
    /// Carry out `event` and say what the script does next.
    pub(super) fn apply(&mut self, event: Event<K, T, D>) -> Effect<K, T, D> {
        match event {
            Event::Arrived(key, payload) => {
                if self.holds > 0 || self.draining {
                    self.push(key, Waits::Hold, payload);
                    Effect::None
                } else {
                    self.push(key, Waits::Answering, payload);
                    Effect::SendOn(key)
                }
            }
            Event::KeepForTest(key) => {
                let _ = self.answering_waits(key, Waits::Test);
                Effect::None
            }
            Event::Schedule(key, due) => match self.answering_waits(key, Waits::Clock(due)) {
                Some(Waits::Clock(due)) => Effect::Orphaned(key, due),
                Some(Waits::Hold | Waits::Test | Waits::Answering) | None => Effect::None,
            },
            Event::Answered(key) => {
                self.take(|entry| entry.key == key && matches!(entry.waits, Waits::Answering))
            }
            Event::HoldTaken => {
                self.holds = self.holds.saturating_add(1);
                Effect::None
            }
            Event::HoldDropped => {
                if self.let_go() {
                    self.draining = true;
                    Effect::Drain
                } else {
                    Effect::None
                }
            }
            Event::HoldLeft => {
                if self.let_go() {
                    self.leave_to_test();
                }
                Effect::None
            }
            Event::DrainNext => self.drain_next(),
            Event::Reported => {
                self.leave_to_test();
                Effect::None
            }
            Event::Withdraw(key) => self.take(|entry| entry.key == key),
            Event::ReleaseOldest => self.take(|entry| entry.waits.is_held()),
            Event::Release(key) => self.take(|entry| entry.key == key && entry.waits.is_held()),
            Event::AnswerPanicked(key) => {
                if let Some(entry) = self
                    .entries
                    .iter_mut()
                    .find(|entry| entry.key == key && entry.waits.is_held())
                {
                    entry.waits = Waits::Test;
                }
                Effect::None
            }
            Event::ClockReached(key) => {
                let entry = self
                    .entries
                    .iter_mut()
                    .find(|entry| entry.key == key && matches!(entry.waits, Waits::Clock(_)));
                let Some(entry) = entry else {
                    return Effect::None;
                };
                // The entry was found waiting for the clock, so it has a due
                // answer to hand out.
                if let Waits::Clock(due) = std::mem::replace(&mut entry.waits, Waits::Answering) {
                    Effect::Due(key, due)
                } else {
                    Effect::None
                }
            }
        }
    }

    /// The request `key`, while its answer is being made, waits for `waits`
    /// from now on, where it is.
    ///
    /// Returns `waits` back when no request `key` is being answered (a
    /// ticket withdrew it meanwhile), so a scheduled answer it carries is
    /// taken off the timeline and dropped by the script, outside any
    /// borrow.
    fn answering_waits(&mut self, key: K, waits: Waits<D>) -> Option<Waits<D>> {
        match self
            .entries
            .iter_mut()
            .find(|entry| entry.key == key && matches!(entry.waits, Waits::Answering))
        {
            Some(entry) => {
                entry.waits = waits;
                None
            }
            None => Some(waits),
        }
    }

    /// One hold fewer. Returns whether that left the kept requests with no
    /// owner: no other hold lives and no drain is sending them on (a drain
    /// keeps them through a hold taken and let go by the code it runs).
    fn let_go(&mut self) -> bool {
        self.holds = self.holds.saturating_sub(1);
        self.holds == 0 && !self.draining
    }

    /// Every request a hold kept waits for the test from now on.
    fn leave_to_test(&mut self) {
        for entry in &mut self.entries {
            if matches!(entry.waits, Waits::Hold) {
                entry.waits = Waits::Test;
            }
        }
    }

    fn drain_next(&mut self) -> Effect<K, T, D> {
        if !self.draining {
            return Effect::None;
        }
        if self.holds == 0
            && let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| matches!(entry.waits, Waits::Hold))
        {
            entry.waits = Waits::Answering;
            return Effect::SendOn(entry.key);
        }
        // Done, or a hold taken meanwhile keeps the rest, in order.
        self.draining = false;
        Effect::None
    }

    fn push(&mut self, key: K, waits: Waits<D>, payload: T) {
        self.entries.push_back(Entry {
            key,
            waits,
            payload,
        });
    }

    fn take(&mut self, which: impl Fn(&Entry<K, T, D>) -> bool) -> Effect<K, T, D> {
        let Some(position) = self.entries.iter().position(which) else {
            return Effect::None;
        };
        match self.entries.remove(position) {
            Some(Entry {
                key,
                waits,
                payload,
            }) => Effect::Taken(key, payload, waits),
            None => Effect::None,
        }
    }

    /// Whether the request `key` is being answered: no ticket withdrew it
    /// since its answer began.
    pub(super) fn is_answering(&self, key: K) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.key == key && matches!(entry.waits, Waits::Answering))
    }

    /// The payload of the request `key`, whatever it waits for.
    pub(super) fn payload(&self, key: K) -> Option<&T> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.payload)
    }

    /// The held requests, oldest first.
    pub(super) fn held_keys(&self) -> Vec<K> {
        self.held().map(|(key, _)| key).collect::<Vec<K>>()
    }

    /// The held request `key`'s payload.
    pub(super) fn held_payload(&self, key: K) -> Option<&T> {
        self.held()
            .find(|(held, _)| *held == key)
            .map(|(_, payload)| payload)
    }

    /// The held requests, oldest first.
    pub(super) fn held(&self) -> impl Iterator<Item = (K, &T)> {
        self.entries
            .iter()
            .filter(|entry| entry.waits.is_held())
            .map(|entry| (entry.key, &entry.payload))
    }

    /// How many requests are unanswered, whatever each waits for.
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// How many holds live.
    pub(super) fn holds(&self) -> usize {
        self.holds
    }
}

#[cfg(test)]
mod tests;
