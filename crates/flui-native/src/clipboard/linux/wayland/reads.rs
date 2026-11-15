//! The reads of a selection another client owns that are under way.
//!
//! A read asks the selection's owner to write its text into a pipe, and a
//! thread of its own waits for it up to the transfer's deadline. An
//! application reading faster than an owner answers would start a thread a
//! read, each held until the deadline. So the reads of one offer share one:
//! the first starts it, the ones that come while it is under way wait on its
//! answer, and every one of them is answered with the same text. At most
//! [`MAX_READERS`] wait at once over every offer, which bounds the threads
//! too; a read past them is refused at once, as the X11 host refuses it.

use std::os::fd::{AsFd, BorrowedFd};
use std::sync::Arc;

use flui_foundation::panic::FirstPanic;
use flui_platform::{ClipboardError, ClipboardRead, capability::Resolver};
use parking_lot::Mutex;

use crate::clipboard::text::TextForm;

/// The most reads of a selection waiting at once. A handle shares one read
/// among all its readers, so each waiting read is another handle's; more are
/// refused at once as [`ClipboardError::Unavailable`].
pub(super) const MAX_READERS: usize = 4;

/// The readers of one read under way; `None` once it was answered.
type Waiting<Reader> = Arc<Mutex<Option<Vec<Reader>>>>;

/// The reads under way of one selection (the clipboard's, or the primary
/// one), each of one offer, with the readers waiting on each.
pub(super) struct Reads<Offer, Reader> {
    under_way: Vec<(Offer, Waiting<Reader>)>,
}

impl<Offer, Reader> Default for Reads<Offer, Reader> {
    fn default() -> Self {
        Self {
            under_way: Vec::new(),
        }
    }
}

/// What becomes of a read.
pub(super) enum Admitted<Reader> {
    /// It waits on the read of its offer already under way.
    Joined,
    /// It is the first of its offer: the caller starts the read, which this
    /// answers.
    Started(Answer<Reader>),
    /// [`MAX_READERS`] wait already: its reader is handed back, to be
    /// refused.
    Refused(Reader),
}

/// The end of one read under way: its readers, each to be answered. Dropped
/// with its readers not taken (no thread could be started), it drops them,
/// which ends each read as abandoned.
pub(super) struct Answer<Reader>(Waiting<Reader>);

impl<Reader> Answer<Reader> {
    /// The readers that waited on the read, which is over: a read of the
    /// same offer from here starts another.
    pub(super) fn readers(self) -> Vec<Reader> {
        self.0.lock().take().unwrap_or_default()
    }
}

impl<Reader> Drop for Answer<Reader> {
    fn drop(&mut self) {
        // A reader's drop wakes its owner: outside the lock.
        let abandoned = self.0.lock().take();
        drop(abandoned);
    }
}

impl<Offer: PartialEq, Reader> Reads<Offer, Reader> {
    /// Admit `reader`'s read of `offer`.
    pub(super) fn admit(&mut self, offer: Offer, reader: Reader) -> Admitted<Reader> {
        self.under_way
            .retain(|(_, waiting)| waiting.lock().is_some());
        let waiting: usize = self
            .under_way
            .iter()
            .map(|(_, waiting)| waiting.lock().as_ref().map_or(0, Vec::len))
            .sum();
        if waiting >= MAX_READERS {
            return Admitted::Refused(reader);
        }
        if let Some(index) = self.under_way.iter().position(|(held, _)| *held == offer) {
            if let Some(readers) = self.under_way[index].1.lock().as_mut() {
                readers.push(reader);
                return Admitted::Joined;
            }
            // Answered since the look above: this read starts another.
            self.under_way.swap_remove(index);
        }
        let waiting = Arc::new(Mutex::new(Some(vec![reader])));
        self.under_way.push((offer, Arc::clone(&waiting)));
        Admitted::Started(Answer(waiting))
    }
}

/// Start the read `end` answers. `receive` asks the offer's owner to write
/// the text into the descriptor it is handed, and a thread named `thread`
/// reads it as `form`, bounded in time and size, and answers every reader
/// that waited with it. Where no pipe or no thread can be had, the readers
/// are refused or abandoned.
pub(super) fn start(
    end: Answer<Resolver<ClipboardRead>>,
    form: TextForm,
    thread: &str,
    receive: impl FnOnce(BorrowedFd<'_>),
) {
    let (reader, writer) = match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
        Ok(ends) => ends,
        Err(error) => {
            answer(end, &Err(ClipboardError::Unavailable));
            tracing::warn!(%error, "no pipe for a selection read");
            return;
        }
    };
    receive(writer.as_fd());
    // The owner holds the write end now; the loop's next flush sends the
    // request.
    drop(writer);
    let spawned = std::thread::Builder::new()
        .name(thread.to_owned())
        .spawn(move || {
            let text = super::read_selection(reader).and_then(|bytes| form.answer(bytes));
            answer(end, &text);
        });
    // The closure, and the readers in it, is dropped: each read ends as
    // abandoned by its provider.
    if let Err(error) = spawned {
        tracing::warn!(%error, "no thread for a selection read");
    }
}

/// Answer every reader of `end` with `text`. A delivery that panics (the
/// owner's wake hook) leaves the others answered, and the first failure
/// goes on.
fn answer(end: Answer<Resolver<ClipboardRead>>, text: &ClipboardRead) {
    let mut failure = FirstPanic::new();
    for reader in end.readers() {
        // A retired owner has nobody left to answer.
        failure.contain(|| reader.resolve(text.clone()));
    }
    failure.resume();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer of a started read; a test's reads start nothing else.
    fn started(admitted: Admitted<u32>) -> Answer<u32> {
        match admitted {
            Admitted::Started(end) => end,
            Admitted::Joined => panic!("joined a read"),
            Admitted::Refused(reader) => panic!("reader {reader} was refused"),
        }
    }

    /// An application reading again and again while the selection's owner
    /// answers nothing starts one read: the next readers wait on it, the
    /// ones past the most that wait are refused at once, and its answer
    /// reaches every one that waited. A read after the answer starts anew.
    #[test]
    fn reads_of_one_offer_share_one_answer() {
        let mut reads = Reads::<&str, u32>::default();
        let end = started(reads.admit("offer", 0));
        let mut refused = Vec::new();
        for reader in 1..10_000 {
            match reads.admit("offer", reader) {
                Admitted::Joined => {}
                Admitted::Refused(reader) => refused.push(reader),
                Admitted::Started(_) => panic!("reader {reader} started a second read"),
            }
        }
        assert_eq!(refused, (4..10_000).collect::<Vec<u32>>());
        assert_eq!(end.readers(), [0, 1, 2, 3], "every one that waited");

        let again = started(reads.admit("offer", 10_000));
        assert_eq!(again.readers(), [10_000]);
    }

    /// A read of another offer, the selection having changed, joins no read
    /// of the one before it, and counts among the most that wait; a read
    /// whose answer is dropped unanswered waits no more.
    #[test]
    fn a_read_of_another_offer_starts_its_own() {
        let mut reads = Reads::<&str, u32>::default();
        let first = started(reads.admit("first", 0));
        assert!(matches!(reads.admit("first", 1), Admitted::Joined));
        let second = started(reads.admit("second", 2));
        assert!(matches!(reads.admit("second", 3), Admitted::Joined));
        assert!(matches!(reads.admit("third", 4), Admitted::Refused(4)));

        drop(first);
        let third = started(reads.admit("third", 4));
        assert_eq!(second.readers(), [2, 3]);
        assert_eq!(third.readers(), [4]);
        assert!(reads.under_way.len() <= MAX_READERS);
    }
}
