//! One host read that every reader asking while it is in flight shares
//! (ADR-0152 §5), so a burst of pastes holds one copy of the text and asks
//! the host once.

use std::{cell::RefCell, rc::Rc};

use flui_foundation::panic::FirstPanic;

use flui_platform::{
    ClipboardRead,
    capability::{Abandoned, Completion, CompletionSource, Resolver},
};

/// The readers one host read answers.
type Readers = Rc<RefCell<Vec<Resolver<ClipboardRead>>>>;

/// The read in flight of every handle of one UI runtime, if one is.
#[derive(Clone, Default)]
pub(crate) struct SharedRead {
    in_flight: Rc<RefCell<Option<Readers>>>,
}

impl SharedRead {
    /// The clipboard changes under this runtime's own write or claim: a read
    /// asked after it must not join a read asked before, whose answer may be
    /// the old text. The read in flight keeps answering its own readers.
    pub(crate) fn changed(&self) {
        let ended = self.in_flight.borrow_mut().take();
        drop(ended);
    }
    /// A completion for one more reader. It joins the read in flight, which
    /// it shares until that read's answer is handed out; otherwise `start`
    /// asks the host, answering a resolver that settles through
    /// `completions`.
    pub(crate) fn read(
        &self,
        completions: &CompletionSource,
        start: impl FnOnce(Resolver<ClipboardRead>),
    ) -> Completion<ClipboardRead> {
        let (completion, answer) = completions.pending();
        let readers = {
            let mut in_flight = self.in_flight.borrow_mut();
            if let Some(readers) = in_flight.as_ref() {
                readers.borrow_mut().push(answer);
                return completion;
            }
            let readers: Readers = Rc::new(RefCell::new(vec![answer]));
            *in_flight = Some(Rc::clone(&readers));
            readers
        };
        let (host, resolver) = completions.pending();
        let fan_out = FanOut {
            in_flight: Rc::clone(&self.in_flight),
            readers,
        };
        host.on_complete(move |outcome| fan_out.answer(outcome));
        // The backend may be an application's: no borrow is held while it
        // runs, and a panic in it drops the resolver, which abandons the
        // read and its readers.
        start(resolver);
        completion
    }
}

/// Hands one host read's answer to its readers.
struct FanOut {
    in_flight: Rc<RefCell<Option<Readers>>>,
    readers: Readers,
}

impl FanOut {
    /// Ends this read: the next reader starts another. Only this read's own
    /// entry is cleared, never a later one.
    fn take_readers(&self) -> Vec<Resolver<ClipboardRead>> {
        {
            let mut in_flight = self.in_flight.borrow_mut();
            if in_flight
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, &self.readers))
            {
                *in_flight = None;
            }
        }
        std::mem::take(&mut *self.readers.borrow_mut())
    }

    /// Every reader gets the same answer. An abandoned read abandons its
    /// readers: each resolver dropped here unresolved answers
    /// [`Abandoned::ProviderDropped`]. Each delivery is contained (it wakes
    /// the owner, whose hook may panic), so the readers after a failed one
    /// still get the answer, and the first failure goes on after them.
    fn answer(&self, outcome: Result<ClipboardRead, Abandoned>) {
        let readers = self.take_readers();
        let mut failure = FirstPanic::new();
        for reader in readers {
            match &outcome {
                // A retired owner has nobody left to answer.
                Ok(read) => failure.contain(|| {
                    let _ = reader.resolve(read.clone());
                }),
                Err(_) => failure.contain(|| drop(reader)),
            };
        }
        failure.resume();
    }
}

impl Drop for FanOut {
    /// The owner retired before the host answered, dropping the waiting
    /// closure unrun: the readers go with it, each drop contained, and the
    /// next read starts afresh. While the thread unwinds they are leaked
    /// rather than dropped into a second panic.
    fn drop(&mut self) {
        let readers = self.take_readers();
        if std::thread::panicking() {
            std::mem::forget(readers);
            return;
        }
        let mut failure = FirstPanic::new();
        for reader in readers {
            failure.contain(|| drop(reader));
        }
        failure.resume();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use flui_platform::capability::{OwnerJob, OwnerPoster, OwnerScope, PendingCompletions};

    use super::*;

    /// Where a test keeps what a reader settled with.
    type Settled = Rc<RefCell<Option<Result<ClipboardRead, Abandoned>>>>;

    /// A wake hook that panics once, after queueing the job it woke for, as
    /// a faulty owner hook does: the readers after it still get the answer.
    #[test]
    fn a_wake_that_panics_mid_fan_out_leaves_the_other_readers_answered() {
        let queue: Arc<Mutex<Vec<OwnerJob>>> = Arc::default();
        let posts = Arc::new(AtomicUsize::new(0));
        let poster = OwnerPoster::new({
            let (queue, posts) = (Arc::clone(&queue), Arc::clone(&posts));
            move |job| {
                queue.lock().expect("queue").push(job);
                // The host's answer is the first post; the first reader's
                // delivery is the second.
                assert_ne!(
                    posts.fetch_add(1, Ordering::SeqCst),
                    1,
                    "the wake hook fails"
                );
                Ok(())
            }
        });
        let pending = PendingCompletions::new();
        let completions = CompletionSource::new(poster, pending.clone());
        let turn = || {
            let jobs = std::mem::take(&mut *queue.lock().expect("queue"));
            let scope = OwnerScope::new(&pending);
            for job in jobs {
                job.run(&scope);
            }
        };

        let shared = SharedRead::default();
        let host = Cell::new(None);
        let settled: Vec<Settled> = (0..3)
            .map(|_| {
                let completion = shared.read(&completions, |answer| host.set(Some(answer)));
                let sink = Rc::new(RefCell::new(None));
                let into = Rc::clone(&sink);
                completion.on_complete(move |outcome| {
                    into.replace(Some(outcome));
                });
                sink
            })
            .collect();
        let text: Arc<str> = Arc::from("shared");
        let _ = host
            .take()
            .expect("one host read")
            .resolve(Ok(Some(Arc::clone(&text))));
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(turn));
        assert!(failed.is_err(), "the first failure goes on");
        turn();
        for reader in &settled {
            assert_eq!(*reader.borrow(), Some(Ok(Ok(Some(Arc::clone(&text))))));
        }
    }
}
