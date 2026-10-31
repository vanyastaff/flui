//! One host read that every reader asking while it is in flight shares
//! (ADR-0152 §5), so a burst of pastes holds one copy of the text and asks
//! the host once.

use std::{cell::RefCell, rc::Rc};

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
    /// [`Abandoned::ProviderDropped`].
    fn answer(&self, outcome: Result<ClipboardRead, Abandoned>) {
        let readers = self.take_readers();
        if let Ok(read) = outcome {
            for reader in readers {
                // A retired owner has nobody left to answer.
                let _ = reader.resolve(read.clone());
            }
        }
    }
}

impl Drop for FanOut {
    /// The owner retired before the host answered, dropping the waiting
    /// closure unrun: the readers go with it, and the next read starts
    /// afresh.
    fn drop(&mut self) {
        drop(self.take_readers());
    }
}
