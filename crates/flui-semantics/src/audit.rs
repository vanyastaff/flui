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
//!   region inside its clip is smaller than the platform's minimum target.
//!
//! An element is interactive when it advertises an action that operates it
//! (`invoke`, `toggle`, `set_value`, `select`, `expand`, `collapse`); focus and
//! reveal alone move the cursor or the viewport, not the control.
//!
//! The hit region is the part of the element's bounds the window and every
//! clipping ancestor leave: the same region the actionability `visible` check
//! reads, from the published tree rather than from pixels. A target drawn large
//! but clipped by a scroll view to a sliver is that sliver until it is scrolled
//! in (ADR-0400 §5). Flutter's `MinimumTapTargetGuideline` judges the same
//! clipped region, and its tracker records a row partly scrolled under a pinned
//! app bar failing that way as a defect (flutter/flutter#144121): this rule
//! reports such a row, by that decision. An element the window does not show at
//! all has no hit region and is not judged.

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
    /// both axes. A non-finite extent, infinite or NaN, does not.
    #[must_use]
    pub fn admits(self, width: f64, height: f64) -> bool {
        width.is_finite() && height.is_finite() && width >= self.0 && height >= self.0
    }
}

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

    /// Every interactive element the window shows whose hit region inside its
    /// clip is smaller than `minimum` on either axis, in tree order (module
    /// docs).
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
                // No hit region: the window does not show it, so nothing can point at it.
                let Some(region) = shown_bounds(state, &node, published_ratio) else {
                    continue;
                };
                let (width, height) = (region.width(), region.height());
                if minimum.admits(width, height) {
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

/// How a finding names a role, for people.
fn role_name(role: Role) -> String {
    let name = role.name();
    if name.is_empty() {
        "element".to_owned()
    } else {
        name.replace('_', " ")
    }
}

/// `value` logical pixels as a finding prints them: at most two decimals.
fn px(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{rounded}")
}
