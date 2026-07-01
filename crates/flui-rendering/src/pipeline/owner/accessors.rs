//! Phase-agnostic accessors, setters, insertion, dirty tracking, and hit-test
//! for `PipelineOwner<Phase: PipelinePhase>`.
//!
//! These methods are pure data access or side-effect-free notifier wiring.
//! They are valid in any phase: the borrow checker still gates `&mut self`
//! against the typestate transitions, but the methods themselves don't
//! care which phase the owner is in.

use flui_foundation::RenderId;
use flui_types::Offset;

use crate::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    pipeline::{
        dirty::DirtyNode,
        handle::{DirtyKind, PipelineOwnerHandle},
        phase::PipelinePhase,
    },
    protocol::{BoxProtocol, MainAxisPosition, SliverProtocol},
    storage::RenderNode,
};

use super::{PipelineOwner, subtree_arena::ensure_stack};

// ============================================================================
// Phase-agnostic accessors / setters / insertion (Mythos Step 7)
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
    // Cross-thread mark-dirty handle (Mythos Step 8)
    // ========================================================================

    /// Returns a clone of the cross-thread mark-dirty handle.
    ///
    /// Each clone is its own `Sender` over the same bounded channel; sends
    /// from different threads do not block each other. Backpressure
    /// surfaces as `SendError::ChannelFull`.
    #[inline]
    pub fn handle(&self) -> PipelineOwnerHandle {
        self.handle.clone()
    }

    /// Binds a [`RepaintHandle`](crate::pipeline::RepaintHandle) to a
    /// live render object — the
    /// capability async producers (image decodes, arriving assets) use
    /// to repaint that node from any thread, with the platform woken on
    /// every request.
    ///
    /// `None` for a stale/foreign id. Once the node is later removed,
    /// the returned handle degrades to a silent no-op (generational id:
    /// the drain drops requests whose generation died).
    pub fn repaint_handle(&self, id: RenderId) -> Option<crate::pipeline::RepaintHandle> {
        let depth = self.render_tree.get(id)?.depth() as usize;
        Some(crate::pipeline::RepaintHandle::new(
            self.handle.clone(),
            id,
            depth,
        ))
    }

    /// Drains the pending dirty-request channel into the scheduler's
    /// dirty sets.
    ///
    /// Called at phase boundaries by the typestate transitions; producers
    /// (background asset loaders, async work) write into the channel via
    /// [`PipelineOwnerHandle::request_mark_dirty`] and the owner observes
    /// them on the next frame. Non-blocking; processes every request
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
            // compositing request skipped the flag (the silent-loss
            // footgun `add_node_needing_compositing_bits_update`
            // exists to prevent), and dead ids were replayed verbatim.
            let Some(node) = self.render_tree.get(req.id) else {
                tracing::trace!(?req, "drain_pending_dirty: stale id, dropped");
                continue;
            };
            let depth = node.depth() as usize;
            match req.kind {
                DirtyKind::Layout => self.mark_needs_layout(req.id),
                DirtyKind::Compositing => {
                    self.add_node_needing_compositing_bits_update(req.id, depth);
                }
                DirtyKind::Paint => self.add_node_needing_paint(req.id, depth),
                DirtyKind::Semantics => self.add_node_needing_semantics(req.id, depth),
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
    /// **D-block PR-A1 U23:** see [`Self::set_root_constraints`].
    #[inline]
    pub fn root_constraints(&self) -> Option<BoxConstraints> {
        self.root_constraints
    }

    /// Sets the constraints to apply to the root render object on the
    /// next layout pass when no cached constraints exist yet
    /// (first-frame initialization).
    ///
    /// **D-block PR-A1 U23:** the binding layer (`flui-view` /
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
    /// # Auto-schedules root relayout (PR #148 review fix)
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

    /// Device pixel ratio threaded into every paint pass.
    pub fn device_pixel_ratio(&self) -> f32 {
        self.device_pixel_ratio
    }

    /// Removes the subtree rooted at `id` — THE dispose site.
    ///
    /// Removal is where owner-side state dies (the inversion of the
    /// "Drop will handle it" idea): a `Drop` impl has no
    /// `&PipelineOwner`, so it cannot evict dirty-queue entries — only
    /// this method can. Order:
    ///
    /// 1. collect the subtree's ids;
    /// 2. evict every id from ALL dirty queues (live + mid-phase) so
    ///    no phase walks a freed slot's stale entry;
    /// 3. cascade-remove the nodes (each freed slot's generation bumps
    ///    — outstanding ids go stale, D2);
    /// 4. clear `root_id` when the root itself was removed.
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

        let subtree = self.render_tree.collect_subtree_ids(id);
        if subtree.is_empty() {
            return 0;
        }
        let removed: rustc_hash::FxHashSet<RenderId> = subtree.iter().copied().collect();
        self.scheduler.evict(&removed);

        let count = self.render_tree.remove_recursive(id);
        if self.root_id == Some(id) {
            self.root_id = None;
        }
        tracing::debug!(?id, count, "remove_render_object: subtree disposed");
        count
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
        // The child-recursion callback must be Send + Sync (ctx trait
        // bounds, U19 inheritance); it captures &self, so the phase
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
        let children: Vec<RenderId> = node.children().to_vec();
        let render_object = entry.render_object();
        // Box bounds gate size, resolved from RenderState (geometry's
        // sole owner — 2B field dedup); threaded into hit_test_raw so the
        // default gate reads `ctx.is_within_own_size()`.
        let own_size = entry.state().geometry().unwrap_or(flui_types::Size::ZERO);

        // Push the node's hit-test transform onto the HitTestResult
        // stack BEFORE recursing, so child entries captured during
        // hit_test_raw see the correct accumulated transform. The
        // transform stays on the stack until after the parent entry
        // is added (Flutter parity: pushTransform in hitTest). The hook
        // gets `own_size` from RenderState for alignment-relative origins.
        let hit_transform = render_object.hit_test_transform(own_size);
        let has_transform = hit_transform.is_some();
        if let Some(t) = hit_transform {
            result.push_transform(t);
        }

        let mut hit_child = |index: usize, override_pos: Option<Offset>| -> bool {
            let Some(&child_id) = children.get(index) else {
                return false;
            };
            let Some(child_node) = self.render_tree.get(child_id) else {
                return false;
            };
            if child_node.as_sliver().is_some() {
                // Explicit positions are already child-local; the layout-offset
                // fallback starts from the child's physical paint offset.
                return match override_pos {
                    Some(position) => {
                        let child_position =
                            Self::sliver_hit_position_from_offset(child_node, position);
                        self.hit_test_sliver_subtree(child_id, child_position, result)
                    }
                    None => {
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
                    }
                };
            }
            match override_pos {
                Some(child_position) => self.hit_test_subtree(child_id, child_position, result),
                None => {
                    let child_offset = child_node.offset();
                    result.with_paint_offset(child_offset, |result| {
                        self.hit_test_subtree(child_id, position - child_offset, result)
                    })
                }
            }
        };

        let hit = render_object.hit_test_raw(position, children.len(), own_size, &mut hit_child);
        if hit {
            // Leaf-first path: children pushed their entries during
            // the callback above; the ancestor follows. The transform
            // is still on the stack, so this entry captures it.
            //
            // A render object that listens for pointer events (RenderListener)
            // advertises a handler here; it rides on the entry so
            // `HitTestResult::dispatch` can invoke it. Additive: every other
            // render object returns `None`, leaving today's handler-less entry.
            let entry = crate::hit_testing::HitTestEntry::new(id);
            let entry = match render_object.pointer_event_handler() {
                Some(handler) => entry.handler(handler),
                None => entry,
            };
            let entry = entry.cursor(render_object.mouse_cursor());
            let entry = match render_object.mouse_tracker_annotation(id) {
                Some(annotation) => entry.mouse_annotation(annotation),
                None => entry,
            };
            result.add(entry);
        }

        if has_transform {
            result.pop_transform();
        }

        hit
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
        let render_object = entry.render_object();
        // Absolute paint size from RenderState — threaded into
        // hit_test_raw for signature uniformity; the sliver hit gate is
        // driver-owned (checked above), so the sliver context ignores it.
        let own_size = entry.state().absolute_paint_size();

        let mut hit_child = |index: usize, override_pos: Option<MainAxisPosition>| -> bool {
            let Some(&child_id) = children.get(index) else {
                return false;
            };
            let Some(child_node) = self.render_tree.get(child_id) else {
                return false;
            };
            if child_node.as_sliver().is_some() {
                // Explicit positions are already in child sliver coordinates.
                // The layout-offset fallback starts from physical paint data.
                let child_position = match override_pos {
                    Some(position) => position,
                    None => Self::sliver_hit_position_minus_paint_offset(
                        constraints,
                        &geometry,
                        child_node,
                        position,
                        child_node.offset(),
                    ),
                };
                self.hit_test_sliver_subtree(child_id, child_position, result)
            } else if let Some(child_entry) = child_node.as_box() {
                let Some(child_size) = child_entry.state().geometry() else {
                    return false;
                };
                let child_position = Self::box_hit_offset_from_sliver_position(
                    constraints,
                    &geometry,
                    child_size,
                    override_pos.unwrap_or(position),
                    child_node.offset(),
                );
                self.hit_test_subtree(child_id, child_position, result)
            } else {
                false
            }
        };

        let hit = render_object.hit_test_raw(position, children.len(), own_size, &mut hit_child);
        if hit {
            let entry =
                crate::hit_testing::HitTestEntry::new(id).cursor(render_object.mouse_cursor());
            let entry = match render_object.mouse_tracker_annotation(id) {
                Some(annotation) => entry.mouse_annotation(annotation),
                None => entry,
            };
            result.add(entry);
        }
        hit
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

        // PR-A2 U33: bootstrap IS_REPAINT_BOUNDARY flag from the
        // render_object's static answer before the dirty pushes (so
        // the compositing walk has accurate boundary info on first
        // run_compositing).
        self.bootstrap_repaint_boundary_flag(id);

        // New nodes need layout and paint
        self.add_node_needing_layout(id, depth);
        self.add_node_needing_paint(id, depth);

        id
    }

    /// Inserts a render object as a child and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object as a child in the RenderTree
    /// 2. Adds the node to the dirty layout list
    /// 3. Adds the node to the dirty paint list
    /// 4. Marks the parent as needing layout (since child structure changed)
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

        // PR-A2 U33: bootstrap IS_REPAINT_BOUNDARY flag from the
        // child render_object's static answer before any compositing
        // walk runs.
        self.bootstrap_repaint_boundary_flag(child_id);

        // Mark child as needing layout and paint
        self.add_node_needing_layout(child_id, child_depth as usize);
        self.add_node_needing_paint(child_id, child_depth as usize);

        // Mark parent as needing layout (child structure changed)
        self.add_node_needing_layout(parent_id, parent_depth as usize);

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

        // PR-A2 U33: bootstrap IS_REPAINT_BOUNDARY flag (matches the
        // `insert` / `insert_child_render_object` paths so every code
        // path that adds nodes leaves the compositing flag in sync
        // with the trait answer).
        self.bootstrap_repaint_boundary_flag(id);

        self.add_node_needing_layout(id, depth);
        self.add_node_needing_paint(id, depth);

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
    /// **D-block PR-A2 U33 (memo R26b).** Every node-insert path
    /// ([`Self::insert`], [`Self::insert_child_render_object`],
    /// [`Self::insert_render_node`]; [`Self::set_root_render_object`]
    /// inherits via `insert`) calls this immediately after the tree
    /// `insert` so the compositing-bits walk (U34) has accurate
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
    /// Pre-U33 the storage flag was effectively `false` for every node
    /// from the moment it entered the tree, which forced the
    /// compositing walk to fall through to the trait answer
    /// (`render_object().is_repaint_boundary()`) at every check site —
    /// a virtual dispatch and a divergence risk if a future caller
    /// flipped the flag dynamically.
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

    // ========================================================================
    // Dirty Node Access (Flutter API)
    // ========================================================================

    // ========================================================================
    // Deferred Mutations (re-entrant layout)
    // ========================================================================

    /// Enqueues a deferred mutation to be applied after the layout pass.
    ///
    /// During layout, render objects may need to add, remove, or update
    /// children. But the layout walk holds `&mut` on the subtree, making
    /// direct mutation impossible. This method collects mutations in a
    /// queue that is drained after the pass completes.
    ///
    /// `logical_index`: if `Some(li)`, the pipeline stamps `li` into the
    /// inserted child's parent-data after insertion (if the parent-data
    /// type implements `crate::parent_data::LogicalIndexParentData`).
    /// Pass `None` for non-lazy inserts.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // During layout:
    /// owner.defer_insert_box(parent_id, Box::new(new_child), None, None);
    /// owner.defer_remove(parent_id, child_id);
    /// owner.defer_update(target_id, Box::new(|obj| { /* mutate */ }));
    /// ```
    pub fn defer_insert_box(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::protocol::RenderObject<BoxProtocol>>,
        index: Option<usize>,
        logical_index: Option<usize>,
        initial_parent_data: Option<Box<dyn crate::parent_data::ParentData>>,
    ) {
        self.deferred_mutations.insert_box(
            parent_id,
            render_object,
            index,
            logical_index,
            initial_parent_data,
        );
    }

    /// Enqueues a deferred Sliver child insertion.
    ///
    /// `logical_index`: if `Some(li)`, stamps `li` into the child's
    /// parent-data after insertion.  Pass `None` for non-lazy inserts.
    ///
    /// `initial_parent_data`: pre-built parent-data to install on the fresh
    /// node immediately after insertion.  See `DeferredMutation::Insert`.
    pub fn defer_insert_sliver(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::protocol::RenderObject<SliverProtocol>>,
        index: Option<usize>,
        logical_index: Option<usize>,
        initial_parent_data: Option<Box<dyn crate::parent_data::ParentData>>,
    ) {
        self.deferred_mutations.insert_sliver(
            parent_id,
            render_object,
            index,
            logical_index,
            initial_parent_data,
        );
    }

    /// Enqueues a deferred removal.
    pub fn defer_remove(&mut self, parent_id: RenderId, child_id: RenderId) {
        self.deferred_mutations.remove(parent_id, child_id);
    }

    /// Enqueues a deferred update (e.g., animation driving properties).
    ///
    /// The updater receives `&mut dyn Any` — the caller downcasts to
    /// the concrete render object type.
    pub fn defer_update(
        &mut self,
        target_id: RenderId,
        updater: Box<dyn FnOnce(&mut dyn std::any::Any) + Send + Sync>,
    ) {
        self.deferred_mutations.update(target_id, updater);
    }

    /// Returns the number of pending deferred mutations.
    pub fn deferred_mutation_count(&self) -> usize {
        self.deferred_mutations.len()
    }

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
    /// **D-block PR-A1 U15** (memo D3) — ports Flutter's `markNeedsLayout`
    /// walk (`.flutter/.../object.dart:2658-2700`). Thin forwarder: the walk
    /// logic lives in `DirtyTracker::mark_needs_layout` so it is
    /// unit-testable without a full owner. The `scheduler` and `render_tree`
    /// fields are disjoint, so the split borrow compiles.
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        self.scheduler.mark_needs_layout(&mut self.render_tree, id);
    }

    /// Adds a node to the paint dirty list.
    ///
    /// Routes into the mid-phase side queue when paint is active, otherwise
    /// into `dirty.needs_paint`. See `DirtyTracker::add_node_needing_paint`
    /// for the full routing and dedup contract.
    pub fn add_node_needing_paint(&mut self, node_id: RenderId, depth: usize) {
        self.scheduler.add_node_needing_paint(node_id, depth);
    }

    /// Adds a node to the compositing bits dirty list.
    ///
    /// Also sets `NEEDS_COMPOSITING_BITS_UPDATE` on the node (atomic) so the
    /// `run_compositing` walk's short-circuit cannot drop this entry. Routes
    /// via `debug_doing_layout` (compositing shares the layout flag). See
    /// `DirtyTracker::add_node_needing_compositing_bits_update` for the
    /// full contract.
    ///
    /// The `scheduler` and `render_tree` are disjoint fields: the shared
    /// borrow `&self.render_tree` passed to the scheduler compiles cleanly.
    pub fn add_node_needing_compositing_bits_update(&mut self, node_id: RenderId, depth: usize) {
        self.scheduler
            .add_node_needing_compositing_bits_update(&self.render_tree, node_id, depth);
    }

    /// Adds a node to the semantics dirty list.
    ///
    /// Routes via `debug_doing_semantics`. See
    /// `DirtyTracker::add_node_needing_semantics` for the full contract.
    pub fn add_node_needing_semantics(&mut self, node_id: RenderId, depth: usize) {
        self.scheduler.add_node_needing_semantics(node_id, depth);
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
    pub fn set_semantics_enabled(&self, enabled: bool) {
        let was_enabled = self
            .semantics_enabled
            .swap(enabled, std::sync::atomic::Ordering::Relaxed);
        if enabled && !was_enabled {
            self.notifier.read().fire_semantics_owner_created();
        } else if !enabled && was_enabled {
            self.notifier.read().fire_semantics_owner_disposed();
        }
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
