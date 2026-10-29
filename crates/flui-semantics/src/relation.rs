//! Relations between semantics nodes, named by anchor (ADR-0205).
//!
//! A widget cannot name another node's `AccessibilityNodeId`: it does not exist
//! when `build` runs and changes when the target remounts. The target instead
//! declares a [`SemanticsAnchor`], the source holds a relation to it, and the
//! publishing owner resolves the anchor against the tree it publishes.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// An opaque name for the node (or nodes) that declare it.
///
/// Identity is the allocation: two anchors are equal only when one is a clone
/// of the other. Every configuration holding an anchor keeps its allocation
/// alive, so a published relation can never resolve through a reused one.
#[derive(Clone, Default)]
pub struct SemanticsAnchor(Arc<()>);

impl SemanticsAnchor {
    /// A fresh anchor, equal to no other.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl PartialEq for SemanticsAnchor {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SemanticsAnchor {}

impl Hash for SemanticsAnchor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl fmt::Debug for SemanticsAnchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SemanticsAnchor({:p})", Arc::as_ptr(&self.0))
    }
}

/// How a node relates to the nodes an anchor names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SemanticsRelation {
    /// The anchored nodes name this one (`aria-labelledby`); used when this
    /// node has no label of its own.
    LabelledBy,
    /// This node controls the anchored nodes, such as a button and the panel
    /// it opens (`aria-controls`).
    Controls,
    /// The anchored nodes describe this one (`aria-describedby`): text read
    /// after its name, such as a field's helper line.
    DescribedBy,
    /// The anchored nodes hold more detail about this one (`aria-details`),
    /// such as a chart's data table.
    Details,
    /// Reading continues at the anchored nodes after this one
    /// (`aria-flowto`), overriding document order.
    FlowTo,
    /// The anchored node says why this node's value is invalid
    /// (`aria-errormessage`). AccessKit takes one node: the first published
    /// target in identity order.
    ErrorMessage,
    /// The anchored node is the active descendant of this focused composite
    /// (`aria-activedescendant`), such as the highlighted option of a combo
    /// box whose focus stays in its field. AccessKit takes one node: the first
    /// published target in identity order.
    ActiveDescendant,
}
