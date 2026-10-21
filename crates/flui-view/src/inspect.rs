//! The element tree as an [`InspectTree`] (ADR-0242).
//!
//! A node is an element, named by its generational [`ElementId`] (`v3`, `v3.2` after the slot
//! is reused), so an id from before a removal answers
//! [`SnapshotError::Gone`](flui_foundation::inspect::SnapshotError::Gone) rather than another
//! element. Its kind is the view that created it (`Padding`, with any type arguments); the tree
//! adds the element's own facts (its element kind, key, lifecycle, dirtiness), and the view adds
//! its configuration through [`View::inspect`](crate::View::inspect). An element that owns a
//! render object links to it.

use flui_foundation::ElementId;
use flui_foundation::inspect::{
    Fields, Inspect, InspectId, InspectTree, Level, Link, Links, NodeView, ToValue, Value,
    kind_args_of, kind_of,
};

use crate::element::Lifecycle;
use crate::tree::ElementTree;
use crate::view::ElementBase;

impl ToValue for Lifecycle {
    fn to_value(&self) -> Value<'_> {
        Value::Variant {
            ty: "Lifecycle",
            name: match self {
                Self::Initial => "Initial",
                Self::Active => "Active",
                Self::Inactive => "Inactive",
                Self::Defunct => "Defunct",
            },
        }
    }
}

impl Inspect for dyn ElementBase {
    fn kind(&self) -> &'static str {
        kind_of(self.view_type_name())
    }

    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        kind_args_of(self.view_type_name(), args);
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        self.inspect_view(out);
    }
}

fn element_id(id: InspectId) -> Option<ElementId> {
    match id.link() {
        Link::Element(id) => Some(id),
        _ => None,
    }
}

impl InspectTree for ElementTree {
    type Node = dyn ElementBase;

    fn root(&self) -> Option<InspectId> {
        ElementTree::root(self).map(InspectId::from)
    }

    /// `None` for an element that is building, which the tree holds out of its slot.
    fn node(&self, id: InspectId) -> Option<NodeView<'_, dyn ElementBase>> {
        let id = element_id(id)?;
        let element = self.get(id)?.element_opt()?;
        Some(NodeView {
            object: element,
            rect: None,
            links: Links {
                element: Some(id),
                render: element.render_id(),
                ..Links::default()
            },
        })
    }

    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        element_id(id)
            .and_then(|id| self.get(id))
            .map(crate::tree::ElementNode::child_ids)
            .unwrap_or_default()
            .iter()
            .map(|&child| child.into())
    }

    fn facts(&self, id: InspectId, out: &mut Fields<'_>) {
        let Some(node) = element_id(id).and_then(|id| self.get(id)) else {
            return;
        };
        let Some(element) = node.element_opt() else {
            return;
        };
        out.field("element", element.debug_kind());
        if let Some(key) = node.key() {
            out.value("key", Value::Debug(&key));
        }
        out.field("lifecycle", &element.lifecycle())
            .default(&Lifecycle::Active);
        out.field("dirty", &element.is_dirty())
            .level(Level::Detail)
            .default(&false);
        out.field("depth", &node.depth()).level(Level::Debug);
        out.field("slot", &node.slot()).level(Level::Debug);
    }
}
