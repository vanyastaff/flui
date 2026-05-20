//! Flutter-style boundary-aware dirty propagation.
//!
//! This file contains:
//! - The `RenderDirtyPropagation` trait — minimal tree operations needed by
//!   boundary-aware propagation
//! - `RenderState<P>` propagation methods (`mark_needs_layout`,
//!   `mark_parent_needs_layout`, `mark_needs_paint`, `mark_needs_compositing`,
//!   `mark_needs_compositing_bits_update`) and their iterative helpers

use flui_foundation::ElementId;

use super::RenderState;
use crate::protocol::Protocol;
use crate::storage::flags::RenderFlags;

// ============================================================================
// TREE OPERATIONS TRAIT
// ============================================================================

/// Minimal trait for tree operations needed by Flutter-style dirty propagation.
///
/// This trait provides the tree operations needed for boundary-aware dirty
/// propagation following Flutter's exact `markNeedsLayout()` and
/// `markNeedsPaint()` semantics.
///
/// # Why This Trait?
///
/// - Decouples render_state.rs from tree implementation details
/// - Allows different tree implementations (HashMap, Arena, etc.)
/// - Testable with mock implementations
/// - Follows dependency inversion principle
///
/// # Note on Naming
///
/// This trait is intentionally named differently from
/// `flui_tree::DirtyTracking` because they serve different purposes:
/// - `flui_tree::DirtyTracking` - Generic per-element flag operations
/// - `RenderDirtyPropagation` - Flutter-style boundary-aware propagation
pub trait RenderDirtyPropagation {
    /// Gets the parent element ID, if any.
    fn parent(&self, id: ElementId) -> Option<ElementId>;

    /// Gets the render state for an element, if it exists.
    ///
    /// Returns None if:
    /// - Element doesn't exist
    /// - Element is not a render element
    /// - Protocol doesn't match
    fn get_render_state<P: Protocol>(&self, id: ElementId) -> Option<&RenderState<P>>;

    /// Registers an element that needs layout in the next frame.
    ///
    /// This is called when a relayout boundary is dirty. The pipeline
    /// owner will process all registered elements in the next frame.
    fn register_needs_layout(&mut self, id: ElementId);

    /// Registers an element that needs paint in the next frame.
    ///
    /// This is called when a repaint boundary is dirty. The pipeline
    /// owner will process all registered elements in the next frame.
    fn register_needs_paint(&mut self, id: ElementId);

    /// Registers an element that needs compositing bits update.
    ///
    /// This is called when a node's compositing status changes. The pipeline
    /// owner will process all registered elements during the compositing phase.
    fn register_needs_compositing_bits_update(&mut self, id: ElementId);

    /// Gets the RenderObject for an element to check `is_repaint_boundary`.
    ///
    /// Returns true if the element is a repaint boundary.
    fn is_repaint_boundary(&self, id: ElementId) -> bool;

    /// Gets the previous repaint boundary status (for transition detection).
    ///
    /// Returns the cached `_wasRepaintBoundary` value.
    fn was_repaint_boundary(&self, id: ElementId) -> bool;
}

// ============================================================================
// FLUTTER-STYLE DIRTY TRACKING
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Marks this render object as needing layout (Flutter-compliant).
    ///
    /// This method implements Flutter's exact `markNeedsLayout()` semantics:
    ///
    /// 1. **Early return if already dirty** - Optimization to avoid redundant
    ///    work
    /// 2. **Mark self as needing layout and paint** - Layout changes affect
    ///    paint
    /// 3. **Smart propagation**:
    ///    - If NOT a relayout boundary → propagate to parent recursively
    ///    - If IS a relayout boundary → register with pipeline owner for next
    ///      frame
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// void markNeedsLayout() {
    ///   if (_needsLayout) return;  // Early return
    ///   _needsLayout = true;
    ///   if (_relayoutBoundary != null) {
    ///     // We are our own relayout boundary
    ///     owner.nodesNeedingLayout.add(this);
    ///   } else {
    ///     // Propagate to parent
    ///     parent.markNeedsLayout();
    ///   }
    /// }
    /// ```
    ///
    /// # Performance
    ///
    /// - Best case: O(1) if already dirty (early return)
    /// - Typical case: O(log n) propagation to nearest boundary
    /// - Worst case: O(height) if no boundaries (rare in real apps)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Mark self dirty, propagates up to first relayout boundary
    /// state.mark_needs_layout(element_id, tree);
    ///
    /// // Subsequent calls are no-ops (early return optimization)
    /// state.mark_needs_layout(element_id, tree); // Fast path: returns immediately
    /// ```
    ///
    /// # When to call
    ///
    /// Call this when:
    /// - Configuration changes (padding, alignment, etc.)
    /// - Child is added or removed
    /// - Constraints change
    /// - Any property that affects layout
    ///
    /// DO NOT call during:
    /// - Layout phase (will assert in debug builds)
    /// - Paint phase (will assert in debug builds)
    pub fn mark_needs_layout(&self, element_id: ElementId, tree: &mut impl RenderDirtyPropagation) {
        // Flutter optimization: early return if already dirty
        if self.flags.needs_layout() {
            return;
        }

        // Mark self dirty (layout implies paint)
        self.flags.mark_needs_layout();

        // Smart propagation based on boundary status
        if self.is_relayout_boundary() {
            // We are a relayout boundary - stop propagation here
            // Register with pipeline owner for next frame processing
            tree.register_needs_layout(element_id);
        } else {
            // Not a boundary - propagate to parent
            // Note: We get parent_id first, then mark it dirty in a separate call
            // to satisfy the borrow checker (can't borrow tree twice).
            let parent_id = tree.parent(element_id);
            if let Some(parent_id) = parent_id {
                // Check if parent exists and mark it (the parent's mark_needs_layout
                // will do its own recursive propagation)
                if let Some(parent_state) = tree.get_render_state::<P>(parent_id) {
                    parent_state.flags.mark_needs_layout();
                    // Need to register or continue propagation for parent
                    if parent_state.is_relayout_boundary() {
                        tree.register_needs_layout(parent_id);
                    } else {
                        // Continue propagation iteratively instead of recursively
                        // to avoid borrow checker issues
                        let mut current = tree.parent(parent_id);
                        while let Some(curr_id) = current {
                            if let Some(state) = tree.get_render_state::<P>(curr_id) {
                                if state.flags.needs_layout() {
                                    break; // Already dirty, stop
                                }
                                state.flags.mark_needs_layout();
                                if state.is_relayout_boundary() {
                                    tree.register_needs_layout(curr_id);
                                    break;
                                }
                            } else {
                                break;
                            }
                            current = tree.parent(curr_id);
                        }
                    }
                }
            }
        }
    }

    /// Marks this render object's parent as needing layout (for intrinsic
    /// changes).
    ///
    /// This is a specialized version of `markNeedsLayout()` that ALWAYS
    /// propagates to the parent, even if this element is a relayout
    /// boundary. This is used when:
    ///
    /// - Intrinsic size changes (minIntrinsicWidth, maxIntrinsicHeight, etc.)
    /// - Baseline position changes
    /// - Any property the parent's layout depends on changes
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @protected
    /// void markParentNeedsLayout() {
    ///   _needsLayout = true;
    ///   assert(this.parent != null);
    ///   parent.markNeedsLayout();  // Always propagate!
    /// }
    /// ```
    ///
    /// # Why ignore relayout boundary?
    ///
    /// Even if this element is a relayout boundary, changes to intrinsic size
    /// affect the parent's layout decisions. The parent needs to relayout to
    /// potentially query new intrinsics and adjust accordingly.
    ///
    /// # Performance
    ///
    /// - Always O(log n) to nearest parent's relayout boundary
    /// - More expensive than `mark_needs_layout()` because it ignores
    ///   boundaries
    /// - Use sparingly - only when parent truly needs notification
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn set_text(&mut self, text: String, element_id: ElementId, tree: &mut impl Tree) {
    ///         self.text = text;
    ///
    ///         // Intrinsic size changed - parent needs to know!
    ///         if let Some(state) = tree.get_render_state(element_id) {
    ///             state.mark_parent_needs_layout(element_id, tree);
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// # When to call
    ///
    /// Call this when:
    /// - `intrinsic_width()` result would change
    /// - `intrinsic_height()` result would change
    /// - `baseline_offset()` result would change
    /// - Parent used any of these values in its last layout
    ///
    /// DO NOT call when:
    /// - Only size changed (use `mark_needs_layout()` instead)
    /// - Parent doesn't use intrinsics (optimization)
    pub fn mark_parent_needs_layout(
        &self,
        element_id: ElementId,
        tree: &mut impl RenderDirtyPropagation,
    ) {
        // Mark self dirty
        self.flags.mark_needs_layout();

        // ALWAYS propagate to parent (ignore relayout boundary)
        // Use iterative approach to avoid borrow checker issues
        let parent_id = tree.parent(element_id);
        if let Some(parent_id) = parent_id {
            // Start propagation from parent using iterative approach
            let mut current = Some(parent_id);
            while let Some(curr_id) = current {
                if let Some(state) = tree.get_render_state::<P>(curr_id) {
                    if state.flags.needs_layout() {
                        break; // Already dirty, stop
                    }
                    state.flags.mark_needs_layout();
                    if state.is_relayout_boundary() {
                        tree.register_needs_layout(curr_id);
                        break;
                    }
                } else {
                    break;
                }
                current = tree.parent(curr_id);
            }
        }
    }

    /// Marks this render object as needing paint (Flutter-compliant).
    ///
    /// This method implements Flutter's exact `markNeedsPaint()` semantics:
    ///
    /// 1. **Early return if already dirty** - Optimization
    /// 2. **Mark self as needing paint** - Paint flag only (layout stays valid)
    /// 3. **Smart propagation**:
    ///    - If NOT a repaint boundary → propagate to parent
    ///    - If IS a repaint boundary → register with pipeline owner
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// void markNeedsPaint() {
    ///   if (_needsPaint) return;  // Early return
    ///   _needsPaint = true;
    ///   if (isRepaintBoundary) {
    ///     owner.nodesNeedingPaint.add(this);
    ///   } else {
    ///     parent.markNeedsPaint();
    ///   }
    /// }
    /// ```
    ///
    /// # Performance
    ///
    /// - Best case: O(1) if already dirty
    /// - Typical case: O(log n) to nearest repaint boundary
    /// - Faster than layout propagation (more boundaries in typical trees)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderColoredBox {
    ///     fn set_color(&mut self, color: Color, element_id: ElementId, tree: &mut impl Tree) {
    ///         self.color = color;
    ///
    ///         // Color changed - only repaint needed (layout unaffected)
    ///         if let Some(state) = tree.get_render_state(element_id) {
    ///             state.mark_needs_paint(element_id, tree);
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// # When to call
    ///
    /// Call this when:
    /// - Visual properties change (color, opacity, decoration)
    /// - Transform changes
    /// - Clipping changes
    /// - Any visual change that doesn't affect layout
    ///
    /// DO NOT call when:
    /// - Layout changes (use `mark_needs_layout()` which marks paint too)
    /// - During paint phase itself
    pub fn mark_needs_paint(&self, element_id: ElementId, tree: &mut impl RenderDirtyPropagation) {
        // Flutter optimization: early return if already dirty
        if self.flags.needs_paint() {
            return;
        }

        // Mark self dirty
        self.flags.mark_needs_paint();

        // Smart propagation based on boundary status
        if self.is_repaint_boundary() {
            // We are a repaint boundary - stop propagation here
            tree.register_needs_paint(element_id);
        } else {
            // Not a boundary - propagate to parent using iterative approach
            // to avoid borrow checker issues with recursive calls
            let parent_id = tree.parent(element_id);
            if let Some(parent_id) = parent_id {
                let mut current = Some(parent_id);
                while let Some(curr_id) = current {
                    if let Some(state) = tree.get_render_state::<P>(curr_id) {
                        if state.flags.needs_paint() {
                            break; // Already dirty, stop
                        }
                        state.flags.mark_needs_paint();
                        if state.is_repaint_boundary() {
                            tree.register_needs_paint(curr_id);
                            break;
                        }
                    } else {
                        break;
                    }
                    current = tree.parent(curr_id);
                }
            }
        }
    }

    /// Marks compositing as dirty (simple flag set).
    ///
    /// Called when layer configuration changes (rarely used directly).
    /// For proper propagation, use `mark_needs_compositing_bits_update`.
    #[inline]
    pub fn mark_needs_compositing(&self) {
        self.flags.set(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Marks this render object as needing compositing bits update
    /// (Flutter-compliant).
    ///
    /// This method implements Flutter's exact
    /// `markNeedsCompositingBitsUpdate()` semantics:
    ///
    /// 1. **Early return if already dirty** - Optimization to avoid redundant
    ///    work
    /// 2. **Mark self as needing compositing bits update**
    /// 3. **Smart propagation**:
    ///    - Propagates to parent unless parent is a repaint boundary
    ///    - Stops at repaint boundary transitions
    ///    - Registers with pipeline owner when propagation stops
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// void markNeedsCompositingBitsUpdate() {
    ///   if (_needsCompositingBitsUpdate) return;
    ///   _needsCompositingBitsUpdate = true;
    ///   if (parent is RenderObject) {
    ///     final RenderObject parent = this.parent!;
    ///     if (parent._needsCompositingBitsUpdate) return;
    ///     if ((!_wasRepaintBoundary || !isRepaintBoundary) &&
    ///         !parent.isRepaintBoundary) {
    ///       parent.markNeedsCompositingBitsUpdate();
    ///       return;
    ///     }
    ///   }
    ///   _nodesNeedingCompositingBitsUpdate.add(this);
    /// }
    /// ```
    ///
    /// # When to call
    ///
    /// Call this when:
    /// - `alwaysNeedsCompositing` getter value changes
    /// - Child is added/removed that might affect compositing
    /// - Repaint boundary status changes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // When opacity changes to require compositing layer
    /// if self.opacity < 1.0 && !self.had_compositing_layer {
    ///     state.mark_needs_compositing_bits_update(element_id, tree);
    /// }
    /// ```
    pub fn mark_needs_compositing_bits_update(
        &self,
        element_id: ElementId,
        tree: &mut impl RenderDirtyPropagation,
    ) {
        // Early return if already marked
        if self.flags.needs_compositing() {
            return;
        }

        // Mark self as needing compositing bits update
        self.flags.set(RenderFlags::NEEDS_COMPOSITING);

        // Check parent for propagation
        if let Some(parent_id) = tree.parent(element_id) {
            // Check if parent already marked
            if let Some(parent_state) = tree.get_render_state::<P>(parent_id)
                && parent_state.flags.needs_compositing()
            {
                return; // Parent already dirty, no need to propagate
            }

            // Determine if we should propagate or stop
            let was_repaint_boundary = tree.was_repaint_boundary(element_id);
            let is_repaint_boundary = tree.is_repaint_boundary(element_id);
            let parent_is_repaint_boundary = tree.is_repaint_boundary(parent_id);

            // Flutter logic: propagate unless:
            // - Both old and new status are repaint boundary (transition)
            // - Parent is a repaint boundary
            let should_propagate =
                (!was_repaint_boundary || !is_repaint_boundary) && !parent_is_repaint_boundary;

            if should_propagate {
                // Propagate to parent iteratively
                let mut current = Some(parent_id);
                while let Some(curr_id) = current {
                    if let Some(state) = tree.get_render_state::<P>(curr_id) {
                        if state.flags.needs_compositing() {
                            break; // Already dirty, stop
                        }
                        state.flags.set(RenderFlags::NEEDS_COMPOSITING);

                        // Check if we should continue propagating
                        let curr_is_repaint_boundary = tree.is_repaint_boundary(curr_id);
                        if curr_is_repaint_boundary {
                            tree.register_needs_compositing_bits_update(curr_id);
                            break;
                        }

                        // Check parent
                        if let Some(parent_id) = tree.parent(curr_id) {
                            if tree.is_repaint_boundary(parent_id) {
                                tree.register_needs_compositing_bits_update(curr_id);
                                break;
                            }
                        } else {
                            // No parent, register self
                            tree.register_needs_compositing_bits_update(curr_id);
                            break;
                        }
                    } else {
                        break;
                    }
                    current = tree.parent(curr_id);
                }
            } else {
                // Stop propagation here, register self
                tree.register_needs_compositing_bits_update(element_id);
            }
        } else {
            // No parent (root), register self
            tree.register_needs_compositing_bits_update(element_id);
        }
    }
}
