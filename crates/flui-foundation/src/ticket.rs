//! Tickets that match an asynchronous answer to the request that asked for
//! it (ADR-0287 §5).
//!
//! An owner that asks something of a host or a worker issues a [`Ticket`]
//! with the request and accepts the answer only for that ticket: an answer
//! that arrives late, after a newer request, after a write or after the owner
//! moved on, carries a ticket that is no longer current.

use std::cell::Cell;
use std::marker::PhantomData;
use std::num::NonZeroU64;

use crate::sequence::{IdExhausted, IdMint};

/// The identity of one request of kind `K`, issued by a [`TicketSource`].
///
/// Opaque: it compares, orders (a later ticket is greater) and hashes, and
/// exposes no number. The kind keeps the tickets of two kinds of request
/// apart at compile time.
pub struct Ticket<K> {
    raw: NonZeroU64,
    _kind: PhantomData<fn() -> K>,
}

impl<K> Clone for Ticket<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Ticket<K> {}

impl<K> PartialEq for Ticket<K> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<K> Eq for Ticket<K> {}

impl<K> PartialOrd for Ticket<K> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Ticket<K> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<K> std::hash::Hash for Ticket<K> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<K> std::fmt::Debug for Ticket<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Ticket").field(&self.raw).finish()
    }
}

/// An owner-local issuer of [`Ticket`]s that knows which one is current.
///
/// The last ticket issued is current until it is redeemed, invalidated or
/// followed by another. Tickets are never reissued: an exhausted source
/// refuses with [`IdExhausted`].
pub struct TicketSource<K> {
    mint: IdMint<NonZeroU64>,
    current: Cell<Option<NonZeroU64>>,
    _kind: PhantomData<fn() -> K>,
}

impl<K> std::fmt::Debug for TicketSource<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TicketSource")
            .field("current", &self.current.get())
            .finish_non_exhaustive()
    }
}

impl<K> Default for TicketSource<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K> TicketSource<K> {
    /// A source whose first ticket is its first.
    #[must_use]
    pub const fn new() -> Self {
        Self::starting_after(0)
    }

    /// A source that issues only tickets after `last`, so a test can reach
    /// the last one without issuing every ticket before it.
    #[must_use]
    pub const fn starting_after(last: u64) -> Self {
        Self {
            mint: IdMint::starting_after(last),
            current: Cell::new(None),
            _kind: PhantomData,
        }
    }

    /// Issue the ticket for a new request; it becomes the current one, and
    /// every ticket issued before it is stale.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once every ticket was issued; the source refuses
    /// permanently, and the ticket that was current stays current.
    pub fn issue(&self) -> Result<Ticket<K>, IdExhausted> {
        let raw = self.mint.next()?;
        self.current.set(Some(raw));
        Ok(Ticket {
            raw,
            _kind: PhantomData,
        })
    }

    /// Whether an answer carrying `ticket` belongs to the latest request.
    #[must_use]
    pub fn is_current(&self, ticket: Ticket<K>) -> bool {
        self.current.get() == Some(ticket.raw)
    }

    /// Accept the answer carrying `ticket` once: `true` when it is current,
    /// and then no ticket is current, so the same answer is not accepted
    /// twice.
    pub fn redeem(&self, ticket: Ticket<K>) -> bool {
        let current = self.is_current(ticket);
        if current {
            self.current.set(None);
        }
        current
    }

    /// Make every outstanding ticket stale: the state a request was asked
    /// against changed (a write, a focus move, the owner retiring).
    pub fn invalidate(&self) {
        self.current.set(None);
    }
}
