//! Identity of one input sequence, and the owner-local mint that issues it
//! (ADR-0207 §4).
//!
//! A sequence is everything one source produces from its start to its end: a
//! contact from Down to Up or Cancel, a wheel burst, a keyboard repeat, an
//! assistive or programmatic action, an auto-scroll. The arena, the scroll
//! channel, drag sessions, recordings and observers name the same sequence by
//! the same [`InputSequenceId`], so none of them needs a translation table.
//!
//! The id lives here, below the contract crates, so `flui-platform-api` and
//! every consumer can name it; only the interaction layer mints it.

use std::cell::Cell;
use std::marker::PhantomData;
use std::num::NonZeroU64;

/// The identity of one input sequence within the binding that minted it.
///
/// Minted only by an [`IdMint`], never reused by it: there is no `Default`
/// and no conversion from a number, so a value always names a sequence some
/// owner started.
///
/// ```rust
/// use flui_foundation::{IdMint, InputSequenceId};
///
/// let mint = IdMint::<InputSequenceId>::new();
/// let first = mint.next()?;
/// let second = mint.next()?;
/// assert!(first < second);
/// # Ok::<(), flui_foundation::IdExhausted>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputSequenceId(NonZeroU64);

impl InputSequenceId {
    /// The raw value, for diagnostics and recording rows only.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

mod sealed {
    pub trait Sealed {}

    /// Proof that the call comes from a mint in this crate.
    #[derive(Debug)]
    pub struct MintToken(pub(super) ());
}

/// An identity an [`IdMint`] can issue. Sealed: each implementor is an id
/// whose only source is a mint.
pub trait MintedId: sealed::Sealed + Copy {
    #[doc(hidden)]
    fn from_mint(raw: NonZeroU64, token: sealed::MintToken) -> Self;
}

impl sealed::Sealed for InputSequenceId {}

impl MintedId for InputSequenceId {
    fn from_mint(raw: NonZeroU64, _token: sealed::MintToken) -> Self {
        Self(raw)
    }
}

/// An owner-local issuer of identities that never reissues one.
///
/// Values are increasing. Once the last representable value has been issued,
/// every later [`next`](Self::next) refuses with [`IdExhausted`]: the mint
/// never wraps, so a stale id can never come back as a fresh one.
#[derive(Debug)]
pub struct IdMint<I> {
    /// The last value issued; `u64::MAX` once exhausted.
    last: Cell<u64>,
    _id: PhantomData<fn() -> I>,
}

impl<I: MintedId> IdMint<I> {
    /// A mint whose first id is 1.
    #[must_use]
    pub const fn new() -> Self {
        Self::starting_after(0)
    }

    /// A mint whose first id is `last + 1`, so a test can reach the terminal
    /// value without issuing every id before it. A mint started at
    /// `u64::MAX` is already exhausted.
    #[must_use]
    pub const fn starting_after(last: u64) -> Self {
        Self {
            last: Cell::new(last),
            _id: PhantomData,
        }
    }

    /// Issue the next identity.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once the last representable value was issued; the mint
    /// refuses permanently from then on.
    pub fn next(&self) -> Result<I, IdExhausted> {
        let next = self.last.get().checked_add(1).ok_or(IdExhausted)?;
        self.last.set(next);
        let raw = NonZeroU64::new(next).expect("BUG: an incremented u64 is never zero");
        Ok(I::from_mint(raw, sealed::MintToken(())))
    }
}

impl<I: MintedId> Default for IdMint<I> {
    fn default() -> Self {
        Self::new()
    }
}

/// An [`IdMint`] issued every identity it can represent and refuses to issue
/// another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the identity space of this mint is exhausted")]
pub struct IdExhausted;
