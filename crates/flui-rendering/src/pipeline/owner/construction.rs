//! Constructor, ctor helpers, and run_frame orchestration for `PipelineOwner<Idle>`.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

use flui_layer::LayerTree;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::storage::RenderTree;

use crate::pipeline::{
    handle::DirtySender,
    notifier::VisualUpdateNotifier,
    phase::{Idle, Layout},
    scheduler::DirtyTracker,
};

use super::{DEFAULT_DIRTY_CHANNEL_CAPACITY, PIPELINE_ID_COUNTER, PipelineOwner, rebind_phase};

impl PipelineOwner<Idle> {
    /// Creates a new pipeline owner in the [`Idle`] phase with the
    /// default dirty-channel capacity (`DEFAULT_DIRTY_CHANNEL_CAPACITY`,
    /// 256).
    pub fn new() -> Self {
        Self::new_with_capacity(DEFAULT_DIRTY_CHANNEL_CAPACITY)
    }

    /// Creates a new pipeline owner in the [`Idle`] phase with a custom
    /// dirty-channel capacity. Use this when the default 256 doesn't match
    /// the producer profile.
    pub fn new_with_capacity(dirty_channel_capacity: usize) -> Self {
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(VisualUpdateNotifier::new()));
        let (dirty_sender, dirty_rx) =
            DirtySender::new_pair(dirty_channel_capacity, std::sync::Arc::clone(&notifier));
        let scheduler = DirtyTracker::new(std::sync::Arc::clone(&notifier));
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            relocation_owner_seal: std::rc::Rc::new(super::relocation::RelocationOwnerSeal),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            scheduler,
            layout_poison: super::poison::LayoutPoison::default(),
            root_constraints: None,
            semantics_enabled: AtomicBool::new(false),
            semantics_owner: None,
            semantics_update_callback: None,
            last_layer_tree: None,
            last_link_registry: None,
            last_follower_offsets: FxHashMap::default(),
            retained_boundaries: FxHashMap::default(),
            last_hidden_follower_ids: FxHashSet::default(),
            device_pixel_ratio: 1.0,
            dirty_sender,
            dirty_rx,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_child_requests: Vec::new(),
            pending_retain_bands: Vec::new(),
            _phase: PhantomData,
        }
    }

    /// Records harness parent metadata for `child_id`, cloned into the
    /// transient child slots before each layout walk.
    ///
    /// A second call for the same `child_id` replaces the previous seed.
    #[cfg(any(test, feature = "testing"))]
    pub fn seed_parent_data(&mut self, child_id: flui_foundation::RenderId, seed: ParentDataSeed) {
        self.parent_data_seeds.insert(child_id, seed);
    }

    /// Creates a new pipeline owner with callbacks in the [`Idle`] phase.
    pub fn with_callbacks<F, G, H>(
        on_need_visual_update: Option<F>,
        on_semantics_owner_created: Option<G>,
        on_semantics_owner_disposed: Option<H>,
    ) -> Self
    where
        F: Fn() + Send + Sync + 'static,
        G: Fn() + Send + Sync + 'static,
        H: Fn() + Send + Sync + 'static,
    {
        let mut notifier = VisualUpdateNotifier::new();
        if let Some(f) = on_need_visual_update {
            notifier.set_need_visual_update(f);
        }
        if let Some(f) = on_semantics_owner_created {
            notifier.set_semantics_owner_created(f);
        }
        if let Some(f) = on_semantics_owner_disposed {
            notifier.set_semantics_owner_disposed(f);
        }
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(notifier));
        let (dirty_sender, dirty_rx) = DirtySender::new_pair(
            DEFAULT_DIRTY_CHANNEL_CAPACITY,
            std::sync::Arc::clone(&notifier),
        );
        let scheduler = DirtyTracker::new(std::sync::Arc::clone(&notifier));
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            relocation_owner_seal: std::rc::Rc::new(super::relocation::RelocationOwnerSeal),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            scheduler,
            layout_poison: super::poison::LayoutPoison::default(),
            root_constraints: None,
            semantics_enabled: AtomicBool::new(false),
            semantics_owner: None,
            semantics_update_callback: None,
            last_layer_tree: None,
            last_link_registry: None,
            last_follower_offsets: FxHashMap::default(),
            retained_boundaries: FxHashMap::default(),
            last_hidden_follower_ids: FxHashSet::default(),
            device_pixel_ratio: 1.0,
            dirty_sender,
            dirty_rx,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_child_requests: Vec::new(),
            pending_retain_bands: Vec::new(),
            _phase: PhantomData,
        }
    }

    /// Transitions an idle pipeline into the [`Layout`] phase.
    ///
    /// Consumes `self`; once transitioned out of `Idle`, the legacy
    /// idle-only API (constructors, `run_frame`) is no longer reachable
    /// until you return through [`finish`](PipelineOwner::<crate::pipeline::phase::Semantics>::finish).
    #[must_use]
    pub fn into_layout(self) -> PipelineOwner<Layout> {
        rebind_phase(self)
    }

    // ========================================================================
    // Full-frame orchestrator
    // ========================================================================

    /// Runs a full frame: layout -> compositing-bits -> paint -> semantics.
    /// Consumes `self`, returns the owner back at [`Idle`] plus a
    /// [`RenderResult`](crate::RenderResult) indicating whether the frame produced a layer
    /// tree or failed mid-phase.
    ///
    /// The phase transitions are the load-bearing mechanism here -- each
    /// `run_*` method lives only on its matching phase's impl block, so
    /// the type system enforces the ordering. There is no runtime branch
    /// that could call `run_paint` before `run_layout`.
    ///
    /// # Error handling
    ///
    /// If any phase returns [`crate::error::RenderError`] (most notably
    /// [`crate::error::RenderError::Poisoned`] from a panicking render
    /// object), the owner is returned at [`Idle`] and the second element of
    /// the tuple is `Err(...)`. A paint tree already committed before a
    /// semantics error remains in the owner's `last_layer_tree`; it is not
    /// returned from this failed call, but a retry can repaint or submit the
    /// retained visual result. The owner is **always** usable for a
    /// subsequent frame on the success and error paths alike.
    #[must_use = "dropping the returned PipelineOwner<Idle> discards the pipeline handle; thread it back into the next frame"]
    pub fn run_frame(
        self,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        self.run_frame_impl(None)
    }

    /// Run a frame whose semantics phase returns the supplied error after
    /// paint has completed. Narrow same-module regression seam.
    #[cfg(test)]
    fn run_frame_with_semantics_error_for_test(
        self,
        error: crate::error::RenderError,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        self.run_frame_impl(Some(error))
    }

    fn run_frame_impl(
        mut self,
        semantics_error_for_test: Option<crate::error::RenderError>,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        // Observe node-bound cross-thread invalidation before any phase runs — an
        // async decode that finished while the app idled lands in this
        // frame, not never.
        self.drain_pending_dirty();

        // Layout
        let mut owner = self.into_layout();
        if let Err(e) = owner.run_layout() {
            return (owner.into_idle(), Err(e));
        }

        // Compositing
        let mut owner = owner.into_compositing();
        if let Err(e) = owner.run_compositing() {
            return (owner.into_idle(), Err(e));
        }

        // Paint
        let mut owner = owner.into_paint();
        if let Err(e) = owner.run_paint() {
            return (owner.into_idle(), Err(e));
        }

        // Semantics
        let mut owner = owner.into_semantics();
        let semantics_result = match semantics_error_for_test {
            Some(error) => Err(error),
            None => owner.run_semantics(),
        };
        if let Err(e) = semantics_result {
            // Semantics phase has no `into_idle` because the transition
            // to <Idle> goes via `finish`. Use `finish` to recover the
            // owner for the error path. The painted tree remains in
            // `last_layer_tree`; this failed call returns no tree, while a
            // retry can reuse the retained visual result.
            return (owner.finish(), Err(e));
        }

        let layer_tree = owner.take_layer_tree();
        (owner.finish(), Ok(layer_tree))
    }
}

#[cfg(test)]
mod tests {
    use flui_layer::LayerLink;
    use flui_tree::Leaf;
    use flui_types::{Offset, Size, geometry::px, painting::Alignment};

    use super::*;
    use crate::{
        constraints::BoxConstraints,
        context::{BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        protocol::BoxProtocol,
        traits::{RenderBox, RenderObject},
    };

    /// Leaf whose paint commits a non-empty, linked leader/follower pair.
    #[derive(Debug)]
    struct LinkedPaintBox {
        link: LayerLink,
    }

    impl flui_foundation::Diagnosticable for LinkedPaintBox {}

    impl RenderBox for LinkedPaintBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(Size::new(px(20.0), px(20.0)))
        }

        fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
            let size = ctx.size();
            ctx.with_leader(self.link, size, |ctx| {
                ctx.with_follower(
                    self.link,
                    size,
                    Offset::ZERO,
                    true,
                    Alignment::TOP_LEFT,
                    Alignment::TOP_LEFT,
                    |_| {},
                );
            });
        }
    }

    #[test]
    fn semantics_error_retains_the_painted_tree_and_its_non_empty_link_registry() {
        let mut owner = PipelineOwner::new();
        let root = owner.insert(Box::new(LinkedPaintBox {
            link: LayerLink::new(),
        }) as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(20.0), px(20.0)))));

        let (owner, result) = owner.run_frame_with_semantics_error_for_test(
            crate::error::RenderError::semantics("intentional transient test error"),
        );
        assert!(
            matches!(
                result,
                Err(crate::error::RenderError::SemanticsError { .. })
            ),
            "the injected error must escape from the semantics stage"
        );
        let retained_tree = format!(
            "{:?}",
            owner
                .layer_tree()
                .expect("painted tree must survive semantics failure")
        );
        let retained_registry = owner
            .link_registry()
            .expect("painted link registry must survive semantics failure");
        assert_eq!(retained_registry.leader_count(), 1);
        assert_eq!(retained_registry.follower_count(), 1);

        let (mut owner, retry) = owner.run_frame();
        let retried_tree = retry
            .expect("the retry must complete")
            .expect("the retained painted tree must be returned");
        let retried_registry = owner
            .take_link_registry()
            .expect("the matching retained registry must be returned");
        assert_eq!(format!("{retried_tree:?}"), retained_tree);
        assert_eq!(retried_registry.leader_count(), 1);
        assert_eq!(retried_registry.follower_count(), 1);
    }
}
