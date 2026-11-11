//! The inherited scope of a node: which element publishes each type to it.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

use flui_foundation::ElementId;

use super::ElementTree;
use crate::view::ElementBase;

/// Compute a node's inherited scope from its parent's.
///
/// An element that publishes nothing returns the parent map unchanged
/// (`Arc::clone` — refcount bump, no allocation); a provider returns
/// `parent_map + (type → id)` for each type it publishes, so the nearest
/// provider of a type shadows. Average/worst case O(k) where k = providers in
/// scope (only at provider nodes; tiny in practice).
///
/// A provider's resolved scope therefore includes ITSELF, for its children.
/// Its own reads go through [`ElementTree::enclosing_provider`], which skips
/// it.
pub(super) fn compute_inherited_scope(
    parent_map: &Arc<HashMap<TypeId, ElementId>>,
    element: &dyn ElementBase,
    id: ElementId,
) -> Arc<HashMap<TypeId, ElementId>> {
    let Some(provider) = element.as_inherited() else {
        return Arc::clone(parent_map);
    };
    let mut map: Option<HashMap<TypeId, ElementId>> = None;
    provider.provided_types(&mut |type_id| {
        map.get_or_insert_with(|| (**parent_map).clone())
            .insert(type_id, id);
    });
    map.map_or_else(|| Arc::clone(parent_map), Arc::new)
}

impl ElementTree {
    /// The nearest provider of `type_id` above `id`, in O(1).
    ///
    /// A provider's own scope names itself, for its children. Its own reads
    /// resolve one level up, so an element never reads the value it
    /// publishes.
    pub(crate) fn enclosing_provider(&self, id: ElementId, type_id: TypeId) -> Option<ElementId> {
        let node = self.get(id)?;
        match node.inherited_provider(type_id) {
            Some(found) if found == id => self.get(node.parent()?)?.inherited_provider(type_id),
            found => found,
        }
    }

    /// Re-derive `id`'s inherited scope from what its element publishes now,
    /// then the ancestry of its whole subtree. Returns every descendant with
    /// its depth, for the caller to wake the ones that read inherited values.
    ///
    /// O(subtree size), paid only when an element starts or stops publishing
    /// a type.
    pub(crate) fn refresh_provided_scope(
        &mut self,
        id: ElementId,
        registry: &crate::element::child_manager::ChildManagerRegistry,
    ) -> Vec<(ElementId, usize)> {
        let Some(node) = self.get(id) else {
            return Vec::new();
        };
        let parent_map = node.parent.and_then(|parent| self.get(parent)).map_or_else(
            || Arc::new(HashMap::new()),
            |parent| Arc::clone(&parent.inherited),
        );
        let scope = compute_inherited_scope(&parent_map, node.element(), id);
        if let Some(node) = self.get_mut(id) {
            node.inherited = scope;
        }
        let mut descendants = Vec::new();
        let mut stack = self.owned_children(id, registry);
        for child in &stack {
            self.recompute_subtree_ancestry(*child, registry);
        }
        while let Some(descendant) = stack.pop() {
            let Some(node) = self.get(descendant) else {
                continue;
            };
            descendants.push((descendant, node.depth()));
            stack.extend(self.owned_children(descendant, registry));
        }
        descendants
    }
}
