//! The providers' work the host has posted to Android's Java main thread
//! and the main thread has not taken yet (`Activity::on_main_thread`).
//!
//! Each piece sits in a slot the main thread and the owner share: the main
//! thread takes it to run it, the owner takes it back to refuse it (at its
//! deadline, or when the Activity ends), and whichever takes it first
//! decides, so a work is never both run and refused. Taken work leaves the
//! queue; the queue holds at most `MAX_POSTED_WORK` waiting pieces.
//! android-activity keeps a closure it cannot deliver, so a slot nobody
//! takes would wait forever without the deadline.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use web_time::Instant;

/// One piece of work, until the main thread or the owner takes it.
pub(crate) type Slot<W> = Arc<Mutex<Option<W>>>;

/// A waiting piece of work and when it was posted.
struct Waiting<W> {
    posted: Instant,
    slot: Slot<W>,
}

impl<W> Waiting<W> {
    fn is_taken(&self) -> bool {
        self.slot.lock().is_none()
    }
}

/// The work posted and not yet taken, owner side.
pub(crate) struct PostedQueue<W> {
    waiting: Vec<Waiting<W>>,
    capacity: usize,
    limit: Duration,
}

impl<W> PostedQueue<W> {
    /// A queue of at most `capacity` waiting pieces, each refused once it
    /// waited `limit`.
    pub(crate) fn new(capacity: usize, limit: Duration) -> Self {
        Self {
            waiting: Vec::new(),
            capacity,
            limit,
        }
    }

    /// Forget the work the main thread took.
    fn sweep(&mut self) {
        self.waiting.retain(|waiting| !waiting.is_taken());
    }

    /// Hold `work`, posted at `now`, and hand back the slot the main thread
    /// takes it from; the work back when `capacity` pieces wait.
    pub(crate) fn admit(&mut self, work: W, now: Instant) -> Result<Slot<W>, W> {
        self.sweep();
        if self.waiting.len() >= self.capacity {
            return Err(work);
        }
        let slot = Arc::new(Mutex::new(Some(work)));
        self.waiting.push(Waiting {
            posted: now,
            slot: Arc::clone(&slot),
        });
        Ok(slot)
    }

    /// When the oldest waiting piece is due.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .filter(|waiting| !waiting.is_taken())
            .map(|waiting| waiting.posted + self.limit)
            .min()
    }

    /// Take back every piece that waited `limit` by `now`, for the owner to
    /// refuse; the main thread finds their slots empty.
    pub(crate) fn take_overdue(&mut self, now: Instant) -> Vec<W> {
        let limit = self.limit;
        self.take_where(|posted| now.saturating_duration_since(posted) >= limit)
    }

    /// Take back every waiting piece: the Activity ended.
    pub(crate) fn take_all(&mut self) -> Vec<W> {
        self.take_where(|_| true)
    }

    fn take_where(&mut self, due: impl Fn(Instant) -> bool) -> Vec<W> {
        let taken = self
            .waiting
            .iter()
            .filter(|waiting| due(waiting.posted))
            .filter_map(|waiting| waiting.slot.lock().take())
            .collect();
        self.sweep();
        taken
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use web_time::Instant;

    use super::PostedQueue;

    const LIMIT: Duration = Duration::from_secs(10);

    /// The queue holds as many waiting pieces as its capacity and hands the
    /// next one back; a piece the main thread took frees its place.
    #[test]
    fn the_queue_holds_its_capacity_of_waiting_work_and_hands_back_the_next() {
        let now = Instant::now();
        let mut queue = PostedQueue::new(2, LIMIT);
        let first = queue.admit(1, now).expect("room");
        queue.admit(2, now).expect("room");
        assert_eq!(queue.admit(3, now).map(|_| ()), Err(3), "full: handed back");
        assert_eq!(first.lock().take(), Some(1), "the main thread takes one");
        queue.admit(4, now).expect("a taken piece freed its place");
    }

    /// A piece is taken back only once it waited the limit and only while
    /// the main thread has not taken it; the deadline is the oldest waiting
    /// piece's, and the end of the Activity takes back every waiting one.
    #[test]
    fn only_work_still_waiting_is_taken_back_at_its_deadline_or_the_end() {
        let start = Instant::now();
        let mut queue = PostedQueue::new(8, LIMIT);
        let taken = queue.admit("taken", start).expect("room");
        queue.admit("old", start).expect("room");
        queue.admit("young", start + LIMIT / 2).expect("room");
        assert_eq!(queue.next_deadline(), Some(start + LIMIT));
        assert_eq!(
            taken.lock().take(),
            Some("taken"),
            "running on the main thread"
        );
        let nothing: Vec<&str> = Vec::new();
        assert_eq!(
            queue.take_overdue(start + LIMIT / 2),
            nothing,
            "not due yet"
        );
        assert_eq!(queue.take_overdue(start + LIMIT), ["old"]);
        assert_eq!(queue.next_deadline(), Some(start + LIMIT / 2 + LIMIT));
        assert_eq!(queue.take_all(), ["young"]);
        assert_eq!(queue.next_deadline(), None);
        assert_eq!(queue.take_all(), nothing);
    }
}
