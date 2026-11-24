//! Hit testing of the render tree: a pointer position walked down box and sliver nodes, with the
//! expanded areas of hit slop.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset};

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    protocol::MainAxisPosition,
    storage::RenderNode,
};

use super::{
    PipelineOwner,
    hit_slop::{HitWalk, expanded_probes},
    subtree_arena::ensure_stack,
};

impl PipelineOwner {
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
    ///
    /// Expanded hit areas (`RenderHitSlop`) never take a position in this
    /// visual walk. When the walk passes through one whose area holds the
    /// position, the walk is repeated for the best such area, which replaces
    /// the visual path only if no other interactive target, no content in
    /// another composited layer and no pointer barrier was hit visually
    /// (`hit_slop` module docs). A tree without slop pays one `None` hook call
    /// per visited box. `kind` is the pointer asking: an expanded area sizes its
    /// minimum target for that kind of pointer (ADR-0207 §6, ADR-0212).
    ///
    /// # Panics
    ///
    /// A render object's hit test is user code. One that panics counts as a
    /// miss — its subtree adds no entry and blocks nothing below — and the
    /// walk goes on. Once the path is complete, the first such panic resumes
    /// here, so `result` already holds every other hit; a caller that
    /// catches it delivers the event to that path and reports the panic
    /// afterwards.
    pub fn hit_test(
        &self,
        position: Offset,
        kind: flui_interaction::events::PointerKind,
        result: &mut flui_interaction::pointer::HitTestResult,
    ) -> bool {
        let Some(root_id) = self.root_id else {
            return false;
        };
        let base = result.len();
        let mut walk = HitWalk::visual(kind);
        let mut hit = self.hit_test_subtree(root_id, position, result, &mut walk);
        if walk.has_candidates() {
            hit = self
                .resolve_hit_slop(root_id, position, result, base, &mut walk)
                .unwrap_or(hit);
        }
        walk.finish();
        hit
    }

    pub(super) fn hit_test_subtree(
        &self,
        id: RenderId,
        position: Offset,
        result: &mut flui_interaction::pointer::HitTestResult,
        walk: &mut HitWalk,
    ) -> bool {
        ensure_stack(|| {
            contain_node(result, walk, |result, walk| {
                self.hit_test_subtree_impl(id, position, result, walk)
            })
        })
    }

    /// Body of [`Self::hit_test_subtree`]; split out so every
    /// recursion level enters through the [`ensure_stack`] probe.
    fn hit_test_subtree_impl(
        &self,
        id: RenderId,
        position: Offset,
        result: &mut flui_interaction::pointer::HitTestResult,
        walk: &mut HitWalk,
    ) -> bool {
        let Some(node) = self.render_tree.get(id) else {
            return false;
        };
        let Some(entry) = node.as_box() else {
            if node.as_sliver().is_some() {
                let sliver_position = Self::sliver_hit_position_from_offset(node, position);
                return self.hit_test_sliver_subtree(id, sliver_position, result, walk);
            }
            return false;
        };

        // Follower hit-testing: composite-resolved follower transforms. Gated behind
        // both side tables being empty — true for the overwhelming
        // majority of trees (followers are rare: tooltips, dropdowns,
        // overlays), so this whole branch is a single cheap `is_empty`
        // check away from a no-op for every other tree.
        let follower_transform = if self.last_follower_transforms.is_empty()
            && self.last_hidden_follower_ids.is_empty()
        {
            None
        } else if self.last_hidden_follower_ids.contains(&id) {
            // Correlated as a follower this frame but resolved to hidden
            // (unlinked and not shown, or its leader clipped out) — mirrors the
            // render path's `SceneVersion::follower_transform -> None -> don't
            // descend` (flui-engine's `layers::walk`).
            // No HitTestEntry, no descent.
            return false;
        } else {
            self.last_follower_transforms.get(&id).copied()
        };

        // Borrowed, not copied: the walk only reads the tree.
        let children = node.children();
        // The generation this node's last layout stamped onto the children it
        // laid out; anything else was not part of that pass.
        let parent_generation = node.layout_generation();
        let render_object = entry.render_object();
        // Box bounds gate size, resolved from RenderState (geometry's
        // sole owner — 2B field dedup); threaded into hit_test_raw so the
        // default gate reads `ctx.is_within_own_size()`.
        let own_size = entry
            .state()
            .geometry()
            .unwrap_or(flui_foundation::geometry::Size::ZERO);

        // Push the node's hit-test transform onto the HitTestResult
        // stack BEFORE recursing, so child entries captured during
        // hit_test_raw see the correct accumulated transform. The
        // transform stays on the stack until after the parent entry
        // is added. The hook gets `own_size` from RenderState for
        // alignment-relative origins.
        //
        // Pushed as the INVERSE of `hit_test_transform`: `HitTestResult`
        // composes the global-to-local mapping by left-multiplying each
        // level's own inverse in descent order (see `HitTestEntry::transform`),
        // so a caller pushing the forward (paint-direction) matrix here
        // recorded the wrong composition for any chain mixing this transform
        // with an outer offset -- correct only when the whole chain
        // commutes (pure translations). A transform without an admitted finite
        // inverse refuses this subtree before any descendant can publish an
        // entry; its forward matrix is never used as a substitute (ADR-0113).
        //
        // The walk inverts it ONCE: the same inverse localizes the entries
        // and maps the position, so the node's `hit_test` receives its
        // position already in the transformed (child) space and no object
        // re-derives or re-inverts its own matrix.
        let hit_transform = render_object.hit_test_transform(own_size);
        // A resolved follower offset rides the SAME transform-stack
        // lifecycle as `hit_test_transform` (flui-rendering ARCHITECTURE.md, follower hit-testing) — the same
        // translation the paint/GPU path applies via
        // `backend.push_offset(resolved)` (renderer.rs:1586), lifted to a
        // `Matrix4` (exact and lossless — the composer only ever
        // shifts coordinate space via translation). Pushed as the inverse
        // translation for the same reason as `hit_test_transform` above.
        //
        // The position handed into this node's own subtree is shifted by the
        // resolved offset in `hit_follower` below — the SAME position-shift
        // pattern `hit_child` applies for ordinary child offsets, applied by
        // the WALK rather than the object: `RenderFollowerLayer::hit_test`
        // stays a plain structural forward (flui-rendering ARCHITECTURE.md, follower hit-testing).
        let slop_area = render_object.hit_slop_area(own_size, walk.kind());

        let hit_node = |result: &mut flui_interaction::pointer::HitTestResult,
                        walk: &mut HitWalk,
                        position: Offset| {
            // Entries already on the path: a node that joins the hits below it
            // joins when its subtree added any, blocking or not.
            let entries_before = result.len();
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
                // `push_offset` calls (`RawBoxHitTestContext::composed_transform`).
                // `with_paint_transform` pushes its INVERSE onto the shared
                // `HitTestResult`, matching every other forward-transform push
                // in this walk (`hit_test_transform` above, the follower offset).
                // Scoped to exactly this one child recursion — the ctx that accumulated it already
                // scoped the push/pop to the same call.
                let mut dispatch_child =
                    |result: &mut flui_interaction::pointer::HitTestResult| -> bool {
                        if child_node.as_sliver().is_some() {
                            // Explicit positions are already child-local; the layout-offset
                            // fallback starts from the child's physical paint offset.
                            return if let Some(position) = override_pos {
                                let child_position =
                                    Self::sliver_hit_position_from_offset(child_node, position);
                                self.hit_test_sliver_subtree(child_id, child_position, result, walk)
                            } else {
                                if !Self::sliver_child_is_visible(child_node) {
                                    return false;
                                }
                                let child_offset = child_node.offset();
                                result
                                    .with_paint_offset(child_offset, |result| {
                                        let child_position =
                                            Self::sliver_hit_position_from_paint_offset(
                                                child_node,
                                                position - child_offset,
                                            );
                                        self.hit_test_sliver_subtree(
                                            child_id,
                                            child_position,
                                            result,
                                            walk,
                                        )
                                    })
                                    .unwrap_or(false)
                            };
                        }
                        if let Some(child_position) = override_pos {
                            self.hit_test_subtree(child_id, child_position, result, walk)
                        } else {
                            let child_offset = child_node.offset();
                            result
                                .with_paint_offset(child_offset, |result| {
                                    self.hit_test_subtree(
                                        child_id,
                                        position - child_offset,
                                        result,
                                        walk,
                                    )
                                })
                                .unwrap_or(false)
                        }
                    };

                match local_transform {
                    Some(local_transform) => result
                        .with_paint_transform(local_transform, dispatch_child)
                        .unwrap_or(false),
                    None => dispatch_child(result),
                }
            };

            let hit =
                render_object.hit_test_raw(position, children.len(), own_size, &mut hit_child);
            let joins_below =
                !hit.add_self && render_object.joins_hits_below() && result.len() > entries_before;
            if hit.add_self || joins_below {
                // Leaf-first path: children pushed their entries during
                // the callback above; the ancestor follows. The transform
                // is still on the stack, so this entry captures it.
                //
                // A render object that listens for pointer events (RenderListener)
                // advertises its data-only pointer target here; it rides on the
                // entry so dispatch can resolve the owner-local handler. Every
                // other render object returns `None`, leaving a target-less entry.
                // A hit that chose its own target and cursor (a paragraph's
                // link) overrides the node-wide ones.
                let entry = flui_interaction::pointer::HitTestEntry::new(id)
                    .subtree_entries(result.len() - entries_before);
                let entry = match hit
                    .pointer_target
                    .or_else(|| render_object.pointer_target())
                {
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
                let entry = match render_object.smart_zoom_target() {
                    Some(target) => entry.smart_zoom_target(target),
                    None => entry,
                };
                let entry = entry
                    .cursor_request(hit.cursor.unwrap_or_else(|| render_object.mouse_cursor()));
                let entry = match render_object.metadata() {
                    Some(payload) => entry.metadata(payload),
                    None => entry,
                };
                let entry = match render_object.mouse_tracker_annotation(id) {
                    Some(annotation) => entry.mouse_annotation(annotation),
                    None => entry,
                };
                result.add(entry);
                if render_object.blocks_hit_slop() {
                    walk.record_barrier();
                }
            }

            hit.blocks_below
        };
        // A node with an expanded area hit-tests visually unless this is the
        // walk that expands it; then it is probed inside its own bounds so its
        // child's path is built (`hit_slop` module docs).
        let visit = |result: &mut flui_interaction::pointer::HitTestResult,
                     walk: &mut HitWalk,
                     position: Offset| {
            let Some(area) = slop_area else {
                return hit_node(result, walk, position);
            };
            if walk.expands(id) {
                for probe in expanded_probes(position, own_size) {
                    let mark = result.len();
                    if hit_node(result, walk, probe) {
                        return true;
                    }
                    result.path_mut().truncate(mark);
                }
                return false;
            }
            let hit = hit_node(result, walk, position);
            if !hit {
                walk.record_miss(id, area, position, own_size);
            }
            hit
        };
        let mut hit_follower = |result: &mut flui_interaction::pointer::HitTestResult,
                                position: Offset| {
            let Some(transform) = follower_transform else {
                return visit(result, walk, position);
            };
            // The resolver only publishes invertible transforms; a refusal
            // here still misses rather than hit-testing in the wrong space.
            let Some(inverse) = transform.try_inverse() else {
                return false;
            };
            let Some((x, y)) = inverse.unproject_to_plane(position.dx, position.dy) else {
                return false;
            };
            result.with_inverse_transform(inverse, |result| visit(result, walk, Offset::new(x, y)))
        };
        let Some(transform) = hit_transform else {
            return hit_follower(result, position);
        };
        let Some(inverse) = transform.try_inverse() else {
            return false;
        };
        let Some((x, y)) = inverse.unproject_to_plane(position.dx, position.dy) else {
            // The position's ray never meets the child's plane: nothing of
            // the child is under it.
            return false;
        };
        result.with_inverse_transform(inverse, |result| hit_follower(result, Offset::new(x, y)))
    }

    fn sliver_hit_position_from_offset(node: &RenderNode, position: Offset) -> MainAxisPosition {
        let axis_direction = node
            .as_sliver()
            .and_then(|entry| entry.state().constraints())
            .map(|constraints| constraints.axis_direction);

        match axis_direction {
            Some(
                crate::constraints::AxisDirection::LeftToRight
                | crate::constraints::AxisDirection::RightToLeft,
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
            crate::constraints::AxisDirection::LeftToRight
            | crate::constraints::AxisDirection::RightToLeft => (position.dx, position.dy),
            crate::constraints::AxisDirection::TopToBottom
            | crate::constraints::AxisDirection::BottomToTop => (position.dy, position.dx),
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
            crate::constraints::AxisDirection::LeftToRight
            | crate::constraints::AxisDirection::RightToLeft => (offset.dx, offset.dy),
            crate::constraints::AxisDirection::TopToBottom
            | crate::constraints::AxisDirection::BottomToTop => (offset.dy, offset.dx),
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
        child_size: flui_foundation::geometry::Size,
        position: MainAxisPosition,
        offset: Offset,
    ) -> Offset {
        let right_way_up = crate::constraints::right_way_up(
            constraints.axis_direction,
            constraints.growth_direction,
        );

        let (paint_main, paint_cross, child_main_extent) = match constraints.axis_direction {
            crate::constraints::AxisDirection::LeftToRight
            | crate::constraints::AxisDirection::RightToLeft => {
                (offset.dx, offset.dy, child_size.width)
            }
            crate::constraints::AxisDirection::TopToBottom
            | crate::constraints::AxisDirection::BottomToTop => {
                (offset.dy, offset.dx, child_size.height)
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
            crate::constraints::AxisDirection::LeftToRight
            | crate::constraints::AxisDirection::RightToLeft => {
                Offset::new(local_main, local_cross)
            }
            crate::constraints::AxisDirection::TopToBottom
            | crate::constraints::AxisDirection::BottomToTop => {
                Offset::new(local_cross, local_main)
            }
        }
    }

    fn hit_test_sliver_subtree(
        &self,
        id: RenderId,
        position: MainAxisPosition,
        result: &mut flui_interaction::pointer::HitTestResult,
        walk: &mut HitWalk,
    ) -> bool {
        ensure_stack(|| {
            contain_node(result, walk, |result, walk| {
                self.hit_test_sliver_subtree_impl(id, position, result, walk)
            })
        })
    }

    fn hit_test_sliver_subtree_impl(
        &self,
        id: RenderId,
        position: MainAxisPosition,
        result: &mut flui_interaction::pointer::HitTestResult,
        walk: &mut HitWalk,
    ) -> bool {
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
        let children = node.children();
        let parent_generation = node.layout_generation();
        let render_object = entry.render_object();
        // Absolute paint size from RenderState — threaded into
        // hit_test_raw for signature uniformity; the sliver hit gate is
        // driver-owned (checked above), so the sliver context ignores it.
        let own_size = entry.state().absolute_paint_size();

        let mut hit_child = |index: usize,
                             override_pos: Option<MainAxisPosition>,
                             // The sliver protocol's ctx-level transform stack is a
                             // permanent no-op (`RawSliverHitTestContext::push_transform`) —
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
            // child's own frame. A sliver protocol without an
            // override-position concept always pushes `paintOffset`
            // regardless of how the caller derived the position, because
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
                    self.hit_test_sliver_subtree(child_id, child_position, result, walk)
                } else {
                    let child_offset = child_node.offset();
                    let child_position = Self::sliver_hit_position_minus_paint_offset(
                        constraints,
                        &geometry,
                        child_node,
                        position,
                        child_offset,
                    );
                    result
                        .with_paint_offset(child_offset, |result| {
                            self.hit_test_sliver_subtree(child_id, child_position, result, walk)
                        })
                        .unwrap_or(false)
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
                result
                    .with_paint_offset(child_offset, |result| {
                        self.hit_test_subtree(child_id, child_position, result, walk)
                    })
                    .unwrap_or(false)
            } else {
                false
            }
        };

        let hit = render_object.hit_test_raw(position, children.len(), own_size, &mut hit_child);
        if hit.add_self {
            let entry = flui_interaction::pointer::HitTestEntry::new(id)
                .cursor_request(hit.cursor.unwrap_or_else(|| render_object.mouse_cursor()));
            let entry = match render_object.mouse_tracker_annotation(id) {
                Some(annotation) => entry.mouse_annotation(annotation),
                None => entry,
            };
            // Attached on the sliver path as well as the box one, with a
            // matching `RenderSliver::metadata` hook so a sliver can actually
            // supply one. No shipped sliver does today -- `RenderMetaData` is a
            // box proxy -- but a walk that reads a hook no implementor can
            // override is a claim of support that is not there.
            let entry = match render_object.metadata() {
                Some(payload) => entry.metadata(payload),
                None => entry,
            };
            result.add(entry);
        }
        hit.blocks_below
    }
}

/// Visits one node of a hit walk with its user code contained.
///
/// Every hook the walk calls on a node — `hit_test_transform`, the expanded
/// area, `hit_test` itself (path clippers, flow delegates), the entry's
/// targets — is user code. A panic in any of them makes the node a miss: what
/// its subtree added to the path is taken off it and destroyed under containment (driver and context scopes restore
/// their transforms on unwind), it blocks nothing below, and the payload waits
/// in `walk` until the path is complete. A descendant's panic is contained at
/// the descendant, so its ancestors still decide for themselves.
fn contain_node(
    result: &mut flui_interaction::pointer::HitTestResult,
    walk: &mut HitWalk,
    visit: impl FnOnce(&mut flui_interaction::pointer::HitTestResult, &mut HitWalk) -> bool,
) -> bool {
    let mark = result.len();
    let slop = walk.mark();
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a hit-test visit runs contained so ancestors still decide"
    )]
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| visit(result, walk))) {
        Ok(hit) => hit,
        Err(payload) => {
            // The payload is held before anything else runs, so a discarded
            // entry whose metadata destructor panics too cannot replace the
            // failure being reported (ADR-0281).
            walk.contain_panic(payload);
            walk.discard(result.path_mut().split_off(mark));
            // Nor does the failed subtree keep a say in expanded areas: a
            // barrier it hit, or a candidate it recorded, is no longer there.
            walk.roll_back(slop);
            false
        }
    }
}
