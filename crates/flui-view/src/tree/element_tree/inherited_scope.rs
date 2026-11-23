//! The inherited scope of a node: which element publishes each type to it.

use std::any::TypeId;
use std::cell::OnceCell;
use std::rc::Rc;

use flui_foundation::ElementId;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::ElementTree;
use crate::view::ElementBase;

/// Everything the providers from the root down to one layer publish.
type Table = FxHashMap<TypeId, ElementId>;

/// How many provider layers apart the layers that may hold a table sit: a
/// layer whose depth is a multiple of `STRIDE` folds its chain once, for the
/// layers `STRIDE` to `2 * STRIDE - 1` below it. A lookup scans fewer than
/// `2 * STRIDE` layers and then asks one table.
const STRIDE: usize = 4;

/// The providers in scope at a node: published type → providing element.
///
/// Shared down the tree: an element that publishes nothing holds its parent's
/// scope, so only a provider allocates. The empty scope holds nothing at all,
/// so a node outside every provider costs no allocation either.
///
/// A provider does not copy what is above it. Its scope is one layer with the
/// types it publishes and a shared pointer to the scope above. Where the
/// tables sit depends on the depth alone, never on which element asks: the
/// layer at a multiple of [`STRIDE`] folds its chain into one table, once, and
/// every layer `STRIDE` to `2 * STRIDE - 1` below it reads that one table. A
/// control whose own providers nest fewer than `STRIDE` deep therefore reads
/// a table its ancestors already hold and copies none, however many providers
/// sit above it.
#[derive(Debug, Clone, Default)]
pub(crate) struct InheritedScope(Option<Rc<Layer>>);

#[derive(Debug)]
struct Layer {
    /// What one provider publishes, a type at most once; these shadow
    /// everything above.
    own: SmallVec<[(TypeId, ElementId); 2]>,
    above: Option<Rc<Layer>>,
    /// The provider layers from the root down to this one, this one
    /// included. A layer is a live element, so the count stays below the
    /// element count and its `+ 1` cannot overflow.
    depth: usize,
    /// Everything this layer and those above it publish, folded once and
    /// read by every layer whose [`base_depth`] is this one's. Only a layer
    /// at a multiple of [`STRIDE`] is ever folded, and it is folded when the
    /// first such layer mounts.
    folded: OnceCell<Box<Table>>,
}

/// The depth of the layer whose table a layer at `depth` reads: the largest
/// multiple of [`STRIDE`] at least `STRIDE` above it, or 0 for none, when the
/// layer's lookup scans to the root.
const fn base_depth(depth: usize) -> usize {
    (depth / STRIDE).saturating_sub(1) * STRIDE
}

impl InheritedScope {
    /// No provider in scope.
    pub(crate) const EMPTY: Self = Self(None);

    /// The provider of `type_id` in scope, if any: fewer than `2 * STRIDE`
    /// layers scanned, then one table asked.
    pub(super) fn get(&self, type_id: TypeId) -> Option<ElementId> {
        let nearest = self.0.as_deref()?;
        let base = base_depth(nearest.depth);
        let mut layer = nearest;
        loop {
            if layer.depth == base {
                return layer.folded().get(&type_id).copied();
            }
            if let Some((_, id)) = layer.own.iter().find(|(known, _)| *known == type_id) {
                return Some(*id);
            }
            layer = layer.above.as_deref()?;
        }
    }
}

impl Layer {
    /// The ancestor of this layer, or this layer, at `depth`; `None` for
    /// depth 0. Walks `self.depth - depth` layers.
    fn at_depth(&self, depth: usize) -> Option<&Self> {
        let mut layer = self;
        while layer.depth > depth {
            layer = layer.above.as_deref()?;
        }
        (layer.depth == depth).then_some(layer)
    }

    /// This layer's chain folded into one table, made on the first call: the
    /// table at its own base, cloned, and the fewer than `2 * STRIDE` layers
    /// below that base. A layer is folded no sooner than a layer below it
    /// reads it, and that one's mount folded the base first (see
    /// [`compute_inherited_scope`]), so a fold never recurses.
    fn folded(&self) -> &Table {
        self.folded.get_or_init(|| {
            let base = base_depth(self.depth);
            // The layers down to the base, the farthest first, so a nearer
            // provider of a type shadows it.
            let mut chain: SmallVec<[&Self; 2 * STRIDE]> = SmallVec::new();
            let mut layer = Some(self);
            while let Some(scanned) = layer.filter(|scanned| scanned.depth > base) {
                chain.push(scanned);
                layer = scanned.above.as_deref();
            }
            let mut table = layer.map_or_default(|base| base.folded().clone());
            for scanned in chain.iter().rev() {
                table.extend(scanned.own.iter().copied());
            }
            Box::new(table)
        })
    }
}

/// Compute a node's inherited scope from its parent's.
///
/// An element that publishes nothing returns the parent scope unchanged (a
/// refcount bump at most, no allocation); a provider returns one layer of
/// `type → id` for each type it publishes over the parent scope, so the
/// nearest provider of a type shadows. A provider one stride below a multiple
/// of [`STRIDE`] is the first to read that layer's table, so it makes the
/// layer fold, once for all of them.
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
    let above = parent.0.clone();
    let depth = above.as_ref().map_or(0, |above| above.depth) + 1;
    let base = base_depth(depth);
    if let Some(above) = &above
        && base != base_depth(above.depth)
    {
        // The base moved one stride down, to an ancestor of the parent fewer
        // than `STRIDE` layers up: fold it now, while its own base is folded.
        if let Some(base) = above.at_depth(base) {
            base.folded();
        }
    }
    InheritedScope(Some(Rc::new(Layer {
        own,
        above,
        depth,
        folded: OnceCell::new(),
    })))
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
        let mut stack = Vec::new();
        self.extend_owned_children(id, registry, &mut stack);
        for child in &stack {
            self.recompute_subtree_ancestry(*child, registry);
        }
        while let Some(descendant) = stack.pop() {
            let Some(node) = self.get(descendant) else {
                continue;
            };
            descendants.push((descendant, node.depth()));
            self.extend_owned_children(descendant, registry, &mut stack);
        }
        descendants
    }
}
