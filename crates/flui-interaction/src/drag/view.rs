//! What a drop target sees of a drag: [`DragView`] (ADR-0211 §1, §4).

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, TransferActions, TransferError, TransferFormat,
    TransferLimits, TransferPayload, TransferRequest,
};
use flui_platform::drag::DragSessionKey;

use super::item::SessionData;
use crate::events::PointerKind;

/// Where a drag comes from, relative to the target looking at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragScope {
    /// Started in the target's own window and still running there: no OS
    /// session, no offer.
    Window,
    /// Started in another presentation of the target's runtime, reaching the
    /// target on its source window's capture or through an OS session: the
    /// Rust value is redeemable.
    Runtime,
    /// Started by another runtime of this process: only the serialized
    /// representations are redeemable (ADR-0211 §4).
    OtherRuntime,
    /// Started elsewhere: another process, or any drag not made by FLUI.
    External,
}

/// Which drag a view is of, so a receiver over which several drags hover
/// keeps them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragIdentity {
    /// A session of a FLUI runtime, this one or another.
    Session(DragSessionKey),
    /// An operating-system drag that names no FLUI session.
    Offer(flui_foundation::DataTransferId),
}

/// A drag as one drop target sees it, valid for the callback it is passed to.
///
/// Redeems the Rust value only for [`DragScope::Window`] and
/// [`DragScope::Runtime`]; serialized representations are requested for every
/// scope through [`Self::request`].
pub struct DragView<'a> {
    pub(super) data: Option<&'a SessionData>,
    pub(super) offer: Option<&'a DataTransferOffer>,
    pub(super) key: Option<DragSessionKey>,
    pub(super) scope: DragScope,
    pub(super) allowed: TransferActions,
    pub(super) source: Option<&'a Arc<dyn DataTransferSource>>,
    pub(super) in_flight: Option<&'a Arc<InFlight>>,
    pub(super) pointer: PointerKind,
}

impl std::fmt::Debug for DragView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragView")
            .field("scope", &self.scope)
            .field("allowed", &self.allowed)
            .finish_non_exhaustive()
    }
}

impl<'a> DragView<'a> {
    /// Where this drag comes from.
    #[must_use]
    pub fn scope(&self) -> DragScope {
        self.scope
    }

    /// What this target may do with the drag: the source's allowance for the
    /// target's scope, narrowed by what the OS currently permits.
    #[must_use]
    pub fn allowed(&self) -> TransferActions {
        self.allowed
    }

    /// The session's identity, when the drag is FLUI's own.
    #[must_use]
    pub fn session(&self) -> Option<DragSessionKey> {
        self.key
    }

    /// Which drag this is: its session, or else its offer. Every view of a
    /// drag carries one or the other.
    #[must_use]
    pub fn identity(&self) -> Option<DragIdentity> {
        self.key
            .map(DragIdentity::Session)
            .or_else(|| self.offer.map(|offer| DragIdentity::Offer(offer.id())))
    }

    /// The kind of pointer that drags, when this runtime started the drag
    /// (in its window, or back through the OS over another of its windows);
    /// [`PointerKind::Unknown`] for another application's drag, whose pointer
    /// the OS does not name.
    #[must_use]
    pub fn pointer_kind(&self) -> PointerKind {
        self.pointer
    }

    /// The OS offer, for a drag that is one. A window-scope drag mints none.
    #[must_use]
    pub fn offer(&self) -> Option<&'a DataTransferOffer> {
        self.offer
    }

    /// How many items the drag carries, when this target can tell: an in-process
    /// drag knows its items; an offer announces representations, not items.
    #[must_use]
    pub fn item_count(&self) -> Option<usize> {
        self.data.map(SessionData::item_count)
    }

    /// The first item's in-process value of type `T`, if the drag is
    /// redeemable here and carries one.
    #[must_use]
    pub fn value<T: Any>(&self) -> Option<&'a T> {
        self.data.and_then(SessionData::value::<T>)
    }

    /// Every item's in-process value of type `T`, in item order.
    pub fn values<T: Any>(&self) -> impl Iterator<Item = &'a T> + use<'a, T> {
        self.data.into_iter().flat_map(SessionData::values::<T>)
    }

    /// The serialized formats the drag offers, in the source's preference order.
    #[must_use]
    pub fn formats(&self) -> Vec<TransferFormat> {
        if let Some(data) = self.data {
            return data.formats();
        }
        self.offer
            .map(|offer| {
                offer
                    .representations()
                    .iter()
                    .map(|descriptor| descriptor.format.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Ask for the first representation in `format`. `None` when the drag offers
    /// none. The request is a future the target spawns on its scheduler; the drop
    /// is not concluded with the OS until it resolves or is dropped.
    #[must_use]
    pub fn request(&self, format: &TransferFormat, limits: TransferLimits) -> Option<DragFetch> {
        if let Some(data) = self.data {
            let index = data.find(format)?;
            let result = data
                .serialized(index)
                .and_then(|payload| within_limit(payload, limits));
            return Some(DragFetch::new(TransferRequest::ready(result), None));
        }
        let offer = self.offer?;
        let index = offer.find(format)?;
        let source = self.source?;
        let guard = self.in_flight.map(FetchGuard::new);
        Some(DragFetch::new(
            source.request(offer.id(), index, limits),
            guard,
        ))
    }
}

fn within_limit(
    payload: TransferPayload,
    limits: TransferLimits,
) -> Result<TransferPayload, TransferError> {
    let actual = payload.byte_len();
    if actual > limits.max_bytes {
        return Err(TransferError::TooLarge {
            actual,
            limit: limits.max_bytes,
        });
    }
    Ok(payload)
}

/// How the owner is woken from another thread: a request made from a drop
/// resolves or is dropped there, and the owner's loop may be asleep.
pub type OwnerWake = Arc<dyn Fn() + Send + Sync>;

/// The requests of one offer that have not resolved, over every window that
/// hovers it. Each hover holds a clone; a request outlives the hover that
/// made it.
pub(crate) struct InFlight {
    count: AtomicUsize,
    wake: OwnerWake,
}

impl InFlight {
    pub(crate) fn new(wake: OwnerWake) -> Arc<Self> {
        Arc::new(Self {
            count: AtomicUsize::new(0),
            wake,
        })
    }

    /// Whether no request of the offer is outstanding.
    pub(crate) fn is_idle(&self) -> bool {
        self.count.load(Ordering::Acquire) == 0
    }
}

/// Holds a drop open at the OS while a request made from it is outstanding.
/// The last one released wakes the owner, which concludes the drop on its next
/// tick: the request may resolve on any thread, with the owner's loop asleep.
struct FetchGuard(Arc<InFlight>);

impl FetchGuard {
    fn new(in_flight: &Arc<InFlight>) -> Self {
        in_flight.count.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(in_flight))
    }
}

impl Drop for FetchGuard {
    fn drop(&mut self) {
        if self.0.count.fetch_sub(1, Ordering::AcqRel) == 1 {
            (self.0.wake)();
        }
    }
}

/// One outstanding request for a representation of a drag.
///
/// A `Send` future. Dropping it before it resolves cancels the delivery. The
/// runtime concludes the drop with the OS once every request made from it has
/// resolved or been dropped, so a target may fetch after its callback returned.
#[must_use = "dropping a DragFetch cancels the request"]
pub struct DragFetch {
    request: TransferRequest,
    // Released when the request resolves or is dropped.
    hold: Option<FetchGuard>,
}

impl DragFetch {
    fn new(request: TransferRequest, hold: Option<FetchGuard>) -> Self {
        Self { request, hold }
    }
}

impl std::fmt::Debug for DragFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragFetch").finish_non_exhaustive()
    }
}

impl Future for DragFetch {
    type Output = Result<TransferPayload, TransferError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let poll = Pin::new(&mut self.request).poll(cx);
        if poll.is_ready() {
            drop(self.hold.take());
        }
        poll
    }
}
