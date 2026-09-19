//! `TreeRead<LayerId>` + `TreeNav<LayerId>` for [`LayerTree`], so the generic
//! walkers in `flui-tree` (`ancestors`, `descendants`,
//! `lowest_common_ancestor`) run over the compositor tree without a second
//! copy of each algorithm here.
//!
//! `TreeWrite` is deliberately not implemented: the tree is append-only (see
//! [`LayerTree::push_child`]), and the trait's `remove`/`clear` shapes would
//! promise a mutation the type cannot honour.

use flui_foundation::LayerId;
use flui_tree::{
    TreeNav, TreeRead,
    iter::{AllSiblings, Ancestors, DescendantsWithDepth},
};

use super::layer_tree::{LayerNode, LayerTree};

impl TreeRead<LayerId> for LayerTree {
    type Node = LayerNode;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: LayerId) -> Option<&Self::Node> {
        LayerTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: LayerId) -> bool {
        LayerTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        LayerTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.ids()
    }
}

impl TreeNav<LayerId> for LayerTree {
    const MAX_DEPTH: usize = 32;
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: LayerId) -> Option<LayerId> {
        LayerTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: LayerId) -> impl Iterator<Item = LayerId> + '_ {
        LayerTree::children(self, id)
            .into_iter()
            .flat_map(|children| children.iter().copied())
    }

    #[inline]
    fn ancestors(&self, start: LayerId) -> impl Iterator<Item = LayerId> + '_ {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: LayerId) -> impl Iterator<Item = (LayerId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: LayerId) -> impl Iterator<Item = LayerId> + '_ {
        AllSiblings::new(self, id)
    }
}

#[cfg(test)]
mod tests {
    use flui_tree::{TreeNav, TreeRead};

    use super::*;
    use crate::{Layer, OffsetLayer};

    fn offset() -> Layer {
        Layer::from(OffsetLayer::zero())
    }

    #[test]
    fn trait_views_agree_with_inherent_ones() {
        let mut tree = LayerTree::new(offset());
        let root = tree.root();
        let a = tree.push_child(root, offset());
        let b = tree.push_child(root, offset());
        let c = tree.push_child(a, offset());

        assert_eq!(TreeRead::len(&tree), 3 + 1);
        assert!(TreeRead::contains(&tree, c));
        assert_eq!(
            TreeRead::node_ids(&tree).collect::<Vec<_>>(),
            vec![root, a, b, c]
        );
        assert_eq!(TreeNav::parent(&tree, c), Some(a));
        assert_eq!(
            TreeNav::children(&tree, root).collect::<Vec<_>>(),
            vec![a, b]
        );
        assert_eq!(tree.ancestors(c).collect::<Vec<_>>(), vec![c, a, root]);
        assert_eq!(tree.lowest_common_ancestor(c, b), Some(root));
        assert_eq!(tree.lowest_common_ancestor(c, a), Some(a));
        assert_eq!(
            tree.descendants(root).map(|(id, _)| id).collect::<Vec<_>>(),
            vec![root, a, c, b]
        );
    }
}
