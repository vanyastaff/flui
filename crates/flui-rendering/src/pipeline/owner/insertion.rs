//! Insertion of render objects into the owner's render tree and the dirty marks a new node starts
//! with.

use flui_foundation::RenderId;

use crate::protocol::{BoxProtocol, SliverProtocol};

use super::PipelineOwner;

impl PipelineOwner {
    /// Inserts a render object into the tree and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object into the RenderTree
    /// 2. Adds the node to the dirty layout list (since new nodes need layout)
    /// 3. Adds the node to the dirty paint list (since new nodes need paint)
    ///
    /// The tree's own insert does none of this; outside this crate this is the
    /// way to add a parentless node with proper dirty
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
    /// The tree's own child insert does none of this; outside this crate this is
    /// the way to add a child with proper
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
    /// — same dirty tracking and ADR-0013 attach wiring. The tree's own
    /// Sliver-child insert, which this wraps, does neither: a Sliver render
    /// object that subscribes to a `Listenable` in `attach` (e.g. a
    /// snap-animation controller) would never get its self-dirty handle.
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
    /// directly — equivalent to the storage flag's answer; the bootstrap
    /// flag is the optimization target for a later sweep that swaps the
    /// paint check too.
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
        let handle = self.local_dirty.bind(id, epoch);
        if let Some(node) = self.render_tree.get_mut(id) {
            let _ = node.attach(epoch, handle);
        }
    }
}
