//! [`Queue`]: the one owner of a script's unanswered requests and of its
//! holds, with the one transition function, [`Queue::step`], every event goes
//! through. The rules it carries out are stated once, on
//! [`Script`](super::Script#held-requests).
//!
//! The queue runs no code of the test's: it moves requests between states
//! and hands back an [`Effect`] for the script to carry out, outside any
//! borrow, one request at a time.

use std::collections::VecDeque;

/// What a request in the queue waits for.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Waits<D> {
    /// The last hold: then it goes on to the steps and the standing answer.
    /// A request waits so only while a hold lives or a drain runs.
    Hold,
    /// The test: `release_oldest`, `release_held` or `abandon_held`.
    Test,
    /// The scenario's clock, to be answered with `D` then.
    Clock(D),
}

impl<D> Waits<D> {
    /// Whether the request waits for a hold or the test, not the clock.
    fn is_held(&self) -> bool {
        matches!(self, Self::Hold | Self::Test)
    }
}

/// Something that happened to a script's requests or holds.
#[derive(Debug)]
pub(super) enum Event<K, T, D> {
    /// A request reached the script.
    Arrived(K, T),
    /// A request the test answers: a step held it, or the code that was to
    /// answer it panicked.
    KeepForTest(K, T),
    /// A request whose answer waits for the scenario's clock.
    Schedule(K, T, D),
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

/// What the script carries out after a [`Queue::step`].
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Effect<K, T, D> {
    /// Nothing.
    None,
    /// Send this request on to the steps and the standing answer.
    SendOn(K, T),
    /// The last hold went: send the kept requests on, one at a time, asking
    /// [`Event::DrainNext`] until it answers anything but `SendOn`.
    Drain,
    /// This request left the queue as the event asked (released, abandoned,
    /// withdrawn, due), with what it was waiting for.
    Taken(K, T, Waits<D>),
}

/// One request in the queue.
#[derive(Debug)]
struct Entry<K, T, D> {
    key: K,
    waits: Waits<D>,
    payload: T,
}

/// A script's unanswered requests, oldest first, its live holds and whether
/// a drain is sending the kept requests on.
#[derive(Debug)]
pub(super) struct Queue<K, T, D> {
    entries: VecDeque<Entry<K, T, D>>,
    holds: usize,
    draining: bool,
}

impl<K, T, D> Default for Queue<K, T, D> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            holds: 0,
            draining: false,
        }
    }
}

impl<K: Copy + Eq, T, D> Queue<K, T, D> {
    /// Carry out `event` and say what the script does next.
    pub(super) fn step(&mut self, event: Event<K, T, D>) -> Effect<K, T, D> {
        match event {
            Event::Arrived(key, payload) => {
                if self.holds > 0 || self.draining {
                    self.push(key, Waits::Hold, payload);
                    Effect::None
                } else {
                    Effect::SendOn(key, payload)
                }
            }
            Event::KeepForTest(key, payload) => {
                self.push(key, Waits::Test, payload);
                Effect::None
            }
            Event::Schedule(key, payload, due) => {
                self.push(key, Waits::Clock(due), payload);
                Effect::None
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
                self.take(|entry| entry.key == key && matches!(entry.waits, Waits::Clock(_)))
            }
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
            && let Some(Entry { key, payload, .. }) =
                self.remove(|entry| matches!(entry.waits, Waits::Hold))
        {
            return Effect::SendOn(key, payload);
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

    fn remove(&mut self, which: impl Fn(&Entry<K, T, D>) -> bool) -> Option<Entry<K, T, D>> {
        let position = self.entries.iter().position(which)?;
        self.entries.remove(position)
    }

    fn take(&mut self, which: impl Fn(&Entry<K, T, D>) -> bool) -> Effect<K, T, D> {
        match self.remove(which) {
            Some(Entry {
                key,
                waits,
                payload,
            }) => Effect::Taken(key, payload, waits),
            None => Effect::None,
        }
    }

    /// The held requests, oldest first.
    pub(super) fn held_keys(&self) -> Vec<K> {
        self.held().map(|(key, _)| key).collect()
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

    /// How many requests are in the queue.
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
