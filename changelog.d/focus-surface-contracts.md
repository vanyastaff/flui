### Changed

- Focus key dispatch and global handlers preserve `KeyEventResult`, including propagation stops that leave native default handling available. Traversal resolution takes `TraversalDirection` instead of a boolean.
