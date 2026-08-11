//! Linear, idle-only render-subtree relocation.

use std::{collections::hash_map::Entry, fmt, rc::Rc};

use flui_foundation::RenderId;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::pipeline::{RenderInvalidationHandle, handle::AttachmentEpoch, phase::Idle};

use super::PipelineOwner;

/// Private allocation identity shared by one owner and its relocation tokens.
#[derive(Debug)]
pub(super) struct RelocationOwnerSeal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetachedRenderNode {
    /// Generational id, so a freed-and-reused slab slot resolves to `None`
    /// rather than to whatever now occupies the index.
    render_id: RenderId,
    root_index: usize,
    attachment_epoch: AttachmentEpoch,
}

/// Exclusive capability for a detached batch of render subtrees.
///
/// The token is opaque and intentionally does not implement `Clone`. It must
/// be consumed exactly once by either
/// [`PipelineOwner::attach_render_subtrees`] or
/// [`PipelineOwner::release_detached_render_subtrees_for_finalization`].
/// Both consumers return ownership of the token when validation fails.
///
/// Linear ownership is structural; cloning is unavailable:
///
/// ```compile_fail
/// use flui_rendering::pipeline::DetachedRenderSubtrees;
/// fn duplicate(token: DetachedRenderSubtrees) {
///     let _duplicate = token.clone();
/// }
/// ```
#[must_use = "a detached render batch must be reattached or released for finalization"]
pub struct DetachedRenderSubtrees {
    owner_seal: Rc<RelocationOwnerSeal>,
    roots: Vec<RenderId>,
    nodes: Vec<DetachedRenderNode>,
    /// Set when one of `roots` was the owner's `root_id` at detach time.
    ///
    /// Detaching the owner root has no parent edge to drop, so `root_id` would
    /// otherwise keep addressing a node whose attachment interval is closed —
    /// and the paint and semantics walks both start from `root_id` without
    /// consulting that interval. Detach clears it and records it here;
    /// reattach restores it. Releasing the token for finalization
    /// deliberately does not, since the canonical `remove_render_object` path
    /// clears `root_id` for a removed root itself.
    vacated_owner_root: Option<RenderId>,
}

impl DetachedRenderSubtrees {
    /// Number of disjoint subtree roots represented by this token.
    #[must_use]
    pub fn root_count(&self) -> usize {
        self.roots.len()
    }

    /// Number of render nodes represented by this token.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

impl fmt::Debug for DetachedRenderSubtrees {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DetachedRenderSubtrees")
            .field("root_count", &self.root_count())
            .field("node_count", &self.node_count())
            .finish_non_exhaustive()
    }
}

/// Why a render-subtree batch could not be detached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DetachRenderSubtreesError {
    /// A relocation batch must contain at least one root.
    #[error("a render-subtree relocation batch cannot be empty")]
    Empty,
    /// A requested root or descendant is not live in this owner.
    #[error("render node {render_id:?} is not live in this pipeline owner")]
    NodeNotFound {
        /// Missing or stale node.
        render_id: RenderId,
    },
    /// The same root was supplied more than once.
    #[error("render relocation root {root:?} was supplied more than once")]
    DuplicateRoot {
        /// Root that appears at least twice in the request.
        root: RenderId,
    },
    /// One requested root lies inside another requested root's subtree.
    #[error("render relocation root {descendant:?} lies inside the subtree of {ancestor:?}")]
    OverlappingRoots {
        /// Root whose subtree contains the other.
        ancestor: RenderId,
        /// Root found inside `ancestor`'s subtree.
        descendant: RenderId,
    },
    /// Every node must begin the operation in an attached interval.
    #[error("render node {render_id:?} is already detached")]
    AlreadyDetached {
        /// Node without a live attachment interval.
        render_id: RenderId,
    },
}

/// Why a detached token could not be attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AttachRenderSubtreesError {
    /// The token was created by a different pipeline owner.
    #[error("detached render-subtree token belongs to a different pipeline owner")]
    WrongOwner,
    /// A recorded node was removed or replaced after detachment.
    #[error("detached render node {render_id:?} is no longer live")]
    NodeNotFound {
        /// Missing or stale node.
        render_id: RenderId,
    },
    /// A recorded node no longer has the detached interval captured by the token.
    #[error("detached render node {render_id:?} changed attachment interval")]
    AttachmentChanged {
        /// Node whose attachment interval changed.
        render_id: RenderId,
    },
    /// Descendant membership changed while the batch was detached.
    #[error("detached render-subtree topology changed")]
    TopologyChanged,
}

/// Failed attach together with the still-live linear token.
#[derive(Debug)]
#[non_exhaustive]
pub struct AttachRenderSubtreesFailure {
    kind: AttachRenderSubtreesError,
    token: DetachedRenderSubtrees,
}

impl AttachRenderSubtreesFailure {
    /// The validation error that prevented mutation.
    #[must_use]
    pub fn kind(&self) -> &AttachRenderSubtreesError {
        &self.kind
    }

    /// Recovers the token so the caller can retry or release it.
    pub fn into_token(self) -> DetachedRenderSubtrees {
        self.token
    }

    /// Splits the failure into its error and recoverable token.
    pub fn into_parts(self) -> (AttachRenderSubtreesError, DetachedRenderSubtrees) {
        (self.kind, self.token)
    }
}

impl fmt::Display for AttachRenderSubtreesFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(formatter)
    }
}

impl std::error::Error for AttachRenderSubtreesFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

/// Why a detached token could not be released to ordinary element finalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReleaseDetachedRenderSubtreesError {
    /// The token was created by a different pipeline owner.
    #[error("detached render-subtree token belongs to a different pipeline owner")]
    WrongOwner,
    /// A recorded node was removed or replaced before finalization began.
    #[error("detached render node {render_id:?} is no longer live")]
    NodeNotFound {
        /// Missing or stale node.
        render_id: RenderId,
    },
    /// A recorded node no longer has the detached interval captured by the token.
    #[error("detached render node {render_id:?} changed attachment interval")]
    AttachmentChanged {
        /// Node whose attachment interval changed.
        render_id: RenderId,
    },
    /// Descendant membership changed while the batch was detached.
    #[error("detached render-subtree topology changed")]
    TopologyChanged,
}

/// Failed finalization release together with the still-live linear token.
#[derive(Debug)]
#[non_exhaustive]
pub struct ReleaseDetachedRenderSubtreesFailure {
    kind: ReleaseDetachedRenderSubtreesError,
    token: DetachedRenderSubtrees,
}

impl ReleaseDetachedRenderSubtreesFailure {
    /// The validation error that prevented release.
    #[must_use]
    pub fn kind(&self) -> &ReleaseDetachedRenderSubtreesError {
        &self.kind
    }

    /// Recovers the token so the caller can retry or reattach it.
    pub fn into_token(self) -> DetachedRenderSubtrees {
        self.token
    }

    /// Splits the failure into its error and recoverable token.
    pub fn into_parts(self) -> (ReleaseDetachedRenderSubtreesError, DetachedRenderSubtrees) {
        (self.kind, self.token)
    }
}

impl fmt::Display for ReleaseDetachedRenderSubtreesFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(formatter)
    }
}

impl std::error::Error for ReleaseDetachedRenderSubtreesFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

impl PipelineOwner<Idle> {
    /// Detaches disjoint live render subtrees as one linear relocation batch.
    ///
    /// Validation completes before any edge, parent data, attachment interval,
    /// dirty queue, or poison record is changed. On success the opaque token
    /// owns the right to either reattach the exact batch or release it for
    /// ordinary element finalization.
    ///
    /// ```
    /// use flui_foundation::RenderId;
    /// use flui_rendering::pipeline::{DetachRenderSubtreesError, PipelineOwner};
    ///
    /// let mut owner = PipelineOwner::new();
    /// let result = owner.detach_render_subtrees(&[RenderId::new(1)]);
    /// assert!(matches!(result, Err(DetachRenderSubtreesError::NodeNotFound { .. })));
    /// ```
    ///
    /// Detachment is unavailable during a frame phase:
    ///
    /// ```compile_fail
    /// use flui_foundation::RenderId;
    /// use flui_rendering::pipeline::{Layout, PipelineOwner};
    /// fn cannot_detach_during_layout(owner: &mut PipelineOwner<Layout>) {
    ///     owner.detach_render_subtrees(&[RenderId::new(1)]);
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// [`DetachRenderSubtreesError::Empty`] for a batch with no roots;
    /// [`DetachRenderSubtreesError::DuplicateRoot`] when one root is supplied
    /// twice; [`DetachRenderSubtreesError::OverlappingRoots`] when one root
    /// lies inside another's subtree; [`DetachRenderSubtreesError::NodeNotFound`]
    /// when a root or descendant is not live in this owner; and
    /// [`DetachRenderSubtreesError::AlreadyDetached`] when a node does not begin
    /// in an attached interval.
    ///
    /// Each is decided during preflight, so a failure leaves edges, parent data,
    /// attachment intervals, dirty queues, and poison records exactly as it
    /// found them. That guarantee covers rejection only: a panic raised part-way
    /// through the mutation is fatal, and no unwind rollback is attempted or
    /// promised.
    pub fn detach_render_subtrees(
        &mut self,
        root_ids: &[RenderId],
    ) -> Result<DetachedRenderSubtrees, DetachRenderSubtreesError> {
        let nodes = self.preflight_detach(root_ids)?;
        let membership: FxHashSet<_> = nodes.iter().map(|record| record.render_id).collect();

        for &root_id in root_ids {
            if let Some(parent_id) = self.render_tree.parent(root_id) {
                self.render_tree.drop_child(parent_id, root_id);
                self.note_render_child_membership_changed(parent_id);
            }
            if let Some(root) = self.render_tree.get_mut(root_id) {
                let _ = root.clear_parent_data();
            }
        }

        for record in &nodes {
            let detached = self
                .render_tree
                .get_mut(record.render_id)
                .expect("BUG: detach preflight guaranteed every recorded node remains live")
                .detach();
            debug_assert!(detached, "detach preflight guaranteed an attached interval");
        }
        self.scheduler.evict(&membership);
        self.layout_poison.evict(&membership);

        // The owner root has no parent edge, so nothing above cleared it.
        // Leaving `root_id` on a node whose interval just closed would let the
        // paint and semantics walks — both of which start from `root_id`
        // without consulting the interval — enter a detached object.
        let vacated_owner_root = self
            .root_id
            .filter(|root_id| root_ids.contains(root_id))
            .inspect(|_| self.root_id = None);

        Ok(DetachedRenderSubtrees {
            owner_seal: Rc::clone(&self.relocation_owner_seal),
            roots: root_ids.to_vec(),
            nodes,
            vacated_owner_root,
        })
    }

    /// Reattaches a detached batch after the caller establishes destination edges.
    ///
    /// The token is consumed on success. A validation failure performs no
    /// mutation and returns an [`AttachRenderSubtreesFailure`] that still owns
    /// the token, allowing a corrected retry or terminal release.
    ///
    /// ```
    /// use flui_rendering::pipeline::{
    ///     AttachRenderSubtreesFailure, DetachedRenderSubtrees, PipelineOwner,
    /// };
    ///
    /// fn reattach(
    ///     owner: &mut PipelineOwner,
    ///     token: DetachedRenderSubtrees,
    /// ) -> Result<(), AttachRenderSubtreesFailure> {
    ///     owner.attach_render_subtrees(token)
    /// }
    /// ```
    ///
    /// This method exists only on `PipelineOwner<Idle>`:
    ///
    /// ```compile_fail
    /// use flui_rendering::pipeline::{DetachedRenderSubtrees, Layout, PipelineOwner};
    /// fn cannot_attach_during_layout(
    ///     owner: &mut PipelineOwner<Layout>,
    ///     token: DetachedRenderSubtrees,
    /// ) {
    ///     owner.attach_render_subtrees(token);
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// [`AttachRenderSubtreesError::WrongOwner`] when the token was minted by a
    /// different pipeline owner — checked by allocation identity, so it still
    /// rejects a token whose recorded [`RenderId`]s all happen to match live
    /// nodes here; [`AttachRenderSubtreesError::NodeNotFound`] when a recorded
    /// node was removed or its slot reused;
    /// [`AttachRenderSubtreesError::AttachmentChanged`] when a node no longer
    /// sits in the detached interval the token captured; and
    /// [`AttachRenderSubtreesError::TopologyChanged`] when descendant membership
    /// or order drifted while the batch was detached.
    ///
    /// The failure owns the token, so a rejected call costs nothing: recover it
    /// with [`AttachRenderSubtreesFailure::into_token`] and either retry after
    /// repairing the destination or release it for finalization. Validation
    /// completes before the first interval reopens; a panic part-way through the
    /// attach loop is fatal, and the intervals already reopened are not undone.
    pub fn attach_render_subtrees(
        &mut self,
        token: DetachedRenderSubtrees,
    ) -> Result<(), AttachRenderSubtreesFailure> {
        if let Err(kind) = self.validate_token_for_attach(&token) {
            return Err(AttachRenderSubtreesFailure { kind, token });
        }

        let next_epochs: Vec<_> = token
            .nodes
            .iter()
            .map(|record| record.attachment_epoch.next())
            .collect();
        for (record, epoch) in token.nodes.iter().zip(next_epochs) {
            let handle =
                RenderInvalidationHandle::new(self.dirty_sender.clone(), record.render_id, epoch);
            let attached = self
                .render_tree
                .get_mut(record.render_id)
                .expect("BUG: attach preflight guaranteed every recorded node remains live")
                .attach(epoch, handle);
            debug_assert!(
                attached,
                "attach preflight guaranteed the next attachment epoch"
            );
        }

        // Restore the root slot this batch vacated, before the dirty marks
        // below — a re-rooted owner must be able to schedule from it.
        if let Some(owner_root) = token.vacated_owner_root {
            self.root_id = Some(owner_root);
        }

        for &root_id in &token.roots {
            if let Some(root) = self.render_tree.get(root_id) {
                root.clear_needs_layout();
                root.clear_needs_compositing_bits_update();
                root.clear_needs_paint();
                root.clear_needs_semantics();
            }
            self.mark_needs_layout(root_id);
            self.mark_needs_compositing_bits_update(root_id);
            self.mark_needs_paint(root_id);
            self.mark_needs_semantics(root_id);
        }
        Ok(())
    }

    /// Releases a detached batch to the element tree's ordinary finalization path.
    ///
    /// This terminal consumer performs no render deletion and invokes no
    /// lifecycle callback. It validates and consumes the linear token before
    /// element finalization starts; normal deepest-first element unmount then
    /// runs every view hook and removes each render node through the canonical
    /// [`PipelineOwner::remove_render_object`] path.
    ///
    /// ```
    /// use flui_rendering::pipeline::{
    ///     DetachedRenderSubtrees, PipelineOwner, ReleaseDetachedRenderSubtreesFailure,
    /// };
    ///
    /// fn release(
    ///     owner: &mut PipelineOwner,
    ///     token: DetachedRenderSubtrees,
    /// ) -> Result<(), ReleaseDetachedRenderSubtreesFailure> {
    ///     owner.release_detached_render_subtrees_for_finalization(token)
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// use flui_rendering::pipeline::{DetachedRenderSubtrees, PaintPhase, PipelineOwner};
    /// fn cannot_release_during_paint(
    ///     owner: &mut PipelineOwner<PaintPhase>,
    ///     token: DetachedRenderSubtrees,
    /// ) {
    ///     owner.release_detached_render_subtrees_for_finalization(token);
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// The same four conditions [`Self::attach_render_subtrees`] rejects,
    /// reported as [`ReleaseDetachedRenderSubtreesError`]: `WrongOwner`,
    /// `NodeNotFound`, `AttachmentChanged`, and `TopologyChanged`. Release
    /// validates rather than trusting the caller because the token may have sat
    /// in an inactive-element record across an arbitrary stretch of the frame.
    ///
    /// The failure returns the token via
    /// [`ReleaseDetachedRenderSubtreesFailure::into_token`]. This method mutates
    /// nothing in any case: on success it only consumes the token, which is what
    /// authorizes the ordinary unmount path to proceed.
    pub fn release_detached_render_subtrees_for_finalization(
        &mut self,
        token: DetachedRenderSubtrees,
    ) -> Result<(), ReleaseDetachedRenderSubtreesFailure> {
        if let Err(kind) = self.validate_token_for_release(&token) {
            return Err(ReleaseDetachedRenderSubtreesFailure { kind, token });
        }
        Ok(())
    }

    fn preflight_detach(
        &self,
        root_ids: &[RenderId],
    ) -> Result<Vec<DetachedRenderNode>, DetachRenderSubtreesError> {
        if root_ids.is_empty() {
            return Err(DetachRenderSubtreesError::Empty);
        }
        let mut records = Vec::new();
        // Maps each visited node to the index of the root that claimed it, so
        // a collision can name the two roots at fault rather than just the
        // node where their traversals met.
        let mut claiming_root = FxHashMap::default();
        let mut stack: Vec<_> = root_ids
            .iter()
            .enumerate()
            .rev()
            .map(|(root_index, &render_id)| (render_id, root_index))
            .collect();
        while let Some((render_id, root_index)) = stack.pop() {
            match claiming_root.entry(render_id) {
                Entry::Occupied(claimed) => {
                    return Err(classify_root_collision(
                        root_ids,
                        render_id,
                        *claimed.get(),
                        root_index,
                    ));
                }
                Entry::Vacant(unclaimed) => {
                    unclaimed.insert(root_index);
                }
            }
            let node = self
                .render_tree
                .get(render_id)
                .ok_or(DetachRenderSubtreesError::NodeNotFound { render_id })?;
            let attachment_epoch = node
                .attachment_epoch()
                .ok_or(DetachRenderSubtreesError::AlreadyDetached { render_id })?;
            records.push(DetachedRenderNode {
                render_id,
                root_index,
                attachment_epoch,
            });
            stack.extend(
                node.children()
                    .iter()
                    .rev()
                    .copied()
                    .map(|child_id| (child_id, root_index)),
            );
        }
        Ok(records)
    }

    fn validate_token_for_attach(
        &self,
        token: &DetachedRenderSubtrees,
    ) -> Result<(), AttachRenderSubtreesError> {
        if !Rc::ptr_eq(&self.relocation_owner_seal, &token.owner_seal) {
            return Err(AttachRenderSubtreesError::WrongOwner);
        }
        self.validate_recorded_nodes(token)
            .map_err(|failure| match failure {
                TokenValidationError::NodeNotFound { render_id } => {
                    AttachRenderSubtreesError::NodeNotFound { render_id }
                }
                TokenValidationError::AttachmentChanged { render_id } => {
                    AttachRenderSubtreesError::AttachmentChanged { render_id }
                }
                TokenValidationError::TopologyChanged => AttachRenderSubtreesError::TopologyChanged,
            })
    }

    fn validate_token_for_release(
        &self,
        token: &DetachedRenderSubtrees,
    ) -> Result<(), ReleaseDetachedRenderSubtreesError> {
        if !Rc::ptr_eq(&self.relocation_owner_seal, &token.owner_seal) {
            return Err(ReleaseDetachedRenderSubtreesError::WrongOwner);
        }
        self.validate_recorded_nodes(token)
            .map_err(|failure| match failure {
                TokenValidationError::NodeNotFound { render_id } => {
                    ReleaseDetachedRenderSubtreesError::NodeNotFound { render_id }
                }
                TokenValidationError::AttachmentChanged { render_id } => {
                    ReleaseDetachedRenderSubtreesError::AttachmentChanged { render_id }
                }
                TokenValidationError::TopologyChanged => {
                    ReleaseDetachedRenderSubtreesError::TopologyChanged
                }
            })
    }

    fn validate_recorded_nodes(
        &self,
        token: &DetachedRenderSubtrees,
    ) -> Result<(), TokenValidationError> {
        for record in &token.nodes {
            let node = self.render_tree.get(record.render_id).ok_or(
                TokenValidationError::NodeNotFound {
                    render_id: record.render_id,
                },
            )?;
            if !node.is_detached_after(record.attachment_epoch) {
                return Err(TokenValidationError::AttachmentChanged {
                    render_id: record.render_id,
                });
            }
        }

        let mut current = Vec::with_capacity(token.nodes.len());
        let mut membership = FxHashSet::default();
        let mut stack: Vec<_> = token
            .roots
            .iter()
            .enumerate()
            .rev()
            .map(|(root_index, &render_id)| (render_id, root_index))
            .collect();
        while let Some((render_id, root_index)) = stack.pop() {
            if !membership.insert(render_id) {
                return Err(TokenValidationError::TopologyChanged);
            }
            let node = self
                .render_tree
                .get(render_id)
                .ok_or(TokenValidationError::NodeNotFound { render_id })?;
            current.push((render_id, root_index));
            stack.extend(
                node.children()
                    .iter()
                    .rev()
                    .copied()
                    .map(|child_id| (child_id, root_index)),
            );
        }
        if current.len() != token.nodes.len()
            || current
                .iter()
                .zip(&token.nodes)
                .any(|(&(render_id, root_index), record)| {
                    render_id != record.render_id || root_index != record.root_index
                })
        {
            return Err(TokenValidationError::TopologyChanged);
        }
        Ok(())
    }
}

/// Names the two roots responsible for a repeated visit during detach preflight.
///
/// The preflight walks one root's whole subtree before starting the next, so
/// `claimant` always indexes an earlier root than `root_index`. Two subtrees of
/// a tree can only intersect when one of the two roots contains the other, and
/// the repeated node is then that contained root — which of the pair it is
/// decides the reported direction.
fn classify_root_collision(
    root_ids: &[RenderId],
    render_id: RenderId,
    claimant: usize,
    root_index: usize,
) -> DetachRenderSubtreesError {
    let claimed_root = root_ids[claimant];
    let current_root = root_ids[root_index];
    if claimed_root == current_root {
        return DetachRenderSubtreesError::DuplicateRoot { root: current_root };
    }
    if render_id == claimed_root {
        // The earlier root turned up inside the current root's subtree.
        return DetachRenderSubtreesError::OverlappingRoots {
            ancestor: current_root,
            descendant: claimed_root,
        };
    }
    // The current root lies inside the earlier root's subtree. A render graph
    // that is not a tree can also land here, where naming the two roots stays
    // the most useful thing to report.
    DetachRenderSubtreesError::OverlappingRoots {
        ancestor: claimed_root,
        descendant: current_root,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenValidationError {
    NodeNotFound { render_id: RenderId },
    AttachmentChanged { render_id: RenderId },
    TopologyChanged,
}
