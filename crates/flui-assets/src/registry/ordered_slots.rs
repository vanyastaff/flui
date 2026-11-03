//! A bounded set of slots, each freed one handed to the earliest-admitted
//! waiter.
//!
//! A registry keeps two: load slots, which a load reserves at admission
//! ([`OrderedSlots::reserve`], [`OrderedSlots::reserve_start`]) and holds
//! from before its read until its decode returns, and decode slots, which it
//! holds while it decodes. Loads reach each queue in whatever order their
//! earlier steps finish; [`OrderedSlots`] orders the waiters by admission
//! instead.
//!
//! The queue is work-conserving: it orders only loads that are waiting. A free
//! slot goes to whoever asks while no one waits, so a load still fetching its
//! bytes reserves no decode slot, and a ready load admitted after it decodes
//! first rather than leaving the decoder idle behind a slow response.
//!
//! A waiter is either a channel its task awaits, or a [`Start`] the queue
//! calls with the slot: a load queued that way holds no task anywhere until
//! its slot is free, so a lane's admission window is never spent on a load
//! that only waits.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::oneshot;

/// A load's place in admission order: lower is earlier.
pub(crate) type AdmissionOrder = u64;

/// What a queued waiter does with its slot once the queue reaches it: starts
/// its load, which keeps the slot, or hands the slot back unused when the
/// load was abandoned while it waited.
pub(crate) type Start = Box<dyn FnOnce(Slot) -> Option<Slot> + Send>;

/// Hands out at most `slots` [`Slot`]s: a free one to any caller while none
/// waits, each freed one to the earliest-admitted waiter.
pub(crate) struct OrderedSlots {
    state: Mutex<QueueState>,
}

struct QueueState {
    free: usize,
    waiting: BinaryHeap<Waiter>,
}

struct Waiter {
    order: AdmissionOrder,
    grant: Grant,
}

/// How a waiter takes its slot.
enum Grant {
    /// Sent to the task that awaits it.
    Send(oneshot::Sender<Slot>),
    /// Handed to a start, on the thread that freed the slot.
    Start(Start),
}

impl PartialEq for Waiter {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
    }
}

impl Eq for Waiter {}

impl PartialOrd for Waiter {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Waiter {
    /// The max-heap's greatest waiter is the earliest admitted.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Reverse(self.order).cmp(&Reverse(other.order))
    }
}

/// One slot held; dropping it frees the slot for the next
/// waiter.
pub(crate) struct Slot {
    queue: Option<Arc<OrderedSlots>>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(queue) = self.queue.take() {
            OrderedSlots::release(&queue);
        }
    }
}

/// A slot taken, or a place held in the queue for one. Dropping it gives the
/// slot back or gives up the place.
pub(crate) enum Reservation {
    Granted(Slot),
    Queued(oneshot::Receiver<Slot>),
}

impl Reservation {
    /// The slot, once the queue reaches this reservation.
    pub(crate) async fn granted(self) -> Slot {
        match self {
            Self::Granted(slot) => slot,
            Self::Queued(rx) => rx
                .await
                .expect("BUG: a queued waiter is only dropped after it is handed a slot"),
        }
    }
}

impl OrderedSlots {
    pub(crate) fn new(slots: usize) -> Self {
        Self {
            state: Mutex::new(QueueState {
                free: slots,
                waiting: BinaryHeap::new(),
            }),
        }
    }

    /// Waits for a slot, behind every waiter admitted before `order`.
    pub(crate) async fn acquire(self: &Arc<Self>, order: AdmissionOrder) -> Slot {
        self.reserve(order).granted().await
    }

    /// Takes a free slot, or a place in the queue, now, on the calling
    /// thread. A caller that reserves at admission, under the lock that
    /// assigns `order`, holds its place however late its task first runs.
    pub(crate) fn reserve(self: &Arc<Self>, order: AdmissionOrder) -> Reservation {
        let mut state = self.state.lock();
        if let Some(slot) = self.take_free(&mut state) {
            return Reservation::Granted(slot);
        }
        let (tx, rx) = oneshot::channel();
        state.waiting.push(Waiter {
            order,
            grant: Grant::Send(tx),
        });
        Reservation::Queued(rx)
    }

    /// Takes a free slot and hands `start` back with it for the caller to
    /// run once it holds no lock, or queues `start` to be called with the
    /// slot freed for it.
    pub(crate) fn reserve_start(
        self: &Arc<Self>,
        order: AdmissionOrder,
        start: Start,
    ) -> Option<(Slot, Start)> {
        let mut state = self.state.lock();
        if let Some(slot) = self.take_free(&mut state) {
            return Some((slot, start));
        }
        state.waiting.push(Waiter {
            order,
            grant: Grant::Start(start),
        });
        None
    }

    /// A free slot, while no one waits for one.
    fn take_free(self: &Arc<Self>, state: &mut QueueState) -> Option<Slot> {
        if state.free == 0 || !state.waiting.is_empty() {
            return None;
        }
        state.free -= 1;
        Some(Slot {
            queue: Some(Arc::clone(self)),
        })
    }

    /// Hands the freed slot to the earliest live waiter, or returns it.
    fn release(queue: &Arc<Self>) {
        loop {
            let waiter = {
                let mut state = queue.state.lock();
                let Some(waiter) = state.waiting.pop() else {
                    state.free += 1;
                    return;
                };
                waiter
            };
            let slot = Slot {
                queue: Some(Arc::clone(queue)),
            };
            // Handed over outside the lock: a start spawns its load, and a
            // waiter abandoned after the send drops the slot with its
            // channel, which releases it again.
            let refused = match waiter.grant {
                Grant::Send(tx) => tx.send(slot).err(),
                Grant::Start(start) => start(slot),
            };
            match refused {
                None => return,
                // Abandoned before the hand-over: disarm the slot it refused
                // and try the next waiter.
                Some(mut refused) => refused.queue = None,
            }
        }
    }
}
