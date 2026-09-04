//! Phase-agnostic accessors, setters, insertion, dirty tracking, and hit-test
//! for `PipelineOwner<Phase: PipelinePhase>`.
//!
//! These methods are pure data access or side-effect-free notifier wiring.
//! They are valid in any phase: the borrow checker still gates `&mut self`
//! against the typestate transitions, but the methods themselves don't
//! care which phase the owner is in.

use flui_foundation::RenderId;
use flui_types::{Matrix4, Offset};

use crate::{
    RenderUpdateImpact,
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    pipeline::{dirty::DirtyNode, handle::DirtyKind, phase::PipelinePhase},
    protocol::{BoxProtocol, MainAxisPosition, SliverProtocol},
    storage::RenderNode,
};

use super::{PipelineOwner, subtree_arena::ensure_stack};

// ============================================================================
// Phase-agnostic accessors / setters / insertion
// ============================================================================

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Returns the unique identifier for this pipeline owner.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Sets the callback for when a visual update is needed.
    pub fn set_on_need_visual_update<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.write().set_need_visual_update(callback);
    }

    /// Sets the callback for when semantics owner is created.
    pub fn set_on_semantics_owner_created<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.write().set_semantics_owner_created(callback);
    }

    /// Sets the callback for when semantics owner is disposed.
    pub fn set_on_semantics_owner_disposed<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.write().set_semantics_owner_disposed(callback);
    }

    /// Requests a visual update.
    ///
    /// Called by render objects when they need to be re-rendered.
    pub fn request_visual_update(&self) {
        self.notifier.read().fire_need_visual_update();
    }

    // ========================================================================
    // Node-bound cross-thread invalidation
    // ========================================================================

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

    /// Drains the pending dirty-request channel into the scheduler's
    /// dirty sets.
    ///
    /// Called at phase boundaries by the typestate transitions; producers
    /// (background asset loaders, async work) write into the channel via
    /// a [`crate::pipeline::RenderInvalidationHandle`] and the owner observes them on
    /// the next frame. Non-blocking; processes every request
    /// available at the time of call and returns the count drained.
    pub fn drain_pending_dirty(&mut self) -> usize {
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
                tracing::trace!(?req, "drain_pending_dirty: stale attachment, dropped");
                continue;
            }
            match req.kind {
                DirtyKind::Layout => self.mark_needs_layout(req.id),
                DirtyKind::Compositing => self.mark_needs_compositing_bits_update(req.id),
                DirtyKind::Paint => self.mark_needs_paint(req.id),
                DirtyKind::Semantics => self.mark_needs_semantics(req.id),
            }
        }
        drained
    }

    /// Returns the root render object ID.
    pub fn root_id(&self) -> Option<RenderId> {
        self.root_id
    }

    /// Sets the root render object ID.
    pub fn set_root_id(&mut self, id: Option<RenderId>) {
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

    /// Returns a reference to the render tree.
    pub fn render_tree(&self) -> &crate::storage::RenderTree {
        &self.render_tree
    }

    // ========================================================================
    // Coordinate conversion (ADR-0021)
    // ========================================================================

    /// The committed laid-out size of a **box** node, or `None` if `id` is
    /// missing, is a sliver, or has not been laid out yet.
    ///
    /// The production twin of `flui_rendering::testing::box_geometry`, which is
    /// behind the `testing` feature and therefore unreachable from shipped code.
    #[must_use]
    pub fn box_size(&self, id: RenderId) -> Option<flui_types::Size> {
        self.render_tree.get(id)?.as_box()?.state().geometry()
    }

    /// The matrix mapping points in `descendant`'s local coordinate space into
    /// `ancestor`'s.
    ///
    /// Flutter's `RenderObject.getTransformTo` (`object.dart:3686`), **narrowed
    /// to a strict descendant → ancestor walk**: Flutter also handles two nodes
    /// that merely share a common ancestor, by inverting the target's half of the
    /// path. Nothing in FLUI needs that yet, and the narrow
    /// walk needs no inverse, so it cannot fail on a singular matrix.
    ///
    /// # When this returns `None`
    ///
    /// **Never as "no transform".** A `None` means the question cannot be
    /// answered, for one of two reasons:
    ///
    /// 1. `ancestor` is not an ancestor of `descendant` — including a missing id,
    ///    or two nodes in different trees. Flutter throws here
    ///    (`object.dart:3708`).
    /// 2. Some node on the path has **not been laid out**. Every step needs its
    ///    parent's committed size, and a size-dependent object — a `FittedBox`, a
    ///    rotation about its centre, a `RenderFractionalTranslation`, a
    ///    `RenderFlow` — yields a different, entirely plausible matrix if that
    ///    size is assumed to be `Size::ZERO`. Flutter asserts `hasSize` at the
    ///    call sites instead (`box.dart:3016`).
    ///
    /// `Some(IDENTITY)` when `descendant == ancestor`: a node's own space maps to
    /// itself whatever its size, and no step runs.
    ///
    /// # Composition
    ///
    /// Walks up from `descendant` collecting the path, then composes downward
    /// from `ancestor`, so the outermost step ends up leftmost:
    /// `T = A · B · … · P`, and a point `v` in `descendant`-local space lands at
    /// `T · v` in `ancestor`-local space. Each step is
    /// [`RenderObject::apply_paint_transform`](crate::traits::RenderObject::apply_paint_transform).
    #[must_use]
    pub fn transform_to(&self, descendant: RenderId, ancestor: RenderId) -> Option<Matrix4> {
        if descendant == ancestor {
            return self.render_tree.get(ancestor).map(|_| Matrix4::IDENTITY);
        }
        self.render_tree.get(descendant)?;

        // `path` ends up as [child-of-ancestor, …, descendant]: the nodes whose
        // local spaces we step *into*, in outermost-first order.
        //
        // Running out of parents is the **only** way to learn that `ancestor` is
        // not an ancestor — Flutter throws there (`object.dart:3708`). Everything
        // below this loop is then guaranteed by the tree's own invariants.
        let mut path = Vec::new();
        let mut current = descendant;
        loop {
            path.push(current);
            let parent = self.render_tree.parent(current)?;
            if parent == ancestor {
                break;
            }
            current = parent;
        }
        path.reverse();

        let mut transform = Matrix4::IDENTITY;
        let mut parent = ancestor;
        for &child in &path {
            let parent_node = self
                .render_tree
                .get(parent)
                .expect("BUG: transform_to walked to a parent that is not in the render tree");
            // `child`'s parent *is* `parent` — the walk above established it — so
            // the tree's parent/children links are inconsistent if this misses.
            // Not a `?`: a silent `None` here would masquerade as "not an
            // ancestor" and mask the corruption.
            let child_index = parent_node
                .children()
                .iter()
                .position(|&id| id == child)
                .expect("BUG: render tree parent link has no matching child link");
            let child_offset = self
                .render_tree
                .get(child)
                .expect("BUG: transform_to walked through a node that is not in the render tree")
                .offset();
            // `None` when `parent` has not been laid out. Not "no transform":
            // a size-dependent object produces a different, plausible matrix at
            // `Size::ZERO`. See `RenderNode::apply_paint_transform`.
            parent_node.apply_paint_transform(child_index, child_offset, &mut transform)?;
            parent = child;
        }
        Some(transform)
    }

    /// Converts `point` from `id`'s local coordinate space into `ancestor`'s, or
    /// into the render root's when `ancestor` is `None`.
    ///
    /// Flutter's `RenderBox.localToGlobal` (`box.dart:3113`), which is
    /// `MatrixUtils.transformPoint(getTransformTo(ancestor), point)`.
    ///
    /// `None` when [`transform_to`](Self::transform_to) is `None`, or when there
    /// is no render root to convert against.
    #[must_use]
    pub fn local_to_global(
        &self,
        id: RenderId,
        point: flui_types::Point,
        ancestor: Option<RenderId>,
    ) -> Option<flui_types::Point> {
        let ancestor = ancestor.or(self.root_id)?;
        let transform = self.transform_to(id, ancestor)?;
        let (x, y) = transform.transform_point(point.x, point.y);
        Some(flui_types::Point::new(x, y))
    }

    /// The inverse of [`local_to_global`](Self::local_to_global): converts
    /// `point` from `ancestor`'s space (the render root's, when `None`) into
    /// `id`'s local space.
    ///
    /// Flutter's `RenderBox.globalToLocal` (`box.dart:3062`) un-projects through
    /// the perspective divide onto the local z = 0 plane. FLUI's transforms are
    /// affine 2-D (`Matrix4::translation`/`scaling`/`rotation_z`/`skew_2d`), so a
    /// plain inverse is exact for every matrix any render object here produces.
    /// **A perspective transform would need Flutter's un-projection**; none exists
    /// in this repository, and one arriving must revisit this method.
    ///
    /// `None` when the transform is missing or **singular** — a zero-scale
    /// `FittedBox`, for instance, which maps every local point to one global
    /// point and cannot be inverted. Flutter returns `Offset.zero` there; a
    /// `None` says so instead of inventing an answer.
    #[must_use]
    pub fn global_to_local(
        &self,
        id: RenderId,
        point: flui_types::Point,
        ancestor: Option<RenderId>,
    ) -> Option<flui_types::Point> {
        let ancestor = ancestor.or(self.root_id)?;
        let inverse = self.transform_to(id, ancestor)?.try_inverse()?;
        let (x, y) = inverse.transform_point(point.x, point.y);
        Some(flui_types::Point::new(x, y))
    }

    /// Returns a mutable reference to the render tree.
    pub fn render_tree_mut(&mut self) -> &mut crate::storage::RenderTree {
        &mut self.render_tree
    }

    /// Returns a reference to the layer tree from the last paint phase.
    pub fn layer_tree(&self) -> Option<&flui_layer::LayerTree> {
        self.last_layer_tree.as_ref()
    }

    /// Takes the layer tree from the last paint phase.
    ///
    /// This removes the layer tree from the pipeline owner, returning ownership
    /// to the caller. Useful for passing to the compositor.
    ///
    /// Phase-agnostic: works in any phase. `run_frame` calls this on
    /// `<Semantics>` to extract the layer tree before transitioning back to
    /// `<Idle>`.
    pub fn take_layer_tree(&mut self) -> Option<flui_layer::LayerTree> {
        self.last_layer_tree.take()
    }

    /// Returns a reference to the leader/follower link registry produced as
    /// a byproduct of the last paint phase (see `FragmentComposer::link_registry`).
    pub fn link_registry(&self) -> Option<&flui_layer::LinkRegistry> {
        self.last_link_registry.as_ref()
    }

    /// Takes the link registry from the last paint phase.
    ///
    /// Pairs with [`Self::take_layer_tree`] — the caller hands both to
    /// `Scene::with_links` so `flui-engine` can resolve `Layer::Follower`
    /// positions at render time against the SAME frame's layer tree.
    pub fn take_link_registry(&mut self) -> Option<flui_layer::LinkRegistry> {
        self.last_link_registry.take()
    }

    /// Device pixel ratio threaded into every paint pass.
    pub fn device_pixel_ratio(&self) -> f32 {
        self.device_pixel_ratio
    }

    /// Number of repaint boundaries whose painted output is currently kept
    /// for reuse.
    ///
    /// Diagnostic surface for the retention cache — a test asserting that
    /// removing a node drops its entry has nothing else to look at, and the
    /// alternative is making the map itself public.
    #[must_use]
    pub fn retained_boundary_count(&self) -> usize {
        self.retained_boundaries.len()
    }

    /// Removes the subtree rooted at `id` — THE dispose site.
    ///
    /// Removal is where owner-side state dies (the inversion of the
    /// "Drop will handle it" idea): a `Drop` impl has no
    /// `&PipelineOwner`, so it cannot evict dirty-queue entries — only
    /// this method can. Order:
    ///
    /// 1. collect the subtree's ids;
    /// 2. call [`RenderObject::detach`](crate::traits::RenderObject::detach)
    ///    on every id in the subtree (ADR-0013) — the tree-lifecycle
    ///    counterpart of `attach`, and the only place a render object can
    ///    tear down an `attach`-time subscription;
    /// 3. evict every id from ALL dirty queues (live + mid-phase) so
    ///    no phase walks a freed slot's stale entry;
    /// 4. cascade-remove the nodes (each freed slot's generation bumps
    ///    — outstanding ids go stale);
    /// 5. clear `root_id` when the root itself was removed.
    ///
    /// `Drop` on render objects remains strictly node-local (decoded
    /// images, shaped text, GPU handles).
    ///
    /// Returns the number of nodes removed. Must not run mid-phase —
    /// the layout/paint walks hold raw borrows into the slab.
    pub fn remove_render_object(&mut self, id: RenderId) -> usize {
        debug_assert!(
            !self.scheduler.debug_doing_layout() && !self.scheduler.debug_doing_paint(),
            "remove_render_object during an active layout/paint phase — \
             the walks hold borrows into the slab; defer removal to \
             between-frame work",
        );

        let former_parent = self.render_tree.parent(id);
        let subtree = self.render_tree.collect_subtree_ids(id);
        if subtree.is_empty() {
            return 0;
        }

        // ADR-0013: detach every node in the subtree before eviction —
        // a render object that subscribed to a `Listenable` in `attach`
        // gets a chance to unsubscribe here. Not a correctness
        // prerequisite (the handle it holds is generational and already
        // goes silent once the node is gone), but the clean stop.
        for &subtree_id in &subtree {
            if let Some(node) = self.render_tree.get_mut(subtree_id) {
                let _ = node.clear_parent_data();
                let _ = node.detach();
            }
        }

        let removed: rustc_hash::FxHashSet<RenderId> = subtree.iter().copied().collect();
        self.scheduler.evict(&removed);
        // Layout-poison records for the removed subtree die here too, so
        // the map does not accumulate entries for nodes that no longer
        // exist.
        self.layout_poison.evict(&removed);
        // So do retained paint outputs. A stale entry can never be SERVED to a
        // different node — `RenderId` is generational, so a recycled slab slot
        // misses the map — but without this the map grows for the lifetime of
        // the owner as boundaries come and go.
        self.retained_boundaries
            .retain(|id, _| !removed.contains(id));

        let count = self.render_tree.remove_recursive(id);
        if self.root_id == Some(id) {
            self.root_id = None;
        }
        if let Some(parent_id) = former_parent {
            self.note_render_child_membership_changed(parent_id);
        }
        tracing::debug!(?id, count, "remove_render_object: subtree disposed");
        count
    }

    /// Adopt an existing render node and invalidate every parent phase whose
    /// output depends on child membership.
    pub fn adopt_render_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        let former_parent = self.render_tree.parent(child_id);
        if former_parent == Some(parent_id) {
            return;
        }
        if let Some(former_parent_id) = former_parent {
            self.render_tree.drop_child(former_parent_id, child_id);
        }
        self.render_tree.adopt_child(parent_id, child_id);
        if let Some(former_parent_id) = former_parent.filter(|id| *id != parent_id) {
            self.note_render_child_membership_changed(former_parent_id);
        }
        self.note_render_child_membership_changed(parent_id);
    }

    /// Detach an existing render child without deleting it and invalidate the
    /// surviving parent's membership-dependent phases.
    pub fn drop_render_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        self.render_tree.drop_child(parent_id, child_id);
        self.note_render_child_membership_changed(parent_id);
    }

    /// Invalidates every parent phase whose output depends on child membership.
    pub(crate) fn note_render_child_membership_changed(&mut self, parent_id: RenderId) {
        self.apply_render_update_impact(
            parent_id,
            RenderUpdateImpact::LAYOUT
                | RenderUpdateImpact::COMPOSITING_BITS
                | RenderUpdateImpact::SEMANTICS,
        );
    }

    /// Record a pure sibling-order change. Membership and semantics content
    /// are unchanged, but the parent must lay its children out again.
    pub fn note_render_children_reordered(&mut self, parent_id: RenderId) {
        self.apply_render_update_impact(parent_id, RenderUpdateImpact::LAYOUT);
    }

    /// Hit-tests the render tree at `position` (root-local
    /// coordinates), appending leaf-first entries to `result`.
    ///
    /// The walk mirrors the paint walk's shape: per node, the
    /// protocol blanket wraps a driver-supplied child-recursion
    /// callback in the typed, arity-gated hit-test context and calls
    /// the object's `hit_test`. `None` child overrides resolve to the
    /// child's laid-out `RenderState.offset` — parents no longer
    /// mirror offsets in their own fields to hit-test children.
    ///
    /// Returns whether anything was hit. O(visited nodes) average;
    /// worst case O(tree) when nothing claims the position.
    pub fn hit_test(&self, position: Offset, result: &mut crate::hit_testing::HitTestResult) -> bool
    where
        // The child-recursion callback must be Send + Sync (inherited
        // ctx trait bounds); it captures &self, so the phase
        // marker must be Sync. Every phase is a ZST — always satisfied
        // at call sites, spelled out for the generic impl.
        Phase: Sync,
    {
        let Some(root_id) = self.root_id else {
            return false;
        };
        self.hit_test_subtree(root_id, position, result)
    }

    fn hit_test_subtree(
        &self,
        id: RenderId,
        position: Offset,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        ensure_stack(|| self.hit_test_subtree_impl(id, position, result))
    }

    /// Body of [`Self::hit_test_subtree`]; split out so every
    /// recursion level enters through the [`ensure_stack`] probe.
    fn hit_test_subtree_impl(
        &self,
        id: RenderId,
        position: Offset,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        let Some(node) = self.render_tree.get(id) else {
            return false;
        };
        let Some(entry) = node.as_box() else {
            if node.as_sliver().is_some() {
                let sliver_position = Self::sliver_hit_position_from_offset(node, position);
                return self.hit_test_sliver_subtree(id, sliver_position, result);
            }
            return false;
        };

        // ADR-0015: composite-resolved follower offsets. Gated behind
        // both side tables being empty — true for the overwhelming
        // majority of trees (followers are rare: tooltips, dropdowns,
        // overlays), so this whole branch is a single cheap `is_empty`
        // check away from a no-op for every other tree.
        let follower_offset =
            if self.last_follower_offsets.is_empty() && self.last_hidden_follower_ids.is_empty() {
                None
            } else if self.last_hidden_follower_ids.contains(&id) {
                // Correlated as a follower this frame but resolved to hidden
                // (unlinked, `show_when_unlinked == false`) — mirrors the
                // render path's `resolve_follower_offset -> None -> don't
                // descend` (flui-engine's `render_layer_recursive`) and
                // oracle's early return in `FollowerLayer.addToScene`
                // (`layer.dart:2857-2865`). No HitTestEntry, no descent.
                return false;
            } else {
                self.last_follower_offsets.get(&id).copied()
            };

        let children: Vec<RenderId> = node.children().to_vec();
        // The generation this node's last layout stamped onto the children it
        // laid out; anything else was not part of that pass.
        let parent_generation = node.layout_generation();
        let render_object = entry.render_object();
        // Box bounds gate size, resolved from RenderState (geometry's
        // sole owner — 2B field dedup); threaded into hit_test_raw so the
        // default gate reads `ctx.is_within_own_size()`.
        let own_size = entry.state().geometry().unwrap_or(flui_types::Size::ZERO);

        // Push the node's hit-test transform onto the HitTestResult
        // stack BEFORE recursing, so child entries captured during
        // hit_test_raw see the correct accumulated transform. The
        // transform stays on the stack until after the parent entry
        // is added (Flutter parity: `addWithPaintTransform`
        // (`rendering/box.dart:799-812`) inverts the incoming transform at
        // line 805 and delegates at line 811 to `addWithRawTransform`,
        // which pushes the (already-inverted) transform at line 876). The
        // hook gets `own_size` from RenderState for alignment-relative
        // origins.
        //
        // Pushed as the INVERSE of `hit_test_transform`: `HitTestResult`
        // composes the global-to-local mapping by left-multiplying each
        // level's own inverse in descent order (see `HitTestEntry::transform`),
        // so a caller pushing the forward (paint-direction) matrix here
        // recorded the wrong composition for any chain mixing this transform
        // with an outer offset -- correct only when the whole chain
        // commutes (pure translations). A non-invertible `hit_test_transform`
        // (e.g. a zero-scale `Transform`) falls back to pushing the
        // still-singular forward matrix: when the determinant is exactly
        // zero, the composed chain stays singular, so delivery still
        // detects and skips it (`LocalEventTransform::capture`). For a
        // merely near-singular transform (`0 < |det| < f32::EPSILON`, which
        // `Matrix4::is_invertible` also rejects) the skip is only
        // threshold-relative, not guaranteed: determinants compose
        // multiplicatively, so a large-determinant ancestor can lift the
        // product back above `f32::EPSILON`, and delivery then hands the
        // entry a garbage local position instead of skipping it -- out of
        // scope to change that skip behavior here.
        let hit_transform = render_object.hit_test_transform(own_size);
        let has_transform = hit_transform.is_some();
        if let Some(t) = hit_transform {
            result.push_transform(t.try_inverse().unwrap_or(t));
        }
        // A resolved follower offset rides the SAME transform-stack
        // lifecycle as `hit_test_transform` (ADR-0015) — the same
        // translation the paint/GPU path applies via
        // `backend.push_offset(resolved)` (renderer.rs:1586), lifted to a
        // `Matrix4` (exact and lossless — the composer only ever
        // shifts coordinate space via translation). Pushed as the inverse
        // translation for the same reason as `hit_test_transform` above.
        if let Some(r) = follower_offset {
            result.push_transform(Matrix4::translation(-r.dx.get(), -r.dy.get(), 0.0));
        }

        // Shift the position handed into this node's own subtree by the
        // resolved offset — the SAME position-shift pattern `hit_child`
        // below already applies for ordinary child offsets, applied here
        // by the WALK rather than the object: `RenderFollowerLayer::hit_test`
        // stays a plain structural forward (ADR-0015).
        let position = match follower_offset {
            Some(r) => position - r,
            None => position,
        };

        let mut hit_child = |index: usize,
                             override_pos: Option<Offset>,
                             local_transform: Option<Matrix4>|
         -> bool {
            let Some(&child_id) = children.get(index) else {
                return false;
            };
            let Some(child_node) = self.render_tree.get(child_id) else {
                return false;
            };
            // Skip a child this pass did not lay out, exactly as paint does.
            // Its committed offset describes a pass that no longer holds, so
            // hitting it would return something the user cannot see — and
            // paint already skips it, so allowing the hit would make the two
            // disagree, which is worse than the stale rect either alone.
            if !child_node.was_placed_by(id, parent_generation) {
                return false;
            }
            // `local_transform` is the object's own FORWARD (paint-direction)
            // transform, accumulated by its ctx-level `push_transform`/
            // `push_offset` calls (`BoxHitTestCtx::composed_transform`).
            // `with_paint_transform` pushes its INVERSE onto the shared
            // `HitTestResult`, matching every other forward-transform push
            // in this walk (`hit_test_transform` above, the follower offset,
            // Flutter's own `addWithPaintTransform`). Scoped to exactly this
            // one child recursion — the ctx that accumulated it already
            // scoped the push/pop to the same call.
            let dispatch_child = |result: &mut crate::hit_testing::HitTestResult| -> bool {
                if child_node.as_sliver().is_some() {
                    // Explicit positions are already child-local; the layout-offset
                    // fallback starts from the child's physical paint offset.
                    return if let Some(position) = override_pos {
                        let child_position =
                            Self::sliver_hit_position_from_offset(child_node, position);
                        self.hit_test_sliver_subtree(child_id, child_position, result)
                    } else {
                        if !Self::sliver_child_is_visible(child_node) {
                            return false;
                        }
                        let child_offset = child_node.offset();
                        result.with_paint_offset(child_offset, |result| {
                            let child_position = Self::sliver_hit_position_from_paint_offset(
                                child_node,
                                position - child_offset,
                            );
                            self.hit_test_sliver_subtree(child_id, child_position, result)
                        })
                    };
                }
                if let Some(child_position) = override_pos {
                    self.hit_test_subtree(child_id, child_position, result)
                } else {
                    let child_offset = child_node.offset();
                    result.with_paint_offset(child_offset, |result| {
                        self.hit_test_subtree(child_id, position - child_offset, result)
                    })
                }
            };

            match local_transform {
                Some(local_transform) => {
                    result.with_paint_transform(local_transform, dispatch_child)
                }
                None => dispatch_child(result),
            }
        };

        let hit = render_object.hit_test_raw(position, children.len(), own_size, &mut hit_child);
        if hit.add_self {
            // Leaf-first path: children pushed their entries during
            // the callback above; the ancestor follows. The transform
            // is still on the stack, so this entry captures it.
            //
            // A render object that listens for pointer events (RenderListener)
            // advertises its data-only pointer target here; it rides on the
            // entry so dispatch can resolve the owner-local handler. Every
            // other render object returns `None`, leaving a target-less entry.
            let entry = crate::hit_testing::HitTestEntry::new(id);
            let entry = match render_object.pointer_target() {
                Some(target) => entry.pointer_target(target),
                None => entry,
            };
            let entry = match render_object.scroll_target() {
                Some(target) => entry.scroll_target(target),
                None => entry,
            };
            let entry = match render_object.pan_zoom_target() {
                Some(target) => entry.pan_zoom_target(target),
                None => entry,
            };
            let entry = entry.cursor(render_object.mouse_cursor());
            let entry = match render_object.mouse_tracker_annotation(id) {
                Some(annotation) => entry.mouse_annotation(annotation),
                None => entry,
            };
            result.add(entry);
        }

        if follower_offset.is_some() {
            result.pop_transform();
        }
        if has_transform {
            result.pop_transform();
        }

        hit.blocks_below
    }

    fn sliver_hit_position_from_offset(node: &RenderNode, position: Offset) -> MainAxisPosition {
        let axis_direction = node
            .as_sliver()
            .and_then(|entry| entry.state().constraints())
            .map(|constraints| constraints.axis_direction);

        match axis_direction {
            Some(
                flui_types::layout::AxisDirection::LeftToRight
                | flui_types::layout::AxisDirection::RightToLeft,
            ) => MainAxisPosition::from_horizontal_offset(position),
            _ => MainAxisPosition::from_vertical_offset(position),
        }
    }

    fn sliver_hit_position_from_paint_offset(
        node: &RenderNode,
        position: Offset,
    ) -> MainAxisPosition {
        let Some(entry) = node.as_sliver() else {
            return MainAxisPosition::from_vertical_offset(position);
        };
        let Some(constraints) = entry.state().constraints() else {
            return MainAxisPosition::from_vertical_offset(position);
        };

        let (raw_main, cross_axis) = match constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => {
                (position.dx.get(), position.dy.get())
            }
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => {
                (position.dy.get(), position.dx.get())
            }
        };
        let effective_axis_direction = match constraints.growth_direction {
            crate::constraints::GrowthDirection::Forward => constraints.axis_direction,
            crate::constraints::GrowthDirection::Reverse => constraints.axis_direction.opposite(),
        };
        let main_axis = if effective_axis_direction.is_reversed() {
            entry
                .state()
                .geometry()
                .map_or(raw_main, |geometry| geometry.paint_extent - raw_main)
        } else {
            raw_main
        };

        MainAxisPosition::new(main_axis, cross_axis)
    }

    fn sliver_child_is_visible(node: &RenderNode) -> bool {
        node.as_sliver()
            .and_then(|entry| entry.state().geometry())
            .is_some_and(|geometry| geometry.visible)
    }

    fn sliver_hit_position_minus_paint_offset(
        parent_constraints: &SliverConstraints,
        parent_geometry: &SliverGeometry,
        child_node: &RenderNode,
        position: MainAxisPosition,
        offset: Offset,
    ) -> MainAxisPosition {
        let Some(child_entry) = child_node.as_sliver() else {
            return position;
        };
        let Some(child_constraints) = child_entry.state().constraints() else {
            return position;
        };
        let Some(child_geometry) = child_entry.state().geometry() else {
            return position;
        };

        let (offset_main, offset_cross) = match child_constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => (offset.dx.get(), offset.dy.get()),
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => (offset.dy.get(), offset.dx.get()),
        };
        let parent_physical_main = if parent_constraints
            .growth_direction
            .apply_to_axis_direction(parent_constraints.axis_direction)
            .is_reversed()
        {
            parent_geometry.paint_extent - position.main_axis
        } else {
            position.main_axis
        };
        let child_physical_main = parent_physical_main - offset_main;
        let child_main = if child_constraints
            .growth_direction
            .apply_to_axis_direction(child_constraints.axis_direction)
            .is_reversed()
        {
            child_geometry.paint_extent - child_physical_main
        } else {
            child_physical_main
        };

        MainAxisPosition::new(child_main, position.cross_axis - offset_cross)
    }

    fn box_hit_offset_from_sliver_position(
        constraints: &SliverConstraints,
        geometry: &SliverGeometry,
        child_size: flui_types::Size,
        position: MainAxisPosition,
        offset: Offset,
    ) -> Offset {
        let right_way_up = crate::constraints::right_way_up(
            constraints.axis_direction,
            constraints.growth_direction,
        );

        let (paint_main, paint_cross, child_main_extent) = match constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => {
                (offset.dx.get(), offset.dy.get(), child_size.width.get())
            }
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => {
                (offset.dy.get(), offset.dx.get(), child_size.height.get())
            }
        };
        let child_main_axis_position = if right_way_up {
            paint_main
        } else {
            geometry.paint_extent - child_main_extent - paint_main
        };
        let mut local_main = position.main_axis - child_main_axis_position;
        if !right_way_up {
            local_main = child_main_extent - local_main;
        }
        let local_cross = position.cross_axis - paint_cross;

        match constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => Offset::new(
                flui_types::geometry::px(local_main),
                flui_types::geometry::px(local_cross),
            ),
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => Offset::new(
                flui_types::geometry::px(local_cross),
                flui_types::geometry::px(local_main),
            ),
        }
    }

    fn hit_test_sliver_subtree(
        &self,
        id: RenderId,
        position: MainAxisPosition,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        ensure_stack(|| self.hit_test_sliver_subtree_impl(id, position, result))
    }

    fn hit_test_sliver_subtree_impl(
        &self,
        id: RenderId,
        position: MainAxisPosition,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        let Some(node) = self.render_tree.get(id) else {
            return false;
        };
        let Some(entry) = node.as_sliver() else {
            return false;
        };
        let Some(geometry) = entry.state().geometry() else {
            return false;
        };
        let Some(constraints) = entry.state().constraints() else {
            return false;
        };
        if position.main_axis < 0.0
            || position.main_axis >= geometry.hit_test_extent
            || position.cross_axis < 0.0
            || position.cross_axis >= constraints.cross_axis_extent
        {
            return false;
        }
        let children: Vec<RenderId> = node.children().to_vec();
        let parent_generation = node.layout_generation();
        let render_object = entry.render_object();
        // Absolute paint size from RenderState — threaded into
        // hit_test_raw for signature uniformity; the sliver hit gate is
        // driver-owned (checked above), so the sliver context ignores it.
        let own_size = entry.state().absolute_paint_size();

        let mut hit_child = |index: usize,
                             override_pos: Option<MainAxisPosition>,
                             // The sliver protocol's ctx-level transform stack is a
                             // permanent no-op (`SliverHitTestCtx::push_transform`) —
                             // this is threaded only for signature uniformity with
                             // the shared `RenderObject::hit_test_raw` trait method
                             // and is always `None`.
                             _local_transform: Option<Matrix4>|
         -> bool {
            let Some(&child_id) = children.get(index) else {
                return false;
            };
            let Some(child_node) = self.render_tree.get(child_id) else {
                return false;
            };
            // See the box walk: a child this pass did not lay out is skipped,
            // matching paint so the two cannot disagree about what is there.
            if !child_node.was_placed_by(id, parent_generation) {
                return false;
            }
            // Rule for whether this closure pushes the child's paint offset
            // onto the `HitTestResult` transform stack: push iff the
            // position handed to the child below was moved into the
            // child's own frame. Flutter parity:
            // `RenderSliverHelpers::hitTestBoxChild` (`rendering/sliver.dart`)
            // and `RenderSliverPadding::hitTestChildren`
            // (`rendering/sliver_padding.dart`) both always push
            // `paintOffset` via `SliverHitTestResult::addWithAxisOffset`
            // regardless of how the caller derived the position, because
            // Flutter's sliver protocol has no override-position concept —
            // every call site both repositions and pushes.
            if child_node.as_sliver().is_some() {
                if let Some(child_position) = override_pos {
                    // NOT moved into a new frame: an override caller already
                    // hands back sliver-local coordinates. Holds today only
                    // because the sole workspace supplier
                    // (`RenderSliverOffstage::hit_test`) is a transparent
                    // passthrough that never repositions its child.
                    debug_assert_eq!(
                        child_node.offset(),
                        Offset::ZERO,
                        "sliver hit-test override skips the transform-stack push, which is \
                         only sound while the child's committed paint offset is zero; a \
                         supplier with a nonzero offset must route through the non-override \
                         path so the offset gets pushed"
                    );
                    self.hit_test_sliver_subtree(child_id, child_position, result)
                } else {
                    let child_offset = child_node.offset();
                    let child_position = Self::sliver_hit_position_minus_paint_offset(
                        constraints,
                        &geometry,
                        child_node,
                        position,
                        child_offset,
                    );
                    result.with_paint_offset(child_offset, |result| {
                        self.hit_test_sliver_subtree(child_id, child_position, result)
                    })
                }
            } else if let Some(child_entry) = child_node.as_box() {
                let Some(child_size) = child_entry.state().geometry() else {
                    return false;
                };
                // Moved into the child's own box frame in both branches:
                // `box_hit_offset_from_sliver_position` always decomposes
                // into box-local coordinates, override or not. Both push.
                let child_offset = child_node.offset();
                let child_main_position = override_pos.unwrap_or(position);
                let child_position = Self::box_hit_offset_from_sliver_position(
                    constraints,
                    &geometry,
                    child_size,
                    child_main_position,
                    child_offset,
                );
                result.with_paint_offset(child_offset, |result| {
                    self.hit_test_subtree(child_id, child_position, result)
                })
            } else {
                false
            }
        };

        let hit = render_object.hit_test_raw(position, children.len(), own_size, &mut hit_child);
        if hit.add_self {
            let entry =
                crate::hit_testing::HitTestEntry::new(id).cursor(render_object.mouse_cursor());
            let entry = match render_object.mouse_tracker_annotation(id) {
                Some(annotation) => entry.mouse_annotation(annotation),
                None => entry,
            };
            result.add(entry);
        }
        hit.blocks_below
    }

    /// Sets the device pixel ratio for subsequent paint passes.
    ///
    /// Called by the platform binding on surface creation and DPI
    /// change. Non-finite or non-positive values are rejected (kept at
    /// the previous ratio) — a zero or NaN DPR poisons every shaped
    /// glyph and snapped hairline downstream.
    pub fn set_device_pixel_ratio(&mut self, dpr: f32) {
        if dpr.is_finite() && dpr > 0.0 {
            self.device_pixel_ratio = dpr;
        } else {
            tracing::warn!(
                dpr,
                "set_device_pixel_ratio: rejecting non-finite / non-positive \
                 ratio; keeping {}",
                self.device_pixel_ratio,
            );
        }
    }

    // ========================================================================
    // RenderObject Insertion (with dirty tracking)
    // ========================================================================

    /// Inserts a render object into the tree and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object into the RenderTree
    /// 2. Adds the node to the dirty layout list (since new nodes need layout)
    /// 3. Adds the node to the dirty paint list (since new nodes need paint)
    ///
    /// Use this instead of `render_tree_mut().insert()` to ensure proper dirty
    /// tracking.
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted node.
    pub fn insert<P>(&mut self, render_object: Box<dyn crate::traits::RenderObject<P>>) -> RenderId
    where
        P: crate::protocol::Protocol,
        crate::storage::RenderNode: From<Box<dyn crate::traits::RenderObject<P>>>,
    {
        use flui_tree::traits::TreeWrite;

        // Convert to RenderNode using From impl (zero-cost, compile-time dispatch)
        let node: crate::storage::RenderNode = render_object.into();
        let id = self.render_tree.insert(node);
        let depth = self.render_tree.depth(id).unwrap_or(0) as usize;

        // Bootstrap the IS_REPAINT_BOUNDARY flag from the
        // render_object's static answer before the dirty pushes (so
        // the compositing walk has accurate boundary info on first
        // run_compositing).
        self.bootstrap_repaint_boundary_flag(id);

        // ADR-0013: hand the freshly-inserted node its self-dirty handle.
        self.attach_inserted_node(id);

        // New nodes need layout and paint
        self.add_node_needing_layout(id, depth);
        self.schedule_initial_paint(id);

        id
    }

    /// Inserts a Box-protocol child and records canonical membership impacts.
    ///
    /// This method:
    /// 1. Inserts the render object as a child in the RenderTree
    /// 2. Adds the child to the dirty layout list
    /// 3. Applies `LAYOUT | COMPOSITING_BITS | SEMANTICS` to the parent
    ///    (semantics queueing remains gated by whether semantics is enabled)
    /// 4. Defers paint queueing until the successful layout pass
    ///
    /// Use this instead of `render_tree_mut().insert_child()` to ensure proper
    /// dirty tracking.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The parent node ID
    /// * `render_object` - The render object to insert as child
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted child, or `None` if parent doesn't exist.
    pub fn insert_child_render_object(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::traits::RenderObject<BoxProtocol>>,
    ) -> Option<RenderId> {
        // Get parent depth before insertion
        let parent_depth = self.render_tree.depth(parent_id)?;

        // Insert child (using Box protocol)
        let child_id = self
            .render_tree
            .insert_box_child(parent_id, render_object)?;
        let child_depth = parent_depth + 1;

        // Bootstrap the IS_REPAINT_BOUNDARY flag from the
        // child render_object's static answer before any compositing
        // walk runs.
        self.bootstrap_repaint_boundary_flag(child_id);

        // ADR-0013: hand the freshly-inserted child its self-dirty handle.
        self.attach_inserted_node(child_id);

        // Layout commits the child's geometry and schedules its eventual paint;
        // membership invalidation applies the parent's other required phases.
        self.add_node_needing_layout(child_id, child_depth as usize);
        self.note_render_child_membership_changed(parent_id);

        Some(child_id)
    }

    /// Inserts a Sliver-protocol child and records canonical membership
    /// impacts.
    ///
    /// The Sliver-protocol counterpart of [`Self::insert_child_render_object`]
    /// — same dirty tracking and ADR-0013 attach wiring, backed by
    /// [`crate::storage::RenderTree::insert_sliver_child`] instead of
    /// `insert_box_child`. Prefer this over calling
    /// `render_tree_mut().insert_sliver_child(..)` directly: the raw tree
    /// method inserts the node but skips dirty tracking AND the
    /// ADR-0013 `attach` handshake, silently starving any Sliver render
    /// object that subscribes to a `Listenable` in `attach` (e.g. a
    /// snap-animation controller) of its self-dirty handle.
    ///
    /// This method:
    /// 1. Inserts the render object as a Sliver child in the RenderTree
    /// 2. Hands it its ADR-0013 self-dirty handle via `attach`
    /// 3. Adds the child to the dirty layout list
    /// 4. Applies `LAYOUT | COMPOSITING_BITS | SEMANTICS` to the parent
    ///    (semantics queueing remains gated by whether semantics is enabled)
    /// 5. Defers paint queueing until the successful layout pass
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The parent node ID
    /// * `render_object` - The Sliver render object to insert as child
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted child, or `None` if parent doesn't exist.
    pub fn insert_sliver_child_render_object(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::traits::RenderObject<SliverProtocol>>,
    ) -> Option<RenderId> {
        // Get parent depth before insertion
        let parent_depth = self.render_tree.depth(parent_id)?;

        // Insert child (using Sliver protocol)
        let child_id = self
            .render_tree
            .insert_sliver_child(parent_id, render_object)?;
        let child_depth = parent_depth + 1;

        // Bootstrap the IS_REPAINT_BOUNDARY flag from the
        // child render_object's static answer before any compositing
        // walk runs.
        self.bootstrap_repaint_boundary_flag(child_id);

        // ADR-0013: hand the freshly-inserted child its self-dirty handle.
        self.attach_inserted_node(child_id);

        // Layout commits the child's geometry and schedules its eventual paint;
        // membership invalidation applies the parent's other required phases.
        self.add_node_needing_layout(child_id, child_depth as usize);
        self.note_render_child_membership_changed(parent_id);

        Some(child_id)
    }

    /// Inserts a raw `RenderNode` directly into the tree.
    ///
    /// This bypasses the `RenderObject<P>` trait requirement and is used for
    /// special nodes like `RenderView` that manage their own layout/paint
    /// lifecycle outside the standard protocol dispatch.
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted node.
    pub fn insert_render_node(&mut self, node: crate::storage::RenderNode) -> RenderId {
        use flui_tree::traits::TreeWrite;

        let id = self.render_tree.insert(node);
        let depth = self.render_tree.depth(id).unwrap_or(0) as usize;

        // Bootstrap the IS_REPAINT_BOUNDARY flag (matches the
        // `insert` / `insert_child_render_object` paths so every code
        // path that adds nodes leaves the compositing flag in sync
        // with the trait answer).
        self.bootstrap_repaint_boundary_flag(id);

        // ADR-0013: hand the freshly-inserted node its self-dirty handle.
        self.attach_inserted_node(id);

        self.add_node_needing_layout(id, depth);
        self.schedule_initial_paint(id);

        id
    }

    /// Sets the root render object and marks it as needing layout.
    ///
    /// This is a convenience method that:
    /// 1. Inserts the render object
    /// 2. Sets it as the root
    /// 3. Ensures it's in the dirty lists
    ///
    /// # Returns
    ///
    /// The `RenderId` of the root node.
    pub fn set_root_render_object(
        &mut self,
        render_object: Box<dyn crate::traits::RenderObject<BoxProtocol>>,
    ) -> RenderId {
        let id = self.insert(render_object);
        self.root_id = Some(id);
        id
    }

    /// Bootstraps the `IS_REPAINT_BOUNDARY` storage flag from the render
    /// object's static trait answer (`RenderObject::is_repaint_boundary()`).
    ///
    /// Every node-insert path
    /// ([`Self::insert`], [`Self::insert_child_render_object`],
    /// [`Self::insert_render_node`]; [`Self::set_root_render_object`]
    /// inherits via `insert`) calls this immediately after the tree
    /// `insert` so the compositing-bits walk has accurate
    /// boundary info via the storage flag from frame 1.
    ///
    /// **Current consumer scope:** the compositing-bits walk consults
    /// `RenderNode::is_repaint_boundary_flag()`. The paint walk
    /// (`paint_subtree`) still reads `render_object.is_repaint_boundary()`
    /// directly — this matches Flutter parity (Flutter's `paint`
    /// reads the `isRepaintBoundary` final getter, equivalent to our
    /// trait answer; the bootstrap flag is the optimization target
    /// for a later sweep that swaps the paint check too).
    ///
    /// Before this bootstrap existed, the storage flag was effectively
    /// `false` for every node from the moment it entered the tree,
    /// which forced the compositing walk to fall through to the trait
    /// answer (`render_object().is_repaint_boundary()`) at every check
    /// site — a virtual dispatch and a divergence risk if a future
    /// caller flipped the flag dynamically.
    ///
    /// No-op if `id` is not present (defensive — every call site holds
    /// a freshly-inserted id, but a stale id passes through silently
    /// rather than panicking).
    #[inline]
    pub(super) fn bootstrap_repaint_boundary_flag(&self, id: RenderId) {
        if let Some(node) = self.render_tree.get(id) {
            let is_boundary = node.is_repaint_boundary();
            node.set_repaint_boundary_flag(is_boundary);
        }
    }

    /// ADR-0013: hands the freshly-inserted node at `id` its self-dirty
    /// [`RenderInvalidationHandle`](crate::pipeline::RenderInvalidationHandle) via
    /// [`RenderObject::attach`](crate::traits::RenderObject::attach).
    ///
    /// Called by every insertion path (`insert`, `insert_child_render_object`,
    /// `insert_sliver_child_render_object`, `insert_render_node`) right after
    /// the id is minted and its `NodeLinks` are wired, alongside the initial
    /// dirty marks those methods already issue. `pub(super)` (rather than
    /// private) so those call sites can reach it. No-op if `id` is somehow
    /// not present (defensive — every call site holds a freshly-inserted id).
    #[inline]
    pub(super) fn attach_inserted_node(&mut self, id: RenderId) {
        let Some(epoch) = self
            .render_tree
            .get(id)
            .and_then(crate::storage::RenderNode::next_attachment_epoch)
        else {
            return;
        };
        let handle =
            crate::pipeline::RenderInvalidationHandle::new(self.dirty_sender.clone(), id, epoch);
        if let Some(node) = self.render_tree.get_mut(id) {
            let _ = node.attach(epoch, handle);
        }
    }

    // ========================================================================
    // Dirty Node Access (Flutter API)
    // ========================================================================

    /// Returns the nodes needing layout.
    ///
    /// These are relayout boundaries that need to be laid out in the next
    /// layout phase.
    #[inline]
    pub fn nodes_needing_layout(&self) -> &[DirtyNode] {
        self.scheduler.nodes_needing_layout()
    }

    /// Returns the nodes needing paint.
    ///
    /// These are repaint boundaries that need to be painted in the next
    /// paint phase.
    #[inline]
    pub fn nodes_needing_paint(&self) -> &[DirtyNode] {
        self.scheduler.nodes_needing_paint()
    }

    /// Returns the nodes needing compositing bits update.
    #[inline]
    pub fn nodes_needing_compositing_bits_update(&self) -> &[DirtyNode] {
        self.scheduler.nodes_needing_compositing_bits_update()
    }

    /// Returns the nodes needing semantics update.
    #[inline]
    pub fn nodes_needing_semantics(&self) -> &[DirtyNode] {
        self.scheduler.nodes_needing_semantics()
    }

    /// Adds a node to the layout dirty list.
    ///
    /// Routes into the mid-phase side queue when layout is active, otherwise
    /// into `dirty.needs_layout`. See `DirtyTracker::add_node_needing_layout`
    /// for the full routing and dedup contract.
    pub fn add_node_needing_layout(&mut self, node_id: RenderId, depth: usize) {
        self.scheduler.add_node_needing_layout(node_id, depth);
    }

    /// Marks a node as needing layout, propagating the `NEEDS_LAYOUT` flag
    /// up the ancestor chain and pushing the **relayout boundary** onto
    /// `dirty.needs_layout` for the next `run_layout` pass.
    ///
    /// Ports Flutter's `markNeedsLayout`
    /// walk (`.flutter/.../object.dart:2658-2700`). Thin forwarder: the walk
    /// logic lives in `DirtyTracker::mark_needs_layout` so it is
    /// unit-testable without a full owner. The `scheduler`, `render_tree`,
    /// and `layout_poison` fields are disjoint, so the split borrow
    /// compiles.
    ///
    /// Every node the walk visits is also un-poisoned: an invalidation mark
    /// means the node's inputs actually changed, which is the one signal
    /// that re-arms a layout-poisoned node for another attempt.
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        self.scheduler
            .mark_needs_layout(&mut self.render_tree, id, &mut |visited| {
                self.layout_poison.unpoison(visited);
            });
    }

    /// Marks a node as needing paint and schedules the nearest established
    /// repaint boundary.
    ///
    /// Ports Flutter's `RenderObject.markNeedsPaint`: the invalidation flag
    /// propagates toward the root until an existing repaint boundary owns the
    /// affected retained layer. A boundary introduced this frame has no layer
    /// to update yet, so the walk continues to the nearest established owner.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        self.scheduler.mark_needs_paint(&self.render_tree, id);
    }

    /// Schedules first paint for a freshly attached render object.
    ///
    /// New render state starts with `NEEDS_PAINT`, but no repaint boundary has
    /// produced a retained layer yet. Initial attachment therefore queues the
    /// new node directly once; every subsequent invalidation must go through
    /// [`Self::mark_needs_paint`].
    pub(super) fn schedule_initial_paint(&mut self, id: RenderId) {
        let Some(node) = self.render_tree.get(id) else {
            return;
        };
        self.scheduler
            .schedule_paint_boundary(id, node.depth() as usize);
    }

    /// Marks compositing bits dirty using Flutter's repaint-boundary walk.
    ///
    /// A stale ID is a no-op. The walk marks the necessary ancestor chain and
    /// queues exactly the node responsible for recomputing the affected bits.
    pub fn mark_needs_compositing_bits_update(&mut self, node_id: RenderId) {
        self.scheduler
            .mark_needs_compositing_bits_update(&self.render_tree, node_id);
    }

    /// Marks a live node as needing semantics when semantics are enabled.
    ///
    /// Semantics dirtiness is currently queue-authoritative: the legacy
    /// `NEEDS_SEMANTICS` render-state flag is not set by this method because
    /// the semantics pass does not consume or clear it. Disabled semantics and
    /// stale IDs are no-ops.
    pub fn mark_needs_semantics(&mut self, node_id: RenderId) {
        if !self.semantics_enabled() {
            return;
        }
        let Some(node) = self.render_tree.get(node_id) else {
            return;
        };
        self.scheduler
            .add_node_needing_semantics(node_id, node.depth() as usize);
    }

    /// Applies the pipeline work returned by a render-object update.
    ///
    /// Work is scheduled in layout, compositing, paint, then semantics order.
    /// Layout owns its eventual paint, so an impact containing layout never
    /// adds a redundant immediate paint mark.
    pub fn apply_render_update_impact(&mut self, node_id: RenderId, impact: RenderUpdateImpact) {
        if impact.needs_layout() {
            self.mark_needs_layout(node_id);
        }
        if impact.needs_compositing_bits_update() {
            self.mark_needs_compositing_bits_update(node_id);
        }
        if impact.needs_paint() && !impact.needs_layout() {
            self.mark_needs_paint(node_id);
        }
        if impact.needs_semantics_update() {
            self.mark_needs_semantics(node_id);
        }
    }

    // ========================================================================
    // Semantics enablement (data access, phase-agnostic)
    // ========================================================================

    /// Returns whether semantics are enabled.
    #[inline]
    pub fn semantics_enabled(&self) -> bool {
        self.semantics_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Sets whether semantics are enabled.
    ///
    /// Lazily creates a [`SemanticsOwner`](flui_semantics::SemanticsOwner) on the
    /// `false` → `true` transition (unless one was already installed via
    /// [`Self::set_semantics_owner`] — e.g. a platform binding that needs
    /// its own update callback — in which case that owner is kept as-is),
    /// firing the already-scaffolded
    /// [`fire_semantics_owner_created`](crate::pipeline::notifier::VisualUpdateNotifier::fire_semantics_owner_created).
    /// Disposes the owner on the `true` → `false` transition, firing
    /// `fire_semantics_owner_disposed` — Flutter's `PipelineOwner
    /// .ensureSemantics()` / handle-drop parity, collapsed onto the single
    /// boolean flag this crate already used to gate `run_semantics`.
    ///
    /// A freshly-created owner has an empty tree even though the render
    /// tree may already hold content built while semantics was off, so this
    /// also seeds the root as needing a semantics rebuild (mirrors
    /// `insert()`'s "new nodes need layout and paint" seeding) — without
    /// it, nothing would ever push the first `run_semantics` assembly pass.
    ///
    /// A lazily-created owner publishes through the callback installed via
    /// [`Self::set_semantics_update_callback`] — the composition root's wire
    /// to the platform accessibility bridge (ADR-0014). With none installed
    /// the owner falls back to the documented no-op placeholder: it still
    /// assembles a tree (inspectable via [`Self::semantics_owner`], the
    /// render harness, or `debug_dump_semantics_tree`) but publishes
    /// nowhere.
    pub fn set_semantics_enabled(&mut self, enabled: bool) {
        let was_enabled = self
            .semantics_enabled
            .swap(enabled, std::sync::atomic::Ordering::Relaxed);
        if enabled && !was_enabled {
            if self.semantics_owner.is_none() {
                let callback = self
                    .semantics_update_callback
                    .clone()
                    .unwrap_or_else(no_op_semantics_update_callback);
                self.semantics_owner = Some(flui_semantics::SemanticsOwner::new(callback));
            }
            self.notifier.read().fire_semantics_owner_created();
            if let Some(root_id) = self.root_id {
                self.mark_needs_semantics(root_id);
            }
        } else if !enabled && was_enabled {
            if let Some(mut owner) = self.semantics_owner.take() {
                owner.dispose();
            }
            self.notifier.read().fire_semantics_owner_disposed();
        }
    }

    /// Installs the platform-publish callback for this pipeline's semantics
    /// owner.
    ///
    /// The composition root calls this once it holds a platform
    /// accessibility bridge, so that every [`SemanticsOwner`]
    /// [`Self::set_semantics_enabled`] lazily creates — now or on any later
    /// re-enable — publishes to the platform instead of the no-op
    /// placeholder. An owner already alive when the callback arrives is
    /// swapped live: its next flush publishes the full rooted tree, so no
    /// state is lost to the ordering.
    ///
    /// [`SemanticsOwner`]: flui_semantics::SemanticsOwner
    pub fn set_semantics_update_callback(
        &mut self,
        callback: flui_semantics::SemanticsUpdateCallback,
    ) {
        if let Some(owner) = self.semantics_owner.as_mut() {
            owner.set_callback(callback.clone());
        }
        self.semantics_update_callback = Some(callback);
    }

    /// Schedules a self-contained full semantics publish, re-seeding the
    /// assembly pass so it actually happens.
    ///
    /// The reconnect path: assistive technology (re)activated, so whatever
    /// the adapter previously held is unknown — the owner forgets its
    /// published-state mirror ([`flui_semantics::SemanticsOwner::schedule_full_publish`])
    /// and the root is marked as needing semantics so the next
    /// `run_semantics` rebuilds and flushes even on an otherwise-idle
    /// frame. A no-op while no owner exists: the enable transition that
    /// creates one starts from an empty mirror, which is already a full
    /// first publish.
    pub fn request_semantics_full_publish(&mut self) {
        let Some(owner) = self.semantics_owner.as_mut() else {
            return;
        };
        owner.schedule_full_publish();
        if let Some(root_id) = self.root_id {
            self.mark_needs_semantics(root_id);
        }
    }

    /// Returns the semantics owner, if semantics is currently enabled.
    ///
    /// `None` until [`Self::set_semantics_enabled`]`(true)` lazily creates
    /// one; `None` again once the matching `false` transition disposes it.
    /// Read-only by design (SP-6 / port-check: no lock, no `&mut` escape —
    /// the owner's tree is written only by `Semantics::run_semantics`).
    #[inline]
    pub fn semantics_owner(&self) -> Option<&flui_semantics::SemanticsOwner> {
        self.semantics_owner.as_ref()
    }

    /// Installs (or removes) the semantics owner directly, bypassing the
    /// lazy-creation path in [`Self::set_semantics_enabled`].
    ///
    /// The escape hatch for a platform binding that needs its own update
    /// callback (forwarding the translated [`flui_semantics::TreeUpdate`]s
    /// to a real OS accessibility bridge), or a test that
    /// wants to observe `flush()`'s callback invocations directly. Call
    /// this *before* `set_semantics_enabled(true)` — the enable path only
    /// lazily creates a no-op-callback owner when none is installed yet.
    pub fn set_semantics_owner(&mut self, owner: Option<flui_semantics::SemanticsOwner>) {
        self.semantics_owner = owner;
    }

    // ========================================================================
    // Debug
    // ========================================================================

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

    /// Returns the total number of dirty nodes across all lists.
    #[inline]
    pub fn dirty_node_count(&self) -> usize {
        self.scheduler.dirty_node_count()
    }

    /// Returns whether there are any dirty nodes.
    #[inline]
    pub fn has_dirty_nodes(&self) -> bool {
        self.scheduler.has_dirty_nodes()
    }

    /// Clears all dirty node lists without processing them.
    ///
    /// Use with caution — this discards pending work.
    pub fn clear_all_dirty_nodes(&mut self) {
        self.scheduler.clear_all();
    }

    /// Drains the mid-phase side queue into the active `dirty` set.
    ///
    /// Thin forwarder to `DirtyTracker::drain_mid_marks`. Available for
    /// external callers (e.g. `flui-app`, integration tests) that need to
    /// drain mid-marks between phase invocations without going through the
    /// full phase-exit flow. The `run_*` phase methods drain via
    /// `DirtyTracker::exit_phase` internally and do NOT call this method.
    #[must_use]
    pub fn drain_mid_layout_marks(&mut self) -> usize {
        self.scheduler.drain_mid_marks()
    }

    /// Returns whether any mid-phase marks are pending drain.
    #[inline]
    pub fn has_mid_layout_marks(&self) -> bool {
        self.scheduler.has_mid_marks()
    }

    /// Directly seed a pending child-build request for wiring tests.
    ///
    /// Mirrors the internal push that `SubtreeArena::request_child_build`
    /// performs during layout, so a cross-crate test (e.g. `flui-app`) can
    /// verify `service_child_requests` wiring without building a full
    /// lazy-sliver tree. Gated behind the `testing` feature (plus this crate's
    /// own `test` cfg) so it never lands in a normal/release build; downstream
    /// crates opt in via `flui-rendering = { features = ["testing"] }` in their
    /// dev-dependencies, exactly as the render-object `testing` harness does.
    #[cfg(any(test, feature = "testing"))]
    pub fn push_pending_child_request_for_test(
        &mut self,
        sliver_id: flui_foundation::RenderId,
        index: usize,
    ) {
        self.pending_child_requests.push((sliver_id, index));
    }
}

/// The fallback semantics-update callback for a lazily-created
/// [`SemanticsOwner`](flui_semantics::SemanticsOwner) whose pipeline was
/// never handed a platform bridge.
///
/// A composition root with a real accessibility adapter installs its own
/// callback via [`PipelineOwner::set_semantics_update_callback`] before (or
/// after — the swap is live) enabling semantics. Without one — the render
/// harness, `flui-testing`, a headless embedder — swallowing updates here is
/// an explicit, documented placeholder (not a silent gap): the assembled
/// tree is still inspectable via [`PipelineOwner::semantics_owner`], the
/// harness, or `debug_dump_semantics_tree`.
///
/// The callback receives an already-translated `accesskit::TreeUpdate`, so a
/// real bridge forwards it to the platform capability without touching
/// semantics types — which is what keeps `flui-platform` (layer 2) free of a
/// dependency on `flui-semantics` (layer 3).
fn no_op_semantics_update_callback() -> flui_semantics::SemanticsUpdateCallback {
    std::sync::Arc::new(|_update: &flui_semantics::TreeUpdate| {})
}
