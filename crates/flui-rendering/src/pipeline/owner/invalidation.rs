//! Application of invalidation accepted from any thread, the font-change replay and the
//! visual-update request to the host.

use flui_foundation::RenderId;
use flui_foundation::panic::{FailureClass, PanicRecovery, RecoveryScope};

use crate::pipeline::handle::DirtyKind;

use super::PipelineOwner;

impl PipelineOwner {
    /// Sets the callback for when a visual update is needed.
    pub fn set_on_need_visual_update<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let outgoing = self.notifier.replace_need_visual_update(callback);
        let wake = self
            .local_dirty
            .has_pending()
            .then(|| self.notifier.visual_update());
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        if let Some(wake) = wake {
            wake.notify_with(&mut recovery.scope());
        }
        recovery.scope().retire(outgoing);
        recovery.finish();
    }

    /// Requests a visual update.
    ///
    /// Called by render objects when they need to be re-rendered.
    pub fn request_visual_update(&self) {
        self.notifier.visual_update().notify();
    }

    /// Binds a [`RenderInvalidationHandle`](crate::pipeline::RenderInvalidationHandle) to a
    /// live render object — the
    /// capability async producers (image decodes, arriving assets) use
    /// to repaint that node from any thread, with the platform woken on
    /// every request.
    ///
    /// `None` for a stale, foreign, or detached id. A handle remains bound to
    /// this exact attachment interval; detaching and reattaching the same
    /// `RenderId` mints a different interval and makes the old handle inert.
    pub fn render_invalidation_handle(
        &self,
        id: RenderId,
    ) -> Option<crate::pipeline::RenderInvalidationHandle> {
        let attachment_epoch = self.render_tree.get(id)?.attachment_epoch()?;
        Some(crate::pipeline::RenderInvalidationHandle::new(
            self.dirty_sender.clone(),
            id,
            attachment_epoch,
        ))
    }

    /// Return an owner-local capability for this node's current attachment.
    /// Pending marks coalesce independently of the bounded external channel.
    /// Detached, removed and foreign nodes have no capability.
    #[must_use]
    pub fn local_render_invalidation_handle(
        &self,
        id: RenderId,
    ) -> Option<crate::pipeline::LocalRenderInvalidationHandle> {
        self.render_tree.get(id)?.local_invalidation()
    }

    /// Drains the pending dirty-request channel into the scheduler's
    /// dirty sets.
    ///
    /// Called at phase boundaries by the typestate transitions; producers
    /// (background asset loaders, async work) write into the channel via
    /// a [`crate::pipeline::RenderInvalidationHandle`] and the owner observes them on
    /// the next frame. Non-blocking; processes every request
    /// available at the time of call and returns the count drained.
    ///
    /// Then applies a font collection change ([`Self::apply_font_change`]),
    /// the other invalidation that arrives from outside the frame.
    pub fn drain_pending_dirty(&mut self) -> usize {
        let mut recovery = PanicRecovery::new();
        let drained = self.drain_pending_dirty_with_recovery(&mut recovery.scope());
        recovery.finish();
        drained
    }

    /// Apply accepted invalidations under an enclosing owner's panic custody.
    /// Each mark and diagnostic is attempted independently so a failed callout
    /// cannot erase the accepted tail. Failures remain in `recovery`; this
    /// operation does not resume them.
    #[doc(hidden)]
    pub fn drain_pending_dirty_with_recovery(&mut self, recovery: &mut RecoveryScope<'_>) -> usize {
        let mut drained = 0;
        while let Ok(req) = self.dirty_rx.try_recv() {
            drained += 1;
            // Generation-validated replay through the SAME mark paths
            // local callers use: a stale id (the node died after the
            // producer captured its handle) falls out silently, the
            // layout mark walks to its relayout boundary, compositing
            // keeps the queue⇒flag invariant, and every path carries
            // its own dedup. The request's depth is advisory — the live
            // node's depth is authoritative.
            //
            // Pre-fix this pushed raw queue entries: a layout request
            // for a non-boundary node became a bogus dirty ROOT, a
            // compositing request skipped the canonical ancestor walk, and
            // dead ids were replayed verbatim.
            if !self
                .render_tree
                .get(req.id)
                .is_some_and(|node| node.is_attached_for(req.attachment_epoch))
            {
                recovery.run_classified(FailureClass::Unknown, |_| {
                    tracing::trace!(?req, "drain_pending_dirty: stale attachment, dropped");
                });
                continue;
            }
            self.apply_pending_mark(req.id, req.kind, recovery);
        }
        let mut local = self.local_dirty.take_pending();
        for queued in local.drain(..) {
            let Some(record) = queued.upgrade() else {
                continue;
            };
            let marks = record.take_marks();
            if !self
                .render_tree
                .get(record.id)
                .is_some_and(|node| node.is_attached_for(record.epoch))
            {
                continue;
            }
            for kind in [
                DirtyKind::Layout,
                DirtyKind::Compositing,
                DirtyKind::Paint,
                DirtyKind::Semantics,
                DirtyKind::CompositedLayerUpdate,
                DirtyKind::SemanticsGeometry,
            ] {
                if marks & super::super::local_invalidation::bit(kind) == 0 {
                    continue;
                }
                drained += 1;
                self.apply_pending_mark(record.id, kind, recovery);
            }
        }
        self.local_dirty.recycle(local);
        self.apply_font_change_with_recovery(recovery);
        drained
    }

    fn apply_pending_mark(
        &mut self,
        id: RenderId,
        kind: DirtyKind,
        recovery: &mut RecoveryScope<'_>,
    ) {
        let mut needs_wake = false;
        // A failed numeric walk has not proved publication. The canonical
        // commit helpers enter no user code and return only after admission.
        recovery.run_classified(FailureClass::Unknown, |_| {
            needs_wake = match kind {
                DirtyKind::Layout => {
                    self.scheduler
                        .commit_needs_layout(&mut self.render_tree, id, &mut |visited| {
                            self.layout_poison.unpoison(visited);
                        })
                }
                DirtyKind::Compositing => self
                    .scheduler
                    .commit_needs_compositing_bits_update(&self.render_tree, id),
                DirtyKind::Paint => self.scheduler.commit_needs_paint(&self.render_tree, id),
                DirtyKind::Semantics => {
                    self.semantics_enabled()
                        && self.render_tree.get(id).is_some_and(|node| {
                            self.scheduler
                                .commit_node_needing_semantics(id, node.depth() as usize)
                        })
                }
                DirtyKind::SemanticsGeometry => {
                    self.semantics_enabled()
                        && self.render_tree.get(id).is_some_and(|node| {
                            self.scheduler
                                .commit_node_needing_semantics_geometry(id, node.depth() as usize)
                        })
                }
                DirtyKind::CompositedLayerUpdate => self
                    .scheduler
                    .commit_needs_composited_layer_update(&self.render_tree, id),
            };
        });
        if needs_wake {
            self.notifier.visual_update().notify_with(recovery);
        }
    }

    /// Lays out again, and repaints, every node that measured text since
    /// the font collection last changed, if it changed since this pipeline
    /// last looked. Returns whether it had changed.
    ///
    /// A face registered on the collection after text was laid out may
    /// change what that text measures to, and the node that measured it has
    /// no way to notice. The pipeline knows who measured: every loan of the
    /// text context records its node, so a render object that measures
    /// through its context needs no code of its own. The marks go through
    /// the ordinary [`Self::mark_needs_layout`] and [`Self::mark_needs_paint`]
    /// paths: a node removed since it measured is skipped, and the layout
    /// mark clears the cached intrinsics of the node and its ancestors and
    /// requests a visual update.
    ///
    /// While the context is lent (a drain run from inside a measurement) it
    /// does nothing and returns `false`: the change stays pending for the
    /// next drain. [`Self::drain_pending_dirty`] calls it, which both frame
    /// entries run first. Each call also forgets the removed nodes once they
    /// outnumber the live tree, so the record stays bounded in an app that
    /// never registers a font.
    pub fn apply_font_change(&mut self) -> bool {
        let mut recovery = PanicRecovery::new();
        let changed = self.apply_font_change_with_recovery(&mut recovery.scope());
        recovery.finish();
        changed
    }

    fn apply_font_change_with_recovery(&mut self, recovery: &mut RecoveryScope<'_>) -> bool {
        let render_tree = &self.render_tree;
        self.text_measurers
            .prune(render_tree.len(), |id| render_tree.get(id).is_some());
        let Some(generation) = self.text.fonts_generation() else {
            return false;
        };
        if generation == self.fonts_seen {
            return false;
        }
        self.fonts_seen = generation;
        let measured = self.text_measurers.take();
        // The generation and complete measured batch are admitted before this
        // diagnostic. Its delivery cannot cancel their independent marks.
        recovery.run_classified(FailureClass::Delivery, |_| {
            tracing::debug!(
                nodes = measured.len(),
                "the font collection changed; laying out the text measured on it again"
            );
        });
        for id in measured {
            if self.render_tree.get(id).is_some() {
                self.apply_pending_mark(id, DirtyKind::Layout, recovery);
                self.apply_pending_mark(id, DirtyKind::Paint, recovery);
            }
        }
        true
    }
}
