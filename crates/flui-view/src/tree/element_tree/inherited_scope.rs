//! The inherited scope of a node: which element publishes each type to it.

use std::any::TypeId;
use std::rc::Rc;

use flui_foundation::ElementId;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::ElementTree;
use crate::view::ElementBase;

/// How many provider layers a lookup scans before it reaches a table. A
/// control publishes from one element, so a few controls nested in each
/// other still copy no table.
const SCANNED_LAYERS: u8 = 4;

/// The providers in scope at a node: published type → providing element.
///
/// Shared down the tree: an element that publishes nothing holds its parent's
/// scope, so only a provider allocates. The empty scope holds nothing at all,
/// so a node outside every provider costs no allocation either.
///
/// A provider does not copy what is above it. Its scope is one layer with the
/// types it publishes and a shared pointer to the scope above. A lookup walks
/// at most [`SCANNED_LAYERS`] layers and then asks one table; the provider
/// whose parent's chain is that long folds the chain into a new table.
#[derive(Debug, Clone, Default)]
pub(crate) struct InheritedScope(Option<Rc<Layer>>);

#[derive(Debug)]
struct Layer {
    /// What one provider publishes, a type at most once; these shadow
    /// everything above.
    own: SmallVec<[(TypeId, ElementId); 2]>,
    above: Above,
    /// The layers from this one down to `Above::Table` or the root, this one
    /// included.
    chain: u8,
}

#[derive(Debug)]
enum Above {
    Nothing,
    Layer(Rc<Layer>),
    /// Everything published above, folded.
    Table(Rc<FxHashMap<TypeId, ElementId>>),
}

impl InheritedScope {
    /// No provider in scope.
    pub(crate) const EMPTY: Self = Self(None);

    /// The provider of `type_id` in scope, if any.
    pub(super) fn get(&self, type_id: TypeId) -> Option<ElementId> {
        let mut layer = self.0.as_deref()?;
        loop {
            if let Some((_, id)) = layer.own.iter().find(|(known, _)| *known == type_id) {
                return Some(*id);
            }
            match &layer.above {
                Above::Nothing => return None,
                Above::Layer(above) => layer = above,
                Above::Table(table) => return table.get(&type_id).copied(),
            }
        }
    }
}

impl Layer {
    /// Everything this chain of layers publishes, folded into one table.
    fn fold(&self) -> FxHashMap<TypeId, ElementId> {
        let mut chain: SmallVec<[&Layer; SCANNED_LAYERS as usize]> = SmallVec::new();
        let mut layer = self;
        let mut table = loop {
            chain.push(layer);
            match &layer.above {
                Above::Nothing => break FxHashMap::default(),
                Above::Layer(above) => layer = above,
                Above::Table(table) => break FxHashMap::clone(table),
            }
        };
        // The farthest first, so a nearer provider of a type shadows it.
        for layer in chain.iter().rev() {
            table.extend(layer.own.iter().copied());
        }
        table
    }
}

/// Compute a node's inherited scope from its parent's.
///
/// An element that publishes nothing returns the parent scope unchanged (a
/// refcount bump at most, no allocation); a provider returns one layer of
/// `type → id` for each type it publishes over the parent scope, so the
/// nearest provider of a type shadows. A provider whose parent's chain is
/// [`SCANNED_LAYERS`] long also folds that chain into a table.
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
    let mut own: SmallVec<[(TypeId, ElementId); 2]> = SmallVec::new();
    provider.provided_types(&mut |type_id| {
        if !own.iter().any(|(known, _)| *known == type_id) {
            own.push((type_id, id));
        }
    });
    if own.is_empty() {
        return parent.clone();
    }
    let (above, chain) = match &parent.0 {
        None => (Above::Nothing, 1),
        Some(above) if above.chain >= SCANNED_LAYERS => (Above::Table(Rc::new(above.fold())), 1),
        Some(above) => (Above::Layer(Rc::clone(above)), above.chain + 1),
    };
    InheritedScope(Some(Rc::new(Layer { own, above, chain })))
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
