//! A bounded set of slots handed out in admission order.
//!
//! A registry keeps two: load slots, which a load holds from before its read
//! until its decode returns, and decode slots, which it holds while it
//! decodes. Loads reach each queue in whatever order their earlier steps
//! finish; [`OrderedSlots`] hands a freed slot to the waiting load that was
//! admitted first instead, so a load requested earlier is not overtaken.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::oneshot;

/// A load's place in admission order: lower is earlier.
pub(crate) type AdmissionOrder = u64;

/// Hands out at most `slots` [`Slot`]s, each freed slot to the earliest
/// admitted waiter.
pub(crate) struct OrderedSlots {
    state: Mutex<QueueState>,
}

struct QueueState {
    free: usize,
    waiting: BinaryHeap<Waiter>,
}

struct Waiter {
    order: AdmissionOrder,
    slot: oneshot::Sender<Slot>,
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

/// The right to run one decode; dropping it frees the slot for the next
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
        let slot = {
            let mut state = self.state.lock();
            if state.free > 0 && state.waiting.is_empty() {
                state.free -= 1;
                None
            } else {
                let (tx, rx) = oneshot::channel();
                state.waiting.push(Waiter { order, slot: tx });
                Some(rx)
            }
        };
        match slot {
            None => Slot {
                queue: Some(Arc::clone(self)),
            },
            Some(rx) => rx
                .await
                .expect("BUG: a queued waiter is only dropped after it is handed a slot"),
        }
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
            // Sent outside the lock. A waiter abandoned after this send drops
            // the slot with its channel, which releases it again.
            match waiter.slot.send(Slot {
                queue: Some(Arc::clone(queue)),
            }) {
                Ok(()) => return,
                // Abandoned before the send: disarm the slot it refused and
                // try the next waiter.
                Err(mut refused) => refused.queue = None,
            }
        }
    }
}
