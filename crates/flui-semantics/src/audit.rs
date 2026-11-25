//! Accessibility rules over the published semantics tree (ADR-0400 §5, ADR-0238 §10).
//!
//! Each rule reads the tree an agent reads, the AccessKit tree the platform
//! adapter is handed, and answers the elements that break it as the protocol's
//! [`AuditFinding`]s. The development agent's `audit.a11y` is their caller. The
//! testing kit's gesture-alternatives check shares only [`MinimumTarget::admits`],
//! against a minimum of its own (`flui-testing`'s `gesture_alternatives`).
//!
//! - [`SemanticsOwner::audit_unnamed`]: an interactive element with no
//!   accessible name.
//! - [`SemanticsOwner::audit_target_size`]: an interactive element whose hit
//!   region inside the clips that do not scroll is smaller than the minimum
//!   target.
//!
//! An element is interactive when it advertises an action that operates it
//! (`invoke`, `toggle`, `set_value`, `select`, `expand`, `collapse`); focus and
//! reveal alone move the cursor or the viewport, not the control.
//!
//! The hit region is read from the published tree, never from pixels: the
//! element's bounds inside the window and every clip that does not scroll
//! (ADR-0400 §5). A clip that never moves (a clip rect, the window's edge)
//! keeps what it cuts, so a target it cuts to a sliver is judged by the sliver.
//! The clip of a scrollable ancestor does not shrink a target along the axis it
//! scrolls, because scrolling reveals the rest: along that axis the target is
//! judged by the extent it has once revealed, its bounds' up to what the
//! scrollable itself shows. Across that axis the scrollable's clip does not
//! move, so a row a vertical list cuts at its side is judged by the width the
//! list leaves. Judging the part a scrollable shows is the defect Flutter's
//! tracker records for its `MinimumTapTargetGuideline`, which fails a row
//! partly scrolled under a pinned app bar (flutter/flutter#144121).
//!
//! A scrollable is a clipping node (AccessKit's `clips_children`) that
//! publishes a scroll position or extent along an axis (`scroll_x`, `scroll_y`
//! and their minimum and maximum), as the semantics assembly publishes it: the
//! assembly folds every clip that does not scroll into the bounds of the nodes
//! under it, and under a scrollable keeps a node's whole bounds so a reader can
//! reveal it. This is not the region the actionability `visible` check reads
//! (`shown_bounds`): a pointer cannot reach what a viewport hides now, while a
//! target's size is what it offers once shown. An element the window does not
//! show at all is not judged.
//!
//! Two kinds of target are not judged, from what the tree publishes:
//!
//! - an inline link: a `link` beside text in the same parent (the regions a
//!   paragraph publishes for its spans), whose height the line sets. WCAG 2.5.8
//!   exempts a target in a sentence or constrained by the line-height of
//!   non-target text; a link alone in its parent is judged.
//! - a target inside an interactive ancestor whose own hit region meets the
//!   minimum: a pointer that misses the small target lands on that ancestor.
//!   Android's Accessibility Test Framework demotes this case to a warning
//!   ("clickable ancestor"); an [`AuditFinding`] has no severity to say so
//!   (`docs/gaps.md`, ADR-0400), so the case is not judged rather than reported
//!   as a defect.
//!
//! A target an ancestor's fixed clip cuts stays judged by what the clip leaves,
//! and its finding names the bounds it was cut from: a pointer reaches only the
//! part shown, and nothing else catches the miss. That is the framework's
//! "clipped by ancestor" case, which it also reports, as a warning.

use accesskit_consumer::{NodeRef, TreeState};
use flui_protocol::{
    audit::AuditFinding, audit::AuditRule, tree::ElementRef, wire::ActionName, wire::Role,
};

use crate::agent::{
    AgentFilter, Placement, Published, WireReadError, element_id, shown_bounds, wire_node,
};
use crate::owner::SemanticsOwner;

/// The smallest hit region, in logical pixels on each axis, a pointer is
/// expected to reach: what [`SemanticsOwner::audit_target_size`] judges against.
///
/// The minimum belongs to the gesture settings of the presentation
/// (`GestureSettings::min_target` in `flui-interaction`, per pointer kind), so
/// the caller picks it for the pointer it judges for and passes it in: this
/// crate sits below the one that owns it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimumTarget(f64);

impl MinimumTarget {
    /// A minimum of `extent` logical pixels on each axis; `None` unless it is
    /// finite and positive.
    #[must_use]
    pub fn try_new(extent: f64) -> Option<Self> {
        (extent.is_finite() && extent > 0.0).then_some(Self(extent))
    }

    /// The extent, in logical pixels.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// Whether a region `width` × `height` logical pixels meets the minimum on
    /// both axes, to within half a hundredth of a logical pixel, which absorbs
    /// the round-off of the logical-to-device round trip a published extent
    /// takes. A non-finite extent, infinite or NaN, does not.
    #[must_use]
    pub fn admits(self, width: f64, height: f64) -> bool {
        let meets = |extent: f64| extent.is_finite() && extent + ADMIT_TOLERANCE >= self.0;
        meets(width) && meets(height)
    }
}

/// How far, in logical pixels, an extent may fall short of a
/// [`MinimumTarget`] and still meet it.
///
/// A published extent is a logical rect scaled to device pixels and back, and
/// at a ratio that is not a power of two the round trip leaves an extent laid
/// out at exactly the minimum a few ulps under it (a 44 px target from 7 px to
/// 51 px: `51 × 1.3 / 1.3 - 7 × 1.3 / 1.3` is `43.999999999999993` in `f64`).
/// Flutter's `MinimumTapTargetGuideline` allows `1e-10`, which the round-off of
/// edges past about 10⁶ px exceeds (there the same round trip misses by up to
/// about `2.3e-10`). Half a
/// hundredth of a pixel is far above any round-off, and a fiftieth of the
/// smallest device pixel a platform draws (at a ratio of 4), so no target a
/// device draws smaller passes; and a target that fails prints, at a finding's
/// two decimals, under the minimum it fails.
const ADMIT_TOLERANCE: f64 = 0.005;

impl SemanticsOwner {
    /// Every interactive element of the published tree with no accessible
    /// name, in tree order (module docs).
    ///
    /// Elements past a scroll view's edge count: they are elements of the
    /// interface a reader moves to, shown or not. A hidden subtree does not.
    ///
    /// # Errors
    ///
    /// [`WireReadError::NoTree`] before the first assembly,
    /// [`WireReadError::Malformed`] when the published tree breaks
    /// AccessKit's contract.
    pub fn audit_unnamed(&self, placement: Placement) -> Result<Vec<AuditFinding>, WireReadError> {
        let filter = AgentFilter::of(self);
        self.audit(|state, _| {
            let mut findings = Vec::new();
            for node in agent_nodes(state, filter) {
                let Some(id) = element_id(&node) else {
                    continue;
                };
                let wire = wire_node(&node, id, false);
                if !is_interactive(&wire.actions) || wire.name.is_some() {
                    continue;
                }
                findings.push(AuditFinding::new(
                    AuditRule::Unnamed,
                    ElementRef::new(placement.window, id),
                    format!(
                        "this {} has no accessible name: give it a label, or a text a reader \
                         can speak",
                        role_name(wire.role)
                    ),
                ));
            }
            findings
        })
    }

    /// Every interactive element the window shows whose hit region, inside the
    /// window and the clips that do not scroll, is smaller than `minimum` on
    /// either axis, in tree order (module docs).
    ///
    /// # Errors
    ///
    /// As [`Self::audit_unnamed`].
    pub fn audit_target_size(
        &self,
        minimum: MinimumTarget,
        placement: Placement,
    ) -> Result<Vec<AuditFinding>, WireReadError> {
        let filter = AgentFilter::of(self);
        let published_ratio = self.device_pixel_ratio();
        self.audit(|state, _| {
            let mut findings = Vec::new();
            for node in agent_nodes(state, filter) {
                let Some(id) = element_id(&node) else {
                    continue;
                };
                let wire = wire_node(&node, id, false);
                if !is_interactive(&wire.actions) {
                    continue;
                }
                // The window shows none of it: nothing can point at it, and it is not judged.
                if shown_bounds(state, &node, published_ratio).is_none() {
                    continue;
                }
                let Some((width, height)) = target_extent(state, &node, published_ratio) else {
                    continue;
                };
                if minimum.admits(width, height)
                    || is_inline_link(&node, wire.role, filter)
                    || has_admitted_interactive_ancestor(state, &node, minimum, published_ratio)
                {
                    continue;
                }
                let bounds = crate::agent::logical_bounds(&node, published_ratio);
                let clipped = bounds.is_some_and(|bounds| {
                    (bounds.width() - width).abs() > CLIP_EPSILON
                        || (bounds.height() - height).abs() > CLIP_EPSILON
                });
                let drawn = match bounds.filter(|_| clipped) {
                    Some(bounds) => format!(
                        ", clipped from its {} × {} bounds",
                        px(bounds.width()),
                        px(bounds.height())
                    ),
                    None => String::new(),
                };
                findings.push(AuditFinding::new(
                    AuditRule::TargetSize,
                    ElementRef::new(placement.window, id),
                    format!(
                        "this {}'s hit region is {} × {} logical px{drawn}, under the \
                         platform's {} px minimum target",
                        role_name(wire.role),
                        px(width),
                        px(height),
                        px(minimum.get())
                    ),
                ));
            }
            findings
        })
    }

    /// Runs `rule` over the published tree.
    fn audit<R>(&self, rule: impl FnOnce(&TreeState, bool) -> R) -> Result<R, WireReadError> {
        self.with_published(rule)
            .map_err(|published| match published {
                Published::NoTree => WireReadError::NoTree,
                Published::Malformed => WireReadError::Malformed,
            })
    }
}

/// Bounds closer than this, in logical pixels, are the same: a region this
/// close to its bounds is not reported as clipped.
const CLIP_EPSILON: f64 = 1e-3;

/// The width and height, in logical pixels, of the hit region `node` has once
/// every scrollable above it has revealed as much of it as it can (module
/// docs): `None` when it has no bounds.
///
/// Each axis is followed from the node up. A clip that does not scroll along
/// the axis, and the window at the end, cut what is followed. A clipping
/// ancestor that scrolls along the axis does not cut it: it caps the extent at
/// what has been followed so far, and from there its own extent is followed,
/// since the node shows at most as much as its scrollable does.
fn target_extent(
    state: &TreeState,
    node: &NodeRef<'_>,
    published_ratio: f64,
) -> Option<(f64, f64)> {
    let rect = crate::agent::logical_bounds(node, published_ratio)?;
    let mut across = AxisExtent::new(rect.left(), rect.right());
    let mut down = AxisExtent::new(rect.top(), rect.bottom());
    let clips =
        std::iter::successors(node.parent(), NodeRef::parent).filter(NodeRef::clips_children);
    for clip in clips {
        let Some(bounds) = crate::agent::logical_bounds(&clip, published_ratio) else {
            continue;
        };
        let data = clip.data();
        let scrolls_across = data.scroll_x().is_some()
            || data.scroll_x_min().is_some()
            || data.scroll_x_max().is_some();
        let scrolls_down = data.scroll_y().is_some()
            || data.scroll_y_min().is_some()
            || data.scroll_y_max().is_some();
        across.pass(bounds.left(), bounds.right(), scrolls_across);
        down.pass(bounds.top(), bounds.bottom(), scrolls_down);
    }
    if let Some(window) = crate::agent::logical_bounds(&state.root(), published_ratio) {
        across.pass(window.left(), window.right(), false);
        down.pass(window.top(), window.bottom(), false);
    }
    Some((across.extent(), down.extent()))
}

/// One axis of [`target_extent`]'s walk.
struct AxisExtent {
    /// The most the target can show along the axis, set by the scrollables
    /// passed so far; infinite before the first.
    cap: f64,
    /// The span followed now, the target's or a scrollable's, as the clips
    /// passed since leave it.
    start: f64,
    end: f64,
}

impl AxisExtent {
    fn new(start: f64, end: f64) -> Self {
        Self {
            cap: f64::INFINITY,
            start,
            end,
        }
    }

    /// Passes a clip spanning `start..end` along the axis, which scrolls
    /// along it or does not.
    fn pass(&mut self, start: f64, end: f64, scrolls: bool) {
        if scrolls {
            self.cap = self.cap.min(self.length());
            (self.start, self.end) = (start, end);
        } else {
            (self.start, self.end) = (self.start.max(start), self.end.min(end));
        }
    }

    fn length(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// The extent the target shows along the axis.
    fn extent(&self) -> f64 {
        self.cap.min(self.length())
    }
}

/// Whether `node`, of `role`, is a link set inline in text (module docs): a
/// `link` with a sibling the agent reads that has a name and operates nothing.
fn is_inline_link(node: &NodeRef<'_>, role: Role, filter: AgentFilter) -> bool {
    if role != Role::Link {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .filtered_children(|child| filter.apply(child))
        .filter(|sibling| sibling.id() != node.id())
        .any(|sibling| {
            element_id(&sibling).is_some_and(|id| {
                let wire = wire_node(&sibling, id, false);
                wire.name.is_some() && !is_interactive(&wire.actions)
            })
        })
}

/// Whether an ancestor of `node` operates a control and its own hit region
/// meets `minimum` (module docs).
fn has_admitted_interactive_ancestor(
    state: &TreeState,
    node: &NodeRef<'_>,
    minimum: MinimumTarget,
    published_ratio: f64,
) -> bool {
    std::iter::successors(node.parent(), NodeRef::parent).any(|ancestor| {
        let interactive = element_id(&ancestor)
            .is_some_and(|id| is_interactive(&wire_node(&ancestor, id, false).actions));
        interactive
            && target_extent(state, &ancestor, published_ratio)
                .is_some_and(|(width, height)| minimum.admits(width, height))
    })
}

/// Whether `actions` include one that operates the control (module docs).
fn is_interactive(actions: &[ActionName]) -> bool {
    actions.iter().any(|action| {
        matches!(
            action,
            ActionName::Invoke
                | ActionName::Toggle
                | ActionName::SetValue
                | ActionName::Select
                | ActionName::Expand
                | ActionName::Collapse
        )
    })
}

/// Every node an agent reads, in tree order (the root first, then each
/// child's subtree before its next sibling). The walk keeps its own stack, so
/// a deep tree does not grow the thread's, and visits each node once.
fn agent_nodes(state: &TreeState, filter: AgentFilter) -> impl Iterator<Item = NodeRef<'_>> {
    let mut pending = vec![state.root()];
    std::iter::from_fn(move || {
        let node = pending.pop()?;
        let start = pending.len();
        pending.extend(node.filtered_children(|child| filter.apply(child)));
        pending[start..].reverse();
        Some(node)
    })
}

/// How a finding names a role, for people: its wire name in words. Every role
/// has a wire name, `unknown` included.
fn role_name(role: Role) -> String {
    role.name().replace('_', " ")
}

/// `value` logical pixels as a finding prints them: at most two decimals.
fn px(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{rounded}")
}
