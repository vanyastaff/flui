### Changed

- Removing an element or render subtree, as a list does for each item it scrolls off, reuses the
  walk's buffers instead of allocating a list per node: `list_scroll_page` allocates about a fifth
  less per step.
