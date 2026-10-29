### Changed

- The dry intrinsic, dry-layout and dry-baseline queries enter the nodes they
  reach instead of borrowing the whole subtree first, so a query costs the
  nodes it visits.

### Removed

- `RenderTree::get_subtree_mut`: no walk borrows a whole subtree up front any
  more.
