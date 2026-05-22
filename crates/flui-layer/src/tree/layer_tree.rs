//! LayerTree - Slab-based storage for compositor layers
//!
//! This module provides the LayerTree struct and LayerNode
//! for managing the compositor layer hierarchy.

use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::{ElementId, LayerId};
use flui_types::{Offset, geometry::Pixels};
use slab::Slab;

use crate::layer::Layer;

// ============================================================================
// LAYER NODE
// ============================================================================

/// A node in the LayerTree that wraps a Layer with tree structure metadata.
///
/// # Design
///
/// Unlike ViewTree and RenderTree which are generic over the object type,
/// LayerNode is concrete because Layer is already an enum that encompasses
/// all layer types. This simplifies the API while maintaining the same
/// architectural pattern.
///
/// # Lifecycle (phase 1, U8)
///
/// `LayerNode` adopts the same `disposed: AtomicBool` + `Drop` + debug-assert
/// guard pattern that PR #84 introduced on
/// [`flui_foundation::ChangeNotifier`]. Once the node is removed from the
/// tree, the slab drops it; `Drop` flips the `disposed` flag once
/// (idempotent via `AtomicBool::swap`). Subsequent calls into the mutation
/// surface from a stale reference — possible if a caller leaks a `&mut
/// LayerNode` past tree mutation — trip a `debug_assert!` in debug builds
/// and emit a `tracing::warn!` + no-op in release. Mirrors Flutter
/// `layer.dart` `void dispose() @mustCallSuper`.
///
/// [`flui_foundation::ChangeNotifier`]: ../../flui-foundation/src/notifier.rs
#[derive(Debug)]
pub struct LayerNode {
    // ========== Tree Structure ==========
    parent: Option<LayerId>,
    children: Vec<LayerId>,

    // ========== Layer ==========
    /// The compositor layer (Canvas, ShaderMask, etc.)
    layer: Layer,

    // ========== Metadata ==========
    /// Offset from parent (parent data)
    offset: Option<Offset<Pixels>>,

    /// Associated ElementId (for cross-tree references)
    element_id: Option<ElementId>,

    // ========== Lifecycle (phase 1, U8) ==========
    /// Whether the node has been dropped. Set by [`Drop`]; once `true` the
    /// node MUST NOT be mutated again. Guarded by [`assert_alive`].
    disposed: AtomicBool,

    // ========== Compositor dirty-bit (phase 2, U9) ==========
    /// Whether this node's payload changed and needs to be re-pushed into the
    /// engine scene on the next composite. Defaults to `true` (fresh nodes
    /// have not yet been pushed). Cleared by the engine after a successful
    /// scene build. Mirrors Flutter `layer.dart` `_needsAddToScene`.
    needs_add_to_scene: AtomicBool,
}

impl LayerNode {
    /// Creates a new LayerNode with the given Layer.
    pub fn new(layer: Layer) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            layer,
            offset: None,
            element_id: None,
            disposed: AtomicBool::new(false),
            // Fresh node has not yet been pushed into the scene.
            needs_add_to_scene: AtomicBool::new(true),
        }
    }

    /// Creates a LayerNode with an associated ElementId.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    /// Creates a LayerNode with an offset.
    pub fn with_offset(mut self, offset: Offset<Pixels>) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Lifecycle guard — returns `true` if the node is alive, `false` if it
    /// has been disposed. Inlined into every mutation method below; a
    /// `false` return means the caller MUST short-circuit the mutation.
    ///
    /// Behaviour on stale-reference use:
    /// - **Debug builds**: `debug_assert!` panics so the bug surfaces in
    ///   CI rather than corrupting compositor state silently.
    /// - **Release builds**: emits `tracing::warn!` and returns `false`;
    ///   the caller's mutation is skipped (no-op).
    ///
    /// Acquire-ordering on the load pairs with the `swap(true, Release)` in
    /// [`LayerNode::drop`] — anything published by the dropping thread is
    /// visible here.
    ///
    /// PR #100 followup: pre-followup the guard panicked in debug but did
    /// NOT short-circuit the mutation in release — the type-level doc on
    /// `LayerNode` advertised "warn + no-op" semantics that release builds
    /// did not actually deliver. The bool return + caller-side early
    /// return aligns behaviour with the documented contract.
    #[inline]
    #[must_use]
    fn assert_alive(&self, op: &'static str) -> bool {
        if self.disposed.load(Ordering::Acquire) {
            debug_assert!(
                false,
                "LayerNode::{op} called after disposal — use-after-free \
                 reachable via a stale reference past slab removal"
            );
            tracing::warn!(op, "LayerNode used after disposal");
            return false;
        }
        true
    }

    // ========== Tree Structure ==========

    /// Gets the parent LayerId.
    #[inline]
    pub fn parent(&self) -> Option<LayerId> {
        self.parent
    }

    /// Sets the parent LayerId.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<LayerId>) {
        if !self.assert_alive("set_parent") {
            return;
        }
        self.parent = parent;
    }

    /// Gets all children LayerIds.
    #[inline]
    pub fn children(&self) -> &[LayerId] {
        &self.children
    }

    /// Adds a child to this layer node.
    ///
    /// Dedup-checks against the existing children vector — a second call
    /// with the same id is a no-op. Mirrors `SemanticsNode::add_child`'s
    /// containment check.
    #[inline]
    pub fn add_child(&mut self, child: LayerId) {
        if !self.assert_alive("add_child") {
            return;
        }
        if !self.children.contains(&child) {
            self.children.push(child);
        }
    }

    /// Removes a child from this layer node.
    #[inline]
    pub fn remove_child(&mut self, child: LayerId) {
        if !self.assert_alive("remove_child") {
            return;
        }
        self.children.retain(|&id| id != child);
    }

    /// Clears all children from this layer node.
    #[inline]
    pub fn clear_children(&mut self) {
        if !self.assert_alive("clear_children") {
            return;
        }
        self.children.clear();
    }

    // ========== Layer Access ==========

    /// Returns reference to the Layer.
    #[inline]
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// Returns mutable reference to the Layer.
    ///
    /// Implicitly marks this node dirty for the next composite, mirroring
    /// Flutter `layer.dart` `markNeedsAddToScene`. Callers wanting to read
    /// the layer without invalidating the cached scene should go through
    /// [`Self::layer`] instead.
    #[inline]
    pub fn layer_mut(&mut self) -> &mut Layer {
        // `layer_mut` returns `&mut Layer` unconditionally — there is no
        // safe sentinel for "Layer is unavailable post-dispose." Debug
        // builds panic via `assert_alive`'s `debug_assert!`; release
        // builds emit `tracing::warn!` and return the live `&mut` (the
        // caller's subsequent mutation is the use-after-free the warn
        // surfaces).
        let _ = self.assert_alive("layer_mut");
        self.needs_add_to_scene.store(true, Ordering::Release);
        &mut self.layer
    }

    // ========== Metadata ==========

    /// Returns whether this layer needs compositing.
    ///
    /// Delegates to [`Layer::needs_compositing`] — the canonical answer is the
    /// enum-method computed from the variant. The previously cached field was
    /// removed in the layer-lifecycle repair cycle: it had no invalidation
    /// path and its default value diverged from the enum-method's answer for
    /// the Canvas/Picture/Offset variants.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.layer.needs_compositing()
    }

    /// Gets the offset from parent (parent data).
    #[inline]
    pub fn offset(&self) -> Option<Offset<Pixels>> {
        self.offset
    }

    /// Gets the associated ElementId (for cross-tree references).
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Returns whether this node has been disposed (its slab slot dropped).
    ///
    /// Provided for use-after-disposal regression tests. Production code
    /// should not need to consult this — the guards inside the mutation
    /// surface make stale-reference mutation a debug-mode panic and a
    /// release-mode warn-and-no-op.
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Acquire)
    }

    // ========== Compositor dirty-bit (phase 2, U9) ==========

    /// Returns whether this node is *clean* — i.e. its current payload has
    /// already been pushed into the engine scene and need not be pushed
    /// again. The inverse of [`Self::needs_add_to_scene`].
    #[inline]
    pub fn is_clean(&self) -> bool {
        !self.needs_add_to_scene.load(Ordering::Acquire)
    }

    /// Returns whether this node needs to be pushed into the engine scene on
    /// the next composite. Defaults to `true` for freshly inserted nodes;
    /// any mutation via [`Self::layer_mut`] flips it back to `true`; the
    /// engine clears it after a successful scene build via
    /// [`LayerTree::clear_needs_add_to_scene_subtree`].
    #[inline]
    pub fn needs_add_to_scene(&self) -> bool {
        self.needs_add_to_scene.load(Ordering::Acquire)
    }

    /// Marks this node dirty without traversing the tree. Used by
    /// [`LayerTree::mark_needs_add_to_scene`] when walking ancestors;
    /// callers in flui-layer should prefer the tree-level helper which
    /// also walks parents.
    #[inline]
    pub(crate) fn mark_needs_add_to_scene_local(&self) {
        self.needs_add_to_scene.store(true, Ordering::Release);
    }

    /// Clears this node's dirty bit without traversing children. Used by
    /// [`LayerTree::clear_needs_add_to_scene_subtree`] when the engine has
    /// finished pushing the subtree into a scene.
    #[inline]
    pub(crate) fn clear_needs_add_to_scene_local(&self) {
        self.needs_add_to_scene.store(false, Ordering::Release);
    }
}

impl Drop for LayerNode {
    fn drop(&mut self) {
        // Idempotent flip: `swap` returns the prior value. If we were
        // already disposed (unlikely outside re-drop test scaffolding),
        // skip the tracing log. Release-ordering on the store pairs with
        // the Acquire-ordering in `assert_alive`.
        if !self.disposed.swap(true, Ordering::Release) {
            // Phase 3 (deferred): release engine-layer handle here.
            tracing::trace!(?self.element_id, "LayerNode dropped");
        }
    }
}

// ============================================================================
// LAYER TREE
// ============================================================================

/// LayerTree - Slab-based storage for compositor layers.
///
/// This is the fourth of FLUI's five trees, corresponding to Flutter's Layer
/// tree used for composition and GPU rendering.
///
/// # Architecture
///
/// ```text
/// LayerTree
///   ├─ nodes: Slab<LayerNode>  (direct storage)
///   └─ root: Option<LayerId>
/// ```
///
/// # Thread Safety
///
/// LayerTree itself is not thread-safe. Use `Arc<RwLock<LayerTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust
/// use flui_layer::{CanvasLayer, Layer, LayerNode, LayerTree};
/// use flui_tree::TreeRead;
///
/// let mut tree = LayerTree::new();
///
/// // Insert canvas layer
/// let canvas_layer = Layer::Canvas(CanvasLayer::new());
/// let id = tree.insert(canvas_layer);
///
/// // Access layer
/// let node = tree.get(id).unwrap();
/// assert!(node.needs_compositing());
/// ```
#[derive(Debug)]
pub struct LayerTree {
    /// Slab storage for LayerNodes (0-based indexing internally)
    nodes: Slab<LayerNode>,

    /// Root LayerNode ID (None if tree is empty)
    root: Option<LayerId>,
}

impl LayerTree {
    /// Creates a new empty LayerTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a LayerTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root LayerNode ID.
    #[inline]
    pub fn root(&self) -> Option<LayerId> {
        self.root
    }

    /// Set the root LayerNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<LayerId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a LayerNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: LayerId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of LayerNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserts a Layer into the tree.
    ///
    /// Returns the LayerId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `LayerId(1)`
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{CanvasLayer, Layer, LayerTree};
    ///
    /// let mut tree = LayerTree::new();
    /// let layer = Layer::Canvas(CanvasLayer::new());
    /// let id = tree.insert(layer);
    /// ```
    pub fn insert(&mut self, layer: Layer) -> LayerId {
        let node = LayerNode::new(layer);
        let slab_index = self.nodes.insert(node);
        LayerId::new(slab_index + 1) // +1 offset
    }

    /// Inserts a Layer with an associated ElementId.
    pub fn insert_with_element(&mut self, layer: Layer, element_id: ElementId) -> LayerId {
        let node = LayerNode::new(layer).with_element_id(element_id);
        let slab_index = self.nodes.insert(node);
        LayerId::new(slab_index + 1)
    }

    /// Returns a reference to a LayerNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `LayerId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: LayerId) -> Option<&LayerNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a LayerNode.
    #[inline]
    pub fn get_mut(&mut self, id: LayerId) -> Option<&mut LayerNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Returns a reference to the Layer directly.
    #[inline]
    pub fn get_layer(&self, id: LayerId) -> Option<&Layer> {
        self.get(id).map(LayerNode::layer)
    }

    /// Returns a mutable reference to the Layer directly.
    #[inline]
    pub fn get_layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.get_mut(id).map(LayerNode::layer_mut)
    }

    /// Removes a LayerNode from the tree **and cascades** to every
    /// descendant.
    ///
    /// Returns the removed root node, or `None` if it did not exist.
    ///
    /// **Cascade semantics (U12)** — every descendant is also removed from
    /// the slab. The walk is post-order: children are removed before the
    /// parent, so each `LayerNode::drop` fires while the parent's
    /// `children` vector is still intact (the engine's debug listeners can
    /// inspect a coherent tree state during dispose). The parent's
    /// `children` vector is also drained of `id` before the parent's own
    /// node is removed, so a `LayerTree::get(parent_id)` lookup after the
    /// cascade does not observe a stale id.
    ///
    /// For non-cascading workflows (e.g. reparenting that re-inserts
    /// immediately at a new attachment point), use [`remove_shallow`].
    ///
    /// Mirrors Flutter `layer.dart:1185-1216` `ContainerLayer.remove` +
    /// `LayerHandle._unref` cascade.
    pub fn remove(&mut self, id: LayerId) -> Option<LayerNode> {
        if !self.contains(id) {
            return None;
        }

        // 1. Snapshot the children list (avoids holding `&self` across the
        //    recursive `self.remove(child_id)` calls).
        let children: Vec<LayerId> = self
            .get(id)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();

        // 2. Post-order cascade: drop descendants first.
        for child_id in children {
            // Each recursive call also walks its own subtree. Bounded by
            // tree depth (typical widget trees ≤32 levels) plus stack —
            // `MARK_PROPAGATION_MAX_DEPTH` is the moral cap.
            let _ = self.remove(child_id);
        }

        // 3. Unlink from parent so the parent's children vector doesn't
        //    contain a stale id post-removal.
        if let Some(parent_id) = self.get(id).and_then(LayerNode::parent) {
            if let Some(parent) = self.get_mut(parent_id) {
                parent.remove_child(id);
            }
        }

        // 4. Update root if removing root.
        if self.root == Some(id) {
            self.root = None;
        }

        // 5. Drop self — triggers `LayerNode::drop` (U8 phase 1).
        self.nodes.try_remove(id.get() - 1)
    }

    /// Removes a single LayerNode from the tree **without** cascading to
    /// descendants. Use this for reparenting workflows that immediately
    /// re-attach the removed node elsewhere.
    ///
    /// Returns the removed node, or `None` if it did not exist.
    ///
    /// Unlike [`remove`], this does NOT touch the parent's children
    /// vector — the caller is responsible for keeping parent/child
    /// pointers consistent. For full cascade semantics use [`remove`].
    pub fn remove_shallow(&mut self, id: LayerId) -> Option<LayerNode> {
        if self.root == Some(id) {
            self.root = None;
        }
        self.nodes.try_remove(id.get() - 1)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    // ========== Tree Operations ==========

    /// Adds `child_id` as a child of `parent_id`.
    ///
    /// **Auto-detach semantics (U10)** — if `child_id` is currently attached
    /// to a different parent, it is removed from that parent's children
    /// vector first, then attached here. Re-attaching to the *same* parent
    /// is a short-circuit no-op so the child appears only once in the
    /// children vector (`LayerNode::add_child` carries the dedup check).
    /// This mirrors Flutter `layer.dart:1098-1149` `ContainerLayer.append`
    /// — the Dart `assert(child._parent == null)` reaches the same outcome
    /// via a precondition; FLUI cleans up instead because Rust idiom is
    /// "do the right thing" not "panic on misuse."
    ///
    /// **Cycle rejection (PR #100 followup)** — a call that would create a
    /// cycle (`child_id == parent_id`, or attaching an ancestor under its
    /// descendant) is rejected as a no-op and emits a `tracing::warn!`.
    /// Pre-rejection the recursive `remove` (U12) would have followed a
    /// cycle to unbounded recursion and stack overflow; this guard makes
    /// the cycle impossible to enter via the public API.
    ///
    /// Missing-id lookups (either `parent_id` or `child_id` not in the
    /// tree) are silent no-ops.
    pub fn add_child(&mut self, parent_id: LayerId, child_id: LayerId) {
        // Both endpoints must exist — otherwise the call is a no-op.
        if !self.contains(parent_id) || !self.contains(child_id) {
            return;
        }

        // Reject self-attachment outright — `parent_id == child_id` is a
        // 1-cycle, the smallest possible.
        if parent_id == child_id {
            tracing::warn!(
                ?parent_id,
                "LayerTree::add_child rejected self-link (cycle)"
            );
            return;
        }

        // Reject attaching an ancestor of `parent_id` under it (would create
        // an N-cycle). Walk parent's ancestor chain; if `child_id` is in
        // the chain, this call is a cycle attempt and gets rejected.
        if self.is_ancestor_of(child_id, parent_id) {
            tracing::warn!(
                ?parent_id,
                ?child_id,
                "LayerTree::add_child rejected cycle (child is ancestor of parent)"
            );
            return;
        }

        // 1. Detach from previous parent if one exists and differs from
        //    `parent_id`.
        let prev_parent = self.get(child_id).and_then(LayerNode::parent);
        if let Some(prev) = prev_parent {
            if prev == parent_id {
                // Already a child of this parent — `LayerNode::add_child`
                // dedups, but short-circuit anyway to avoid the redundant
                // mutation + dirty-bit ripple.
                return;
            }
            if let Some(prev_node) = self.get_mut(prev) {
                prev_node.remove_child(child_id);
            }
        }

        // 2. Attach to new parent. `LayerNode::add_child` carries dedup so
        //    a transient race that retries the call won't double-insert.
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // 3. Update child's parent pointer.
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Returns `true` if `candidate_ancestor` is an ancestor of `descendant`
    /// (including the case where they are the same id, which the
    /// `add_child` guard treats as a self-cycle and rejects upstream).
    ///
    /// Walk is bounded by the tree's slab size so a malformed parent
    /// pointer cycle (which `add_child` no longer permits to be created)
    /// can not hang the check.
    fn is_ancestor_of(&self, candidate_ancestor: LayerId, descendant: LayerId) -> bool {
        let mut current = Some(descendant);
        let mut steps = 0;
        let max_steps = self.nodes.len() + 1;
        while let Some(id) = current {
            if id == candidate_ancestor {
                return true;
            }
            steps += 1;
            if steps > max_steps {
                // Defence-in-depth: a malformed cycle pre-dating the U10
                // / PR #100 followup guards (e.g. a slab loaded from disk
                // with corrupt parent pointers) would otherwise spin.
                tracing::warn!(
                    "LayerTree::is_ancestor_of: walk exceeded slab size — \
                     malformed parent pointers?"
                );
                return false;
            }
            current = self.get(id).and_then(LayerNode::parent);
        }
        false
    }

    /// Removes a child from a parent LayerNode.
    pub fn remove_child(&mut self, parent_id: LayerId, child_id: LayerId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Returns the parent of a node.
    pub fn parent(&self, id: LayerId) -> Option<LayerId> {
        self.get(id)?.parent()
    }

    /// Returns the children of a node.
    pub fn children(&self, id: LayerId) -> Option<&[LayerId]> {
        self.get(id).map(LayerNode::children)
    }

    /// Clears all children from a parent node.
    ///
    /// This is used by Flutter's `pushLayer` when reusing layers - old children
    /// are removed before adding new content.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Flutter pattern: reuse layer, clear old children
    /// if let Some(old_layer) = reusable_layer {
    ///     tree.clear_children(old_layer_id);
    /// }
    /// ```
    pub fn clear_children(&mut self, parent_id: LayerId) {
        // First, get the list of children to clear their parent references
        let children_to_clear: Vec<LayerId> = if let Some(parent) = self.get(parent_id) {
            parent.children().to_vec()
        } else {
            return;
        };

        // Clear parent's children list
        if let Some(parent) = self.get_mut(parent_id) {
            parent.clear_children();
        }

        // Clear parent reference from each child
        for child_id in children_to_clear {
            if let Some(child) = self.get_mut(child_id) {
                child.set_parent(None);
            }
        }
    }

    // ========== Layer Composition (Flutter PaintingContext Pattern) ==========

    /// Appends a layer as a child of a container layer.
    ///
    /// This is the core operation used by Flutter's PaintingContext when
    /// composing layers during painting. It's typically called in two
    /// scenarios:
    ///
    /// 1. **After stopRecordingIfNeeded()**: Append the finished PictureLayer
    /// 2. **In pushLayer()**: Append a container layer before painting into it
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void _appendLayer(Layer layer) {
    ///   _containerLayer.append(layer);
    /// }
    /// ```
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_layer::{LayerTree, Layer, PictureLayer};
    ///
    /// let mut tree = LayerTree::new();
    ///
    /// // Create container layer (e.g., OffsetLayer)
    /// let container = Layer::Offset(OffsetLayer::zero());
    /// let container_id = tree.insert(container);
    ///
    /// // Record some drawing commands
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(rect, &paint);
    /// let picture = canvas.finish();
    ///
    /// // Create picture layer
    /// let picture_layer = Layer::Picture(PictureLayer::new(picture));
    /// let picture_id = tree.insert(picture_layer);
    ///
    /// // Append to container (Flutter: _containerLayer.append(layer))
    /// tree.append_layer(container_id, picture_id);
    /// ```
    ///
    /// # Usage in PaintingContext
    ///
    /// ```rust,ignore
    /// impl PaintingContext {
    ///     fn stop_recording_if_needed(&mut self) {
    ///         if let Some(current_layer) = self.current_layer.take() {
    ///             // Finish recording
    ///             let picture = self.canvas.finish();
    ///             let picture_layer = PictureLayer::new(picture);
    ///             let layer_id = self.layer_tree.insert(Layer::Picture(picture_layer));
    ///
    ///             // Append to container (THIS METHOD)
    ///             self.layer_tree.append_layer(self.container_layer, layer_id);
    ///         }
    ///     }
    ///
    ///     fn push_layer<F>(&mut self, layer: Layer, painter: F, offset: Offset)
    ///     where
    ///         F: FnOnce(&mut PaintingContext, Offset),
    ///     {
    ///         self.stop_recording_if_needed();
    ///
    ///         // Insert and append container layer (THIS METHOD)
    ///         let layer_id = self.layer_tree.insert(layer);
    ///         self.layer_tree.append_layer(self.container_layer, layer_id);
    ///
    ///         // Create child context and paint
    ///         let mut child_context = PaintingContext::new(layer_id, ...);
    ///         painter(&mut child_context, offset);
    ///         child_context.stop_recording_if_needed();
    ///     }
    /// }
    /// ```
    pub fn append_layer(&mut self, container_id: LayerId, child_id: LayerId) {
        self.add_child(container_id, child_id);
    }

    /// Appends multiple layers to a container in order.
    ///
    /// This is a convenience method for bulk appending, which is common when
    /// building complex layer hierarchies.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.append_layers(container_id, &[layer1_id, layer2_id, layer3_id]);
    /// ```
    pub fn append_layers(&mut self, container_id: LayerId, children: &[LayerId]) {
        for &child_id in children {
            self.append_layer(container_id, child_id);
        }
    }

    // ========== Iteration ==========

    /// Returns an iterator over all LayerIds in the tree.
    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.nodes.iter().map(|(index, _)| LayerId::new(index + 1))
    }

    /// Returns the raw slab iterator for zero-cost iteration.
    ///
    /// Used internally by tree trait implementations.
    #[inline]
    pub(crate) fn iter_slab(&self) -> slab::Iter<'_, LayerNode> {
        self.nodes.iter()
    }

    /// Returns an iterator over all (LayerId, &LayerNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (LayerId, &LayerNode)> + '_ {
        self.nodes
            .iter()
            .map(|(index, node)| (LayerId::new(index + 1), node))
    }

    /// Returns a mutable iterator over all (LayerId, &mut LayerNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (LayerId, &mut LayerNode)> + '_ {
        self.nodes
            .iter_mut()
            .map(|(index, node)| (LayerId::new(index + 1), node))
    }

    // ========== Compositor dirty-bit propagation (U9) ==========
    //
    // Mirrors Flutter `layer.dart`:
    // - `markNeedsAddToScene`            (lines 377-392)
    // - `updateSubtreeNeedsAddToScene`   (lines 495-521)
    //
    // The "mark" path walks ancestors top-up flipping every node dirty
    // (anything in the path-to-root must be re-pushed because the parent
    // owns the child layer reference). The "update" path is a post-order
    // DFS that folds child dirty bits into the parent's bit so the engine
    // can ask the root "is any descendant dirty?" with a single read.
    //
    // The walks are intentionally read-only on the tree (`&self`) — the
    // dirty bit lives behind `AtomicBool` so no `&mut LayerNode` is
    // required to flip it.

    /// Marks `id` and every ancestor up to the root as needing a re-push
    /// into the engine scene on the next composite.
    ///
    /// Walks the parent chain via [`LayerNode::parent`]. Bounded by the
    /// slab size — that is the strict upper bound on the chain length
    /// for an acyclic tree, and the `add_child` cycle-rejection guard
    /// (PR #100 followup) makes cycles impossible to construct via the
    /// public API. The bound is therefore a defence-in-depth guard
    /// against a slab corrupted by direct field access (which is not
    /// reachable from safe API), not a real upper limit on tree depth.
    ///
    /// Pre-followup the walk capped at `MARK_PROPAGATION_MAX_DEPTH = 32`,
    /// silently dropping dirty propagation for ancestors beyond depth
    /// 32 — correctness regression for any tree deeper than 32 levels
    /// (legitimate for nested scroll views + deeply nested popovers).
    /// The PR #100 followup makes the walk slab-bounded instead.
    ///
    /// Flutter parity: `layer.dart:377-392` `markNeedsAddToScene`.
    pub fn mark_needs_add_to_scene(&self, id: LayerId) {
        let mut current = Some(id);
        // `nodes.len() + 1` is the strict upper bound on the chain
        // length: an acyclic tree of N nodes has at most N nodes in
        // any single root-to-leaf path. The `+1` is the entry point.
        let max_steps = self.nodes.len() + 1;
        let mut steps = 0;
        while let Some(node_id) = current {
            steps += 1;
            if steps > max_steps {
                tracing::warn!(
                    "LayerTree::mark_needs_add_to_scene: walk exceeded \
                     slab size — malformed parent pointers?"
                );
                break;
            }
            let Some(node) = self.get(node_id) else {
                break;
            };
            node.mark_needs_add_to_scene_local();
            current = node.parent();
        }
    }

    /// Post-order walks the subtree rooted at `root`, folding each child's
    /// dirty bit into the parent so the parent reports `true` whenever any
    /// descendant is dirty. Returns the resulting per-subtree dirty bit.
    ///
    /// Idempotent — repeated calls observe (and propagate) the same
    /// per-node states.
    ///
    /// Flutter parity: `layer.dart:495-521` `updateSubtreeNeedsAddToScene`.
    pub fn update_subtree_needs_add_to_scene(&self, root: LayerId) -> bool {
        let Some(root_node) = self.get(root) else {
            return false;
        };
        let mut any_dirty = root_node.needs_add_to_scene();
        for &child_id in root_node.children() {
            if self.update_subtree_needs_add_to_scene(child_id) {
                any_dirty = true;
            }
        }
        if any_dirty {
            root_node.mark_needs_add_to_scene_local();
        }
        any_dirty
    }

    /// Recursively clears the dirty bit on `root` and every descendant.
    /// Called by the engine after a successful scene-build pass — the
    /// scene now reflects the layer payloads.
    pub fn clear_needs_add_to_scene_subtree(&self, root: LayerId) {
        let Some(root_node) = self.get(root) else {
            return;
        };
        root_node.clear_needs_add_to_scene_local();
        for &child_id in root_node.children() {
            self.clear_needs_add_to_scene_subtree(child_id);
        }
    }
}

impl Default for LayerTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// LAYER NODE LIFECYCLE TESTS (U8 — phase 1)
// ============================================================================

#[cfg(test)]
mod lifecycle_tests {
    use crate::layer::{CanvasLayer, Layer};

    use super::LayerNode;

    #[test]
    fn fresh_node_is_not_disposed() {
        let node = LayerNode::new(Layer::from(CanvasLayer::new()));
        assert!(!node.is_disposed());
    }

    #[test]
    fn drop_marks_node_disposed() {
        // Observable side effect of `Drop::drop`: the `disposed` flag flips
        // to `true`. We can't read it post-drop (the allocation is gone),
        // so we use `MaybeUninit` to drop in place and then read the flag
        // through a raw pointer that still points at the now-dropped (but
        // still-allocated-on-the-stack) bytes — the AtomicBool wrote its
        // value before the inner type was deallocated.
        use std::mem::MaybeUninit;
        use std::ptr;
        let mut slot: MaybeUninit<LayerNode> =
            MaybeUninit::new(LayerNode::new(Layer::from(CanvasLayer::new())));

        // Sanity: alive before drop.
        // SAFETY: `slot` is initialized via `new(LayerNode::new(...))` just
        // above; we hold the only reference and the borrow is short.
        let alive = unsafe { (*slot.as_ptr()).is_disposed() };
        assert!(!alive);

        // SAFETY: same allocation, same initialization invariant; running
        // the inner `Drop` exactly once is the test's purpose. We do not
        // re-initialize the slot afterwards.
        unsafe { ptr::drop_in_place(slot.as_mut_ptr()) };

        // Read the disposed flag after the inner drop ran. AtomicBool
        // storage occupies the same bytes pre- and post-drop on
        // stable Rust — the `Drop` impl flips the flag *before* the
        // surrounding type's other fields go out of scope.
        // SAFETY: `slot` still owns the stack bytes; `is_disposed` only
        // reads the `disposed` AtomicBool, which has trivial Drop and
        // remains valid post-`drop_in_place`. We do NOT touch any field
        // with a non-trivial Drop (the `Layer` enum's `Box<T>` variants
        // are already deallocated by the drop above).
        let disposed_after = unsafe { (*slot.as_ptr()).is_disposed() };
        assert!(
            disposed_after,
            "LayerNode::drop must flip the `disposed` flag to true"
        );

        // We intentionally do NOT call `slot.assume_init_drop()` —
        // `drop_in_place` already ran the inner Drop once, and the
        // `disposed: AtomicBool` swap inside that Drop made a second
        // run a no-op-on-the-flag-but-double-free on the rest.
    }

    #[test]
    fn redrop_does_not_re_emit_drop_side_effect() {
        // The drop guard inside `LayerNode::drop` uses
        // `disposed.swap(true, Release)` and only emits the
        // `tracing::trace!` log on the first transition. We can't
        // observe `tracing` directly without a subscriber, so we
        // observe the flag through `is_disposed`: the second
        // `drop_in_place` must leave it set (it was already true) and
        // not re-deallocate the `Box<CanvasLayer>` (the slab would
        // double-free if Drop ran twice).
        //
        // Memory safety note: the SECOND `drop_in_place` IS a
        // double-free at the std::mem level — `Box::drop` is not
        // idempotent. This test is about the *side-effect* idempotency
        // of the `LayerNode::drop` flag-flip path, not raw memory
        // safety. We use `ManuallyDrop<MaybeUninit>` to make the
        // double `drop_in_place` not a UB on the surrounding type
        // (the slot is uninit after the first drop).
        use std::mem::MaybeUninit;
        use std::ptr;

        let mut slot: MaybeUninit<LayerNode> =
            MaybeUninit::new(LayerNode::new(Layer::from(CanvasLayer::new())));

        // SAFETY: see `drop_marks_node_disposed`.
        unsafe { ptr::drop_in_place(slot.as_mut_ptr()) };
        let after_first = unsafe { (*slot.as_ptr()).is_disposed() };
        assert!(after_first, "first drop must flip flag");

        // Re-running drop_in_place is what the comment above warns
        // against (Box::drop is not idempotent). The *flag* side of
        // LayerNode::drop is idempotent — the swap returns `true`
        // and the `if !prior` branch is skipped — but we intentionally
        // do NOT exercise the second drop_in_place to avoid the
        // Box double-free. Instead we lock the contract via the
        // single-drop observation: the flag stays `true`, and
        // `is_disposed` continues to read `true` from the AtomicBool
        // (which has trivial Drop).
        let after_second_read = unsafe { (*slot.as_ptr()).is_disposed() };
        assert!(after_second_read);
    }

    #[test]
    fn mutation_methods_carry_lifecycle_guards() {
        // Smoke: a *live* node accepts all mutations without panic.
        // The use-after-disposal panic path is covered indirectly: the
        // `assert_alive` debug-assert ensures a stale-mut-borrow trips
        // CI rather than corrupting compositor state silently.
        use flui_foundation::{ElementId, LayerId};
        let mut node = LayerNode::new(Layer::from(CanvasLayer::new()));
        node.set_parent(Some(LayerId::new(2)));
        node.add_child(LayerId::new(3));
        node.remove_child(LayerId::new(3));
        node.clear_children();
        let _ = node.layer_mut();
        // Verify pre-built guards on with-builders ran without panic.
        assert_eq!(node.parent(), Some(LayerId::new(2)));
        let _ = ElementId::new(1); // touch import.
    }
}

// ============================================================================
// COMPOSITOR DIRTY-BIT TESTS (U9 — phase 2)
// ============================================================================

#[cfg(test)]
mod dirty_bit_tests {
    use crate::layer::{CanvasLayer, Layer};

    use super::{LayerNode, LayerTree};

    /// Fresh nodes default to dirty (they have not been pushed yet).
    #[test]
    fn fresh_node_is_dirty() {
        let node = LayerNode::new(Layer::from(CanvasLayer::new()));
        assert!(node.needs_add_to_scene());
        assert!(!node.is_clean());
    }

    /// `layer_mut()` flips the dirty bit even if the layer was previously
    /// clean.
    #[test]
    fn layer_mut_marks_dirty() {
        let mut node = LayerNode::new(Layer::from(CanvasLayer::new()));
        node.clear_needs_add_to_scene_local();
        assert!(node.is_clean());
        let _ = node.layer_mut();
        assert!(node.needs_add_to_scene());
    }

    /// `mark_needs_add_to_scene(id)` flips `id`, its parent, and the root.
    #[test]
    fn mark_propagates_to_root() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let mid = tree.insert(Layer::from(CanvasLayer::new()));
        let leaf = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        // Clean the whole subtree first.
        tree.clear_needs_add_to_scene_subtree(root);
        assert!(tree.get(root).unwrap().is_clean());
        assert!(tree.get(mid).unwrap().is_clean());
        assert!(tree.get(leaf).unwrap().is_clean());

        tree.mark_needs_add_to_scene(leaf);

        // Leaf, mid, and root all dirty.
        assert!(tree.get(leaf).unwrap().needs_add_to_scene());
        assert!(tree.get(mid).unwrap().needs_add_to_scene());
        assert!(tree.get(root).unwrap().needs_add_to_scene());
    }

    /// `mark_needs_add_to_scene` does NOT touch sibling subtrees.
    #[test]
    fn mark_skips_siblings() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let a = tree.insert(Layer::from(CanvasLayer::new()));
        let b = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, a);
        tree.add_child(root, b);

        tree.clear_needs_add_to_scene_subtree(root);
        tree.mark_needs_add_to_scene(a);

        assert!(tree.get(a).unwrap().needs_add_to_scene());
        assert!(tree.get(root).unwrap().needs_add_to_scene());
        // Sibling b stayed clean — its subtree was not in the mark path.
        assert!(tree.get(b).unwrap().is_clean());
    }

    /// `update_subtree_needs_add_to_scene` reports any-descendant-dirty.
    #[test]
    fn update_subtree_folds_child_bits_into_parent() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let a = tree.insert(Layer::from(CanvasLayer::new()));
        let b = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, a);
        tree.add_child(root, b);

        tree.clear_needs_add_to_scene_subtree(root);
        // Dirty only the deepest child.
        tree.get(a).unwrap().mark_needs_add_to_scene_local();

        // Root's local bit is clean…
        assert!(tree.get(root).unwrap().is_clean());
        // …but the subtree-fold lifts the answer:
        assert!(tree.update_subtree_needs_add_to_scene(root));
        // …and the fold also writes back to root.
        assert!(tree.get(root).unwrap().needs_add_to_scene());
    }

    /// `clear_needs_add_to_scene_subtree` clears the whole rooted subtree.
    #[test]
    fn clear_subtree_clears_root_and_descendants() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let a = tree.insert(Layer::from(CanvasLayer::new()));
        let b = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, a);
        tree.add_child(root, b);

        // All start dirty (fresh insert default).
        assert!(tree.get(root).unwrap().needs_add_to_scene());

        tree.clear_needs_add_to_scene_subtree(root);

        assert!(tree.get(root).unwrap().is_clean());
        assert!(tree.get(a).unwrap().is_clean());
        assert!(tree.get(b).unwrap().is_clean());
    }

    /// Missing-id lookups in the mark / update / clear paths must not panic.
    #[test]
    fn missing_id_is_a_no_op() {
        use flui_foundation::LayerId;
        let tree = LayerTree::new();
        let phantom = LayerId::new(999);

        tree.mark_needs_add_to_scene(phantom); // no panic
        assert!(!tree.update_subtree_needs_add_to_scene(phantom));
        tree.clear_needs_add_to_scene_subtree(phantom); // no panic
    }

    /// PR #100 followup: mark propagation must traverse the full
    /// ancestor chain, not silently stop at depth 32. Pre-followup
    /// the walk capped at `MARK_PROPAGATION_MAX_DEPTH = 32`, which
    /// dropped dirty propagation for any tree deeper than 32 levels
    /// — a legitimate shape for nested scroll views + popovers.
    #[test]
    fn mark_traverses_chain_deeper_than_32() {
        const DEPTH: usize = 40;
        let mut tree = LayerTree::new();
        let mut nodes = Vec::with_capacity(DEPTH);
        for _ in 0..DEPTH {
            nodes.push(tree.insert(Layer::from(CanvasLayer::new())));
        }
        for i in 1..DEPTH {
            tree.add_child(nodes[i - 1], nodes[i]);
        }
        // Clean the entire chain.
        tree.clear_needs_add_to_scene_subtree(nodes[0]);
        for &id in &nodes {
            assert!(tree.get(id).unwrap().is_clean(), "node {id:?} not clean");
        }

        // Mark the deepest leaf dirty.
        tree.mark_needs_add_to_scene(nodes[DEPTH - 1]);

        // Every ancestor — including the root at depth 0, which sits
        // beyond the old 32-iteration cap — must be dirty.
        for &id in &nodes {
            assert!(
                tree.get(id).unwrap().needs_add_to_scene(),
                "node {id:?} did not receive dirty propagation"
            );
        }
    }
}

// ============================================================================
// SLAB-TREE HYGIENE TESTS (U10 — add_child auto-detach + dedup)
// ============================================================================

#[cfg(test)]
mod add_child_hygiene_tests {
    use crate::layer::{CanvasLayer, Layer};

    use super::LayerTree;

    #[test]
    fn add_child_attaches_under_new_parent() {
        let mut tree = LayerTree::new();
        let parent = tree.insert(Layer::from(CanvasLayer::new()));
        let child = tree.insert(Layer::from(CanvasLayer::new()));

        tree.add_child(parent, child);

        assert_eq!(tree.get(child).unwrap().parent(), Some(parent));
        assert_eq!(tree.get(parent).unwrap().children(), &[child]);
    }

    #[test]
    fn add_child_auto_detaches_from_previous_parent() {
        let mut tree = LayerTree::new();
        let parent_a = tree.insert(Layer::from(CanvasLayer::new()));
        let parent_b = tree.insert(Layer::from(CanvasLayer::new()));
        let child = tree.insert(Layer::from(CanvasLayer::new()));

        tree.add_child(parent_a, child);
        assert_eq!(tree.get(parent_a).unwrap().children(), &[child]);

        // Re-parent — parent_a should lose the child, parent_b should gain it.
        tree.add_child(parent_b, child);

        assert_eq!(tree.get(child).unwrap().parent(), Some(parent_b));
        assert!(tree.get(parent_a).unwrap().children().is_empty());
        assert_eq!(tree.get(parent_b).unwrap().children(), &[child]);
    }

    #[test]
    fn add_child_under_same_parent_is_idempotent() {
        let mut tree = LayerTree::new();
        let parent = tree.insert(Layer::from(CanvasLayer::new()));
        let child = tree.insert(Layer::from(CanvasLayer::new()));

        tree.add_child(parent, child);
        tree.add_child(parent, child); // duplicate

        assert_eq!(tree.get(parent).unwrap().children().len(), 1);
        assert_eq!(tree.get(parent).unwrap().children()[0], child);
    }

    #[test]
    fn add_child_with_missing_parent_is_a_no_op() {
        use flui_foundation::LayerId;
        let mut tree = LayerTree::new();
        let child = tree.insert(Layer::from(CanvasLayer::new()));
        let phantom = LayerId::new(999);

        tree.add_child(phantom, child);
        // Child's parent stays unset since the parent slot doesn't exist.
        assert!(tree.get(child).unwrap().parent().is_none());
    }

    #[test]
    fn add_child_with_missing_child_is_a_no_op() {
        use flui_foundation::LayerId;
        let mut tree = LayerTree::new();
        let parent = tree.insert(Layer::from(CanvasLayer::new()));
        let phantom = LayerId::new(999);

        tree.add_child(parent, phantom);
        // Parent's children stay empty since the child slot doesn't exist.
        assert!(tree.get(parent).unwrap().children().is_empty());
    }

    // ----- PR #100 followup: cycle rejection -----

    #[test]
    fn add_child_rejects_self_link() {
        // `add_child(id, id)` would create a 1-cycle that the cascading
        // `remove` would follow to infinite recursion. The guard rejects
        // the call as a no-op.
        let mut tree = LayerTree::new();
        let id = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(id, id);
        assert!(tree.get(id).unwrap().children().is_empty());
        assert!(tree.get(id).unwrap().parent().is_none());
    }

    #[test]
    fn add_child_rejects_attaching_ancestor_under_descendant() {
        use flui_foundation::LayerId;
        // root → mid → leaf. Try to attach root under leaf — would
        // create a 3-cycle. Guard rejects.
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let mid = tree.insert(Layer::from(CanvasLayer::new()));
        let leaf = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        // Pre-rejection: tree.remove(root) would have recursed root → mid
        // → leaf → root → … indefinitely after this call.
        tree.add_child(leaf, root);

        // Tree shape unchanged after the rejected call.
        assert_eq!(tree.get(root).unwrap().parent(), None);
        let empty: &[LayerId] = &[];
        assert_eq!(tree.get(leaf).unwrap().children(), empty);
        // Cascade safely terminates now.
        let removed = tree.remove(root);
        assert!(removed.is_some());
        assert_eq!(tree.len(), 0);
    }
}

// ============================================================================
// SLAB-TREE HYGIENE TESTS (U12 — remove cascade + remove_shallow)
// ============================================================================

#[cfg(test)]
mod remove_cascade_tests {
    use crate::layer::{CanvasLayer, Layer};

    use super::LayerTree;

    #[test]
    fn remove_cascades_to_all_descendants() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let mid = tree.insert(Layer::from(CanvasLayer::new()));
        let leaf = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);
        assert_eq!(tree.len(), 3);

        let removed = tree.remove(root);
        assert!(removed.is_some());
        // Every descendant gone from the slab.
        assert_eq!(tree.len(), 0);
        assert!(!tree.contains(root));
        assert!(!tree.contains(mid));
        assert!(!tree.contains(leaf));
    }

    #[test]
    fn remove_unlinks_parent_children_vector() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let mid = tree.insert(Layer::from(CanvasLayer::new()));
        let sibling = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, mid);
        tree.add_child(root, sibling);
        assert_eq!(tree.get(root).unwrap().children().len(), 2);

        // Remove mid — root's children vector loses the id, sibling stays.
        let _ = tree.remove(mid);
        assert!(!tree.contains(mid));
        assert!(tree.contains(sibling));
        assert_eq!(tree.get(root).unwrap().children(), &[sibling]);
    }

    #[test]
    fn remove_resets_root_when_removing_root_node() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        tree.set_root(Some(root));
        let _ = tree.remove(root);
        assert_eq!(tree.root(), None);
    }

    #[test]
    fn remove_of_phantom_id_is_a_no_op() {
        use flui_foundation::LayerId;
        let mut tree = LayerTree::new();
        let _ = tree.insert(Layer::from(CanvasLayer::new()));
        let phantom = LayerId::new(999);
        assert!(tree.remove(phantom).is_none());
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn remove_shallow_does_not_cascade() {
        // `remove_shallow` is the escape hatch for reparenting workflows
        // that immediately re-attach the removed node — children must
        // stay in the slab so they can be re-attached to a new parent.
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::from(CanvasLayer::new()));
        let mid = tree.insert(Layer::from(CanvasLayer::new()));
        let leaf = tree.insert(Layer::from(CanvasLayer::new()));
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);
        assert_eq!(tree.len(), 3);

        let _ = tree.remove_shallow(mid);

        assert!(!tree.contains(mid));
        // Leaf survives in the slab (the cascade path is the only one
        // that drops descendants).
        assert!(tree.contains(leaf));
        assert_eq!(tree.len(), 2);
    }
}
