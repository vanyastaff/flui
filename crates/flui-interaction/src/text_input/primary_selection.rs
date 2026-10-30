//! The primary selection: the text the user selected last, which a middle
//! click pastes, on the platforms that have one (X11 and Wayland). A
//! capability of the seam beside the clipboard (ADR-0152); the platforms
//! without it register no provider, so an acquisition there is refused.
//!
//! A widget acquires a [`PrimarySelectionHandle`] through
//! `cx.capability::<PrimarySelectionCapability>()` in a lifecycle hook. A
//! selection is offered lazily: [`claim`](PrimarySelectionHandle::claim)
//! hands the platform a [`SelectionText`], which is asked for its text only
//! when another application reads the selection, so selecting with a drag
//! copies nothing per frame. The returned [`PrimaryClaim`] holds the
//! selection; another claim, or another application selecting, makes it
//! stale, and dropping it releases the selection only while it is still
//! current.

use std::{fmt, num::NonZeroU64, rc::Rc};

use flui_platform::capability::{Capability, Completion, CompletionSource, Resolver};

/// The primary selection capability:
/// `cx.capability::<PrimarySelectionCapability>()` hands out a
/// [`PrimarySelectionHandle`].
#[derive(Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum PrimarySelectionCapability {}

impl Capability for PrimarySelectionCapability {
    type Handle = PrimarySelectionHandle;
    const NAME: &'static str = "primary-selection";
}

/// What a claim of the primary selection offers: asked for its text when
/// another application, or this one, reads the selection.
///
/// Owner-local; asked on the owner thread, never while the platform holds a
/// borrow of its own state, so an implementation may read the widget state
/// it shares. Richer forms of the selection come as provided methods, so an
/// implementation that offers text alone keeps compiling.
pub trait SelectionText: 'static {
    /// The selected text now, or `None` once there is none to give.
    fn text(&self) -> Option<String>;
}

impl<F: Fn() -> Option<String> + 'static> SelectionText for F {
    fn text(&self) -> Option<String> {
        self()
    }
}

/// One claim of the primary selection, as [`PrimaryClaims`] numbers them.
///
/// Claims are numbered once per selection and never reuse a number: a claim
/// made after the numbers run out is never current, so a stale claim can
/// never look current again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryClaimId(Option<NonZeroU64>);

/// The claims of one primary selection: which one is current and what it
/// offers. A [`PrimarySelectionBackend`] keeps one and answers through it.
///
/// It runs no user code: what it gives back (a released or replaced
/// [`SelectionText`]) the caller drops, or asks, once it has let go of any
/// borrow of its own.
pub struct PrimaryClaims {
    /// The number the next claim takes; `None` once the numbers ran out.
    next: Option<NonZeroU64>,
    current: Option<(NonZeroU64, Rc<dyn SelectionText>)>,
}

impl PrimaryClaims {
    /// No claim yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: Some(NonZeroU64::MIN),
            current: None,
        }
    }

    /// Make `text` the current claim: its number, and the claim it replaced.
    pub fn claim(
        &mut self,
        text: Rc<dyn SelectionText>,
    ) -> (PrimaryClaimId, Option<Rc<dyn SelectionText>>) {
        let Some(number) = self.next else {
            // Exhausted for good: the claim is never current, and the
            // current one stays.
            return (PrimaryClaimId(None), None);
        };
        self.next = number.checked_add(1);
        let replaced = self.current.replace((number, text)).map(|(_, text)| text);
        (PrimaryClaimId(Some(number)), replaced)
    }

    /// End `claim` if it is still current: what it offered, which the
    /// platform stops offering; `None` for a stale claim.
    pub fn release(&mut self, claim: PrimaryClaimId) -> Option<Rc<dyn SelectionText>> {
        if !self.is_current(claim) {
            return None;
        }
        self.current.take().map(|(_, text)| text)
    }

    /// Another application selected: no claim of this one is current.
    /// Returns what the current claim offered.
    pub fn lose(&mut self) -> Option<Rc<dyn SelectionText>> {
        self.current.take().map(|(_, text)| text)
    }

    /// Whether `claim` holds the selection.
    #[must_use]
    pub fn is_current(&self, claim: PrimaryClaimId) -> bool {
        claim.0.is_some_and(|number| {
            self.current
                .as_ref()
                .is_some_and(|(current, _)| *current == number)
        })
    }

    /// What the current claim offers, to ask for its text.
    #[must_use]
    pub fn current(&self) -> Option<Rc<dyn SelectionText>> {
        self.current.as_ref().map(|(_, text)| Rc::clone(text))
    }

    /// The last number before exhaustion, where a test starts claims that
    /// make no 2^64 of them.
    #[cfg(test)]
    pub(crate) const fn at_last_number() -> Self {
        Self {
            next: Some(NonZeroU64::MAX),
            current: None,
        }
    }
}

impl Default for PrimaryClaims {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PrimaryClaims {
    fn drop(&mut self) {
        // Dropped while unwinding, the current claim's captures are kept
        // rather than dropped into a second panic (ADR-0127).
        if std::thread::panicking()
            && let Some(current) = self.current.take()
        {
            std::mem::forget(current);
        }
    }
}

impl fmt::Debug for PrimaryClaims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrimaryClaims")
            .field("next", &self.next)
            .field("current", &self.current.as_ref().map(|(number, _)| number))
            .finish()
    }
}

/// The platform's primary selection as a handle reaches it.
///
/// Owner-local. A backend answers a read through its [`Resolver`] when the
/// platform answers, at once or later; it never calls back into the handle,
/// and asks a [`SelectionText`] only with no borrow of its own held.
///
/// What a claim or a release lets go of, the backend returns rather than
/// drops: the handle drops it once the new claim has a guard, and keeps it
/// instead while unwinding (ADR-0127), so a panicking capture never leaves
/// a claim without its guard nor turns into a second panic.
pub trait PrimarySelectionBackend: 'static {
    /// Offer `text` as the primary selection, replacing any claim before:
    /// the claim's number, and what the replaced claim offered.
    fn claim(&self, text: Rc<dyn SelectionText>)
    -> (PrimaryClaimId, Option<Rc<dyn SelectionText>>);

    /// Stop offering `claim`'s text if it is still current, and return it;
    /// nothing for a stale claim.
    fn release(&self, claim: PrimaryClaimId) -> Option<Rc<dyn SelectionText>>;

    /// Whether `claim` still holds the selection: no claim came since and
    /// no other application selected.
    fn is_current(&self, claim: PrimaryClaimId) -> bool;

    /// Read the selection's text and answer `answer` with it, or with `None`
    /// when there is none.
    fn read_text(&self, answer: Resolver<Option<String>>);
}

/// Widget-facing primary selection for one presentation.
///
/// Owner-thread (`!Send`), cheap to clone. A read answers with a
/// [`Completion`], which settles at an owner turn after the platform answers.
#[derive(Clone)]
pub struct PrimarySelectionHandle {
    backend: Rc<dyn PrimarySelectionBackend>,
    completions: CompletionSource,
}

impl PrimarySelectionHandle {
    /// A handle over `backend`, whose reads settle through `completions`.
    #[must_use]
    pub fn new(backend: impl PrimarySelectionBackend, completions: CompletionSource) -> Self {
        Self {
            backend: Rc::new(backend),
            completions,
        }
    }

    /// Offer `text` as the primary selection until the returned claim drops
    /// or another claim replaces it.
    #[must_use = "dropping the claim releases the selection"]
    pub fn claim(&self, text: impl SelectionText) -> PrimaryClaim {
        let (id, replaced) = self.backend.claim(Rc::new(text));
        let claim = PrimaryClaim {
            backend: Rc::clone(&self.backend),
            id,
        };
        // The replaced captures drop with the new claim guarded: if their
        // `Drop` panics, the unwind drops `claim`, which withdraws it unless
        // that `Drop` claimed anew since.
        drop(replaced);
        claim
    }

    /// Read the primary selection's text: `None` when there is none.
    ///
    /// The completion settles at an owner turn after the platform answers,
    /// never inside this call.
    pub fn read_text(&self) -> Completion<Option<String>> {
        let (completion, answer) = self.completions.pending();
        self.backend.read_text(answer);
        completion
    }
}

impl fmt::Debug for PrimarySelectionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrimarySelectionHandle")
            .finish_non_exhaustive()
    }
}

/// A claim of the primary selection. Dropping it releases the selection if
/// it is still current; a stale claim releases nothing, so a widget that
/// lost the selection never clears another's.
#[must_use = "dropping the claim releases the selection"]
pub struct PrimaryClaim {
    backend: Rc<dyn PrimarySelectionBackend>,
    id: PrimaryClaimId,
}

impl PrimaryClaim {
    /// Whether this claim still holds the selection.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.backend.is_current(self.id)
    }
}

impl Drop for PrimaryClaim {
    fn drop(&mut self) {
        let released = self.backend.release(self.id);
        // While unwinding, what the claim offered is withdrawn but kept, not
        // dropped into a second panic (ADR-0127).
        if std::thread::panicking() {
            std::mem::forget(released);
        }
    }
}

impl fmt::Debug for PrimaryClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrimaryClaim")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &'static str) -> Rc<dyn SelectionText> {
        Rc::new(move || Some(value.to_owned()))
    }

    /// A newer claim makes the older stale: releasing the older leaves the
    /// newer current, and losing to another application ends both.
    #[test]
    fn only_the_current_claim_releases() {
        let mut claims = PrimaryClaims::new();
        let (first, replaced) = claims.claim(text("first"));
        assert!(replaced.is_none());
        let (second, replaced) = claims.claim(text("second"));
        assert!(replaced.is_some(), "the second claim replaces the first");
        assert!(!claims.is_current(first));
        assert!(
            claims.release(first).is_none(),
            "a stale claim releases nothing"
        );
        assert!(claims.is_current(second));
        assert!(claims.lose().is_some());
        assert!(!claims.is_current(second));
        assert!(claims.release(second).is_none());
    }

    /// Past the last number a claim is never current and leaves the current
    /// one in place.
    #[test]
    fn exhausted_numbers_make_no_current_claim() {
        let mut claims = PrimaryClaims::at_last_number();
        let (last, _) = claims.claim(text("last"));
        assert!(claims.is_current(last));
        let (past, replaced) = claims.claim(text("past"));
        assert!(replaced.is_none());
        assert!(!claims.is_current(past));
        assert!(
            claims.is_current(last),
            "the exhausted claim replaced nothing"
        );
        assert!(claims.release(past).is_none());
    }
}
