//! The inherited scope of a node: which element publishes each type to it.

use std::any::TypeId;
use std::sync::Arc;

use flui_foundation::ElementId;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::ElementTree;
use crate::view::ElementBase;

/// How many types the nearest providers add before a scope folds them into
/// its table. A control publishes one or two, so a control under a control
/// still copies no table.
const RECENT: usize = 4;

/// The providers in scope at a node: published type → providing element.
///
/// Shared down the tree: an element that publishes nothing holds its parent's
/// scope, so only a provider allocates. The empty scope holds nothing at all,
/// so a node outside every provider costs no allocation either.
///
/// A provider does not copy the table of everything above it. Its scope is
/// that table, shared, and the few types the nearest providers added since
/// it was built, the nearest last. Only when those outgrow [`RECENT`] does a
/// provider build a new table from both. A lookup scans the few and then
/// asks the table once.
#[derive(Debug, Clone, Default)]
pub(crate) struct InheritedScope(Option<Arc<Layers>>);

#[derive(Debug)]
struct Layers {
    /// What the nearest providers publish, a type at most once; these shadow
    /// `table`.
    recent: SmallVec<[(TypeId, ElementId); RECENT]>,
    /// Everything published above them.
    table: Option<Arc<FxHashMap<TypeId, ElementId>>>,
}

impl InheritedScope {
    /// No provider in scope.
    pub(crate) const EMPTY: Self = Self(None);

    /// The provider of `type_id` in scope, if any.
    pub(super) fn get(&self, type_id: TypeId) -> Option<ElementId> {
        let layers = self.0.as_ref()?;
        layers
            .recent
            .iter()
            .find(|(known, _)| *known == type_id)
            .map(|(_, id)| *id)
            .or_else(|| layers.table.as_ref()?.get(&type_id).copied())
    }
}

impl Layers {
    /// These layers with `type_id` published by `id`, shadowing any provider
    /// of the type above.
    fn publish(&mut self, type_id: TypeId, id: ElementId) {
        if let Some((_, provider)) = self.recent.iter_mut().find(|(known, _)| *known == type_id) {
            *provider = id;
            return;
        }
        if self.recent.len() == RECENT {
            let mut table: FxHashMap<TypeId, ElementId> =
                self.table.as_deref().cloned().unwrap_or_default();
            table.extend(self.recent.drain(..));
            self.table = Some(Arc::new(table));
        }
        self.recent.push((type_id, id));
    }
}

/// Compute a node's inherited scope from its parent's.
///
/// An element that publishes nothing returns the parent scope unchanged (a
/// refcount bump at most, no allocation); a provider returns
/// `parent + (type → id)` for each type it publishes, so the nearest
/// provider of a type shadows. A provider allocates its layers, and copies
/// the table above only when the types added since it was built outgrow
/// [`RECENT`].
///
/// A provider's resolved scope therefore includes ITSELF, for its children.
/// Its own reads go through [`ElementTree::enclosing_provider`], which skips
/// it.
pub(super) fn compute_inherited_scope(
    parent: &InheritedScope,
    element: &dyn ElementBase,
    id: ElementId,
) -> InheritedScope {
    let Some(provider) = element.as_inherited() else {
        return parent.clone();
    };
    let mut layers: Option<Layers> = None;
    provider.provided_types(&mut |type_id| {
        layers
            .get_or_insert_with(|| match parent.0.as_deref() {
                Some(above) => Layers {
                    recent: above.recent.clone(),
                    table: above.table.clone(),
                },
                None => Layers {
                    recent: SmallVec::new(),
                    table: None,
                },
            })
            .publish(type_id, id);
    });
    layers.map_or_else(
        || parent.clone(),
        |layers| InheritedScope(Some(Arc::new(layers))),
    )
}

impl ElementTree {
    /// The nearest provider of `type_id` above `id`: a scan of the few
    /// nearest and one table lookup.
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
        let parent_map = node
            .parent
            .and_then(|parent| self.get(parent))
            .map_or_else(|| InheritedScope::EMPTY, |parent| parent.inherited.clone());
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
