//! What a drag carries: items with an in-process value and ordered serialized
//! representations (ADR-0211 §1).

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use flui_platform::data_transfer::{
    RepresentationDescriptor, TransferError, TransferFormat, TransferPayload,
};

/// A serialized representation computed only when someone asks for it.
///
/// Neither `Send` nor `Sync`: it runs on the source runtime's owner thread, and
/// runs at most once; its result is kept for later requests.
pub struct LazyPayload(Box<dyn FnOnce() -> Result<TransferPayload, TransferError>>);

impl LazyPayload {
    /// Defer `provider` until the first request for this representation.
    pub fn new(
        provider: impl FnOnce() -> Result<TransferPayload, TransferError> + 'static,
    ) -> Self {
        Self(Box::new(provider))
    }

    /// A representation whose payload is already in memory.
    pub fn ready(payload: TransferPayload) -> Self {
        Self::new(move || Ok(payload))
    }
}

impl fmt::Debug for LazyPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyPayload").finish_non_exhaustive()
    }
}

/// One way a drag item can be received.
#[derive(Debug)]
pub enum DragRepresentation {
    /// A Rust value, redeemed only from the source's own runtime.
    InProcess(Rc<dyn Any>),
    /// Bytes in a typed format, redeemable by any target that understands it.
    Serialized(TransferFormat, LazyPayload),
}

/// One thing a drag carries, in as many representations as the source offers.
///
/// Serialized representations are in the source's preference order, as in an
/// offer. An item with only [`InProcess`](DragRepresentation::InProcess)
/// representations can be dropped only inside the source's runtime.
#[derive(Debug, Default)]
pub struct DragItem {
    representations: Vec<DragRepresentation>,
}

impl DragItem {
    /// An item with no representation yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An item carrying `value` for targets in the source's runtime.
    #[must_use]
    pub fn with_value<T: Any>(self, value: T) -> Self {
        self.with(DragRepresentation::InProcess(Rc::new(value)))
    }

    /// Add a serialized representation, computed on first request.
    #[must_use]
    pub fn with_serialized(
        self,
        format: TransferFormat,
        provider: impl FnOnce() -> Result<TransferPayload, TransferError> + 'static,
    ) -> Self {
        self.with(DragRepresentation::Serialized(
            format,
            LazyPayload::new(provider),
        ))
    }

    /// Add a plain-text representation.
    #[must_use]
    pub fn with_text(self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.with(DragRepresentation::Serialized(
            TransferFormat::Text,
            LazyPayload::ready(TransferPayload::Text(text)),
        ))
    }

    /// Add `representation` after the existing ones.
    #[must_use]
    pub fn with(mut self, representation: DragRepresentation) -> Self {
        self.representations.push(representation);
        self
    }

    /// The first in-process value of type `T`.
    #[must_use]
    pub fn value<T: Any>(&self) -> Option<&T> {
        self.representations
            .iter()
            .find_map(|representation| match representation {
                DragRepresentation::InProcess(value) => value.downcast_ref::<T>(),
                DragRepresentation::Serialized(..) => None,
            })
    }

    /// Every representation, in the order added.
    #[must_use]
    pub fn representations(&self) -> &[DragRepresentation] {
        &self.representations
    }
}

/// A serialized representation and its state.
struct SerializedSlot {
    format: TransferFormat,
    state: RefCell<LazyState>,
}

enum LazyState {
    Pending(LazyPayload),
    /// The provider is running; a request from inside it cannot be served.
    Running,
    Done(Result<TransferPayload, TransferError>),
}

struct ItemData {
    values: Vec<Rc<dyn Any>>,
    serialized: Vec<SerializedSlot>,
}

/// What a live session holds of its items, shared with the views its targets
/// receive. Alive exactly as long as the session or a view of it.
pub(crate) struct SessionData {
    items: Vec<ItemData>,
}

impl SessionData {
    pub(crate) fn new(items: Vec<DragItem>) -> Self {
        let items = items
            .into_iter()
            .map(|item| {
                let mut values = Vec::new();
                let mut serialized = Vec::new();
                for representation in item.representations {
                    match representation {
                        DragRepresentation::InProcess(value) => values.push(value),
                        DragRepresentation::Serialized(format, lazy) => {
                            serialized.push(SerializedSlot {
                                format,
                                state: RefCell::new(LazyState::Pending(lazy)),
                            });
                        }
                    }
                }
                ItemData { values, serialized }
            })
            .collect();
        Self { items }
    }

    pub(crate) fn has_serialized(&self) -> bool {
        self.items.iter().any(|item| !item.serialized.is_empty())
    }

    pub(crate) fn item_count(&self) -> usize {
        self.items.len()
    }

    /// The serialized slots in the order an offer announces them: items in
    /// order, each item's representations in its own order.
    fn flat(&self) -> impl Iterator<Item = (usize, usize, &SerializedSlot)> {
        self.items.iter().enumerate().flat_map(|(item, data)| {
            data.serialized
                .iter()
                .enumerate()
                .map(move |(slot, serialized)| (item, slot, serialized))
        })
    }

    /// The descriptors of the serialized representations, in offer order.
    pub(crate) fn descriptors(&self) -> Vec<RepresentationDescriptor> {
        self.flat()
            .map(|(_, _, slot)| RepresentationDescriptor {
                format: slot.format.clone(),
                declared_len: None,
            })
            .collect()
    }

    /// The formats of the serialized representations, in offer order.
    pub(crate) fn formats(&self) -> Vec<TransferFormat> {
        self.flat()
            .map(|(_, _, slot)| slot.format.clone())
            .collect()
    }

    /// The `index`th serialized representation in offer order, computed on
    /// first request and cached. No borrow is held while the provider runs.
    pub(crate) fn serialized(&self, index: usize) -> Result<TransferPayload, TransferError> {
        let (_, _, slot) = self.flat().nth(index).ok_or(TransferError::Cancelled)?;
        let pending = {
            let mut state = slot.state.borrow_mut();
            match &*state {
                LazyState::Done(result) => return result.clone(),
                LazyState::Running => return Err(TransferError::SourceGone),
                LazyState::Pending(_) => std::mem::replace(&mut *state, LazyState::Running),
            }
        };
        let LazyState::Pending(LazyPayload(provider)) = pending else {
            unreachable!("BUG: only a pending state is taken");
        };
        // A provider that panics leaves a definite answer behind, not a slot
        // stuck running; the panic itself still reaches the requester.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(provider)) {
            Ok(result) => {
                *slot.state.borrow_mut() = LazyState::Done(result.clone());
                result
            }
            Err(payload) => {
                *slot.state.borrow_mut() = LazyState::Done(Err(TransferError::SourceGone));
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// The offer-order index of the first serialized representation of `format`.
    pub(crate) fn find(&self, format: &TransferFormat) -> Option<usize> {
        self.flat().position(|(_, _, slot)| slot.format == *format)
    }

    pub(crate) fn value<T: Any>(&self) -> Option<&T> {
        self.values::<T>().next()
    }

    pub(crate) fn values<T: Any>(&self) -> impl Iterator<Item = &T> {
        self.items.iter().filter_map(|item| {
            item.values
                .iter()
                .find_map(|value| value.downcast_ref::<T>())
        })
    }
}
