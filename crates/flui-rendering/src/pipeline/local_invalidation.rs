//! Owner-local invalidation, coalesced for one live render attachment.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

use flui_foundation::RenderId;
use parking_lot::RwLock;

use super::{
    handle::{AttachmentEpoch, DirtyKind, SendError},
    notifier::VisualUpdateNotifier,
};

pub(crate) struct LocalDirtyRecord {
    pub(crate) id: RenderId,
    pub(crate) epoch: AttachmentEpoch,
    active: Cell<bool>,
    queued: Cell<bool>,
    marks: Cell<u8>,
}

impl LocalDirtyRecord {
    pub(crate) fn take_marks(&self) -> u8 {
        self.queued.set(false);
        self.marks.replace(0)
    }
}

pub(crate) fn bit(kind: DirtyKind) -> u8 {
    match kind {
        DirtyKind::Layout => 1,
        DirtyKind::Compositing => 2,
        DirtyKind::Paint => 4,
        DirtyKind::Semantics => 8,
        DirtyKind::CompositedLayerUpdate => 16,
    }
}

pub(crate) struct LocalDirtyInbox {
    pending: RefCell<Vec<Weak<LocalDirtyRecord>>>,
    notifier: Arc<RwLock<VisualUpdateNotifier>>,
    /// Set while the owner paints. Paint must not change geometry, so a
    /// layout mark in that window is a bug in the render object that made it.
    painting: Cell<bool>,
    /// The first node that requested layout while `painting` was set.
    layout_during_paint: Cell<Option<RenderId>>,
}

impl LocalDirtyInbox {
    pub(crate) fn new(notifier: Arc<RwLock<VisualUpdateNotifier>>) -> Rc<Self> {
        Rc::new(Self {
            pending: RefCell::new(Vec::new()),
            notifier,
            painting: Cell::new(false),
            layout_during_paint: Cell::new(None),
        })
    }

    pub(crate) fn bind(
        self: &Rc<Self>,
        id: RenderId,
        epoch: AttachmentEpoch,
    ) -> LocalRenderInvalidationHandle {
        LocalRenderInvalidationHandle {
            inbox: Rc::downgrade(self),
            record: Rc::new(LocalDirtyRecord {
                id,
                epoch,
                active: Cell::new(true),
                queued: Cell::new(false),
                marks: Cell::new(0),
            }),
        }
    }

    pub(crate) fn begin_paint(&self) {
        self.painting.set(true);
    }

    /// Leaves the paint window and returns the first node that requested
    /// layout inside it.
    pub(crate) fn end_paint(&self) -> Option<RenderId> {
        self.painting.set(false);
        self.layout_during_paint.take()
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.borrow().is_empty()
    }

    pub(crate) fn take_pending(&self) -> Vec<Weak<LocalDirtyRecord>> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }

    pub(crate) fn recycle(&self, mut batch: Vec<Weak<LocalDirtyRecord>>) {
        batch.clear();
        let mut pending = self.pending.borrow_mut();
        if batch.capacity() > pending.capacity() {
            batch.append(&mut pending);
            std::mem::swap(&mut *pending, &mut batch);
        }
    }
}

/// An owner-local invalidation capability for one render-node attachment.
///
/// UI subscriptions commit marks before waking the frame driver. Repeated marks
/// coalesce by phase; they remain pending until the owner drains them, even when
/// no further animation sample arrives. Detach withdraws the capability before
/// invoking user cleanup. This handle cannot cross threads; external producers
/// use [`super::RenderInvalidationHandle`] and its bounded channel instead.
#[derive(Clone)]
pub struct LocalRenderInvalidationHandle {
    inbox: Weak<LocalDirtyInbox>,
    record: Rc<LocalDirtyRecord>,
}

impl std::fmt::Debug for LocalRenderInvalidationHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalRenderInvalidationHandle")
            .field("id", &self.record.id)
            .field("active", &self.record.active.get())
            .finish_non_exhaustive()
    }
}

impl LocalRenderInvalidationHandle {
    /// The node this capability addresses during its attachment interval.
    #[must_use]
    pub fn id(&self) -> RenderId {
        self.record.id
    }

    fn mark(&self, kind: DirtyKind) -> Result<(), SendError> {
        self.retain(bit(kind))
    }

    fn retain(&self, marks: u8) -> Result<(), SendError> {
        if !self.record.active.get() {
            return Err(SendError::OwnerGone);
        }
        let Some(inbox) = self.inbox.upgrade() else {
            return Err(SendError::OwnerGone);
        };
        if marks & bit(DirtyKind::Layout) != 0
            && inbox.painting.get()
            && inbox.layout_during_paint.get().is_none()
        {
            inbox.layout_during_paint.set(Some(self.record.id));
            tracing::error!(
                id = ?self.record.id,
                "a render object requested layout during paint; the mark applies next frame"
            );
        }
        self.record.marks.set(self.record.marks.get() | marks);
        if !self.record.queued.replace(true) {
            inbox.pending.borrow_mut().push(Rc::downgrade(&self.record));
        }
        // Retry a missing or failed wake even if this phase was already queued.
        // No queue borrow, notifier guard or strong inbox reference survives
        // the callback: releasing the owner there must revoke every peer.
        let wake = { inbox.notifier.read().deferred_visual_update() };
        drop(inbox);
        wake.notify();
        Ok(())
    }

    /// Commit every phase of an update together before invoking the frame wake.
    ///
    /// # Errors
    /// Returns [`SendError::OwnerGone`] after detach or owner release.
    pub fn request_update(&self, impact: crate::RenderUpdateImpact) -> Result<(), SendError> {
        let mut marks = 0;
        for (needed, kind) in [
            (impact.needs_layout(), DirtyKind::Layout),
            (impact.needs_paint(), DirtyKind::Paint),
            (
                impact.needs_compositing_bits_update(),
                DirtyKind::Compositing,
            ),
            (impact.needs_semantics_update(), DirtyKind::Semantics),
            (
                impact.needs_composited_layer_update(),
                DirtyKind::CompositedLayerUpdate,
            ),
        ] {
            if needed {
                marks |= bit(kind);
            }
        }
        if marks == 0 {
            return Ok(());
        }
        self.retain(marks)
    }

    pub(crate) fn withdraw(&self) {
        self.record.active.set(false);
        self.record.marks.set(0);
        if self.record.queued.replace(false)
            && let Some(inbox) = self.inbox.upgrade()
        {
            let identity = Rc::downgrade(&self.record);
            inbox
                .pending
                .borrow_mut()
                .retain(|entry| !entry.ptr_eq(&identity));
        }
    }

    /// Retain a layout mark and request a frame.
    ///
    /// # Errors
    /// Returns [`SendError::OwnerGone`] after detach or owner release.
    pub fn mark_needs_layout(&self) -> Result<(), SendError> {
        self.mark(DirtyKind::Layout)
    }

    /// Retain a paint mark and request a frame.
    ///
    /// # Errors
    /// Returns [`SendError::OwnerGone`] after detach or owner release.
    pub fn mark_needs_paint(&self) -> Result<(), SendError> {
        self.mark(DirtyKind::Paint)
    }

    /// Retain a compositing-bits mark and request a frame.
    ///
    /// # Errors
    /// Returns [`SendError::OwnerGone`] after detach or owner release.
    pub fn mark_needs_compositing_bits_update(&self) -> Result<(), SendError> {
        self.mark(DirtyKind::Compositing)
    }

    /// Retain an effect-layer update and request a frame.
    ///
    /// # Errors
    /// Returns [`SendError::OwnerGone`] after detach or owner release.
    pub fn mark_needs_composited_layer_update(&self) -> Result<(), SendError> {
        self.mark(DirtyKind::CompositedLayerUpdate)
    }

    /// Retain a semantics mark and request a frame.
    ///
    /// # Errors
    /// Returns [`SendError::OwnerGone`] after detach or owner release.
    pub fn mark_needs_semantics(&self) -> Result<(), SendError> {
        self.mark(DirtyKind::Semantics)
    }
}
