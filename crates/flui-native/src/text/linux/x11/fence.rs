//! The fences of one window's input context: the last focus asked of the
//! input method and the last it answered. Until they match, what it sends
//! was for the field it served before, and reaches none.

use std::cell::Cell;
use std::num::NonZeroU64;

use flui_foundation::{IdExhausted, IdMint};

/// One focus asked of the input method, named again by its answer
/// ([`Heard::Fenced`](super::bus::Heard::Fenced)). Minted by its window's
/// [`Fences`] and compared only with that window's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fence(NonZeroU64);

/// A window's fences, kept on the owner thread.
#[derive(Debug)]
pub(super) struct Fences {
    mint: IdMint<NonZeroU64>,
    /// The last focus asked, `Ok(None)` before the first; `Err` once a focus
    /// found the mint exhausted, which no answer matches.
    asked: Cell<Result<Option<Fence>, IdExhausted>>,
    /// The last focus the input method answered.
    answered: Cell<Option<Fence>>,
}

impl Default for Fences {
    /// No focus asked yet: what the input method sends is current.
    fn default() -> Self {
        Self {
            mint: IdMint::new(),
            asked: Cell::new(Ok(None)),
            answered: Cell::new(None),
        }
    }
}

impl Fences {
    /// A focus is asked: the fence its `FocusIn` carries.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once every fence was issued. What the input method
    /// sends is then never current again: no fence is left to tell the
    /// field it serves from the one before.
    pub(super) fn ask(&self) -> Result<Fence, IdExhausted> {
        let fence = self.mint.next().map(Fence);
        self.asked.set(fence.map(Some));
        fence
    }

    /// The input method answered the focus `fence` asked.
    pub(super) fn answer(&self, fence: Fence) {
        self.answered.set(Some(fence));
    }

    /// The context was replaced: the new one has sent nothing for an
    /// earlier field, so the last focus asked counts as answered, unless
    /// none could be asked.
    pub(super) fn settle(&self) {
        if let Ok(asked) = self.asked.get() {
            self.answered.set(asked);
        }
    }

    /// Whether what the input method sends now is for the field served now:
    /// it answered the last focus asked of it.
    pub(super) fn current(&self) -> bool {
        self.asked.get() == Ok(self.answered.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_focus_is_current_once_its_own_fence_is_answered() {
        let fences = Fences::default();
        assert!(fences.current(), "nothing asked, nothing fenced");
        let first = fences.ask().expect("a fresh mint issues");
        assert!(!fences.current());
        let second = fences.ask().expect("a fresh mint issues");
        fences.answer(first);
        assert!(!fences.current(), "an older answer fences nothing newer");
        fences.answer(second);
        assert!(fences.current());
    }

    /// Red-check: leave `asked` at the last fence when the mint refuses, or
    /// wrap or saturate it, and the answer to the focus before makes what
    /// the input method sends for that field look current.
    #[test]
    fn an_exhausted_mint_leaves_no_fence_current() {
        let fences = Fences {
            mint: IdMint::starting_after(u64::MAX - 1),
            ..Fences::default()
        };
        let last = fences.ask().expect("the last fence issues");
        fences.answer(last);
        assert!(fences.current());
        assert_eq!(fences.ask(), Err(IdExhausted));
        assert!(!fences.current());
        fences.answer(last);
        assert!(!fences.current(), "a late answer fences no refused focus");
        fences.settle();
        assert!(!fences.current(), "a new context does not either");
    }
}
