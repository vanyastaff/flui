//! The requests that leave the main thread: whatever touches the file
//! system, waits on a daemon or asks Launch Services runs as a work item on
//! a global Grand Central Dispatch queue (ADR-0360 §2), holding one of the
//! runtime's off-owner places ([`crate::off_owner`]), and answers its
//! caller's resolver from there.

use flui_platform::capability::Resolver;

use crate::off_owner::OffOwner;

impl OffOwner {
    /// Run `work` on a global queue and answer `answer` with what it
    /// returns; answer `busy` at once when no place is free.
    ///
    /// The work item moves its captures into the boundary, so the place,
    /// the resolver and `work` drop inside it: a panic in `work` is logged
    /// and the resolver, dropped unanswered, ends the caller's completion
    /// as `ProviderDropped`.
    pub(crate) fn dispatch<T: Send + 'static>(
        &self,
        answer: Resolver<T>,
        busy: T,
        work: impl FnOnce() -> T + Send + 'static,
    ) {
        use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier};

        use flui_foundation::panic::{AfterPanic, os_entry};

        let Some(place) = self.place() else {
            // The owner retiring first leaves nothing waiting for the answer.
            let _ = answer.resolve(busy);
            return;
        };
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
            DispatchQoS::UserInitiated,
        ));
        queue.exec_async(move || {
            os_entry(
                "flui-system work item",
                (),
                AfterPanic::Answer(|()| ()),
                move || {
                    let _place = place;
                    let value = work();
                    // The owner retiring first leaves nothing waiting.
                    let _ = answer.resolve(value);
                },
            );
        });
    }
}
