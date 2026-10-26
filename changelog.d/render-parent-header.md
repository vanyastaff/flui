### Changed

- A render node's parent is read from the tree: `RenderTree::parent` and the new
  `RenderTree::is_root` replace `RenderNode::parent`, `RenderNode::is_root` and
  `NodeLinks::parent`, which are removed along with the `*_with_parent`
  constructors (now `*_at_depth`). `RenderEntry::layout_leaf_only` and
  `RenderNode::layout_leaf_erased` take `has_parent`.
