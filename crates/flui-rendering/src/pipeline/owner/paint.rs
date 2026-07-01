//! Paint phase implementation for `PipelineOwner<PaintPhase>`.

use flui_foundation::{LayerId, RenderId};
use flui_layer::{
    BackdropFilterLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer, FollowerLayer, Layer,
    LayerTree, LeaderLayer, LinkRegistry, OffsetLayer, OpacityLayer, PictureLayer, ShaderMaskLayer,
    TransformLayer,
};
use flui_painting::DisplayList;
use flui_types::Offset;
use rustc_hash::FxHashSet;

use crate::{
    context::{FragmentOp, FragmentRecorder, FragmentScope},
    pipeline::{
        phase::{Idle, PaintPhase, Semantics},
        scheduler::PhaseKind,
    },
};

use super::{PipelineOwner, rebind_phase, subtree_arena::ensure_stack};

// ============================================================================
// Paint phase: run_paint + helpers
// ============================================================================

impl PipelineOwner<PaintPhase> {
    /// Transitions a paint-phase pipeline into the [`Semantics`] phase.
    #[must_use]
    pub fn into_semantics(self) -> PipelineOwner<Semantics> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the paint phase.
    #[must_use]
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Paints all dirty render objects.
    ///
    /// Phase 3 of the rendering pipeline, as a **fragment composition**
    /// (sans-IO paint model): each node's `paint_raw` records a
    /// node-local fragment — draw runs, child markers, clip scopes —
    /// which is immediately replayed into the frame's [`LayerTree`].
    /// Adjacent inline draw runs merge into shared `PictureLayer`s;
    /// repaint-boundary children are rebased to `Offset::ZERO` under
    /// their own `OffsetLayer`; clip scopes become real clip layers.
    ///
    /// A fresh full `LayerTree` is produced every paint pass —
    /// cross-frame retention of boundary subtrees is deliberately out
    /// of scope until the layer tree grows a structural-sharing
    /// substrate and the engine an incremental upload path.
    pub fn run_paint(&mut self) -> crate::error::RenderResult<()> {
        if !self.scheduler.has_paint_work() {
            return Ok(());
        }

        let _span = tracing::debug_span!("paint", dirty_nodes = self.scheduler.paint_queue_len(),)
            .entered();

        self.scheduler.enter_phase(PhaseKind::Paint);

        // Deepest-first ordering retained (Flutter `flushPaint`): the
        // full-tree descent below repaints everything, but per-boundary
        // dirty-driven repaints will rely on this order once retention
        // lands, and keeping it now means the dirty-list semantics
        // don't shift under that change.
        self.scheduler.sort_paint_deep_first();

        if let Some(root_id) = self.root_id
            && self.render_tree.get(root_id).is_some()
        {
            // Build a set of dirty node IDs for O(1) lookup during the
            // paint walk.
            let dirty_ids: FxHashSet<RenderId> = self
                .scheduler
                .nodes_needing_paint()
                .iter()
                .map(|d| d.id)
                .collect();

            let mut composer = FragmentComposer::new(self.device_pixel_ratio);
            match self.paint_subtree(&mut composer, root_id, Offset::ZERO, &dirty_ids) {
                Ok(()) => {
                    let (layer_tree, link_registry) = composer.finish();
                    tracing::debug!("run_paint: layer tree has {} layers", layer_tree.len());
                    self.last_layer_tree = Some(layer_tree);
                    self.last_link_registry = Some(link_registry);
                }
                Err(e) => {
                    // Restore the debug invariant before propagating so
                    // the owner stays consistent on the error path.
                    let _ = self.scheduler.exit_phase(PhaseKind::Paint);
                    return Err(e);
                }
            }
        }

        // Dirty-list residue scan: any node still flagged needs_paint
        // AFTER the root descent was not reached by it (multi-root or
        // detached subtree). Warn + clear so the bug is visible AND the
        // dirty list doesn't accumulate across frames.
        for dirty_node in self.scheduler.nodes_needing_paint() {
            if let Some(render_node) = self.render_tree.get(dirty_node.id)
                && render_node.needs_paint()
            {
                tracing::warn!(
                    id = ?dirty_node.id,
                    depth = dirty_node.depth,
                    "run_paint: dirty node not reached by root descent (multi-root \
                     or detached subtree?); paint dropped, flag cleared"
                );
                render_node.clear_needs_paint();
            }
        }
        // `clear()` retains capacity (preserve Vec backing across frames).
        self.scheduler.clear_paint_queue();

        // exit_phase clears debug_doing_paint AND drains mid-paint marks back
        // into dirty so paint marks made during this pass become next-frame
        // work rather than being stranded — Flutter's flushPaint semantics.
        //
        // Finding 2 (intentional improvement over pre-refactor behavior):
        // exit_phase also drains mid-marks on the ERROR path (the early-return
        // above calls exit_phase before returning Err). Pre-refactor, the error
        // path only cleared debug_doing_paint and did NOT drain mid-marks, so
        // any mark made between enter_phase and the error was silently lost.
        // The always-drain contract of exit_phase is the correct behavior:
        // mid-paint marks scheduled before the error survive into the next
        // frame's retry rather than being dropped.
        let _ = self.scheduler.exit_phase(PhaseKind::Paint);

        Ok(())
    }

    /// Records one node's paint fragment and replays it into the
    /// composer, recursing at child markers.
    ///
    /// Per-node order follows Flutter's `PaintingContext._paintWithContext`:
    /// `WAS_REPAINT_BOUNDARY` is written and `NEEDS_PAINT` cleared
    /// **before** the node paints, so a paint body that re-marks its own
    /// node is caught by the debug check below instead of silently
    /// erasing the evidence.
    fn paint_subtree(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
        dirty_set: &FxHashSet<RenderId>,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| self.paint_subtree_impl(composer, node_id, origin, dirty_set))
    }

    /// Body of [`Self::paint_subtree`]; split out so every recursion
    /// level enters through the [`ensure_stack`] probe.
    fn paint_subtree_impl(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
        dirty_set: &FxHashSet<RenderId>,
    ) -> crate::error::RenderResult<()> {
        let Some(render_node) = self.render_tree.get(node_id) else {
            return Ok(());
        };

        let is_repaint_boundary = render_node.is_repaint_boundary();

        let alpha = render_node.paint_alpha();
        let layer_blend = render_node.paint_layer_blend();
        let transform = render_node.paint_transform();
        let child_ids: Vec<RenderId> = render_node.children().to_vec();

        // Written unconditionally PRE-paint (Flutter object.dart:3560):
        // a node flipping boundary→non-boundary leaves exactly one
        // `WAS_REPAINT_BOUNDARY=true` trail for the next compositing
        // walk's lost-boundary branch.
        render_node.set_was_repaint_boundary(is_repaint_boundary);

        // Clear BEFORE paint so the post-paint check catches a paint
        // body that marks its own node dirty (paint-must-not-redirty).
        render_node.clear_needs_paint();

        // Fully transparent subtree: skip recording entirely. Children
        // keep whatever dirty flags they carry; the residue scan in
        // run_paint clears them with a warning.
        // Uses `skip_paint()` rather than `alpha == Some(0)` so that
        // `paint_alpha()` encoding only controls layer-emission; the
        // skip-paint decision is a separate, explicit contract
        // (Flutter: `if (_alpha == 0) return;` in RenderOpacity.paint).
        if render_node.skip_paint() {
            return Ok(());
        }

        // Flutter object.dart:3497 — a node that still needs layout must
        // not paint stale geometry. Layout runs before paint in the
        // pipeline, so this guards descendant-error and partial-frame
        // paths where a poisoned layout left the flag set.
        if render_node.needs_layout() {
            return Ok(());
        }

        // Sliver visibility cull: a sliver with zero paint extent
        // (`!visible`) paints nothing and splices no children (Flutter:
        // the viewport skips invisible slivers). The gate lives here, in
        // the driver — next to the sliver hit-test extent gate — so sliver
        // objects no longer cache `geometry` just to short-circuit their
        // own `paint`. Box nodes (`geometry_sliver() == None`) are never
        // culled here. Same dirty-residue handling as the `alpha == 0`
        // skip above.
        if render_node.geometry_sliver().is_some_and(|g| !g.visible) {
            return Ok(());
        }

        // Record the node's fragment. paint_raw sees ONLY the recorder
        // (sans-IO): no tree access, no layer access, no recursion.
        let debug_name = render_node.debug_name();
        let mut recorder = FragmentRecorder::new(origin, self.device_pixel_ratio);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_node.paint_raw(&mut recorder, child_ids.len());
        }))
        .map_err(|_| crate::error::RenderError::poisoned(debug_name, "paint"))?;
        let fragment = recorder.finish();

        debug_assert!(
            !render_node.needs_paint(),
            "paint-must-not-redirty: a render object marked ITSELF \
             needs-paint during its own paint; derive visual changes \
             from state read at paint time instead of re-marking",
        );

        // Effect hooks wrap the ENTIRE node fragment (self draws AND
        // children). The pre-fragment walk wrapped children only; hook
        // implementors draw nothing themselves, so the visible result
        // is identical and the new rule matches Flutter (RenderOpacity
        // wraps its child's whole paint).
        // Alpha–blend coupling: the layer is emitted only when `paint_alpha()` returns
        // `Some`.  A render object that overrides `paint_layer_blend() -> Some(mode)`
        // but leaves `paint_alpha() -> None` will silently drop the blend layer —
        // the advanced compositor never sees it.  When wiring the first render-tree
        // consumer of an advanced blend, override BOTH hooks and return `Some(255)`
        // from `paint_alpha()` for an opaque-blend-only layer.
        let mut effect_layers = 0usize;
        if let Some(alpha) = alpha {
            let alpha_f32 = f32::from(alpha) / 255.0;
            let opacity_layer = match layer_blend {
                Some(blend) => OpacityLayer::with_blend(alpha_f32, Offset::ZERO, blend),
                None => OpacityLayer::with_offset(alpha_f32, Offset::ZERO),
            };
            composer.push_layer(Layer::Opacity(opacity_layer));
            effect_layers += 1;
        }
        if let Some(matrix) = transform {
            // The node reports its transform in LOCAL coordinates, but
            // every run inside this layer space is recorded with the
            // accumulated `origin` baked into its canvas transform.
            // Conjugate by the origin so the matrix pivots around the
            // node's own origin instead of the layer origin — a raw
            // local matrix would translate/rotate the whole accumulated
            // space. Shared with the per-child `PushTransform` fragment
            // op below (RenderFlow and friends): same math, same reason.
            composer.push_layer(Layer::Transform(TransformLayer::new(conjugate(
                matrix, origin,
            ))));
            effect_layers += 1;
        }

        for op in fragment.ops {
            match op {
                FragmentOp::Run(list) => composer.append_run(list),
                FragmentOp::Push(scope) => composer.push_layer(scope_layer(*scope, origin)),
                FragmentOp::PushTransform(matrix) => {
                    composer.push_layer(Layer::Transform(TransformLayer::new(conjugate(
                        *matrix, origin,
                    ))));
                }
                FragmentOp::Pop => composer.pop_layer(),
                FragmentOp::Child {
                    index,
                    offset_override,
                } => {
                    let Some(&child_id) = child_ids.get(index) else {
                        debug_assert!(
                            false,
                            "fragment child marker {index} out of range ({} children) — \
                             PaintCx bounds-checks markers, so a mismatch means the \
                             tree changed during paint",
                            child_ids.len(),
                        );
                        continue;
                    };
                    let Some(child_node) = self.render_tree.get(child_id) else {
                        continue;
                    };
                    if child_node
                        .as_sliver()
                        .and_then(|entry| entry.state().geometry())
                        .is_some_and(|geometry| !geometry.visible)
                    {
                        continue;
                    }
                    // Authoritative child position: RenderState.offset,
                    // committed by the layout walk; paint_child_at
                    // overrides it explicitly.
                    let child_offset = offset_override.unwrap_or_else(|| child_node.offset());
                    let child_is_boundary = child_node.is_repaint_boundary();

                    if child_is_boundary {
                        // Boundary children rebase to ZERO under their
                        // own OffsetLayer so a future offset-only move
                        // is a layer-property update, not a repaint.
                        composer.push_layer(Layer::Offset(OffsetLayer::new(origin + child_offset)));
                        self.paint_subtree(composer, child_id, Offset::ZERO, dirty_set)?;
                        composer.pop_layer();
                    } else {
                        // Inline children bake into the shared picture
                        // space — runs merge, no extra layer.
                        self.paint_subtree(composer, child_id, origin + child_offset, dirty_set)?;
                    }
                }
            }
        }

        for _ in 0..effect_layers {
            composer.pop_layer();
        }

        Ok(())
    }
}

// ============================================================================
// Fragment composition (paint phase plumbing)
// ============================================================================

/// Builds the frame's [`LayerTree`] from replayed paint fragments,
/// merging adjacent inline draw runs into shared `PictureLayer`s.
///
/// Sealing discipline mirrors the recorder's: the open run is flushed
/// into a `PictureLayer` whenever a layer boundary needs ordering
/// (push/pop) and at [`Self::finish`]. The stack always holds at least
/// the root `OffsetLayer`.
#[derive(Debug)]
struct FragmentComposer {
    tree: LayerTree,
    stack: Vec<LayerId>,
    open: DisplayList,
    /// Leader/follower link relationships, populated as a byproduct of
    /// [`Self::push_layer`] pushing a `Layer::Leader`/`Layer::Follower`.
    /// Handed to `Scene::with_links` by the binding layer so `flui-engine`
    /// can resolve follower positions at render time against this same
    /// frame's fully-built `tree` (design research plan §4.3).
    link_registry: LinkRegistry,
}

impl FragmentComposer {
    /// `device_pixel_ratio` becomes the root layer's scale: the
    /// framework paints in LOGICAL pixels, the engine rasterizes in
    /// physical surface pixels — the root transform is the single
    /// place the two meet (Flutter's RenderView root transform).
    fn new(device_pixel_ratio: f32) -> Self {
        let mut tree = LayerTree::new();
        let root_layer = if (device_pixel_ratio - 1.0).abs() < f32::EPSILON {
            Layer::Offset(OffsetLayer::zero())
        } else {
            Layer::Transform(TransformLayer::new(flui_types::Matrix4::scaling(
                device_pixel_ratio,
                device_pixel_ratio,
                1.0,
            )))
        };
        let root = tree.insert(root_layer);
        tree.set_root(Some(root));
        Self {
            tree,
            stack: vec![root],
            open: DisplayList::new(),
            link_registry: LinkRegistry::new(),
        }
    }

    /// Merges a sealed fragment run into the open picture.
    fn append_run(&mut self, run: DisplayList) {
        self.open.append(run);
    }

    /// Flushes the open picture into a `PictureLayer` under the
    /// current stack top (no-op when empty).
    fn seal_picture(&mut self) {
        if flui_painting::DisplayListCore::is_empty(&self.open) {
            return;
        }
        let list = std::mem::take(&mut self.open);
        let layer_id = self.tree.insert(Layer::from(PictureLayer::new(list)));
        let parent = *self
            .stack
            .last()
            .expect("composer stack always holds the root layer (popping it is rejected)");
        self.tree.add_child(parent, layer_id);
    }

    fn push_layer(&mut self, layer: Layer) {
        self.seal_picture();
        // Extract the link-registry-relevant fields BEFORE `layer` moves
        // into the tree — `Leader`/`Follower` are `Copy`-field-bearing, so
        // this is a cheap read, not a clone of the layer itself.
        let leader_registration = layer
            .as_leader()
            .map(|leader| (leader.link(), leader.get_offset(), leader.size()));
        let follower_link = layer.as_follower().map(FollowerLayer::link);

        let id = self.tree.insert(layer);
        if let Some((link, offset, size)) = leader_registration {
            self.link_registry.register_leader(link, id, offset, size);
        }
        if let Some(link) = follower_link {
            self.link_registry.register_follower(id, link);
        }

        let parent = *self
            .stack
            .last()
            .expect("composer stack always holds the root layer (popping it is rejected)");
        self.tree.add_child(parent, id);
        self.stack.push(id);
    }

    fn pop_layer(&mut self) {
        self.seal_picture();
        debug_assert!(
            self.stack.len() > 1,
            "composer pop without matching push — fragment scope ops are \
             balanced by the recorder, so an underflow means the replay \
             loop pushed/popped asymmetrically",
        );
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    fn finish(mut self) -> (LayerTree, LinkRegistry) {
        self.seal_picture();
        debug_assert_eq!(
            self.stack.len(),
            1,
            "composer finished with unbalanced layer stack — every \
             push_layer in the replay loop must have a matching pop_layer",
        );
        (self.tree, self.link_registry)
    }
}

/// Conjugates `matrix` so it pivots around this layer's local `origin`
/// rather than the layer tree's own (0, 0).
///
/// Both callers report a transform in LOCAL coordinates while every run
/// they bracket carries the accumulated `origin` baked into its canvas
/// transform: the per-node [`RenderObject::paint_transform`](crate::traits::RenderObject::paint_transform)
/// hook (one transform for the whole node, applied here) and the
/// per-child [`FragmentOp::PushTransform`] op (`RenderFlow` and any
/// other Variable-arity node giving each child its own paint-time
/// transform). Flutter `PaintingContext.pushTransform`:
/// `T(offset)·M·T(−offset)`.
fn conjugate(matrix: flui_types::Matrix4, origin: Offset) -> flui_types::Matrix4 {
    if origin == Offset::ZERO {
        matrix
    } else {
        let (dx, dy) = (origin.dx.get(), origin.dy.get());
        flui_types::Matrix4::translation(dx, dy, 0.0)
            * matrix
            * flui_types::Matrix4::translation(-dx, -dy, 0.0)
    }
}

/// Maps a recorded effect-layer scope onto its `flui-layer` layer.
///
/// Scope shapes/bounds are recorded in the node's LOCAL coordinates, while
/// the runs they bracket carry the accumulated `origin` baked into their
/// canvas transforms — so every variant is shifted by `origin` here
/// (Flutter `pushClipRect`: `clipRect.shift(offset)`; `RenderShaderMask`'s
/// `maskRect = offset & size`; `RenderBackdropFilter`'s backdrop bounds
/// follow the same `offset & size` convention), or a scope away from the
/// parent origin would apply at the layer's (0,0) instead of the node's
/// position.
///
/// Always a real layer today; lowering non-composited clips back into
/// canvas clips inside the merged picture is a composer-side optimization
/// gated on the `needs_compositing` bits — correctness is identical
/// either way, so the recording API does not expose the choice.
fn scope_layer(scope: FragmentScope, origin: Offset) -> Layer {
    match scope {
        FragmentScope::Rect { rect, behavior } => {
            Layer::ClipRect(ClipRectLayer::new(rect.translate_offset(origin), behavior))
        }
        FragmentScope::RRect { rrect, behavior } => Layer::ClipRRect(ClipRRectLayer::new(
            rrect.translate_offset(origin),
            behavior,
        )),
        FragmentScope::Path { path, behavior } => {
            let path = if origin == Offset::ZERO {
                *path
            } else {
                path.translate(origin)
            };
            Layer::ClipPath(Box::new(ClipPathLayer::new(path, behavior)))
        }
        FragmentScope::ShaderMask {
            shader,
            blend_mode,
            bounds,
        } => Layer::ShaderMask(ShaderMaskLayer::new(
            shader,
            blend_mode,
            bounds.translate_offset(origin),
        )),
        FragmentScope::BackdropFilter {
            filter,
            blend_mode,
            bounds,
        } => Layer::BackdropFilter(BackdropFilterLayer::new(
            filter,
            blend_mode,
            bounds.translate_offset(origin),
        )),
        // Oracle `LeaderLayer(link: link, offset: offset)` — `offset` is
        // this node's own accumulated position, exactly what `origin`
        // already is at this call site (`:270`). Unlike the clip/mask
        // variants above, `size` is NOT shifted by `origin` — it is a
        // pure dimension, not a position.
        FragmentScope::Leader { link, size } => {
            Layer::Leader(LeaderLayer::with_offset(link, size, origin))
        }
        // `Layer::Follower` carries no resolved position at all —
        // matching oracle, where a `FollowerLayer`'s `linkedOffset`/
        // `unlinkedOffset` are inputs to a LATER resolution pass, never
        // stored as the final on-screen transform. `target_offset` is
        // recorded as-authored (not origin-shifted): resolving it against
        // the leader's position is deliberately deferred past this pass
        // (design research plan §4/§8 — a `flui-engine`/`flui-layer`
        // follow-up, not performed here).
        FragmentScope::Follower {
            link,
            size,
            target_offset,
            show_when_unlinked,
            leader_anchor,
            follower_anchor,
        } => Layer::Follower(
            FollowerLayer::new(link)
                .with_size(size)
                .with_target_offset(target_offset)
                .with_show_when_unlinked(show_when_unlinked)
                .with_leader_anchor(leader_anchor)
                .with_follower_anchor(follower_anchor),
        ),
    }
}
