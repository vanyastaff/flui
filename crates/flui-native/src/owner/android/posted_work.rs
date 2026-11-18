//! The providers' work the host has posted to Android's Java main thread
//! and the main thread has not taken yet (`Activity::on_main_thread`).
//!
//! Each piece sits in a slot the main thread and the owner share: the main
//! thread takes it to run it ([`take_and_run`]), the owner takes it back to
//! refuse it (at its deadline, or when the Activity ends), and whichever
//! takes it first decides, so a work is never both run and refused. Taken
//! work leaves the queue; the queue holds at most `capacity` waiting pieces.
//! android-activity keeps a closure it cannot deliver, so a slot nobody
//! takes would wait forever without the deadline.
//!
//! A piece posted while `capacity` wait is refused as `TooMuchPosted`, but
//! not at once: the refusal is held ([`Admitted::Deferred`]) until a place
//! frees (the main thread took a piece, or the deadline took one back), so a
//! provider that posts again on the refusal finds room instead of spinning
//! the owner one turn per refusal. At most `capacity` refusals are held; one
//! more is refused at once.

use std::sync::Arc;
use std::time::Duration;

use flui_platform::capability::MainThreadRefused;
use parking_lot::Mutex;
use web_time::Instant;

/// One piece of work, until the main thread or the owner takes it.
pub(crate) type Slot<W> = Arc<Mutex<Option<W>>>;

/// Work the queue holds: run once in a frame `F`, or refused.
pub(crate) trait Work<F> {
    /// Run in `frame`.
    fn run(self, frame: F);
    /// Refuse for `reason`.
    fn refuse(self, reason: MainThreadRefused);
}

impl Work<flui_platform::capability::JniFrame> for flui_platform::capability::PostedWork {
    fn run(self, frame: flui_platform::capability::JniFrame) {
        Self::run(self, frame);
    }

    fn refuse(self, reason: MainThreadRefused) {
        Self::refuse(self, reason);
    }
}

/// What [`PostedQueue::admit`] did with a piece of work.
pub(crate) enum Admitted<W> {
    /// Waiting in this slot for the main thread.
    Posted(Slot<W>),
    /// Refused for want of room, the refusal held until a place frees.
    Deferred,
    /// Refused at once: the held refusals are as many as the queue holds.
    Refused(W),
}

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
    /// Work refused for want of room, whose refusal waits for a place.
    deferred: Vec<W>,
    capacity: usize,
    limit: Duration,
}

impl<W> PostedQueue<W> {
    /// A queue of at most `capacity` waiting pieces, each refused once it
    /// waited `limit`.
    pub(crate) fn new(capacity: usize, limit: Duration) -> Self {
        Self {
            waiting: Vec::new(),
            deferred: Vec::new(),
            capacity,
            limit,
        }
    }

    /// Forget the work the main thread took.
    fn sweep(&mut self) {
        self.waiting.retain(|waiting| !waiting.is_taken());
    }

    /// Hold `work`, posted at `now`: in a slot the main thread takes it
    /// from, else deferred while `capacity` pieces wait, else handed back.
    pub(crate) fn admit(&mut self, work: W, now: Instant) -> Admitted<W> {
        self.sweep();
        if self.waiting.len() >= self.capacity {
            if self.deferred.len() >= self.capacity {
                return Admitted::Refused(work);
            }
            self.deferred.push(work);
            return Admitted::Deferred;
        }
        let slot = Arc::new(Mutex::new(Some(work)));
        self.waiting.push(Waiting {
            posted: now,
            slot: Arc::clone(&slot),
        });
        Admitted::Posted(slot)
    }

    /// When the oldest waiting piece is due.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .filter(|waiting| !waiting.is_taken())
            .map(|waiting| waiting.posted + self.limit)
            .min()
    }

    /// The deferred work whose refusal may go out now: all of it once a
    /// place is free, none while `capacity` pieces still wait.
    pub(crate) fn take_room_refusals(&mut self) -> Vec<W> {
        self.sweep();
        if self.waiting.len() < self.capacity {
            std::mem::take(&mut self.deferred)
        } else {
            Vec::new()
        }
    }

    /// Take back every piece that waited `limit` by `now`, for the owner to
    /// refuse; the main thread finds their slots empty.
    pub(crate) fn take_overdue(&mut self, now: Instant) -> Vec<W> {
        let limit = self.limit;
        self.take_where(|posted| now.saturating_duration_since(posted) >= limit)
    }

    /// Take back every waiting and deferred piece: the Activity ended.
    pub(crate) fn take_all(&mut self) -> Vec<W> {
        let mut taken = self.take_where(|_| true);
        taken.append(&mut self.deferred);
        taken
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

/// Main thread: take the work from `slot` and run it in the frame `reach`
/// lends to the callback it is given (`None` when the Activity could not be
/// lent). A work the owner already took back is not run; one whose frame was
/// not lent, before or instead of the callback, is refused as
/// `HandOffFailed`. Answers what `reach` answered.
///
/// # Errors
///
/// What `reach` failed with.
pub(crate) fn take_and_run<W: Work<F>, F, E>(
    slot: &Slot<W>,
    reach: impl FnOnce(&mut dyn FnMut(Option<F>)) -> Result<(), E>,
) -> Result<(), E> {
    // Taken first: the owner no longer refuses it at the deadline.
    let Some(work) = slot.lock().take() else {
        return Ok(());
    };
    let mut unrun = Some(work);
    let reached = reach(&mut |frame| {
        if let Some(frame) = frame
            && let Some(work) = unrun.take()
        {
            work.run(frame);
        }
    });
    if let Some(work) = unrun {
        work.refuse(MainThreadRefused::HandOffFailed);
    }
    reached
}

/// The work left in `slot` when the closure that held it ended without
/// running it (dropped unrun), refused as `HandOffFailed`. A work that ran,
/// or panicked running, left the slot empty: nothing is owed here.
pub(crate) fn refuse_unrun<W: Work<F>, F>(slot: &Slot<W>) {
    let work = slot.lock().take();
    if let Some(work) = work {
        work.refuse(MainThreadRefused::HandOffFailed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use flui_platform::capability::MainThreadRefused;
    use parking_lot::Mutex;
    use web_time::Instant;

    use super::{Admitted, PostedQueue, Work, refuse_unrun, take_and_run};

    const LIMIT: Duration = Duration::from_secs(10);

    /// A piece of work that records how it ended.
    struct Recorded(&'static str, Arc<Mutex<Vec<String>>>);

    impl Work<u8> for Recorded {
        fn run(self, frame: u8) {
            self.1.lock().push(format!("{} ran in {frame}", self.0));
        }

        fn refuse(self, reason: MainThreadRefused) {
            self.1.lock().push(format!("{} refused {reason:?}", self.0));
        }
    }

    fn posted<W>(admitted: Admitted<W>) -> super::Slot<W> {
        match admitted {
            Admitted::Posted(slot) => slot,
            Admitted::Deferred | Admitted::Refused(_) => panic!("expected room"),
        }
    }

    /// The queue holds as many waiting pieces as its capacity; past it a
    /// piece's refusal is held until a place frees, and once as many
    /// refusals are held, the next piece is handed back at once.
    #[test]
    fn the_queue_holds_its_capacity_of_waiting_work_and_hands_back_the_next() {
        let now = Instant::now();
        let mut queue = PostedQueue::new(2, LIMIT);
        let first = posted(queue.admit(1, now));
        posted(queue.admit(2, now));
        assert!(matches!(queue.admit(3, now), Admitted::Deferred));
        assert!(matches!(queue.admit(4, now), Admitted::Deferred));
        assert!(
            matches!(queue.admit(5, now), Admitted::Refused(5)),
            "the held refusals are bounded too"
        );
        assert_eq!(first.lock().take(), Some(1), "the main thread takes one");
        assert_eq!(queue.take_room_refusals(), [3, 4], "a place freed");
        posted(queue.admit(6, now));
    }

    /// While the queue stays full, a provider that posts again on each
    /// refusal gets no refusal back: the owner is not woken once per turn.
    /// The refusals go out once a place frees.
    #[test]
    fn a_full_queue_holds_refusals_until_a_place_frees_instead_of_spinning() {
        let now = Instant::now();
        let mut queue = PostedQueue::new(1, LIMIT);
        let only = posted(queue.admit("waiting", now));
        assert!(matches!(queue.admit("retry", now), Admitted::Deferred));
        let mut released = 0;
        for _ in 0..100 {
            released += queue.take_room_refusals().len();
        }
        assert_eq!(released, 0, "no refusal while full: nothing to retry into");
        assert_eq!(only.lock().take(), Some("waiting"));
        assert_eq!(queue.take_room_refusals(), ["retry"]);
    }

    /// A piece is taken back only once it waited the limit and only while
    /// the main thread has not taken it; the deadline is the oldest waiting
    /// piece's, and the end of the Activity takes back every waiting and
    /// deferred one.
    #[test]
    fn only_work_still_waiting_is_taken_back_at_its_deadline_or_the_end() {
        let start = Instant::now();
        let mut queue = PostedQueue::new(3, LIMIT);
        let taken = posted(queue.admit("taken", start));
        posted(queue.admit("old", start));
        posted(queue.admit("young", start + LIMIT / 2));
        assert!(matches!(queue.admit("deferred", start), Admitted::Deferred));
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
        assert_eq!(queue.take_all(), ["young", "deferred"]);
        assert_eq!(queue.next_deadline(), None);
        assert_eq!(queue.take_all(), nothing);
    }

    /// The main thread runs a work in the frame it reaches; a work whose
    /// frame was not lent is refused; one the owner took back is not run;
    /// and the failure path refuses only a work still in its slot.
    #[test]
    fn the_main_thread_runs_the_work_it_reaches_and_refuses_the_rest() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let slot = |name| Arc::new(Mutex::new(Some(Recorded(name, Arc::clone(&log)))));

        let lent = slot("lent");
        assert_eq!(
            take_and_run(&lent, |run| -> Result<(), ()> {
                run(Some(7));
                Ok(())
            }),
            Ok(())
        );
        let unlent = slot("unlent");
        assert_eq!(
            take_and_run(&unlent, |run| -> Result<(), ()> {
                run(None);
                Ok(())
            }),
            Ok(())
        );
        let unreached = slot("unreached");
        assert_eq!(
            take_and_run(&unreached, |_run| -> Result<(), &str> { Err("no thread") }),
            Err("no thread")
        );
        let taken_back = slot("taken back");
        drop(taken_back.lock().take());
        assert_eq!(
            take_and_run(&taken_back, |_run| -> Result<(), ()> {
                panic!("a work taken back is not reached for")
            }),
            Ok(())
        );
        refuse_unrun(&lent);
        let dropped = slot("dropped");
        refuse_unrun(&dropped);
        assert_eq!(
            *log.lock(),
            [
                "lent ran in 7",
                "unlent refused HandOffFailed",
                "unreached refused HandOffFailed",
                "dropped refused HandOffFailed",
            ]
        );
    }
}
