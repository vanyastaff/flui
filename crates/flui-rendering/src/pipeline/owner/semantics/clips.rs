//! The clips a render node imposes on the semantics of its children, and which children it
//! presents to the semantics tree.

use flui_foundation::geometry::{Matrix4, Rect, Size};

use crate::storage::RenderNode;

use super::transformed_rect;

/// The clips an ancestor chain imposes on a node, in ROOT coordinates.
///
/// Publication stays clipped to paint bounds where there is an intersection.
/// Scroll caches limit ordinary publication but cannot erase a materialized
/// descendant with a reveal ancestor. Authored bounds and occlusion retain
/// their exclusion and hidden behavior independently of viewport clipping.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SemanticsClips {
    /// Outside this, a child is painted nowhere the user can see.
    pub(super) paint: Option<Rect<f64>>,
    /// Outside this, a child has no accessibility presence at all.
    semantics: Option<Rect<f64>>,
    cache: Option<Rect<f64>>,
    occlusion: Option<Rect<f64>>,
}

/// What [`SemanticsClips::apply`] decided about one node's rect.
pub(super) struct ClippedRect {
    /// The rect to publish, already narrowed to the surviving part.
    pub(super) rect: Rect<f64>,
    /// Authored occlusion or ordinary off-screen content makes the node hidden.
    pub(super) hidden: bool,
    /// Nothing of the node survives the semantics clip: publish no node for
    /// it. Its children are still walked — an overflowing child can extend
    /// back into the clip its parent fell outside of.
    pub(super) dropped: bool,
}

impl SemanticsClips {
    /// Narrows `rect` to what these clips leave of it.
    pub(super) fn apply(self, rect: Rect<f64>, revealable: bool) -> ClippedRect {
        let was_empty = rect.is_empty();

        // `Rect::intersect` reports a zero-area overlap as `Some(empty)`;
        // for a clip that is the same answer as no overlap at all.
        let intersect = |clip: &Rect<f64>, rect: &Rect<f64>| {
            clip.intersect(rect).filter(|kept| !kept.is_empty())
        };

        let semantics = if revealable {
            self.semantics
        } else {
            intersect_clips(self.semantics, self.cache)
        };
        let rect = match semantics {
            Some(clip) => match intersect(&clip, &rect) {
                Some(kept) => kept,
                // Nothing survives. An already-empty rect is not "clipped
                // away" — a zero-size annotated node is the caller's own
                // shape and keeps whatever treatment it had.
                None if !was_empty => {
                    return ClippedRect {
                        rect,
                        hidden: false,
                        dropped: true,
                    };
                }
                None => rect,
            },
            None => rect,
        };

        let Some(paint) = self.paint else {
            return ClippedRect {
                rect,
                hidden: false,
                dropped: false,
            };
        };

        match intersect(&paint, &rect) {
            Some(visible) => ClippedRect {
                rect: visible,
                hidden: false,
                dropped: false,
            },
            // Entirely outside what is painted: keep the semantics rect so a
            // "scroll to" action has somewhere to aim, and flag it hidden.
            None => ClippedRect {
                rect,
                hidden: !was_empty
                    && (!revealable
                        || self
                            .occlusion
                            .is_some_and(|clip| intersect(&clip, &rect).is_none())),
                dropped: false,
            },
        }
    }

    /// Whether [`apply`](Self::apply) could mark a rect hidden under clips of
    /// these kinds, wherever the rect and the clips land. A geometry move
    /// shifts the clips inside the moved subtree against the ones above it,
    /// so how they overlap this pass says nothing about the last one: only
    /// which clips exist is the same in both. Without a paint clip nothing is
    /// hidden; with one, content outside it is hidden unless a reveal ancestor
    /// keeps it, and authored occlusion hides it even then.
    pub(super) fn can_hide(self, revealable: bool) -> bool {
        self.paint.is_some() && (!revealable || self.occlusion.is_some())
    }

    /// The clips a child of this node inherits.
    ///
    /// Paint clips intersect. A scroll cache replaces only the cache; authored
    /// bounds replace the hard clip. Other paint clips narrow inherited bounds
    /// and remain authored occlusion even inside a revealable scroll subtree.
    pub(super) fn descend(
        self,
        local_paint: Option<Rect<f64>>,
        local_semantics: Option<crate::traits::SemanticsClip>,
    ) -> Self {
        use crate::traits::SemanticsClip;
        let paint = intersect_clips(self.paint, local_paint);
        let semantics = match local_semantics {
            Some(SemanticsClip::Bounds(replacement)) => Some(replacement),
            Some(SemanticsClip::ScrollCache(_)) => self.semantics,
            None => match (self.semantics, local_paint) {
                // Disjoint means nothing survives, which is an EMPTY clip —
                // `None` here would read as "no clip at all" and republish
                // the whole subtree unclipped.
                (Some(inherited), Some(narrowing)) => {
                    Some(inherited.intersect(&narrowing).unwrap_or(Rect::ZERO))
                }
                (Some(inherited), None) => Some(inherited),
                (None, _) => None,
            },
        };
        let cache = match local_semantics {
            Some(SemanticsClip::ScrollCache(bounds)) => Some(bounds),
            Some(SemanticsClip::Bounds(_)) => None,
            None => intersect_clips(self.cache, local_paint),
        };
        let occlusion = if matches!(local_semantics, Some(SemanticsClip::ScrollCache(_))) {
            self.occlusion
        } else {
            intersect_clips(self.occlusion, local_paint)
        };
        Self {
            paint,
            semantics,
            cache,
            occlusion,
        }
    }
}

/// Intersection where `None` means "no clip", not "empty".
fn intersect_clips(a: Option<Rect<f64>>, b: Option<Rect<f64>>) -> Option<Rect<f64>> {
    match (a, b) {
        (Some(a), Some(b)) => a.intersect(&b).or(Some(Rect::ZERO)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// The clips `node` imposes on the child in `child_slot`, moved from the
/// node's own coordinates into the walk's root coordinates.
/// Whether `node` presents the child in `child_slot` to the semantics tree.
///
/// Dispatched the same way `child_clips_of` is, because it answers the same
/// kind of question — one the parent knows about a particular child and the
/// walk cannot infer.
pub(super) fn visits_child_for_semantics(node: &RenderNode, child_slot: usize) -> bool {
    match node {
        RenderNode::Box(entry) => entry
            .render_object()
            .visits_child_for_semantics(child_slot, entry.state().geometry().unwrap_or(Size::ZERO)),
        RenderNode::Sliver(entry) => entry
            .render_object()
            .visits_child_for_semantics(child_slot, entry.state().absolute_paint_size()),
    }
}

/// The clips `node` imposes on the child in `child_slot`, moved from the
/// node's own coordinates into the walk's root coordinates.
pub(super) fn child_clips_of(
    node: &RenderNode,
    transform: Matrix4,
    child_slot: usize,
) -> (Option<Rect<f64>>, Option<crate::traits::SemanticsClip>) {
    // The node's own size is passed in rather than cached by each implementor.
    // A clip is always a function of the box it clips, so every implementor
    // would otherwise have to commit its own copy of a value the walk already
    // has — and keep that copy honest across every layout path.
    let (paint, semantics) = match node {
        RenderNode::Box(entry) => {
            let size = entry.state().geometry().unwrap_or(Size::ZERO);
            (
                entry
                    .render_object()
                    .describe_approximate_paint_clip(child_slot, size),
                entry
                    .render_object()
                    .describe_semantics_clip(child_slot, size),
            )
        }
        RenderNode::Sliver(entry) => {
            let size = entry.state().absolute_paint_size();
            (
                entry
                    .render_object()
                    .describe_approximate_paint_clip(child_slot, size),
                entry
                    .render_object()
                    .describe_semantics_clip(child_slot, size),
            )
        }
    };
    (
        paint.map(|rect| transformed_rect(transform, rect).unwrap_or(Rect::ZERO)),
        semantics.map(|clip| match clip {
            crate::traits::SemanticsClip::Bounds(rect) => crate::traits::SemanticsClip::Bounds(
                transformed_rect(transform, rect).unwrap_or(Rect::ZERO),
            ),
            crate::traits::SemanticsClip::ScrollCache(rect) => {
                crate::traits::SemanticsClip::ScrollCache(
                    transformed_rect(transform, rect).unwrap_or(Rect::ZERO),
                )
            }
        }),
    )
}
