//! What the web host's page holds of the clipboard: the text of its last
//! write the clipboard took, which a read the browser refuses answers.
//!
//! The browser settles each `writeText` later, and may refuse it. The page
//! keeps a write's text only once the system clipboard took it, so a refused
//! write is never what a later read in the page answers (ADR-0337 §1), and a
//! page with no Clipboard API at all keeps every write for itself, which its
//! write answers `ApplicationOnly`. Writes settle in
//! any order: a write's
//! text is kept only if no later write's text was kept before it.

#![forbid(unsafe_code)]

use std::sync::Arc;

use flui_platform::{ClipboardError, ClipboardRead, ClipboardWritten};
use parking_lot::Mutex;

use crate::host::owned_text;

/// The page's own clipboard. Shared by the clipboard and the tasks that wait
/// for the browser's promises; the page has one thread.
#[derive(Default)]
pub(crate) struct PageClipboard {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The text the page holds, with the number of the write that left it.
    text: Option<(u64, Arc<str>)>,
    /// How many writes the page began.
    begun: u64,
}

/// One write the page began, waiting for the browser's answer.
#[derive(Debug)]
pub(crate) struct PageWrite {
    number: u64,
    text: Arc<str>,
}

impl PageClipboard {
    /// Begin a write of `text`; its number orders it among the others.
    pub(crate) fn begin(&self, text: Arc<str>) -> PageWrite {
        let mut state = self.state.lock();
        state.begun = state.begun.saturating_add(1);
        PageWrite {
            number: state.begun,
            text,
        }
    }

    /// The browser answered `write` with `written`: the page keeps its text
    /// when the system clipboard took it, or the page alone keeps it
    /// (`ApplicationOnly`), and no later write's text is kept.
    /// Returns `written`, the write's answer.
    pub(crate) fn settle(&self, write: PageWrite, written: ClipboardWritten) -> ClipboardWritten {
        if matches!(written, Ok(()) | Err(ClipboardError::ApplicationOnly)) {
            let mut state = self.state.lock();
            let newer_kept = state
                .text
                .as_ref()
                .is_some_and(|(number, _)| *number > write.number);
            if !newer_kept {
                // The text replaced is the page's own: dropping it runs no
                // user code.
                state.text = Some((write.number, write.text));
            }
        }
        written
    }

    /// What a read the browser did not answer gives: the text the page
    /// holds, or `error` when it holds none.
    pub(crate) fn refused_read(&self, error: ClipboardError) -> ClipboardRead {
        // The lock is let go before the read is admitted: a refusal logs, and
        // a subscriber is user code.
        let held = self
            .state
            .lock()
            .text
            .as_ref()
            .map(|(_, text)| Arc::clone(text));
        held.as_ref().map_or(Err(error), owned_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(read: ClipboardRead) -> Result<Option<String>, ClipboardError> {
        read.map(|text| text.as_deref().map(str::to_owned))
    }

    /// A write the browser refused is not what a later refused read answers:
    /// the page keeps the text of the last write the clipboard took.
    #[test]
    fn a_refused_write_is_not_read_back() {
        let page = PageClipboard::default();
        let taken = page.begin(Arc::from("taken"));
        assert_eq!(page.settle(taken, Ok(())), Ok(()));
        let refused = page.begin(Arc::from("refused"));
        assert_eq!(
            page.settle(refused, Err(ClipboardError::Denied)),
            Err(ClipboardError::Denied)
        );
        assert_eq!(
            text(page.refused_read(ClipboardError::Denied)),
            Ok(Some("taken".to_owned()))
        );
    }

    /// A write no system clipboard could take answers `ApplicationOnly`, not
    /// `Ok`, and the page's own reads answer its text.
    #[test]
    fn a_write_the_page_alone_keeps_is_not_ok_but_is_read_back() {
        let page = PageClipboard::default();
        let kept = page.begin(Arc::from("kept"));
        assert_eq!(
            page.settle(kept, Err(ClipboardError::ApplicationOnly)),
            Err(ClipboardError::ApplicationOnly)
        );
        assert_eq!(
            text(page.refused_read(ClipboardError::Unavailable)),
            Ok(Some("kept".to_owned()))
        );
    }

    /// Before any write was taken, a refused read answers its refusal.
    #[test]
    fn a_page_that_holds_nothing_answers_the_refusal() {
        let page = PageClipboard::default();
        let refused = page.begin(Arc::from("refused"));
        let _ = page.settle(refused, Err(ClipboardError::Unavailable));
        assert_eq!(
            text(page.refused_read(ClipboardError::Denied)),
            Err(ClipboardError::Denied)
        );
    }

    /// Writes the browser answers out of order leave the later write's text.
    #[test]
    fn a_write_answered_late_does_not_replace_a_later_one() {
        let page = PageClipboard::default();
        let first = page.begin(Arc::from("first"));
        let second = page.begin(Arc::from("second"));
        let _ = page.settle(second, Ok(()));
        let _ = page.settle(first, Ok(()));
        assert_eq!(
            text(page.refused_read(ClipboardError::Denied)),
            Ok(Some("second".to_owned()))
        );
    }
}
