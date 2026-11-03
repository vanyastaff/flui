//! Where an element is in the tree, as an error names it.

use flui_foundation::inspect::{InspectId, kind_of};
use flui_foundation::{CreationSite, ElementId, PathEntry, TreeContext};

use super::ElementTree;
use crate::view::ElementBase;

impl ElementTree {
    /// Where `id` is in the element tree, as an error names it: the element, then its parents up
    /// to [`TREE_CONTEXT_DEPTH`](flui_foundation::TREE_CONTEXT_DEPTH), each by its view's kind as
    /// the inspector shows it and its id, with where the element's view was created. Empty for an
    /// id that is not a current node.
    ///
    /// An element that is building is held out of its slot and has no kind to show: it is left
    /// out of the path (and gives no site when it is `id` itself), while the walk goes on through
    /// its parent.
    #[must_use]
    pub fn tree_context(&self, id: ElementId) -> TreeContext {
        let Some(node) = self.get(id) else {
            return TreeContext::default();
        };
        let site = node
            .element_opt()
            .map_or_else(CreationSite::unknown, ElementBase::creation_site);
        let path = std::iter::successors(Some(id), |&at| self.get(at)?.parent()).filter_map(|at| {
            let element = self.get(at)?.element_opt()?;
            Some(PathEntry::new(
                kind_of(element.view_type_name()),
                InspectId::from(at),
            ))
        });
        TreeContext::new(path, site)
    }
}
