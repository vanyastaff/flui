//! Identity of one input sequence, and the owner-local mint that issues it
//! (ADR-0207 §4).
//!
//! A sequence is everything one source produces from its start to its end: a
//! contact from Down to Up or Cancel, a wheel burst, a keyboard repeat, an
//! assistive or programmatic action, an auto-scroll. The arena, the scroll
//! channel, drag sessions, recordings and observers name the same sequence by
//! the same [`InputSequenceId`], so none of them needs a translation table.
//!
//! The [`IdMint`] family is the workspace's only issuer of identities: the
//! owner-local [`IdMint`], and its cross-thread twin [`AtomicIdMint`] for ids
//! minted on two threads. Both share the sealed [`MintedId`]
//! contract and the same refusal rule: an issued id is never issued again,
//! and an exhausted mint refuses, typed, forever.
//!
//! The sequence id lives here, below the contract crates, so `flui-platform`
//! and every consumer can name it; only the interaction layer mints it.

use std::cell::Cell;
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The raw value, for a recording that names the sequence by value; there is
/// no conversion back.
impl From<InputSequenceId> for NonZeroU64 {
    fn from(id: InputSequenceId) -> Self {
        id.0
    }
}

mod sealed {
    pub trait Sealed {}

    /// Proof that the call comes from a mint in this crate.
    #[derive(Debug)]
    pub struct MintToken(pub(super) ());
}

/// An identity an [`IdMint`] can issue. Sealed: each implementor is an id
/// whose only source is a mint — no `Default`, no conversion from a number
/// (`ListenerId`, `FrameCallbackId`, `TaskId`, `InputSequenceId`), an id a
/// private wire representation names back (`DragSessionId`), or the raw
/// `NonZeroU64` issue for an identity type defined above this crate, whose
/// owner keeps its constructor private.
pub trait MintedId: sealed::Sealed + Copy {
    /// The highest raw value a mint for this id hands out: the id type's own
    /// space width, so every raw stays convertible where
    /// [`MintedId::from_mint`] wraps it, and no mint site has to remember
    /// the width.
    #[doc(hidden)]
    const MINT_CEILING: u64;

    #[doc(hidden)]
    fn from_mint(raw: NonZeroU64, token: sealed::MintToken) -> Self;
}

impl sealed::Sealed for InputSequenceId {}

impl MintedId for InputSequenceId {
    const MINT_CEILING: u64 = u64::MAX;

    fn from_mint(raw: NonZeroU64, _token: sealed::MintToken) -> Self {
        Self(raw)
    }
}

/// The identity of one drag-and-drop session within the UI runtime that minted
/// it (ADR-0211 §3). Combined with the process nonce and the runtime id it names
/// a session across presentations; minted only by an [`IdMint`], so exhaustion
/// refuses permanently instead of wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DragSessionId(NonZeroU64);

impl DragSessionId {
    /// The raw value, for the private identity representation and diagnostics.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// The identity a private identity representation named. The value is a
    /// claim, not proof of a live session: a session table still checks it.
    #[must_use]
    pub const fn from_wire(raw: NonZeroU64) -> Self {
        Self(raw)
    }
}

impl sealed::Sealed for DragSessionId {}

impl MintedId for DragSessionId {
    const MINT_CEILING: u64 = u64::MAX;

    fn from_mint(raw: NonZeroU64, _token: sealed::MintToken) -> Self {
        Self(raw)
    }
}

impl sealed::Sealed for NonZeroU64 {}

/// The raw issue, for an identity type defined above this crate: its owner
/// keeps the mint and the type's constructor private to itself, so the
/// never-reissue guarantee holds for the wrapped type too.
impl MintedId for NonZeroU64 {
    const MINT_CEILING: u64 = u64::MAX;

    fn from_mint(raw: NonZeroU64, _token: sealed::MintToken) -> Self {
        raw
    }
}

/// Declares an opaque identity minted only by the [`IdMint`] family: a
/// private `NonZeroU64`, no `Default`, no conversion from a number, so the
/// mint's never-reissue rule holds for every value of the type.
macro_rules! minted_ids {
    ($($(#[$meta:meta])* $name:ident;)*) => {$(
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// The raw value, for diagnostics only; there is no conversion
            /// back.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl sealed::Sealed for $name {}

        impl MintedId for $name {
            const MINT_CEILING: u64 = u64::MAX;

            fn from_mint(raw: NonZeroU64, _token: sealed::MintToken) -> Self {
                Self(raw)
            }
        }
    )*};
}

minted_ids! {
    /// The identity of one listener registration within the listener list
    /// that minted it.
    ///
    /// The list's owner mints it from its own [`IdMint`], on its own
    /// thread. A source outside the framework that keeps its own listener
    /// ledger mints from an `IdMint<ListenerId>` of its own; no value is
    /// built from a number.
    ///
    /// ```compile_fail
    /// // A listener id is minted, never built from a number.
    /// let forged = flui_foundation::ListenerId::new(1);
    /// ```
    ListenerId;

    /// The identity of one scheduler frame callback (transient, persistent or
    /// post-frame), minted by its scheduler's own [`IdMint`] on the owner
    /// thread.
    ///
    /// ```compile_fail
    /// // A frame-callback id is minted, never built from a number.
    /// let forged = flui_foundation::FrameCallbackId::zip(1);
    /// ```
    FrameCallbackId;

    /// The identity of one task in a priority task queue, minted by an
    /// [`AtomicIdMint`]: tasks are created on whichever thread asks.
    ///
    /// ```compile_fail
    /// // A task id is minted, never built from a number.
    /// let forged = flui_foundation::TaskId::new(1);
    /// ```
    TaskId;
}

/// An owner-local issuer of identities that never reissues one.
///
/// Values are increasing. Once the last value the mint may issue has been
/// issued, every later [`next`](Self::next) refuses with [`IdExhausted`]: the
/// mint never wraps, so a stale id can never come back as a fresh one.
#[derive(Debug)]
pub struct IdMint<I> {
    /// The last value issued; at or past `ceiling` once exhausted.
    last: Cell<u64>,
    /// The last value this mint may issue.
    ceiling: u64,
    _id: PhantomData<fn() -> I>,
}

impl<I: MintedId> IdMint<I> {
    /// A mint whose first id is 1.
    #[must_use]
    pub const fn new() -> Self {
        Self::starting_after(0)
    }

    /// A mint whose first id is `last + 1`, so a test can reach the terminal
    /// value without issuing every id before it. A mint started at or past
    /// its ceiling is already exhausted.
    #[must_use]
    pub const fn starting_after(last: u64) -> Self {
        Self {
            last: Cell::new(last),
            // No wider than the id's own space: `[MintedId::MINT_CEILING]`
            // caps the raw [`from_mint`](MintedId::from_mint) converts back.
            ceiling: I::MINT_CEILING,
            _id: PhantomData,
        }
    }

    /// This mint issuing no value above `ceiling`: the values past it are
    /// reserved for identities no mint issues. Narrows only — a ceiling
    /// above the current one is refused in favor of the current one, which
    /// is the id type's own ceiling or an already-narrowed reservation.
    /// A mint whose start is at or past its ceiling is already exhausted.
    #[must_use]
    pub const fn ending_at(self, ceiling: u64) -> Self {
        Self {
            // `u64::min` is not a const fn yet; the comparison is.
            ceiling: if ceiling < self.ceiling {
                ceiling
            } else {
                self.ceiling
            },
            ..self
        }
    }

    /// Never issue `value` or any value below it, for an owner that hands out
    /// the low identities itself. Never moves the mint backwards.
    pub fn reserve_through(&self, value: u64) {
        self.last.set(self.last.get().max(value));
    }

    /// Whether the next [`next`](Self::next) refuses.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.last.get() >= self.ceiling
    }

    /// Issue the next identity.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once the last value the mint may issue was issued; the
    /// mint refuses permanently from then on.
    pub fn next(&self) -> Result<I, IdExhausted> {
        let raw = self
            .last
            .get()
            .checked_add(1)
            .filter(|next| *next <= self.ceiling)
            .and_then(NonZeroU64::new)
            .ok_or(IdExhausted)?;
        self.last.set(raw.get());
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

/// The cross-thread twin of [`IdMint`]: the same sealed [`MintedId`] contract
/// and the same refusal rule, issued from any thread.
///
/// For an id domain more than one thread mints: the process's task
/// identities, created on whichever thread asks. Values are
/// increasing. Once the last value the mint may issue has been issued, every
/// later [`next`](Self::next) refuses with [`IdExhausted`] from every thread:
/// the mint never wraps, so a stale id can never come back as a fresh one.
///
/// ```rust
/// use std::num::NonZeroU64;
/// use flui_foundation::{AtomicIdMint, IdExhausted};
///
/// let mint = AtomicIdMint::<NonZeroU64>::new();
/// let first = mint.next()?;
/// let second = mint.next()?;
/// assert!(first < second);
///
/// let pinned = AtomicIdMint::<NonZeroU64>::starting_after(u64::MAX);
/// assert_eq!(pinned.next(), Err(IdExhausted));
/// # Ok::<(), flui_foundation::IdExhausted>(())
/// ```
#[derive(Debug)]
pub struct AtomicIdMint<I> {
    /// The last value issued; at or past `ceiling` once exhausted.
    last: AtomicU64,
    /// The last value this mint may issue.
    ceiling: u64,
    _id: PhantomData<fn() -> I>,
}

impl<I: MintedId> AtomicIdMint<I> {
    /// A mint whose first id is 1.
    #[must_use]
    pub const fn new() -> Self {
        Self::starting_after(0)
    }

    /// A mint whose first id is `last + 1`, so a test can reach the terminal
    /// value without issuing every id before it. A mint started at or past
    /// its ceiling is already exhausted.
    #[must_use]
    pub const fn starting_after(last: u64) -> Self {
        Self {
            last: AtomicU64::new(last),
            // No wider than the id's own space: `MintedId::MINT_CEILING`
            // caps the raw `from_mint` converts back.
            ceiling: I::MINT_CEILING,
            _id: PhantomData,
        }
    }

    /// Issue the next identity, from any thread that shares `&self`.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once the last value the mint may issue was issued; the
    /// mint refuses permanently, from every thread, from then on.
    pub fn next(&self) -> Result<I, IdExhausted> {
        let mut current = self.last.load(Ordering::Relaxed);
        loop {
            let Some(next) = current
                .checked_add(1)
                .filter(|next| *next <= self.ceiling)
                .and_then(NonZeroU64::new)
            else {
                return Err(IdExhausted);
            };
            match self.last.compare_exchange(
                current,
                next.get(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(I::from_mint(next, sealed::MintToken(()))),
                Err(observed) => current = observed,
            }
        }
    }
}

impl<I: MintedId> Default for AtomicIdMint<I> {
    fn default() -> Self {
        Self::new()
    }
}
