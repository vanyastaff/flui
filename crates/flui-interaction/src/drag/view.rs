//! What a drop target sees of a drag: [`DragView`] (ADR-0211 §1, §4).

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use flui_platform_api::data_transfer::{
    DataTransferOffer, DataTransferSource, TransferActions, TransferError, TransferFormat,
    TransferLimits, TransferPayload, TransferRequest,
};
use flui_platform_api::drag::DragSessionKey;

use super::item::SessionData;

/// Where a drag comes from, relative to the target looking at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragScope {
    /// Started in the target's own window and still running there: no OS
    /// session, no offer.
    Window,
    /// Started in another presentation of the target's runtime, through an OS
    /// session: the Rust value is redeemable.
    Runtime,
    /// Started by another runtime of this process: only the serialized
    /// representations are redeemable (ADR-0211 §4).
    OtherRuntime,
    /// Started elsewhere: another process, or any drag not made by FLUI.
    External,
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
    pub(super) in_flight: Option<&'a Arc<AtomicUsize>>,
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

/// Holds a drop open at the OS while a request made from it is outstanding.
struct FetchGuard(Arc<AtomicUsize>);

impl FetchGuard {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(counter))
    }
}

impl Drop for FetchGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
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
