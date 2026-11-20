//! The inputs a pipeline owner lays out against: the root, its constraints, the device pixel
//! ratio, text sizing, the layout premise and the render tree handle.

use flui_foundation::RenderId;

use crate::{
    constraints::BoxConstraints,
    pipeline::phase::{Idle, PipelinePhase},
};

use super::{LayoutPremise, PipelineOwner};

impl PipelineOwner<Idle> {
    /// Withhold retained layout when its external input cannot progress.
    /// A live layout invalidation makes the retained work runnable again.
    #[doc(hidden)]
    pub fn defer_layout_until_input(&mut self) {
        self.scheduler.defer_layout_until_input();
    }

    /// Resume retained layout after its input becomes ready. The caller drives
    /// the resumed attempt; this neither admits new input nor requests a wake.
    #[doc(hidden)]
    pub fn resume_retained_layout(&mut self) {
        self.scheduler.resume_retained_layout();
    }
}
impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Capture the accepted layout-input premise for a suspended segment.
    /// Drain pending invalidations first: queued external requests become live
    /// changes only after generation-validated replay.
    #[doc(hidden)]
    pub fn layout_premise(&self) -> LayoutPremise {
        LayoutPremise {
            owner: std::rc::Rc::clone(&self.relocation_owner_seal),
            stamp: self.scheduler.layout_stamp(),
        }
    }

    /// Whether this owner still accepts the captured layout-input premise.
    /// Exhaustion permanently refuses compatibility rather than wrapping.
    #[doc(hidden)]
    #[must_use]
    pub fn accepts_layout_premise(&self, saved: &LayoutPremise) -> bool {
        std::rc::Rc::ptr_eq(&self.relocation_owner_seal, &saved.owner)
            && self.scheduler.layout_stamp().compatible(saved.stamp)
    }

    /// Returns this presentation's current numeric text-sizing authority.
    #[inline]
    pub fn text_sizing(&self) -> &flui_painting::TextSizing {
        &self.text_sizing
    }

    /// Publish numeric text sizing for this presentation only.
    /// Shared shaping and font resources retain no sizing authority.
    /// Commit every measured node before returning an owned wake. Deliver it
    /// after releasing the owner and any other publication guards.
    #[must_use = "deliver the deferred wake outside the pipeline loan"]
    pub fn set_text_sizing(
        &mut self,
        sizing: flui_painting::TextSizing,
    ) -> Option<crate::pipeline::DeferredVisualUpdate> {
        let changed = self.text_sizing != sizing;
        // An attempt view can change while the capture's geometry stays equal.
        self.text_sizing = sizing;
        if !changed {
            return self
                .has_dirty_nodes()
                .then(|| self.notifier.visual_update());
        }
        let measured = self.text_measurers.take();
        for id in measured {
            if self.render_tree.get(id).is_some() {
                self.scheduler
                    .commit_needs_layout(&mut self.render_tree, id, &mut |visited| {
                        self.layout_poison.unpoison(visited);
                    });
            }
        }
        self.has_dirty_nodes()
            .then(|| self.notifier.visual_update())
    }

    /// How many nodes the pipeline has recorded as measuring text since the
    /// font collection last changed, removed ones not yet pruned included.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn text_measurer_count(&self) -> usize {
        self.text_measurers.len()
    }

    /// Returns the root render object ID.
    pub fn root_id(&self) -> Option<RenderId> {
        self.root_id
    }

    /// Sets the root render object ID.
    pub fn set_root_id(&mut self, id: Option<RenderId>) {
        if id != self.root_id {
            self.scheduler.invalidate_layout_premise();
        }
        self.root_id = id;
    }

    /// Returns the constraints to apply to the root render object on
    /// the next layout pass (if no cached constraints yet).
    ///
    /// See [`Self::set_root_constraints`].
    #[inline]
    pub fn root_constraints(&self) -> Option<BoxConstraints> {
        self.root_constraints
    }

    /// Sets the constraints to apply to the root render object on the
    /// next layout pass when no cached constraints exist yet
    /// (first-frame initialization).
    ///
    /// The binding layer (`flui-view` /
    /// `flui-app` / `flui-hot-reload`) calls this once after
    /// constructing the pipeline + before the first `run_frame`
    /// invocation. On subsequent frames the root's cached
    /// `state.constraints()` (post-layout) supersedes this field; the
    /// fallback only fires on the very first layout pass.
    ///
    /// Pass `None` to clear (e.g., when the binding wants to defer to
    /// a yet-unmounted root render object that supplies its own
    /// constraints via `RootRenderElement::mount`).
    ///
    /// # Auto-schedules root relayout
    ///
    /// When `Some(_)` is passed AND `root_id` is set AND the new
    /// constraints differ from the prior value, this method also
    /// calls [`Self::mark_needs_layout`] on the root id so the
    /// next `run_layout` invocation picks up the change. This
    /// avoids the silent-no-relayout footgun the prior shape had —
    /// the binding no longer needs to call `mark_needs_layout`
    /// separately after `set_root_constraints`.
    ///
    /// Setting to the SAME constraints value (or to `None`) does
    /// NOT mark dirty — those cases either don't change the layout
    /// result or are explicit clears that the caller manages
    /// independently.
    pub fn set_root_constraints(&mut self, constraints: Option<BoxConstraints>) {
        let changed = constraints.is_some() && constraints != self.root_constraints;
        self.root_constraints = constraints;
        if changed && let Some(root_id) = self.root_id {
            self.mark_needs_layout(root_id);
        }
    }

    /// Where `id` is in the render tree, as an error names it: the node, then its parents up to
    /// [`TREE_CONTEXT_DEPTH`](flui_foundation::TREE_CONTEXT_DEPTH), each by its render object's
    /// debug name and id, with where the node's view was created. Empty for an id that is not a
    /// current node.
    #[must_use]
    pub fn tree_context(&self, id: flui_foundation::RenderId) -> flui_foundation::TreeContext {
        let tree = &self.render_tree;
        let Some(node) = tree.get(id) else {
            return flui_foundation::TreeContext::default();
        };
        let site = node.creation_site();
        let path = std::iter::successors(Some(id), |&at| tree.parent(at)).filter_map(|at| {
            tree.get(at).map(|node| {
                flui_foundation::PathEntry::new(
                    node.kind(),
                    flui_foundation::inspect::InspectId::from(at),
                )
            })
        });
        flui_foundation::TreeContext::new(path, site)
    }

    /// `error` with the context of the node it names, when it names one.
    pub(super) fn in_tree(
        &self,
        error: crate::error::RenderError,
    ) -> flui_foundation::TreeError<crate::error::RenderError> {
        let context = error.node().map_or_default(|id| self.tree_context(id));
        flui_foundation::TreeError::new(error, context)
    }

    /// Returns a reference to the render tree.
    pub fn render_tree(&self) -> &crate::storage::RenderTree {
        &self.render_tree
    }

    /// Returns a mutable reference to the render tree, for this crate's tests
    /// and the `testing::raw_tree` kit. Every other edit goes through an owner
    /// operation that applies its own dirty marks (`tree_edit`).
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn render_tree_mut(&mut self) -> &mut crate::storage::RenderTree {
        &mut self.render_tree
    }

    /// Device pixel ratio threaded into every paint pass:
    /// [`DevicePixelRatio::ONE`] until [`Self::set_device_pixel_ratio`] sets
    /// another.
    ///
    /// [`DevicePixelRatio::ONE`]: flui_foundation::geometry::DevicePixelRatio::ONE
    pub fn device_pixel_ratio(&self) -> flui_foundation::geometry::DevicePixelRatio {
        self.device_pixel_ratio
    }

    /// Sets the device pixel ratio for subsequent paint passes.
    ///
    /// Called by the platform binding on surface creation and DPI
    /// change. The ratio is a [`DevicePixelRatio`], so it is in the range a
    /// screen can have by its type, and the pipeline only ever holds a ratio
    /// that type admitted (or its initial `1.0`).
    ///
    /// [`DevicePixelRatio`]: flui_foundation::geometry::DevicePixelRatio
    pub fn set_device_pixel_ratio(&mut self, ratio: flui_foundation::geometry::DevicePixelRatio) {
        self.device_pixel_ratio = ratio;
        let dpr = ratio.get();
        // Assistive technology reads bounds in physical pixels: the
        // semantics owner republishes its root under the new scale, and
        // the root is marked so a frame with nothing else dirty still
        // runs the semantics pass that flushes it.
        if let Some(owner) = self.semantics_owner.as_mut() {
            #[expect(
                clippy::float_cmp,
                reason = "an unchanged ratio is bit-identical, not approximately equal"
            )]
            let changed = owner.device_pixel_ratio() != dpr;
            owner.set_device_pixel_ratio(dpr);
            if changed && let Some(root_id) = self.root_id {
                self.mark_needs_semantics(root_id);
            }
        }
    }

    /// Returns whether layout is currently being performed.
    #[inline]
    pub fn debug_doing_layout(&self) -> bool {
        self.scheduler.debug_doing_layout()
    }

    /// Returns whether paint is currently being performed.
    #[inline]
    pub fn debug_doing_paint(&self) -> bool {
        self.scheduler.debug_doing_paint()
    }

    /// Returns whether semantics update is currently being performed.
    #[inline]
    pub fn debug_doing_semantics(&self) -> bool {
        self.scheduler.debug_doing_semantics()
    }

    /// Returns whether any pipeline phase is currently active.
    #[inline]
    pub fn debug_doing_any_phase(&self) -> bool {
        self.scheduler.debug_doing_any_phase()
    }
}
