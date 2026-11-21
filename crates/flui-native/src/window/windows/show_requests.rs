//! The app's requests to show a Win32 window, as a restore that hides the
//! window again reads them: a restore runs user callbacks, and one of them
//! asking for the window to be shown overtakes the hide that would follow.

use std::cell::Cell;
use std::num::NonZeroU64;

use flui_foundation::IdMint;

/// One request of the app to show its window, minted by the window's
/// [`ShowRequests`] and compared only with that window's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShowRequest(NonZeroU64);

/// What a step read of the show requests before it ran code it does not own
/// ([`ShowRequests::mark`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShowMark(Option<ShowRequest>);

/// A window's show requests: each one is minted, never counted, so no number
/// of requests between a mark and its check reads as none.
#[derive(Debug, Default)]
pub(crate) struct ShowRequests {
    mint: IdMint<NonZeroU64>,
    /// The last request noted; `None` before the first.
    last: Cell<Option<ShowRequest>>,
}

impl ShowRequests {
    /// Note that the app asked for the window to be shown. Once the mint is
    /// exhausted the request goes unnoted, and [`Self::overtook`] answers
    /// true from then on.
    pub(crate) fn note(&self) {
        if let Ok(raw) = self.mint.next() {
            self.last.set(Some(ShowRequest(raw)));
        }
    }

    /// The requests as they stand, for [`Self::overtook`] to compare.
    pub(crate) fn mark(&self) -> ShowMark {
        ShowMark(self.last.get())
    }

    /// Whether a show was asked for since `mark` was taken. An exhausted mint
    /// can no longer tell a request from none, so it answers true: a window
    /// left shown loses no request the app made, a window hidden again would.
    pub(crate) fn overtook(&self, mark: ShowMark) -> bool {
        self.mint.is_exhausted() || self.last.get() != mark.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_show_noted_after_a_mark_overtakes_it() {
        let requests = ShowRequests::default();
        let mark = requests.mark();
        assert!(!requests.overtook(mark));
        requests.note();
        assert!(requests.overtook(mark));
        let later = requests.mark();
        assert!(!requests.overtook(later));
    }

    /// Red-check: let the mint wrap or saturate, and the request noted past
    /// the end leaves the last one in place, so the restore hides a window
    /// the app asked to show.
    #[test]
    fn an_exhausted_mint_overtakes_every_mark() {
        let requests = ShowRequests {
            mint: IdMint::starting_after(u64::MAX - 1),
            last: Cell::new(None),
        };
        requests.note();
        let mark = requests.mark();
        assert!(requests.overtook(mark));
        requests.note();
        assert!(requests.overtook(mark));
    }
}
