//! Announcements: messages spoken to the user without a node of the author's
//! own (ADR-0204 §4).
//!
//! AccessKit has no announcement API, so a presentation keeps two synthetic
//! live-region nodes, polite and assertive, as the last children of its root,
//! and an announcement is a change of one node's value. [`Announcements`] is
//! the presentation's queue and the identities of those nodes; it outlives
//! any one [`SemanticsOwner`](crate::SemanticsOwner), which accessibility
//! enable and disable replace. [`Announcer`] is the handle application code
//! holds.
//!
//! An accepted message is queued before the frame is requested, and leaves
//! the queue only once an update carrying it is delivered: a failed delivery,
//! or a frame request that does not lead to a frame, leaves it for the next
//! one.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use crate::identity::AccessibilityNodeId;
use crate::regions::SemanticsRegionId;
use crate::role::Urgency;

/// Appended to a repeated message so the node's value changes and the
/// adapter announces it again: a zero-width space, which no screen reader
/// speaks.
const REPEAT_MARK: char = '\u{200B}';

/// What one urgency's node shows and what waits to replace it.
#[derive(Debug, Default)]
struct Channel {
    /// Accepted, undelivered messages, oldest first.
    pending: Vec<String>,
    /// The value the node was last delivered with.
    shown: String,
}

impl Channel {
    /// The value the node shows once the pending messages are delivered: one
    /// line per message, marked when it repeats what the node shows.
    fn next_value(&self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let joined = self.pending.join("\n");
        let shown = self.shown.trim_end_matches(REPEAT_MARK);
        Some(if joined == shown && !self.shown.ends_with(REPEAT_MARK) {
            let mut marked = joined;
            marked.push(REPEAT_MARK);
            marked
        } else {
            joined
        })
    }
}

#[derive(Debug, Default)]
struct Queue {
    /// Whether an owner publishes: with no assistive technology listening a
    /// message has no one to reach, and is not kept for a later one.
    listening: bool,
    polite: Channel,
    assertive: Channel,
}

impl Queue {
    fn channel(&mut self, urgency: Urgency) -> &mut Channel {
        match urgency {
            Urgency::Assertive => &mut self.assertive,
            Urgency::Polite => &mut self.polite,
        }
    }
}

/// The node values one publication carries and the messages they consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedAnnouncements {
    /// The polite node's value.
    pub(crate) polite: String,
    /// The assertive node's value.
    pub(crate) assertive: String,
    consumed: [usize; 2],
}

/// A presentation's announcement queue and the identities of its two
/// announcer nodes.
///
/// Cloning shares the queue. The pipeline that owns a presentation's
/// semantics keeps one and installs a clone in each
/// [`SemanticsOwner`](crate::SemanticsOwner) it creates
/// ([`SemanticsOwner::set_announcements`](crate::SemanticsOwner::set_announcements)).
#[derive(Debug, Clone)]
pub struct Announcements {
    queue: Arc<Mutex<Queue>>,
    polite: AccessibilityNodeId,
    assertive: AccessibilityNodeId,
}

impl Announcements {
    /// A queue whose nodes publish under `polite` and `assertive`, two
    /// identities from the presentation's
    /// [`SemanticsIdentitySource`](crate::SemanticsIdentitySource).
    #[must_use]
    pub fn new(polite: SemanticsRegionId, assertive: SemanticsRegionId) -> Self {
        Self {
            queue: Arc::default(),
            polite: polite.accessibility_id(),
            assertive: assertive.accessibility_id(),
        }
    }

    /// A handle that queues messages and then calls `request_frame`, so an
    /// idle presentation produces the frame that publishes them.
    #[must_use]
    pub fn announcer(&self, request_frame: Arc<dyn Fn() + Send + Sync>) -> Announcer {
        Announcer {
            queue: Arc::downgrade(&self.queue),
            request_frame,
        }
    }

    /// Whether an owner publishes these announcements. While no one listens,
    /// [`Announcer::announce`] accepts and drops messages, and pending ones
    /// are dropped: a screen reader that connects later is not read a backlog.
    pub fn set_listening(&self, listening: bool) {
        let mut queue = self.lock();
        queue.listening = listening;
        if !listening {
            queue.polite.pending.clear();
            queue.assertive.pending.clear();
        }
    }

    /// The identity of the node an announcement of `urgency` is written to.
    #[must_use]
    pub fn node_id(&self, urgency: Urgency) -> AccessibilityNodeId {
        match urgency {
            Urgency::Assertive => self.assertive,
            Urgency::Polite => self.polite,
        }
    }

    /// Queues `message` without requesting a frame: the owner's own path, for
    /// a caller already inside a frame.
    pub(crate) fn enqueue(&self, message: &str, urgency: Urgency) {
        enqueue(&self.queue, message, urgency);
    }

    /// Whether a message waits for delivery.
    pub(crate) fn has_pending(&self) -> bool {
        let queue = self.lock();
        !(queue.polite.pending.is_empty() && queue.assertive.pending.is_empty())
    }

    /// The values the next publication carries. Nothing leaves the queue
    /// until [`Self::commit`].
    pub(crate) fn prepare(&self) -> PreparedAnnouncements {
        let queue = self.lock();
        PreparedAnnouncements {
            polite: queue
                .polite
                .next_value()
                .unwrap_or_else(|| queue.polite.shown.clone()),
            assertive: queue
                .assertive
                .next_value()
                .unwrap_or_else(|| queue.assertive.shown.clone()),
            consumed: [queue.polite.pending.len(), queue.assertive.pending.len()],
        }
    }

    /// Records `delivered` as what the nodes show and removes the messages
    /// it consumed. A message queued after [`Self::prepare`] stays.
    pub(crate) fn commit(&self, delivered: &PreparedAnnouncements) {
        let mut queue = self.lock();
        let Queue {
            polite, assertive, ..
        } = &mut *queue;
        for (channel, (value, consumed)) in [
            (polite, (&delivered.polite, delivered.consumed[0])),
            (assertive, (&delivered.assertive, delivered.consumed[1])),
        ] {
            let consumed = consumed.min(channel.pending.len());
            channel.pending.drain(..consumed);
            channel.shown.clone_from(value);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Queue> {
        lock(&self.queue)
    }
}

/// The queue's lock. No user code runs under it, so a poisoned lock holds a
/// consistent queue.
fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Queues `message`; returns whether anyone listens.
fn enqueue(queue: &Mutex<Queue>, message: &str, urgency: Urgency) -> bool {
    let mut queue = lock(queue);
    if !queue.listening || message.is_empty() {
        return queue.listening;
    }
    queue.channel(urgency).pending.push(message.to_owned());
    true
}

/// Speaks a message to the user through assistive technology, without a
/// visible change.
///
/// Acquire it from `LifecycleContext::announcer` in a lifecycle hook and keep
/// it; it is `Send`, so a completion on another thread can announce. A
/// declarative live region (`Semantics::live_region`) is the primary way to
/// make a change heard; announce what has no node of its own, such as
/// "Message sent".
#[derive(Clone)]
pub struct Announcer {
    queue: Weak<Mutex<Queue>>,
    request_frame: Arc<dyn Fn() + Send + Sync>,
}

/// Why an announcement was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnnounceError {
    /// The presentation the announcer belongs to has closed.
    #[error("the presentation that would announce has closed")]
    PresentationClosed,
}

impl Announcer {
    /// Queues `message` for the next frame and requests that frame.
    ///
    /// Messages of one frame are spoken together, one per line; a message
    /// equal to the last one is still spoken. While no assistive technology
    /// listens the message is accepted and dropped. An empty message is
    /// accepted and not spoken.
    ///
    /// # Errors
    /// [`AnnounceError::PresentationClosed`] once the presentation is gone.
    pub fn announce(
        &self,
        message: impl AsRef<str>,
        urgency: Urgency,
    ) -> Result<(), AnnounceError> {
        let queue = self
            .queue
            .upgrade()
            .ok_or(AnnounceError::PresentationClosed)?;
        // Queued first, so a frame that runs before the request returns
        // still finds the message.
        if enqueue(&queue, message.as_ref(), urgency) {
            drop(queue);
            (self.request_frame)();
        }
        Ok(())
    }
}

impl std::fmt::Debug for Announcer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Announcer")
            .field("open", &(self.queue.strong_count() > 0))
            .finish_non_exhaustive()
    }
}
